import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import com.google.gson.JsonPrimitive;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.regex.Pattern;

/** Evaluates the shared endpoint expression tree and source precedence. */
final class Endpoints {
  private Endpoints() {}
  private static final Pattern LABEL=Pattern.compile("[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?");
  private static String text(JsonObject value,String name) { return ClientRuntime.text(value,name); }
  private static JsonElement get(JsonObject value,String name) { var item=value.get(name); return item==null ? JsonNull.INSTANCE : item; }
  private static boolean string(JsonElement value) { return value.isJsonPrimitive() && value.getAsJsonPrimitive().isString(); }
  private static boolean bool(JsonElement value) { return value.isJsonPrimitive() && value.getAsJsonPrimitive().isBoolean(); }
  private static boolean hostLabel(String value,boolean subdomains) { if(value.isEmpty()) return false; for(String part:subdomains ? value.split("\\.",-1) : new String[]{value}) if(!LABEL.matcher(part).matches()) return false; return true; }
  static JsonObject parseUrl(String value) {
    try {
      var url=URI.create(value);
      if (!("http".equals(url.getScheme()) || "https".equals(url.getScheme())) || url.getHost()==null || url.getRawUserInfo()!=null || url.getRawQuery()!=null || url.getRawFragment()!=null || url.getPort()>65535 || url.getPort()==0 || value.chars().anyMatch(c->c<=32 || c==127)) return null;
      String host=url.getHost().toLowerCase(java.util.Locale.ROOT); boolean ip=host.startsWith("[");
      if(!ip) { var parts=host.split("\\.",-1); ip=parts.length==4; for(String part:parts) if(!part.matches("[0-9]{1,3}") || Integer.parseInt(part)>255) { ip=false; break; } if(!hostLabel(host.endsWith(".") ? host.substring(0,host.length()-1) : host,true)) return null; }
      int port=url.getPort(); String authority=host+(port<0 || port==(url.getScheme().equals("https") ? 443 : 80) ? "" : ":"+port);
      String path=url.getRawPath(); if(path==null || path.isEmpty()) path="/";
      var result=new JsonObject(); result.addProperty("scheme",url.getScheme()); result.addProperty("authority",authority); result.addProperty("path",path); result.addProperty("normalizedPath",path.endsWith("/") ? path : path+"/"); result.addProperty("isIp",ip); return result;
    } catch(IllegalArgumentException invalid) { return null; }
  }
  private static JsonElement attribute(JsonElement value,JsonElement path) {
    JsonArray steps;
    if(path.isJsonArray()) steps=path.getAsJsonArray();
    else {
      steps=new JsonArray();
      for(String part:path.getAsString().split("\\.")) {
        String key=part.split("\\[",2)[0];
        if(!key.isEmpty()) { var step=new JsonObject(); step.addProperty("kind","member"); step.addProperty("name",key); steps.add(step); }
        var matcher=Pattern.compile("\\[(-?\\d+)\\]").matcher(part);
        while(matcher.find()) { var step=new JsonObject(); step.addProperty("kind","index"); step.addProperty("index",Integer.parseInt(matcher.group(1))); steps.add(step); }
      }
    }
    for(var item:steps) {
      var step=item.getAsJsonObject();
      if("member".equals(text(step,"kind"))) value=value.isJsonObject() ? get(value.getAsJsonObject(),text(step,"name")) : JsonNull.INSTANCE;
      else if(value.isJsonArray()) { var array=value.getAsJsonArray(); int index=step.get("index").getAsInt(); if(index<0) index+=array.size(); value=index>=0 && index<array.size() ? array.get(index) : JsonNull.INSTANCE; }
      else value=JsonNull.INSTANCE;
    }
    return value;
  }
  private static JsonElement function(String name,List<JsonElement> args) {
    var first=args.get(0);
    switch(name) {
      case "isSet": return new JsonPrimitive(!first.isJsonNull());
      case "not": if(!bool(first)) throw new IllegalArgumentException("Endpoint not expects boolean"); return new JsonPrimitive(!first.getAsBoolean());
      case "booleanEquals": return new JsonPrimitive(args.stream().allMatch(Endpoints::bool) && first.equals(args.get(1)));
      case "stringEquals": return new JsonPrimitive(args.stream().allMatch(Endpoints::string) && first.equals(args.get(1)));
      case "getAttr": return attribute(first,args.get(1));
      case "substring": { if(!string(first)) return JsonNull.INSTANCE; String value=first.getAsString(); int start=args.get(1).getAsInt(),end=args.get(2).getAsInt(); if(value.chars().anyMatch(c->c>=128) || start<0 || end<start || end>value.length()) return JsonNull.INSTANCE; return new JsonPrimitive(args.get(3).getAsBoolean() ? value.substring(value.length()-end,value.length()-start) : value.substring(start,end)); }
      case "isValidHostLabel": return new JsonPrimitive(string(first) && hostLabel(first.getAsString(),args.get(1).getAsBoolean()));
      case "parseUrl": { var result=string(first) ? parseUrl(first.getAsString()) : null; return result==null ? JsonNull.INSTANCE : result; }
      case "uriEncode": return new JsonPrimitive(java.net.URLEncoder.encode(first.getAsString(),StandardCharsets.UTF_8).replace("+","%20").replace("*","%2A").replace("%7E","~"));
      default: throw new IllegalArgumentException("Unsupported compiled endpoint function "+name);
    }
  }
  private static List<JsonElement> arguments(JsonObject node,JsonObject values) { var result=new ArrayList<JsonElement>(); for(var item:node.getAsJsonArray("arguments")) result.add(expression(item.getAsJsonObject(),values)); return result; }
  private static JsonElement expression(JsonObject node,JsonObject values) {
    switch(text(node,"kind")) {
      case "reference": return get(values,text(node,"name"));
      case "boolean": case "integer": return node.get("value");
      case "array": { var result=new JsonArray(); for(var item:node.getAsJsonArray("values")) result.add(expression(item.getAsJsonObject(),values)); return result; }
      case "object": { var result=new JsonObject(); for(var entry:node.getAsJsonObject("values").entrySet()) result.add(entry.getKey(),expression(entry.getValue().getAsJsonObject(),values)); return result; }
      case "call": return function(text(node,"function"),arguments(node,values));
      case "string": {
        var result=new StringBuilder();
        for(var entry:node.getAsJsonArray("parts")) { var part=entry.getAsJsonObject(); var value="literal".equals(text(part,"kind")) ? part.get("value") : attribute(get(values,text(part,"name")),part.get("path")); if(!value.isJsonPrimitive()) throw new IllegalArgumentException("Endpoint template references missing or nonscalar parameter"); result.append(value.getAsString()); }
        return new JsonPrimitive(result.toString());
      }
      default: throw new IllegalArgumentException("Unsupported compiled endpoint expression");
    }
  }
  private static JsonObject rules(JsonArray nodes,JsonObject parameters) {
    for(var item:nodes) {
      var rule=item.getAsJsonObject(); var scope=parameters.deepCopy(); boolean matches=true;
      for(var entry:rule.getAsJsonArray("conditions")) { var condition=entry.getAsJsonObject(); var value=function(text(condition,"function"),arguments(condition,scope)); if(value.isJsonNull() || bool(value) && !value.getAsBoolean()) { matches=false; break; } String assign=text(condition,"assign"); if(assign!=null) scope.add(assign,value); }
      if(!matches) continue;
      var result=rule.getAsJsonObject("result");
      switch(text(result,"kind")) {
        case "tree": { var nested=rules(result.getAsJsonArray("rules"),scope); if(nested!=null) return nested; break; }
        case "error": throw new IllegalArgumentException(expression(result.getAsJsonObject("message"),scope).getAsString());
        default: {
          var url=expression(result.getAsJsonObject("url"),scope); if(parseUrl(url.getAsString())==null) throw new IllegalArgumentException("Endpoint rule returned invalid HTTP(S) URL");
          var headers=new JsonObject(); for(var entry:result.getAsJsonObject("headers").entrySet()) { var entries=new JsonArray(); for(var value:entry.getValue().getAsJsonArray()) entries.add(expression(value.getAsJsonObject(),scope)); headers.add(entry.getKey(),entries); }
          var properties=new JsonObject(); for(var entry:result.getAsJsonObject("properties").entrySet()) properties.add(entry.getKey(),expression(entry.getValue().getAsJsonObject(),scope));
          var resolved=new JsonObject(); resolved.add("url",url); resolved.add("headers",headers); resolved.add("properties",properties); return resolved;
        }
      }
    }
    return null;
  }
  static JsonObject resolve(JsonObject ruleSet,JsonObject sources,JsonElement input,JsonObject client,JsonObject builtins) {
    var parameters=new JsonObject();
    for(var entry:ruleSet.getAsJsonObject("parameters").entrySet()) {
      String name=entry.getKey(); var parameter=entry.getValue().getAsJsonObject(); JsonElement value=JsonNull.INSTANCE;
      for(var item:sources.has(name) ? sources.getAsJsonArray(name) : new JsonArray()) {
        var source=item.getAsJsonObject(); value=switch(text(source,"kind")) {
          case "static", "default" -> source.get("value");
          case "inputMember" -> input.isJsonObject() ? get(input.getAsJsonObject(),text(source,"member")) : JsonNull.INSTANCE;
          case "inputPath" -> Query.evaluate(source.getAsJsonObject("path").getAsJsonObject("expression"),input);
          case "client" -> get(client,text(source,"name"));
          case "builtIn" -> get(builtins,text(source,"name"));
          default -> throw new IllegalArgumentException("Unknown endpoint parameter source");
        };
        if(!value.isJsonNull()) break;
      }
      if(value.isJsonNull() && parameter.get("required").getAsBoolean()) throw new IllegalArgumentException("Required endpoint parameter "+name+" is missing");
      if(!value.isJsonNull() && !(switch(text(parameter,"kind")) { case "string" -> string(value); case "boolean" -> bool(value); default -> value.isJsonArray() && value.getAsJsonArray().asList().stream().allMatch(Endpoints::string); })) throw new IllegalArgumentException("Endpoint parameter "+name+" has wrong type");
      parameters.add(name,value);
    }
    var result=rules(ruleSet.getAsJsonArray("rules"),parameters); if(result==null) throw new IllegalArgumentException("No endpoint rule matched"); return result;
  }
  static String hostPrefix(String endpoint,JsonObject description,JsonObject input) {
    var prefix=new StringBuilder();
    for(var item:description.getAsJsonArray("parts")) { var part=item.getAsJsonObject(); if("literal".equals(text(part,"kind"))) prefix.append(text(part,"value")); else { var value=get(input,text(part,"member")); if(!string(value) || !hostLabel(value.getAsString(),false)) throw new IllegalArgumentException("Invalid endpoint host label"); prefix.append(value.getAsString()); } }
    var url=URI.create(endpoint); String host=prefix+url.getHost(); if(host.length()>253 || !hostLabel(host,true)) throw new IllegalArgumentException("Invalid endpoint hostname after prefix expansion");
    return url.getScheme()+"://"+host+(url.getPort()<0 ? "" : ":"+url.getPort())+url.getRawPath();
  }
}
