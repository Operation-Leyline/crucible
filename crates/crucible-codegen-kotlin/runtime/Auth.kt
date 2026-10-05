import io.ktor.http.Url
import kotlin.io.encoding.Base64
import kotlin.time.Instant
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.booleanOrNull

internal suspend fun applyAuth(config: ClientConfig, schema: JsonObject, op: JsonObject, request: Request): Boolean {
    val options = (op["auth"] as? JsonArray) ?: JsonArray(if (op.flag("bearer")) listOf(JsonPrimitive("smithy.api#httpBearerAuth")) else emptyList())
    for (option in options) {
        val id = option.jsonPrimitive.content
        var scheme = (schema["authSchemes"] as? JsonObject)?.get(id) as? JsonObject
        val kind = scheme?.text("kind") ?: if (id == "smithy.api#httpBearerAuth") "bearer" else error("Unknown authentication scheme $id")
        val endpointAuth=((op["resolvedEndpoint"] as? JsonObject)?.get("properties") as? JsonObject)?.get("authSchemes") as? JsonArray
        if(endpointAuth!=null) {
            val selected=endpointAuth.firstOrNull { it.jsonObject.text("name")?.equals(kind,ignoreCase=true)==true }?.jsonObject ?: continue
            scheme=JsonObject((scheme ?: JsonObject(mapOf("kind" to JsonPrimitive(kind))))+selected.filterKeys { it!="name" })
        }
        val identity: Any = when (kind) {
            "bearer" -> config.token?.invoke()
            "basic" -> config.basicAuth?.invoke()
            "apiKey" -> config.apiKey?.invoke()
            "sigV4" -> config.credentials?.invoke()
            else -> error("Unsupported authentication scheme $kind")
        } ?: continue
        fun set(name: String, value: String) { request.headers.keys.filter { it.equals(name, ignoreCase=true) }.forEach { request.headers.remove(it) }; request.headers[name] = value }
        when (kind) {
            "bearer" -> set("authorization", "Bearer $identity")
            "basic" -> { val basic=identity as BasicCredentials; set("authorization", "Basic " + Base64.encode("${basic.username}:${basic.password}".encodeToByteArray())) }
            "apiKey" -> {
                val key = identity as String; require(key.isNotEmpty()) { "Empty API key" }
                if (scheme!!.text("location") == "query") {
                    val name = scheme.text("name")!!
                    val parts = request.uri.encodedQuery.split('&').filter { it.isNotEmpty() && authDecode(it.substringBefore('=')) != name }
                    request.uri = Url(request.uri.toString().substringBefore('?') + "?" + (parts + (authEscape(name) + "=" + authEscape(key))).joinToString("&"))
                } else set(scheme.text("name")!!, (scheme.text("scheme")?.let { "$it " } ?: "") + key)
            }
            "sigV4" -> signV4(request, identity as AwsCredentials, scheme!!.text("signingName")!!, requireNotNull(scheme.text("signingRegion") ?: config.region) { "SigV4 requires region" }, config.clock(), op.flag("unsignedPayload"), (scheme["disableDoubleEncoding"] as? JsonPrimitive)?.booleanOrNull, (scheme["disableNormalizePath"] as? JsonPrimitive)?.booleanOrNull)
        }
        return true
    }
    require(options.none { it.jsonPrimitive.content != "smithy.api#httpBearerAuth" } || op.flag("optionalAuth")) { "No credentials available for operation authentication schemes" }
    return false
}

private fun authEscape(value: String): String = value.encodeToByteArray().joinToString("") { byte ->
    val number = byte.toInt() and 255
    if (number in 65..90 || number in 97..122 || number in 48..57 || number.toChar() in "-._~") number.toChar().toString()
    else "%" + number.toString(16).uppercase().padStart(2, '0')
}
private fun authDecode(value: String): String {
    val bytes = mutableListOf<Byte>(); var index=0
    while (index < value.length) {
        if (value[index] == '%') { require(index+2 < value.length); bytes.add(value.substring(index+1,index+3).toInt(16).toByte()); index+=3 }
        else { bytes.addAll(value[index++].toString().encodeToByteArray().toList()) }
    }
    return bytes.toByteArray().decodeToString()
}
private fun digestBytes(bytes: ByteArray): ByteArray = sha256(bytes).chunked(2).map { it.toInt(16).toByte() }.toByteArray()
private fun hmac(key: ByteArray, value: String): ByteArray {
    val padded = (if (key.size > 64) digestBytes(key) else key).copyOf(64)
    val inner = ByteArray(64) { (padded[it].toInt() xor 0x36).toByte() }
    val outer = ByteArray(64) { (padded[it].toInt() xor 0x5c).toByte() }
    return digestBytes(outer + digestBytes(inner + value.encodeToByteArray()))
}
internal fun signV4(request: Request, credentials: AwsCredentials, service: String, region: String, now: Instant, unsigned: Boolean, disableDoubleEncoding: Boolean? = null, disableNormalizePath: Boolean? = null) {
    require(service.isNotEmpty() && region.isNotEmpty() && (service+region).none { it in "/\r\n" }) { "Invalid SigV4 service or region" }
    require(credentials.expiresAt == null || credentials.expiresAt > now) { "AWS credentials expired" }
    val timestamp = now.toString().substringBefore('.').removeSuffix("Z").replace("-", "").replace(":", "") + "Z"
    val date = timestamp.take(8)
    val streamDigest=request.stream?.sha256 ?: request.headers.entries.firstOrNull {it.key.equals("x-amz-content-sha256",true)}?.value
    request.headers.keys.filter { it.equals("authorization",true) || it.equals("x-amz-security-token",true) || it.equals("x-amz-date",true) || it.equals("x-amz-content-sha256",true) }.forEach { request.headers.remove(it) }
    request.headers["x-amz-date"] = timestamp
    credentials.sessionToken?.let { request.headers["x-amz-security-token"] = it }
    val digest = if (unsigned) "UNSIGNED-PAYLOAD" else if(request.stream!=null) requireNotNull(streamDigest) { "Streaming SigV4 requests require precomputed SHA-256" }.also {require(Regex("[a-fA-F0-9]{64}").matches(it)) { "Invalid stream SHA-256" }} else sha256(request.body ?: byteArrayOf())
    request.headers["x-amz-content-sha256"] = digest
    var path = request.uri.encodedPath.ifEmpty { "/" }
    if (!(disableNormalizePath ?: (service == "s3"))) {
        val segments = mutableListOf<String>()
        path.split('/').forEach { if (it == "..") { if (segments.isNotEmpty()) segments.removeAt(segments.lastIndex) } else if (it.isNotEmpty() && it != ".") segments.add(it) }
        var normalized = "/" + segments.joinToString("/")
        if (path.endsWith('/') && !normalized.endsWith('/')) normalized += "/"
        path = normalized
    }
    if (!(disableDoubleEncoding ?: (service == "s3"))) path=authEscape(path).replace("%2F", "/")
    val query = request.uri.encodedQuery.split('&').filter { it.isNotEmpty() }.map {
        authEscape(authDecode(it.substringBefore('='))) to authEscape(authDecode(if ('=' in it) it.substringAfter('=') else ""))
    }.sortedWith(compareBy<Pair<String,String>> { it.first }.thenBy { it.second }).joinToString("&") { "${it.first}=${it.second}" }
    val excluded = setOf("authorization","connection","expect","transfer-encoding","user-agent","x-amzn-trace-id","accept-encoding","content-length")
    val headers = mutableMapOf<String,String>()
    request.headers.forEach { (key,value) ->
        val name=key.lowercase()
        if (name !in excluded) { val normalized=value.trim().replace(Regex("\\s+"), " "); headers[name]=headers[name]?.let { "$it,$normalized" } ?: normalized }
    }
    val port = request.uri.port
    headers["host"] = request.uri.host.lowercase() + if (port == request.uri.protocol.defaultPort) "" else ":$port"
    val names = headers.keys.sorted()
    val signed = names.joinToString(";")
    val canonicalHeaders = names.joinToString("") { "$it:${headers[it]}\n" }
    val canonical = listOf(request.method,path,query,canonicalHeaders,signed,digest).joinToString("\n")
    val scope = "$date/$region/$service/aws4_request"
    val toSign = "AWS4-HMAC-SHA256\n$timestamp\n$scope\n" + sha256(canonical.encodeToByteArray())
    var key = ("AWS4" + credentials.secretAccessKey).encodeToByteArray()
    for (component in listOf(date,region,service,"aws4_request")) key = hmac(key, component)
    val signature = hmac(key,toSign).joinToString("") { (it.toInt() and 255).toString(16).padStart(2,'0') }
    request.headers["authorization"] = "AWS4-HMAC-SHA256 Credential=${credentials.accessKeyId}/$scope, SignedHeaders=$signed, Signature=$signature"
}
