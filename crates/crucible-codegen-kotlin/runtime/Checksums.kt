import io.ktor.utils.io.readAvailable
import kotlin.io.encoding.Base64
import kotlinx.serialization.json.*

internal object Checksums {
    private val supported=setOf("CRC32","CRC32C","CRC64NVME","MD5","SHA1","SHA256","SHA512")
    data class Plan(val operation: String,val algorithm: String?,val legacy: Boolean,val validate: Boolean,val descriptor: JsonObject?)
    fun prepare(operation: String,op: JsonObject,values: MutableMap<String,JsonElement>,config: ClientConfig): Plan {
        val checksum=op["checksum"] as? JsonObject ?: return Plan(operation,null,false,false,null)
        if(checksum.text("kind")=="md5")return Plan(operation,"MD5",true,false,checksum)
        fun value(member: String?)=(values[member] as? JsonPrimitive)?.takeUnless { it is JsonNull }?.content
        val member=checksum.text("requestAlgorithmMember")
        var algorithm=value(member)
        val required=checksum.flag("requestRequired")
        val legacy=member==null && required
        if(legacy)algorithm="MD5"
        else if(member!=null && algorithm==null && (required || config.requestChecksumCalculation==ChecksumPolicy.WHEN_SUPPORTED)) { algorithm="CRC32";values[member]=JsonPrimitive(algorithm) }
        if(algorithm!=null) {
            require(algorithm in supported) { "Unsupported checksum algorithm: $algorithm" }
        }
        val mode=checksum.text("validationModeMember")
        if(mode!=null && value(mode)==null && config.responseChecksumValidation==ChecksumPolicy.WHEN_SUPPORTED)values[mode]=JsonPrimitive("ENABLED")
        val validate=mode!=null && value(mode)=="ENABLED"
        return Plan(operation,algorithm,legacy,validate,checksum)
    }
    private fun requestHeader(plan: Plan,headers: Map<String,String>): String? {
        val algorithm=plan.algorithm ?: return null
        val name=if(plan.legacy)"content-md5" else header(algorithm)
        // A caller-supplied checksum header is sent unchanged.
        return name.takeUnless { headers.keys.any { it.equals(name,true) } }
    }
    fun request(plan: Plan,headers: MutableMap<String,String>,body: ByteArray?) {
        val name=requestHeader(plan,headers) ?: return
        headers[name]=Base64.encode(Hasher(plan.algorithm!!).apply { update(body ?: byteArrayOf(),0,body?.size ?: 0) }.finish())
    }
    /** A replayable body is read once more to compute its checksum; a one-shot body needs a supplied header. */
    suspend fun requestStream(plan: Plan,headers: Map<String,String>,stream: ByteStream): Map<String,String> {
        val name=requestHeader(plan,headers) ?: return emptyMap()
        val algorithm=plan.algorithm!!
        if(algorithm=="SHA256" && stream.sha256!=null) return mapOf(name to Base64.encode(stream.sha256.chunked(2).map { it.toInt(16).toByte() }.toByteArray()))
        require(stream.replayable) { "A one-shot stream requires a caller-supplied $name checksum header" }
        val hasher=Hasher(algorithm);val channel=stream.openChannel();val buffer=ByteArray(16*1024)
        try {
            while(true) { val size=channel.readAvailable(buffer,0,buffer.size);if(size<0)break;hasher.update(buffer,0,size) }
        } catch(error: Exception) { throw stream.failure ?: error } finally { channel.cancel(null) }
        return mapOf(name to Base64.encode(hasher.finish()))
    }
    /** Verifies the first supported modeled algorithm the response carries, or returns null. */
    fun verifier(plan: Plan,headers: Map<String,List<String>>,config: ClientConfig): Verifier? {
        if(!plan.validate)return null
        for(item in (plan.descriptor?.get("responseAlgorithms") as? JsonArray).orEmpty()) {
            val algorithm=item.jsonPrimitive.content;if(algorithm !in supported)continue
            val supplied=headers.entries.firstOrNull { it.key.equals(header(algorithm),true) }?.value?.firstOrNull() ?: continue
            val expected=try {Base64.decode(supplied)}catch(_:IllegalArgumentException){throw ChecksumMismatch(algorithm)}
            return Verifier(plan.operation,algorithm,expected,config)
        }
        return null
    }
    fun validate(plan: Plan,headers: Map<String,List<String>>,body: ByteArray,config: ClientConfig) {
        verifier(plan,headers,config)?.run { update(body,0,body.size);finish() }
    }
    class Verifier(private val operation: String,val algorithm: String,private val expected: ByteArray,private val config: ClientConfig) {
        private val hasher=Hasher(algorithm)
        fun update(bytes: ByteArray,offset: Int,length: Int)=hasher.update(bytes,offset,length)
        /** Compares in constant time, then reports the single verified algorithm. */
        fun finish() {
            val computed=hasher.finish()
            var mismatch=expected.size xor computed.size
            for(index in computed.indices)mismatch=mismatch or ((expected.getOrElse(index){0}.toInt() xor computed[index].toInt()) and 255)
            if(mismatch!=0)throw ChecksumMismatch(algorithm)
            config.checksumValidated?.invoke(operation,algorithm)
        }
    }
    private fun header(algorithm: String)="x-amz-checksum-"+algorithm.lowercase()
    fun digest(algorithm: String,bytes: ByteArray): ByteArray = Hasher(algorithm).apply { update(bytes,0,bytes.size) }.finish()
    /** Incremental digest state, so streamed bodies are checksummed without buffering. */
    class Hasher(algorithm: String) {
        private val state: Pair<(ByteArray,Int,Int)->Unit,()->ByteArray> = when(algorithm) {
            "MD5" -> org.kotlincrypto.hash.md.MD5().let { d -> Pair({ b: ByteArray,o: Int,l: Int -> d.update(b,o,l) },{ d.digest() }) }
            "SHA1" -> org.kotlincrypto.hash.sha1.SHA1().let { d -> Pair({ b: ByteArray,o: Int,l: Int -> d.update(b,o,l) },{ d.digest() }) }
            "SHA256" -> org.kotlincrypto.hash.sha2.SHA256().let { d -> Pair({ b: ByteArray,o: Int,l: Int -> d.update(b,o,l) },{ d.digest() }) }
            "SHA512" -> org.kotlincrypto.hash.sha2.SHA512().let { d -> Pair({ b: ByteArray,o: Int,l: Int -> d.update(b,o,l) },{ d.digest() }) }
            "CRC32" -> crc(crc32,false)
            "CRC32C" -> crc(crc32c,false)
            "CRC64NVME" -> crc(crc64nvme,true)
            else -> throw IllegalArgumentException("Unsupported checksum algorithm: $algorithm")
        }
        fun update(bytes: ByteArray,offset: Int,length: Int)=state.first(bytes,offset,length)
        fun finish(): ByteArray=state.second()
    }
    private fun crc(table: LongArray,wide: Boolean): Pair<(ByteArray,Int,Int)->Unit,()->ByteArray> {
        var value=if(wide)-1L else 0xffffffffL
        return Pair({ bytes: ByteArray,offset: Int,length: Int -> for(index in offset until offset+length) value=table[((value xor bytes[index].toLong()) and 255).toInt()] xor (value ushr 8) },
            { val size=if(wide)8 else 4;val result=value.inv();ByteArray(size){ index -> (result ushr ((size-1-index)*8)).toByte() } })
    }
    // Reflected polynomials; one table lookup per byte keeps large streamed bodies fast.
    private fun table(polynomial: Long)=LongArray(256) { index -> var value=index.toLong();repeat(8){ value=if(value and 1L!=0L)(value ushr 1) xor polynomial else value ushr 1 };value }
    private val crc32 by lazy { table(0xedb88320L) }
    private val crc32c by lazy { table(0x82f63b78L) }
    private val crc64nvme by lazy { table(0x9a6c9329ac4bc9b5uL.toLong()) }
}
