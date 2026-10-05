import io.ktor.http.Url
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/** Evaluates the shared endpoint expression tree and source precedence. */
internal object Endpoints {
    private val label=Regex("[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?")
    private fun hostLabel(value:String,subdomains:Boolean)=value.isNotEmpty() && (if(subdomains) value.split('.') else listOf(value)).all { label.matches(it) }
    private fun ipv6(value:String):Boolean {
        if(value.count { it==':' }<2 || value.count { it=='%' }>0 || value.contains(":::")) return false
        val halves=value.split("::"); if(halves.size>2) return false
        val parts=halves.flatMap { it.split(':').filter(String::isNotEmpty) }
        var count=0
        for((index,part) in parts.withIndex()) {
            if('.' in part) { if(index!=parts.lastIndex || part.split('.').size!=4 || part.split('.').any { it.isEmpty() || it.any { c-> !c.isDigit() } || (it.toIntOrNull() ?: -1) !in 0..255 }) return false; count+=2 }
            else { if(part.length !in 1..4 || part.any { it !in "0123456789abcdefABCDEF" }) return false; count++ }
        }
        return if(halves.size==2) count<8 else count==8
    }
    fun parseUrl(value:String):JsonObject? {
        if(value.any { it.code<=32 || it.code==127 } || '?' in value || '#' in value || '\\' in value || Regex("%(?![A-Fa-f0-9]{2})").containsMatchIn(value)) return null
        return try {
            if(!value.startsWith("https://") && !value.startsWith("http://")) return null
            val url=Url(value); val host=url.host.removeSurrounding("[","]")
            if(host.isEmpty() || url.user!=null || url.password!=null || url.port !in 1..65535) return null
            val address=if(':' in host) ipv6(host) else host.split('.').let { it.size==4 && it.all { s->s.isNotEmpty() && s.all(Char::isDigit) && (s.toIntOrNull() ?: -1) in 0..255 } }
            if(!address && !hostLabel(host.trimEnd('.'),true)) return null
            val authority=(if(':' in host) "[$host]" else host)+(if(url.port==url.protocol.defaultPort) "" else ":${url.port}")
            val path=url.encodedPath.ifEmpty { "/" }
            JsonObject(mapOf("scheme" to JsonPrimitive(url.protocol.name),"authority" to JsonPrimitive(authority),"path" to JsonPrimitive(path),"normalizedPath" to JsonPrimitive(if(path.endsWith('/')) path else "$path/"),"isIp" to JsonPrimitive(address)))
        } catch (_:Exception) { null }
    }
    private fun attribute(value:JsonElement,path:JsonElement):JsonElement {
        val steps=if(path is JsonArray) path else JsonArray(path.jsonPrimitive.content.split('.').flatMap { part ->
            buildList { val key=part.substringBefore('['); if(key.isNotEmpty()) add(JsonObject(mapOf("kind" to JsonPrimitive("member"),"name" to JsonPrimitive(key))))
                Regex("\\[(-?\\d+)\\]").findAll(part).forEach { add(JsonObject(mapOf("kind" to JsonPrimitive("index"),"index" to JsonPrimitive(it.groupValues[1].toInt())))) } }
        })
        var result=value
        for(step in steps) { val item=step.jsonObject; result=if(item.text("kind")=="member") (result as? JsonObject)?.get(item.text("name")) ?: JsonNull else (result as? JsonArray)?.let { val index=item.getValue("index").jsonPrimitive.int; it.getOrNull(if(index<0) it.size+index else index) } ?: JsonNull }
        return result
    }
    private fun function(name:String,args:List<JsonElement>):JsonElement = when(name) {
        "isSet" -> JsonPrimitive(args[0]!=JsonNull)
        "not" -> JsonPrimitive(!requireNotNull(args[0].jsonPrimitive.booleanOrNull) { "Endpoint not expects boolean" })
        "booleanEquals" -> JsonPrimitive(args.all { it is JsonPrimitive && !it.isString && it.booleanOrNull!=null } && args[0]==args[1])
        "stringEquals" -> JsonPrimitive(args.all { it is JsonPrimitive && it.isString } && args[0]==args[1])
        "getAttr" -> attribute(args[0],args[1])
        "substring" -> { val text=args[0].jsonPrimitive.content; val start=args[1].jsonPrimitive.int; val end=args[2].jsonPrimitive.int
            if(args[0].jsonPrimitive.isString && text.all { it.code<128 } && start>=0 && start<=end && end<=text.length) JsonPrimitive(if(args[3].jsonPrimitive.booleanOrNull==true) text.substring(text.length-end,text.length-start) else text.substring(start,end)) else JsonNull }
        "isValidHostLabel" -> JsonPrimitive(args[0] is JsonPrimitive && args[0].jsonPrimitive.isString && hostLabel(args[0].jsonPrimitive.content,args[1].jsonPrimitive.booleanOrNull==true))
        "parseUrl" -> if(args[0] is JsonPrimitive && args[0].jsonPrimitive.isString) parseUrl(args[0].jsonPrimitive.content) ?: JsonNull else JsonNull
        "uriEncode" -> JsonPrimitive(args[0].jsonPrimitive.content.encodeToByteArray().joinToString("") { val n=it.toInt() and 255; if(n in 65..90 || n in 97..122 || n in 48..57 || n.toChar() in "-._~") n.toChar().toString() else "%"+n.toString(16).uppercase().padStart(2,'0') })
        else -> error("Unsupported compiled endpoint function $name")
    }
    private fun expression(node:JsonObject,values:Map<String,JsonElement>):JsonElement = when(node.text("kind")) {
        "reference" -> values[node.text("name")] ?: JsonNull
        "boolean", "integer" -> node.getValue("value")
        "array" -> JsonArray(node.array("values").map { expression(it.jsonObject,values) })
        "object" -> JsonObject(node.obj("values").mapValues { expression(it.value.jsonObject,values) })
        "call" -> function(node.text("function")!!,node.array("arguments").map { expression(it.jsonObject,values) })
        "string" -> JsonPrimitive(node.array("parts").joinToString("") { part -> val item=part.jsonObject; val value=if(item.text("kind")=="literal") item.getValue("value") else attribute(values[item.text("name")] ?: JsonNull,item.getValue("path")); require(value is JsonPrimitive && value!=JsonNull) { "Endpoint template references missing or nonscalar parameter" }; value.content })
        else -> error("Unsupported compiled endpoint expression")
    }
    private fun rules(nodes:JsonArray,parameters:Map<String,JsonElement>):JsonObject? {
        for(item in nodes) {
            val rule=item.jsonObject; val scope=parameters.toMutableMap(); var matches=true
            for(entry in rule.array("conditions")) { val condition=entry.jsonObject; val value=function(condition.text("function")!!,condition.array("arguments").map { expression(it.jsonObject,scope) }); if(value==JsonNull || value is JsonPrimitive && !value.isString && value.booleanOrNull==false) { matches=false; break }; condition.text("assign")?.let { scope[it]=value } }
            if(!matches) continue
            val result=rule.obj("result")
            when(result.text("kind")) {
                "tree" -> rules(result.array("rules"),scope)?.let { return it }
                "error" -> throw IllegalArgumentException(expression(result.obj("message"),scope).jsonPrimitive.content)
                else -> { val url=expression(result.obj("url"),scope); require(parseUrl(url.jsonPrimitive.content)!=null) { "Endpoint rule returned invalid HTTP(S) URL" }
                    return JsonObject(mapOf("url" to url,"headers" to JsonObject(result.obj("headers").mapValues { (_,items)->JsonArray(items.jsonArray.map { expression(it.jsonObject,scope) }) }),"properties" to JsonObject(result.obj("properties").mapValues { expression(it.value.jsonObject,scope) }))) }
            }
        }
        return null
    }
    fun resolve(ruleSet:JsonObject,sources:JsonObject,input:JsonElement,client:Map<String,JsonElement>,builtins:Map<String,JsonElement>):JsonObject {
        val parameters=mutableMapOf<String,JsonElement>()
        for((name,item) in ruleSet.obj("parameters")) {
            val parameter=item.jsonObject; var value:JsonElement=JsonNull
            for(entry in (sources[name] as? JsonArray) ?: JsonArray(emptyList())) {
                val source=entry.jsonObject
                value=when(source.text("kind")) {
                    "static", "default" -> source.getValue("value")
                    "inputMember" -> (input as? JsonObject)?.get(source.text("member")) ?: JsonNull
                    "inputPath" -> Query.evaluate(source.obj("path").obj("expression"),input)
                    "client" -> client[source.text("name")] ?: JsonNull
                    "builtIn" -> builtins[source.text("name")] ?: JsonNull
                    else -> error("Unknown endpoint parameter source")
                }
                if(value!=JsonNull) break
            }
            require(value!=JsonNull || !parameter.flag("required")) { "Required endpoint parameter $name is missing" }
            if(value!=JsonNull) require(when(parameter.text("kind")) { "string"->value is JsonPrimitive && value.isString; "boolean"->value is JsonPrimitive && !value.isString && value.booleanOrNull!=null; else->value is JsonArray && value.all { it is JsonPrimitive && it.isString } }) { "Endpoint parameter $name has wrong type" }
            parameters[name]=value
        }
        return requireNotNull(rules(ruleSet.array("rules"),parameters)) { "No endpoint rule matched" }
    }
    fun hostPrefix(endpoint:String,description:JsonObject,input:JsonObject):String {
        val prefix=description.array("parts").joinToString("") { entry-> val part=entry.jsonObject; if(part.text("kind")=="literal") part.text("value")!! else { val value=input[part.text("member")]; require(value is JsonPrimitive && value.isString && hostLabel(value.content,false)) { "Invalid endpoint host label" }; value.content } }
        val url=Url(endpoint); val host=prefix+url.host; require(host.length<=253 && hostLabel(host,true)) { "Invalid endpoint hostname after prefix expansion" }
        return url.protocol.name+"://"+host+(if(url.port==url.protocol.defaultPort) "" else ":${url.port}")+url.encodedPath
    }
}
