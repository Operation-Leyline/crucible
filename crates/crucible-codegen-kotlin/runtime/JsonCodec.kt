import com.google.gson.GsonBuilder
import com.google.gson.JsonArray
import com.google.gson.JsonElement
import com.google.gson.JsonNull
import com.google.gson.JsonObject
import com.google.gson.JsonPrimitive
import com.google.gson.Strictness
import java.math.BigDecimal
import java.time.Instant
import java.time.ZoneOffset
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.util.Locale
import java.util.Base64

internal class JsonCodec(val schema: JsonObject) {
    private val json = GsonBuilder().disableHtmlEscaping().serializeNulls().setStrictness(Strictness.STRICT).create()
    fun parse(text: String): JsonElement = json.fromJson(text, JsonElement::class.java) ?: JsonNull.INSTANCE
    fun stringify(value: JsonElement): String = json.toJson(value)
    fun shape(target: String): JsonObject = schema.getAsJsonObject("shapes").getAsJsonObject(target)
    fun string(value: JsonElement): String {
        require(value.isJsonPrimitive && value.asJsonPrimitive.isString) { "Expected string" }
        return value.asString
    }
    private fun number(value: JsonElement): BigDecimal {
        require(value.isJsonPrimitive && value.asJsonPrimitive.isNumber) { "Expected number" }
        return value.asBigDecimal
    }
    fun encode(target: String?, value: Any?, format: String? = null): JsonElement {
        if (target == null) return JsonObject()
        val shape = shape(target)
        if (shape.text("kind") == "unit") return JsonObject()
        if (value == null) return JsonNull.INSTANCE
        return when (shape.text("kind")) {
            "timestamp" -> writeTime(value as Instant, format ?: shape.text("format"))
            "document" -> (value as JsonElement).deepCopy()
            "blob" -> JsonPrimitive(Base64.getEncoder().encodeToString(value as ByteArray))
            "list" -> JsonArray().also { result ->
                val member = shape.getAsJsonObject("member")
                (value as Iterable<*>).forEach {
                    require(it != null || shape(member.text("target")!!).text("kind") == "document") { "Null element in non-sparse list" }
                    result.add(encode(member.text("target"), it, member.text("format")))
                }
            }
            "map" -> JsonObject().also { result ->
                val member = shape.getAsJsonObject("value")
                (value as Map<*, *>).forEach { (key, item) ->
                    require(item != null || shape(member.text("target")!!).text("kind") == "document") { "Null value in non-sparse map" }
                    result.add(key.toString(), encode(member.text("target"), item, member.text("format")))
                }
            }
            "structure", "union", "enum" -> ModelCodecs.encode(target, value, this)
            else -> json.toJsonTree(value)
        }
    }
    fun decode(target: String?, value: JsonElement, format: String? = null): Any? {
        if (target == null) return kotlin.Unit
        val shape = shape(target)
        if (value.isJsonNull) return if (shape.text("kind") == "document") JsonNull.INSTANCE else null
        return when (shape.text("kind")) {
            "unit" -> { require(value.isJsonObject && value.asJsonObject.size() == 0) { "Unit requires empty object" }; kotlin.Unit }
            "string" -> string(value)
            "boolean" -> { require(value.isJsonPrimitive && value.asJsonPrimitive.isBoolean); value.asBoolean }
            "byte" -> number(value).byteValueExact()
            "short" -> number(value).shortValueExact()
            "integer" -> number(value).intValueExact()
            "long" -> number(value).longValueExact()
            "float" -> number(value).toFloat().also { require(it.isFinite()) { "Float overflow" } }
            "double" -> number(value).toDouble().also { require(it.isFinite()) { "Double overflow" } }
            "timestamp" -> readTime(value, format ?: shape.text("format"))
            "document" -> value.deepCopy()
            "blob" -> Base64.getDecoder().decode(string(value))
            else -> ModelCodecs.decode(target, value, this)
        }
    }
    fun readList(target: String, value: JsonElement): List<Any?> {
        val member = shape(target).getAsJsonObject("member")
        return value.asJsonArray.map {
            requireNotNull(decode(member.text("target"), it, member.text("format"))) { "Null element in non-sparse list" }
        }
    }
    fun readMap(target: String, value: JsonElement): Map<Any?, Any?> {
        val shape = shape(target); val key = shape.getAsJsonObject("key"); val member = shape.getAsJsonObject("value")
        return value.asJsonObject.entrySet().associate { (name, item) ->
            decode(key.text("target"), JsonPrimitive(name)) to
                requireNotNull(decode(member.text("target"), item, member.text("format"))) { "Null value in non-sparse map" }
        }
    }
    fun put(result: JsonObject, target: String, name: String, input: Any?) {
        val value = if (input is OptionalValue<*>) { if (!input.present) return; input.value } else input
        val member = shape(target).getAsJsonObject("members").getAsJsonObject(name)
        result.add(name, encode(member.text("target"), value, member.text("format")))
    }
    fun readMember(target: String, body: JsonObject, name: String, optional: Boolean): Any? {
        val member = shape(target).getAsJsonObject("members").getAsJsonObject(name)
        val value = body.get(name) ?: member.get("default").takeUnless { it.isJsonNull }
        if (value == null) return if (optional) OptionalValue.unset<Any>() else null
        val decoded = decode(member.text("target"), value, member.text("format"))
        return if (optional) OptionalValue.of(decoded) else decoded
    }
    fun union(value: JsonElement): Pair<String, JsonElement> {
        require(value.isJsonObject && value.asJsonObject.size() == 1) { "Union requires exactly one member" }
        return value.asJsonObject.entrySet().first().let { it.key to it.value }
    }
    fun readUnionMember(target: String, name: String, value: JsonElement): Any? {
        val member = shape(target).getAsJsonObject("members").getAsJsonObject(name)
        return decode(member.text("target"), value, member.text("format"))
    }
    fun unknownUnion(target: String, name: String, value: JsonElement): JsonObject {
        require(!shape(target).getAsJsonObject("members").has(name)) { "Unknown union name is declared" }
        return JsonObject().also { it.add(name, value.deepCopy()) }
    }
    private fun writeTime(value: Instant, format: String?): JsonElement = when (format) {
        "epoch-seconds" -> JsonPrimitive(BigDecimal.valueOf(value.epochSecond).add(BigDecimal.valueOf(value.nano.toLong(), 9)))
        "http-date" -> JsonPrimitive(DateTimeFormatter.ofPattern("EEE, dd MMM uuuu HH:mm:ss 'GMT'", Locale.US).format(value.atZone(ZoneOffset.UTC)))
        else -> JsonPrimitive(value.toString())
    }
    private fun readTime(value: JsonElement, format: String?): Instant = when (format) {
        "epoch-seconds" -> number(value).let { Instant.ofEpochSecond(it.setScale(0, java.math.RoundingMode.DOWN).longValueExact(), it.remainder(BigDecimal.ONE).movePointRight(9).longValueExact()) }
        "http-date" -> ZonedDateTime.parse(string(value), DateTimeFormatter.RFC_1123_DATE_TIME).toInstant()
        else -> Instant.parse(string(value))
    }
    fun bound(value: JsonElement): String = if (value.isJsonArray) value.asJsonArray.joinToString(", ") {
        val text = it.asString
        if (text.isEmpty() || text.any { c -> c == ',' || c == '"' || c == '\\' } || text.trim() != text) json.toJson(text) else text
    } else value.asString
    fun header(member: JsonObject, value: String): JsonElement {
        val shape = shape(member.text("target")!!)
        return when (shape.text("kind")) {
            "list" -> {
                val result = JsonArray()
                if (value.isBlank()) return result
                val parts = mutableListOf<String>(); var start = 0; var quoted = false; var escaped = false
                value.forEachIndexed { index, c ->
                    when {
                        escaped -> escaped = false
                        c == '\\' && quoted -> escaped = true
                        c == '"' -> quoted = !quoted
                        c == ',' && !quoted -> { parts.add(value.substring(start, index).trim()); start = index + 1 }
                    }
                }
                require(!quoted) { "Unterminated quoted header" }
                parts.add(value.substring(start).trim())
                parts.forEach { result.add(header(shape.getAsJsonObject("member"), if (it.startsWith('"')) string(parse(it)) else it)) }
                result
            }
            "byte", "short", "integer", "long", "float", "double", "boolean" -> parse(value)
            "timestamp" -> if (member.text("format") == "epoch-seconds") parse(value) else JsonPrimitive(value)
            else -> JsonPrimitive(value)
        }
    }
}

internal fun JsonObject.text(name: String): String? = get(name)?.takeUnless { it.isJsonNull }?.asString
internal fun JsonObject.flag(name: String): Boolean = get(name)?.takeUnless { it.isJsonNull }?.asBoolean == true
