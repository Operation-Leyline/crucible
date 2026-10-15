import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.JsonUnquotedLiteral
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlin.io.encoding.Base64
import kotlin.time.Instant

internal fun wireObject(block: (MutableMap<String, JsonElement>) -> Unit): JsonObject =
    linkedMapOf<String, JsonElement>().let { block(it); JsonObject(it) }
internal val JsonElement.asJsonObject: JsonObject get() = jsonObject
internal fun JsonObject.text(name: String): String? = get(name)?.takeUnless { it == JsonNull }?.jsonPrimitive?.content
internal fun JsonObject.flag(name: String): Boolean = get(name)?.jsonPrimitive?.booleanOrNull == true
internal fun JsonObject.obj(name: String): JsonObject = getValue(name).jsonObject
internal fun JsonObject.array(name: String): JsonArray = getValue(name).jsonArray
private val jsonNumber = Regex("-?(?:0|[1-9][0-9]*)(?:\\.[0-9]+)?(?:[eE][+-]?[0-9]+)?")

internal fun decimalText(value: com.ionspin.kotlin.bignum.decimal.BigDecimal): String {
    val text = value.toString()
    val point = text.indexOf('.')
    // bignum 0.3.10 leaves a dangling point for negative powers of ten.
    return if (point >= 0 && (point == text.lastIndex || text[point + 1] == 'E' || text[point + 1] == 'e'))
        text.removeRange(point, point + 1) else text
}

/** Compare numbers exactly and objects independent of member order. */
private fun collectionKey(value: JsonElement): String = when (value) {
    is JsonArray -> value.joinToString(",", "[", "]") { collectionKey(it) }
    is JsonObject -> value.keys.sorted().joinToString(",", "{", "}") { JsonPrimitive(it).toString() + ":" + collectionKey(value.getValue(it)) }
    else -> if (value == JsonNull || value.jsonPrimitive.isString || value.jsonPrimitive.booleanOrNull != null) value.toString() else {
        val raw=value.jsonPrimitive.content.lowercase(); val mantissa=raw.substringBefore('e'); val fraction=mantissa.substringAfter('.', "")
        val digits=mantissa.removePrefix("-").replace(".", "").trimStart('0')
        if (digits.isEmpty()) "0" else {
            val coefficient=digits.trimEnd('0')
            val exponent=com.ionspin.kotlin.bignum.integer.BigInteger.parseString(raw.substringAfter('e',"0").removePrefix("+")) +
                com.ionspin.kotlin.bignum.integer.BigInteger.parseString((digits.length-coefficient.length-fraction.length).toString())
            (if (raw.startsWith('-')) "-" else "") + coefficient + "e" + exponent.toString()
        }
    }
}

internal class JsonCodec(val schema: JsonObject) {
    fun parse(text: String): JsonElement = checkedJson(Json.parseToJsonElement(text))
    private fun checkedJson(value: JsonElement): JsonElement {
        when (value) {
            is JsonObject -> value.values.forEach { checkedJson(it) }
            is JsonArray -> value.forEach { checkedJson(it) }
            is JsonPrimitive -> if (!value.isString && value != JsonNull && value.booleanOrNull == null &&
                !jsonNumber.matches(value.content))
                throw kotlinx.serialization.SerializationException("Invalid JSON numeric token")
        }
        return value
    }
    fun stringify(value: JsonElement): String = value.toString()
    fun shape(target: String): JsonObject = schema.obj("shapes").obj(target)
    fun wireName(target: String, name: String): String = shape(target).obj("members").obj(name).text("jsonName") ?: name
    fun inputValues(target: String?, input: Any?): MutableMap<String, JsonElement> {
        if (target == null || input == null) return mutableMapOf()
        val wire = encode(target, input).jsonObject
        return shape(target).obj("members").keys.mapNotNull { name -> wire[wireName(target, name)]?.let { name to it } }.toMap().toMutableMap()
    }
    fun queryValue(target: String?, value: JsonElement, format: String? = null): JsonElement {
        if (target==null || value==JsonNull) return value
        val shape=shape(target)
        return when (shape.text("kind")) {
            "timestamp" -> JsonUnquotedLiteral(epochText(decode(target,value,format) as Instant))
            "structure", "union" -> JsonObject(value.jsonObject.map { (wire,item) ->
                val member=shape.obj("members").keys.firstOrNull { wireName(target,it)==wire }
                if (member==null) wire to item else member to shape.obj("members").obj(member).let { queryValue(it.text("target"),item,it.text("format")) }
            }.toMap())
            "list" -> JsonArray(value.jsonArray.map { shape.obj("member").let { member -> queryValue(member.text("target"),it,member.text("format")) } })
            "map" -> JsonObject(value.jsonObject.mapValues { (_,item) -> shape.obj("value").let { queryValue(it.text("target"),item,it.text("format")) } })
            else -> value
        }
    }
    fun string(value: JsonElement): String {
        require(value is JsonPrimitive && value.isString) { "Expected string" }
        return value.content
    }
    private fun number(value: JsonElement): String {
        require(value is JsonPrimitive && !value.isString && value != JsonNull && value.booleanOrNull == null) { "Expected number" }
        return value.content
    }
    private fun integer(value: JsonElement, min: Long, max: Long): Long = WireDecimal(number(value)).longExact().also {
        require(it in min..max) { "Integer outside modeled range" }
    }
    fun intValue(value: JsonElement): Int = integer(value, Int.MIN_VALUE.toLong(), Int.MAX_VALUE.toLong()).toInt()
    fun encode(target: String?, value: Any?, format: String? = null): JsonElement {
        if (target == null) return JsonObject(emptyMap())
        val shape = shape(target)
        if (shape.text("kind") == "unit") return JsonObject(emptyMap())
        if (value == null) return JsonNull
        return when (shape.text("kind")) {
            "timestamp" -> writeTime(value as Instant, format ?: shape.text("format"))
            "document" -> checkedJson(value as JsonElement)
            "blob" -> if(shape.flag("streaming")) JsonPrimitive("") else JsonPrimitive(Base64.encode(value as ByteArray))
            "bigInteger" -> JsonUnquotedLiteral(value.toString())
            "bigDecimal" -> JsonUnquotedLiteral(decimalText(value as com.ionspin.kotlin.bignum.decimal.BigDecimal))
            "list" -> {
                val member = shape.obj("member")
                val items = (value as Iterable<*>).map {
                    require(shape.flag("sparse") || it != null && it != JsonNull) { "Null element in non-sparse list" }
                    encode(member.text("target"), it, member.text("format"))
                }
                require(!shape.flag("set") || items.map { collectionKey(it) }.toSet().size == items.size) { "Duplicate set member" }
                JsonArray(items)
            }
            "map" -> {
                val member = shape.obj("value")
                JsonObject((value as Map<*, *>).entries.associate { (key, item) ->
                    require(shape.flag("sparse") || item != null && item != JsonNull) { "Null value in non-sparse map" }
                    key.toString() to encode(member.text("target"), item, member.text("format"))
                })
            }
            "structure", "union", "enum", "intEnum" -> ModelCodecs.encode(target, value, this)
            "boolean" -> JsonPrimitive(value as Boolean)
            "string" -> JsonPrimitive(value as String)
            "float" -> (value as Float).let { if (it.isFinite()) JsonPrimitive(it) else throw IllegalArgumentException("Float overflow") }
            "double" -> (value as Double).let { if (it.isFinite()) JsonPrimitive(it) else throw IllegalArgumentException("Double overflow") }
            else -> JsonPrimitive(value as Number)
        }
    }
    fun decode(target: String?, value: JsonElement, format: String? = null): Any? {
        if (target == null) return kotlin.Unit
        val shape = shape(target)
        if (value == JsonNull) return if (shape.text("kind") == "document") JsonNull else null
        return when (shape.text("kind")) {
            "unit" -> { require(value is JsonObject && value.isEmpty()) { "Unit requires empty object" }; kotlin.Unit }
            "string" -> string(value)
            "boolean" -> value.jsonPrimitive.let { require(!it.isString); requireNotNull(it.booleanOrNull) { "Expected boolean" } }
            "byte" -> integer(value, Byte.MIN_VALUE.toLong(), Byte.MAX_VALUE.toLong()).toByte()
            "short" -> integer(value, Short.MIN_VALUE.toLong(), Short.MAX_VALUE.toLong()).toShort()
            "integer" -> integer(value, Int.MIN_VALUE.toLong(), Int.MAX_VALUE.toLong()).toInt()
            "long" -> integer(value, Long.MIN_VALUE, Long.MAX_VALUE)
            "bigInteger" -> com.ionspin.kotlin.bignum.integer.BigInteger.parseString(WireDecimal(number(value)).integerText())
            "bigDecimal" -> com.ionspin.kotlin.bignum.decimal.BigDecimal.parseString(number(value))
            "float" -> number(value).toFloat().also { require(it.isFinite()) { "Float overflow" } }
            "double" -> number(value).toDouble().also { require(it.isFinite()) { "Double overflow" } }
            "timestamp" -> readTime(value, format ?: shape.text("format"))
            "document" -> checkedJson(value)
            "blob" -> if(shape.flag("streaming")) ByteStream.fromBytes(byteArrayOf()) else Base64.decode(string(value))
            else -> ModelCodecs.decode(target, value, this)
        }
    }
    fun readList(target: String, value: JsonElement): List<Any?> {
        val shape = shape(target); val member = shape.obj("member")
        val values = value.jsonArray.map { item ->
            require(shape.flag("sparse") || item != JsonNull) { "Null element in non-sparse list" }
            decode(member.text("target"), item, member.text("format")).also {
            require(shape.flag("sparse") || it != null) { "Null element in non-sparse list" }
        } }
        require(!shape.flag("set") || values.toSet().size == values.size && value.jsonArray.map { collectionKey(it) }.toSet().size == value.jsonArray.size) { "Duplicate set member" }
        return values
    }
    fun readMap(target: String, value: JsonElement): Map<Any?, Any?> {
        val shape = shape(target); val key = shape.obj("key"); val member = shape.obj("value")
        return value.jsonObject.entries.associate { (name, item) ->
            require(shape.flag("sparse") || item != JsonNull) { "Null value in non-sparse map" }
            decode(key.text("target"), JsonPrimitive(name)) to decode(member.text("target"), item, member.text("format")).also { require(shape.flag("sparse") || it != null) { "Null value in non-sparse map" } }
        }
    }
    fun put(result: MutableMap<String, JsonElement>, target: String, name: String, input: Any?) {
        val value = if (input is OptionalValue<*>) { if (!input.present) return; input.value } else input
        val member = shape(target).obj("members").obj(name)
        result[wireName(target, name)] = encode(member.text("target"), value, member.text("format"))
    }
    fun readMember(target: String, body: JsonObject, name: String, optional: Boolean): Any? {
        val member = shape(target).obj("members").obj(name)
        val actual = body[wireName(target, name)]
        val value = actual ?: member["default"]?.takeUnless { it == JsonNull }
        if (value == null) return if (optional) OptionalValue.unset<Any>() else null
        val decoded = decode(member.text("target"), value, member.text("format"))
        return if (optional) OptionalValue.of(decoded) else decoded
    }
    fun union(target: String, value: JsonElement): Pair<String, JsonElement> {
        require(value is JsonObject) { "Union requires object" }
        require(value.size == 1) { "Union requires exactly one nonnull member" }
        return value.entries.first().let { it.key to it.value }
    }
    fun readUnionMember(target: String, name: String, value: JsonElement): Any? {
        val member = shape(target).obj("members").obj(name)
        return decode(member.text("target"), value, member.text("format"))
    }
    fun unknownUnion(target: String, name: String, value: JsonElement): JsonObject {
        require(shape(target).obj("members").keys.none { wireName(target, it) == name }) { "Unknown union name is declared" }
        return JsonObject(mapOf(name to value))
    }
    private fun writeTime(value: Instant, format: String?): JsonElement = when (format) {
        "epoch-seconds" -> JsonUnquotedLiteral(epochText(value))
        "http-date" -> JsonPrimitive(httpDate(value))
        else -> JsonPrimitive(value.toString())
    }
    private fun readTime(value: JsonElement, format: String?): Instant = when (format) {
        "epoch-seconds" -> WireDecimal(number(value)).instant()
        "http-date" -> parseHttpDate(string(value))
        else -> Instant.parse(string(value))
    }
    fun boundaryTime(member: JsonObject, value: JsonElement, format: String?, encode: Boolean): JsonElement {
        if (format==null || value==JsonNull) return value
        val target=shape(member.text("target")!!)
        return when(target.text("kind")) {
            "timestamp" -> if(encode) writeTime(readTime(value,member.text("format") ?: target.text("format")),format) else writeTime(readTime(value,format),member.text("format") ?: target.text("format"))
            "list" -> JsonArray(value.jsonArray.map { boundaryTime(target.obj("member"),it,format,encode) })
            "map" -> JsonObject(value.jsonObject.mapValues { boundaryTime(target.obj("value"),it.value,format,encode) })
            else -> value
        }
    }
    fun headerText(member: JsonObject, value: JsonElement, format: String?): String {
        val target=shape(member.text("target")!!)
        return if (format=="http-date" && target.text("kind")=="list" && shape(target.obj("member").text("target")!!).text("kind")=="timestamp") value.jsonArray.joinToString(", ") { it.jsonPrimitive.content } else bound(value)
    }
    fun bound(value: JsonElement): String = if (value is JsonArray) value.joinToString(", ") {
        val text = it.jsonPrimitive.content
        if (text.isEmpty() || text.any { c -> c == ',' || c == '"' || c == '\\' } || text.trim() != text) JsonPrimitive(text).toString() else text
    } else value.jsonPrimitive.content
    fun header(member: JsonObject, value: String, format: String? = null): JsonElement {
        val shape = shape(member.text("target")!!)
        return when (shape.text("kind")) {
            "list" -> {
                if (value.isBlank()) return JsonArray(emptyList())
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
                val child=shape.obj("member")
                if ((format ?: child.text("format"))=="http-date" && this.shape(child.text("target")!!).text("kind")=="timestamp") {
                    var index=0; val timestamps=mutableListOf<JsonElement>()
                    while(index<parts.size) { var part=parts[index++]; if(Regex("[A-Za-z]{3}").matches(part)) { require(index<parts.size) { "Incomplete HTTP date" }; part+=", "+parts[index++] }; timestamps.add(header(child,if(part.startsWith('"')) string(parse(part)) else part,format)) }
                    JsonArray(timestamps)
                } else JsonArray(parts.map { header(child, if (it.startsWith('"')) string(parse(it)) else it,format) })
            }
            "float", "double" -> parse(value)
            "byte", "short", "integer", "long", "intEnum", "bigInteger", "bigDecimal", "boolean" -> parse(value)
            "timestamp" -> if ((format ?: member.text("format")) == "epoch-seconds") parse(value) else JsonPrimitive(value)
            else -> JsonPrimitive(value)
        }
    }
}
