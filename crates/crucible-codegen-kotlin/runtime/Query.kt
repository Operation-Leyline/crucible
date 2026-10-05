import com.ionspin.kotlin.bignum.decimal.BigDecimal
import com.ionspin.kotlin.bignum.decimal.DecimalMode
import com.ionspin.kotlin.bignum.decimal.RoundingMode
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.JsonUnquotedLiteral
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/** Executes the shared compiled JMESPath tree; target runtimes do not parse expressions. */
internal object Query {
    private fun truth(value: JsonElement): Boolean = when (value) {
        JsonNull -> false
        is JsonArray -> value.isNotEmpty()
        is JsonObject -> value.isNotEmpty()
        is JsonPrimitive -> if (value.isString) value.content.isNotEmpty() else value.booleanOrNull != false
    }
    private fun isNumber(value: JsonElement): Boolean = value is JsonPrimitive && value != JsonNull && !value.isString && value.booleanOrNull == null
    private fun number(value: JsonElement): BigDecimal { require(isNumber(value)) { "JMESPath expected number" }; return BigDecimal.parseString(value.jsonPrimitive.content) }
    private fun numeric(value: BigDecimal): JsonElement = JsonUnquotedLiteral(value.toString())
    private fun string(value: JsonElement): String { require(value is JsonPrimitive && value.isString) { "JMESPath expected string" }; return value.content }
    private fun array(value: JsonElement): JsonArray { require(value is JsonArray) { "JMESPath expected array" }; return value }
    private fun objectValue(value: JsonElement): JsonObject { require(value is JsonObject) { "JMESPath expected object" }; return value }
    private fun characters(value: String): List<String> {
        val result=mutableListOf<String>(); var index=0
        while (index < value.length) { val count=if (value[index].isHighSurrogate() && index+1 < value.length && value[index+1].isLowSurrogate()) 2 else 1; result.add(value.substring(index,index+count)); index+=count }
        return result
    }
    private fun compare(a: JsonElement,b: JsonElement): Int {
        if (isNumber(a) && isNumber(b)) return number(a).compareTo(number(b))
        val left=characters(string(a)); val right=characters(string(b))
        fun point(value: String): Int = if (value.length==1) value[0].code else 0x10000 + ((value[0].code-0xd800) shl 10) + value[1].code-0xdc00
        for (index in 0 until minOf(left.size,right.size)) { val result=point(left[index]).compareTo(point(right[index])); if (result!=0) return result }
        return left.size.compareTo(right.size)
    }
    private fun equal(a: JsonElement,b: JsonElement): Boolean = when {
        isNumber(a) -> isNumber(b) && number(a).compareTo(number(b))==0
        a is JsonArray && b is JsonArray -> a.size==b.size && a.indices.all { equal(a[it],b[it]) }
        a is JsonObject && b is JsonObject -> a.keys==b.keys && a.keys.all { equal(a.getValue(it),b.getValue(it)) }
        else -> a==b
    }
    fun evaluate(node: JsonObject,value: JsonElement): JsonElement {
        fun child(name: String, input: JsonElement=value) = evaluate(node.obj(name), input)
        return when (node.text("kind")) {
            "identity" -> value
            "literal" -> node.getValue("value")
            "field" -> (value as? JsonObject)?.get(node.text("name")) ?: JsonNull
            "index" -> (value as? JsonArray)?.let { val index=node.text("index")!!.toInt(); it.getOrNull(if (index<0) it.size+index else index) } ?: JsonNull
            "subexpression" -> child("right",child("left"))
            "projection" -> (child("left") as? JsonArray)?.let { JsonArray(it.map { item -> child("right",item) }.filter { it!=JsonNull }) } ?: JsonNull
            "flatten" -> (child("expression") as? JsonArray)?.let { JsonArray(it.flatMap { item -> if (item is JsonArray) item else listOf(item) }) } ?: JsonNull
            "objectValues" -> (child("expression") as? JsonObject)?.let { JsonArray(it.values.toList()) } ?: JsonNull
            "multiList" -> if (value==JsonNull) JsonNull else JsonArray(node.array("expressions").map { evaluate(it.jsonObject,value) })
            "multiHash" -> if (value==JsonNull) JsonNull else JsonObject(node.array("entries").associate { val entry=it.jsonObject; entry.text("key")!! to evaluate(entry.obj("value"),value) })
            "and" -> child("left").let { if (truth(it)) child("right") else it }
            "or" -> child("left").let { if (truth(it)) it else child("right") }
            "not" -> JsonPrimitive(!truth(child("expression")))
            "condition" -> if (truth(child("predicate"))) child("then") else JsonNull
            "compare" -> {
                val left=child("left"); val right=child("right"); val comparator=node.text("comparator")
                if (comparator=="equal") JsonPrimitive(equal(left,right)) else if (comparator=="notEqual") JsonPrimitive(!equal(left,right))
                else if (!isNumber(left) || !isNumber(right)) JsonNull else {
                    val result=compare(left,right)
                    JsonPrimitive(when (comparator) { "lessThan"->result<0; "lessThanEqual"->result<=0; "greaterThan"->result>0; "greaterThanEqual"->result>=0; else->error("Unknown comparator") })
                }
            }
            "slice" -> {
                if (value !is JsonArray) JsonNull else {
                    val step=node.text("step")!!.toInt(); require(step!=0)
                    fun boundary(name:String, default:Int):Int { val raw=node.text(name)?.toInt() ?: return default; return (if (raw<0) value.size.toLong()+raw else raw.toLong()).coerceIn(if (step>0) 0L else -1L,if (step>0) value.size.toLong() else value.size.toLong()-1).toInt() }
                    var index=boundary("start",if (step>0) 0 else value.lastIndex).toLong(); val stop=boundary("stop",if (step>0) value.size else -1).toLong(); val result=mutableListOf<JsonElement>()
                    while (if (step>0) index<stop else index>stop) { result.add(value[index.toInt()]); index+=step }
                    JsonArray(result)
                }
            }
            "function" -> function(node.text("function")!!,node.array("arguments").map { it.jsonObject },value)
            else -> error("Unknown compiled JMESPath node ${node.text("kind")}")
        }
    }
    private fun function(name:String,nodes:List<JsonObject>,input:JsonElement):JsonElement {
        require(if (name in setOf("merge","not_null")) nodes.isNotEmpty() else nodes.size==if (name in setOf("contains","starts_with","ends_with","join","map","max_by","min_by","sort_by")) 2 else 1) { "JMESPath invalid function arity" }
        fun arg(index:Int)=evaluate(nodes[index],input)
        fun reference(index:Int,value:JsonElement):JsonElement { require(nodes[index].text("kind")=="expressionReference"); return evaluate(nodes[index].obj("expression"),value) }
        fun ordered(value:JsonElement):List<JsonElement> { val items=array(value); if (items.isNotEmpty()) require(items.all { if (isNumber(items[0])) isNumber(it) else it is JsonPrimitive && it.isString }); return items.sortedWith(Comparator(::compare)) }
        return when (name) {
            "abs" -> numeric(number(arg(0)).abs())
            "ceil", "floor" -> numeric(number(arg(0)).let { if (name=="ceil") it.ceil() else it.floor() })
            "sum", "avg" -> {
                val values=array(arg(0)); val sum=values.fold(BigDecimal.ZERO) { total,item -> total+number(item) }
                if (name=="sum") numeric(sum) else if (values.isEmpty()) JsonNull else numeric(sum.divide(BigDecimal.fromInt(values.size),DecimalMode(34,RoundingMode.ROUND_HALF_AWAY_FROM_ZERO)))
            }
            "contains" -> arg(0).let { value -> JsonPrimitive(if (value is JsonPrimitive && value.isString) string(arg(1)) in value.content else array(value).any { equal(it,arg(1)) }) }
            "starts_with" -> JsonPrimitive(string(arg(0)).startsWith(string(arg(1))))
            "ends_with" -> JsonPrimitive(string(arg(0)).endsWith(string(arg(1))))
            "join" -> JsonPrimitive(array(arg(1)).joinToString(string(arg(0))) { string(it) })
            "keys" -> JsonArray(objectValue(arg(0)).keys.map { JsonPrimitive(it) })
            "values" -> JsonArray(objectValue(arg(0)).values.toList())
            "length" -> JsonPrimitive(when (val value=arg(0)) { is JsonArray->value.size; is JsonObject->value.size; else->characters(string(value)).size })
            "map" -> JsonArray(array(arg(1)).map { reference(0,it) })
            "max", "min", "sort" -> ordered(arg(0)).let { if (name=="sort") JsonArray(it) else if (name=="max") it.lastOrNull() ?: JsonNull else it.firstOrNull() ?: JsonNull }
            "max_by", "min_by", "sort_by" -> {
                val values=array(arg(0)); val keys=values.map { reference(1,it) }; ordered(JsonArray(keys))
                val indices=values.indices.sortedWith { a,b -> compare(keys[a],keys[b]) }
                if (name=="sort_by") JsonArray(indices.map { values[it] }) else if (values.isEmpty()) JsonNull else {
                    val selected=values.indices.reduce { best,index -> val order=compare(keys[index],keys[best]); if (if (name=="max_by") order>0 else order<0) index else best }; values[selected]
                }
            }
            "merge" -> JsonObject(nodes.indices.flatMap { objectValue(arg(it)).entries.map { entry -> entry.key to entry.value } }.toMap())
            "not_null" -> nodes.indices.firstNotNullOfOrNull { arg(it).takeUnless { value -> value==JsonNull } } ?: JsonNull
            "reverse" -> arg(0).let { if (it is JsonArray) JsonArray(it.reversed()) else JsonPrimitive(characters(string(it)).reversed().joinToString("")) }
            "to_array" -> arg(0).let { if (it is JsonArray) it else JsonArray(listOf(it)) }
            "to_number" -> arg(0).let { if (isNumber(it)) it else if (it is JsonPrimitive && it.isString) try { numeric(BigDecimal.parseString(it.content)) } catch (_: Exception) { JsonNull } else JsonNull }
            "to_string" -> arg(0).let { if (it is JsonPrimitive && it.isString) it else JsonPrimitive(it.toString()) }
            "type" -> JsonPrimitive(when (val value=arg(0)) { JsonNull->"null"; is JsonArray->"array"; is JsonObject->"object"; else->if (value.jsonPrimitive.isString) "string" else if (value.jsonPrimitive.booleanOrNull!=null) "boolean" else "number" })
            else -> error("Unknown compiled JMESPath function $name")
        }
    }
}
