import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import com.google.gson.JsonPrimitive;
import java.math.BigDecimal;
import java.math.MathContext;
import java.math.RoundingMode;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Set;

/** Executes the shared compiled JMESPath tree without parsing expression strings. */
final class Query {
  private Query() {}
  private static final JsonElement NIL=JsonNull.INSTANCE;
  private static String text(JsonObject node,String name) { return ClientRuntime.text(node,name); }
  private static JsonElement value(JsonElement value) { return value==null ? NIL : value; }
  private static boolean numeric(JsonElement value) { return value.isJsonPrimitive() && value.getAsJsonPrimitive().isNumber(); }
  private static BigDecimal number(JsonElement value) { if (!numeric(value)) throw new IllegalArgumentException("JMESPath expected number"); return value.getAsBigDecimal(); }
  private static String string(JsonElement value) { if (!value.isJsonPrimitive() || !value.getAsJsonPrimitive().isString()) throw new IllegalArgumentException("JMESPath expected string"); return value.getAsString(); }
  private static List<JsonElement> array(JsonElement value) { if (!value.isJsonArray()) throw new IllegalArgumentException("JMESPath expected array"); return value.getAsJsonArray().asList(); }
  private static JsonObject object(JsonElement value) { if (!value.isJsonObject()) throw new IllegalArgumentException("JMESPath expected object"); return value.getAsJsonObject(); }
  private static JsonArray list(List<JsonElement> items) { var result=new JsonArray(); items.forEach(result::add); return result; }
  private static boolean truth(JsonElement value) {
    if (value.isJsonNull()) return false;
    if (value.isJsonArray()) return !value.getAsJsonArray().isEmpty();
    if (value.isJsonObject()) return !value.getAsJsonObject().isEmpty();
    var primitive=value.getAsJsonPrimitive(); return primitive.isString() ? !primitive.getAsString().isEmpty() : !primitive.isBoolean() || primitive.getAsBoolean();
  }
  private static boolean equal(JsonElement a,JsonElement b) {
    if (numeric(a)) return numeric(b) && number(a).compareTo(number(b))==0;
    if (a.isJsonArray() && b.isJsonArray()) { var x=array(a); var y=array(b); if (x.size()!=y.size()) return false; for (int i=0;i<x.size();i++) if (!equal(x.get(i),y.get(i))) return false; return true; }
    if (a.isJsonObject() && b.isJsonObject()) { var x=object(a); var y=object(b); if (!x.keySet().equals(y.keySet())) return false; for (String key:x.keySet()) if (!equal(x.get(key),y.get(key))) return false; return true; }
    return a.equals(b);
  }
  private static int compare(JsonElement a,JsonElement b) {
    if (numeric(a) && numeric(b)) return number(a).compareTo(number(b));
    int[] x=string(a).codePoints().toArray(), y=string(b).codePoints().toArray();
    for (int i=0;i<Math.min(x.length,y.length);i++) if (x[i]!=y[i]) return Integer.compare(x[i],y[i]);
    return Integer.compare(x.length,y.length);
  }
  static JsonElement evaluate(JsonObject node,JsonElement input) {
    input=value(input);
    switch(text(node,"kind")) {
      case "identity": return input;
      case "literal": return node.get("value");
      case "field": return input.isJsonObject() ? value(object(input).get(text(node,"name"))) : NIL;
      case "index": { if (!input.isJsonArray()) return NIL; int index=node.get("index").getAsInt(), size=array(input).size(); long position=index<0 ? (long)size+index : index; return position>=0 && position<size ? array(input).get((int)position) : NIL; }
      case "subexpression": return evaluate(node.getAsJsonObject("right"),evaluate(node.getAsJsonObject("left"),input));
      case "projection": { var source=evaluate(node.getAsJsonObject("left"),input); if (!source.isJsonArray()) return NIL; var result=new JsonArray(); for (var item:array(source)) { var next=evaluate(node.getAsJsonObject("right"),item); if (!next.isJsonNull()) result.add(next); } return result; }
      case "flatten": { var source=evaluate(node.getAsJsonObject("expression"),input); if (!source.isJsonArray()) return NIL; var result=new JsonArray(); for (var item:array(source)) { if (item.isJsonArray()) item.getAsJsonArray().forEach(result::add); else result.add(item); } return result; }
      case "objectValues": { var source=evaluate(node.getAsJsonObject("expression"),input); return source.isJsonObject() ? list(new ArrayList<>(object(source).asMap().values())) : NIL; }
      case "multiList": { if (input.isJsonNull()) return NIL; var result=new JsonArray(); for (var item:node.getAsJsonArray("expressions")) result.add(evaluate(item.getAsJsonObject(),input)); return result; }
      case "multiHash": { if (input.isJsonNull()) return NIL; var result=new JsonObject(); for (var item:node.getAsJsonArray("entries")) { var entry=item.getAsJsonObject(); result.add(text(entry,"key"),evaluate(entry.getAsJsonObject("value"),input)); } return result; }
      case "and": { var left=evaluate(node.getAsJsonObject("left"),input); return truth(left) ? evaluate(node.getAsJsonObject("right"),input) : left; }
      case "or": { var left=evaluate(node.getAsJsonObject("left"),input); return truth(left) ? left : evaluate(node.getAsJsonObject("right"),input); }
      case "not": return new JsonPrimitive(!truth(evaluate(node.getAsJsonObject("expression"),input)));
      case "condition": return truth(evaluate(node.getAsJsonObject("predicate"),input)) ? evaluate(node.getAsJsonObject("then"),input) : NIL;
      case "compare": {
        var left=evaluate(node.getAsJsonObject("left"),input); var right=evaluate(node.getAsJsonObject("right"),input); String comparator=text(node,"comparator");
        if (comparator.equals("equal")) return new JsonPrimitive(equal(left,right)); if (comparator.equals("notEqual")) return new JsonPrimitive(!equal(left,right));
        if (!numeric(left) || !numeric(right)) return NIL; int order=compare(left,right);
        return new JsonPrimitive(switch(comparator) { case "lessThan" -> order<0; case "lessThanEqual" -> order<=0; case "greaterThan" -> order>0; case "greaterThanEqual" -> order>=0; default -> throw new IllegalArgumentException("Unknown comparator"); });
      }
      case "slice": {
        if (!input.isJsonArray()) return NIL; var source=array(input); int step=node.get("step").getAsInt(); if (step==0) throw new IllegalArgumentException("Zero slice step");
        long index=boundary(node,"start",source.size(),step>0 ? 0 : source.size()-1,step), stop=boundary(node,"stop",source.size(),step>0 ? source.size() : -1,step); var result=new JsonArray();
        while (step>0 ? index<stop : index>stop) { result.add(source.get((int)index)); index+=step; } return result;
      }
      case "function": return function(text(node,"function"),node.getAsJsonArray("arguments").asList(),input);
      default: throw new IllegalArgumentException("Unknown compiled JMESPath node "+text(node,"kind"));
    }
  }
  private static long boundary(JsonObject node,String name,int size,int fallback,int step) { if (text(node,name)==null) return fallback; long value=node.get(name).getAsInt(); if (value<0) value+=size; return Math.min(step>0 ? size : size-1,Math.max(step>0 ? 0 : -1,value)); }
  private static List<JsonElement> ordered(JsonElement value) {
    var result=new ArrayList<>(array(value));
    if (!result.isEmpty()) for (var item:result) { if (numeric(result.get(0))) number(item); else string(item); }
    result.sort(Query::compare); return result;
  }
  private static JsonElement reference(List<JsonElement> nodes,int index,JsonElement input) {
    var node=nodes.get(index).getAsJsonObject(); if (!text(node,"kind").equals("expressionReference")) throw new IllegalArgumentException("JMESPath expected expression reference");
    return evaluate(node.getAsJsonObject("expression"),input);
  }
  private static JsonElement function(String name,List<JsonElement> nodes,JsonElement input) {
    boolean variadic=Set.of("merge","not_null").contains(name); int arity=Set.of("contains","starts_with","ends_with","join","map","max_by","min_by","sort_by").contains(name) ? 2 : 1;
    if (variadic ? nodes.isEmpty() : nodes.size()!=arity) throw new IllegalArgumentException("JMESPath invalid function arity");
    java.util.function.IntFunction<JsonElement> arg=index -> evaluate(nodes.get(index).getAsJsonObject(),input);
    switch(name) {
      case "abs": return new JsonPrimitive(number(arg.apply(0)).abs());
      case "ceil": case "floor": return new JsonPrimitive(number(arg.apply(0)).setScale(0,name.equals("ceil") ? RoundingMode.CEILING : RoundingMode.FLOOR));
      case "sum": case "avg": { var items=array(arg.apply(0)); BigDecimal sum=BigDecimal.ZERO; for (var item:items) sum=sum.add(number(item)); return name.equals("sum") ? new JsonPrimitive(sum) : items.isEmpty() ? NIL : new JsonPrimitive(sum.divide(BigDecimal.valueOf(items.size()),MathContext.DECIMAL128)); }
      case "contains": { var source=arg.apply(0); var wanted=arg.apply(1); return new JsonPrimitive(source.isJsonPrimitive() && source.getAsJsonPrimitive().isString() ? string(source).contains(string(wanted)) : array(source).stream().anyMatch(item -> equal(item,wanted))); }
      case "starts_with": return new JsonPrimitive(string(arg.apply(0)).startsWith(string(arg.apply(1))));
      case "ends_with": return new JsonPrimitive(string(arg.apply(0)).endsWith(string(arg.apply(1))));
      case "join": { var items=new ArrayList<String>(); for (var item:array(arg.apply(1))) items.add(string(item)); return new JsonPrimitive(String.join(string(arg.apply(0)),items)); }
      case "keys": { var result=new JsonArray(); object(arg.apply(0)).keySet().forEach(result::add); return result; }
      case "values": return list(new ArrayList<>(object(arg.apply(0)).asMap().values()));
      case "length": { var source=arg.apply(0); return new JsonPrimitive(source.isJsonArray() ? array(source).size() : source.isJsonObject() ? object(source).size() : string(source).codePointCount(0,string(source).length())); }
      case "map": { var result=new JsonArray(); for (var item:array(arg.apply(1))) result.add(reference(nodes,0,item)); return result; }
      case "max": case "min": case "sort": { var items=ordered(arg.apply(0)); return name.equals("sort") ? list(items) : items.isEmpty() ? NIL : items.get(name.equals("max") ? items.size()-1 : 0); }
      case "max_by": case "min_by": case "sort_by": {
        var items=array(arg.apply(0)); var keys=new ArrayList<JsonElement>(); for (var item:items) keys.add(reference(nodes,1,item)); ordered(list(keys));
        if (items.isEmpty()) return name.equals("sort_by") ? new JsonArray() : NIL;
        if (name.equals("sort_by")) { var indices=new ArrayList<Integer>(); for (int i=0;i<items.size();i++) indices.add(i); indices.sort((a,b)->compare(keys.get(a),keys.get(b))); var result=new JsonArray(); for (int index:indices) result.add(items.get(index)); return result; }
        int selected=0; for (int i=1;i<items.size();i++) { int order=compare(keys.get(i),keys.get(selected)); if (name.equals("max_by") ? order>0 : order<0) selected=i; } return items.get(selected);
      }
      case "merge": { var result=new JsonObject(); for (int i=0;i<nodes.size();i++) object(arg.apply(i)).entrySet().forEach(entry -> result.add(entry.getKey(),entry.getValue())); return result; }
      case "not_null": { for (int i=0;i<nodes.size();i++) { var next=arg.apply(i); if (!next.isJsonNull()) return next; } return NIL; }
      case "reverse": { var source=arg.apply(0); if (source.isJsonArray()) { var items=new ArrayList<>(array(source)); Collections.reverse(items); return list(items); } int[] points=string(source).codePoints().toArray(); var result=new StringBuilder(); for (int i=points.length-1;i>=0;i--) result.appendCodePoint(points[i]); return new JsonPrimitive(result.toString()); }
      case "to_array": { var source=arg.apply(0); return source.isJsonArray() ? source : list(List.of(source)); }
      case "to_number": { var source=arg.apply(0); if (numeric(source)) return source; if (source.isJsonPrimitive() && source.getAsJsonPrimitive().isString()) try { return new JsonPrimitive(new BigDecimal(source.getAsString())); } catch (NumberFormatException ignored) {} return NIL; }
      case "to_string": { var source=arg.apply(0); return source.isJsonPrimitive() && source.getAsJsonPrimitive().isString() ? source : new JsonPrimitive(source.toString()); }
      case "type": { var source=arg.apply(0); return new JsonPrimitive(source.isJsonNull() ? "null" : source.isJsonArray() ? "array" : source.isJsonObject() ? "object" : source.getAsJsonPrimitive().isString() ? "string" : source.getAsJsonPrimitive().isBoolean() ? "boolean" : "number"); }
      default: throw new IllegalArgumentException("Unknown compiled JMESPath function "+name);
    }
  }
}
