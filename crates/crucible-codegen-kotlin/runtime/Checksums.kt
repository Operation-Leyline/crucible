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
    fun request(plan: Plan,headers: MutableMap<String,String>,body: ByteArray?) {
        val algorithm=plan.algorithm ?: return
        val name=if(plan.legacy)"content-md5" else header(algorithm)
        if(headers.keys.any { it.equals(name,true) })return
        headers[name]=Base64.encode(digest(algorithm,body ?: byteArrayOf()))
    }
    fun validate(plan: Plan,headers: Map<String,List<String>>,body: ByteArray,config: ClientConfig) {
        if(!plan.validate)return
        for(item in (plan.descriptor?.get("responseAlgorithms") as? JsonArray).orEmpty()) {
            val algorithm=item.jsonPrimitive.content;if(algorithm !in supported)continue
            val supplied=headers.entries.firstOrNull { it.key.equals(header(algorithm),true) }?.value?.firstOrNull() ?: continue
            val expected=try {Base64.decode(supplied)}catch(_:IllegalArgumentException){throw ChecksumMismatch(algorithm)}
            val computed=digest(algorithm,body)
            var mismatch=expected.size xor computed.size
            for(index in computed.indices)mismatch=mismatch or ((expected.getOrElse(index){0}.toInt() xor computed[index].toInt()) and 255)
            if(mismatch!=0)throw ChecksumMismatch(algorithm)
            config.checksumValidated?.invoke(plan.operation,algorithm)
            return
        }
    }
    private fun header(algorithm: String)="x-amz-checksum-"+algorithm.lowercase()
    fun digest(algorithm: String,bytes: ByteArray): ByteArray = when(algorithm) {
        "MD5" -> org.kotlincrypto.hash.md.MD5().digest(bytes)
        "SHA1" -> org.kotlincrypto.hash.sha1.SHA1().digest(bytes)
        "SHA256" -> org.kotlincrypto.hash.sha2.SHA256().digest(bytes)
        "SHA512" -> org.kotlincrypto.hash.sha2.SHA512().digest(bytes)
        "CRC32","CRC32C","CRC64NVME" -> {
            val wide=algorithm=="CRC64NVME"
            val polynomial=when(algorithm){"CRC32"->0xedb88320uL;"CRC32C"->0x82f63b78uL;else->0x9a6c9329ac4bc9b5uL}
            var value=if(wide)ULong.MAX_VALUE else 0xffffffffuL
            for(byte in bytes) { value=value xor byte.toUByte().toULong();repeat(8){value=(value shr 1) xor if(value and 1uL != 0uL)polynomial else 0uL} }
            value=value.inv();val size=if(wide)8 else 4
            ByteArray(size){index -> (value shr ((size-1-index)*8)).toByte()}
        }
        else -> throw IllegalArgumentException("Unsupported checksum algorithm: $algorithm")
    }
}
