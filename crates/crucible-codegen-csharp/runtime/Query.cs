using System.Globalization;
using System.Numerics;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

// The parser and semantic checks live in the shared compiler; this evaluates its tree.
internal static class Query
{
    private static string Text(JsonNode? value) => value?.GetValue<string>() ?? throw new ArgumentException("JMESPath expected string");
    private static bool Numeric(JsonNode? value) => value?.GetValueKind() == JsonValueKind.Number;
    private static BigDecimal Number(JsonNode? value) => Numeric(value) ? new(value!.ToJsonString()) : throw new ArgumentException("JMESPath expected number");
    private static JsonArray Array(JsonNode? value) => value as JsonArray ?? throw new ArgumentException("JMESPath expected array");
    private static JsonObject Object(JsonNode? value) => value as JsonObject ?? throw new ArgumentException("JMESPath expected object");
    private static JsonArray List(IEnumerable<JsonNode?> values) => new(values.Select(value => value?.DeepClone()).ToArray());
    private static JsonNode Value(string value) => JsonValue.Create(value)!;
    private static JsonNode Value(bool value) => JsonValue.Create(value)!;
    private static JsonNode Integer(BigInteger value) => JsonNode.Parse(value.ToString(CultureInfo.InvariantCulture))!;
    private static bool Truth(JsonNode? value) => value switch {
        null => false, JsonArray list => list.Count != 0, JsonObject map => map.Count != 0,
        _ => value.GetValueKind() switch { JsonValueKind.String => Text(value).Length != 0, JsonValueKind.False => false, _ => true }
    };
    private static bool Equal(JsonNode? a, JsonNode? b)
    {
        if (Numeric(a)) return Numeric(b) && Number(a) == Number(b);
        if (a is JsonArray x && b is JsonArray y) return x.Count == y.Count && x.Zip(y).All(pair => Equal(pair.First, pair.Second));
        if (a is JsonObject xo && b is JsonObject yo) return xo.Count == yo.Count && xo.All(entry => yo.ContainsKey(entry.Key) && Equal(entry.Value, yo[entry.Key]));
        return JsonNode.DeepEquals(a,b);
    }
    private static int Compare(JsonNode? a, JsonNode? b)
    {
        if (Numeric(a) && Numeric(b)) return Number(a).CompareTo(Number(b));
        var x = Text(a).EnumerateRunes().ToArray(); var y = Text(b).EnumerateRunes().ToArray();
        for (int i = 0; i < Math.Min(x.Length,y.Length); i++) if (x[i] != y[i]) return x[i].Value.CompareTo(y[i].Value);
        return x.Length.CompareTo(y.Length);
    }
    internal static JsonNode? Evaluate(JsonNode node, JsonNode? input)
    {
        JsonNode? Eval(string name) => Evaluate(node[name]!,input);
        switch (Text(node["kind"]))
        {
            case "identity": return input;
            case "literal": return node["value"];
            case "field": return input is JsonObject map ? map[Text(node["name"])] : null;
            case "index": {
                if (input is not JsonArray list) return null;
                long index = node["index"]!.GetValue<int>(); if (index < 0) index += list.Count;
                return index >= 0 && index < list.Count ? list[(int)index] : null;
            }
            case "subexpression": return Evaluate(node["right"]!,Eval("left"));
            case "projection": {
                if (Eval("left") is not JsonArray list) return null;
                return List(list.Select(item => Evaluate(node["right"]!,item)).Where(item => item is not null));
            }
            case "flatten": {
                if (Eval("expression") is not JsonArray list) return null;
                return List(list.SelectMany(item => item is JsonArray nested ? nested.AsEnumerable() : new[] { item }));
            }
            case "objectValues": return Eval("expression") is JsonObject obj ? List(obj.Select(entry => entry.Value)) : null;
            case "multiList": return input is null ? null : List(Array(node["expressions"]).Select(item => Evaluate(item!,input)));
            case "multiHash": {
                if (input is null) return null; var result = new JsonObject();
                foreach (var entry in Array(node["entries"])) result[Text(entry!["key"])] = Evaluate(entry["value"]!,input)?.DeepClone();
                return result;
            }
            case "and": { var left = Eval("left"); return Truth(left) ? Eval("right") : left; }
            case "or": { var left = Eval("left"); return Truth(left) ? left : Eval("right"); }
            case "not": return Value(!Truth(Eval("expression")));
            case "condition": return Truth(Eval("predicate")) ? Eval("then") : null;
            case "compare": {
                var left = Eval("left"); var right = Eval("right"); var comparator = Text(node["comparator"]);
                if (comparator == "equal") return Value(Equal(left,right)); if (comparator == "notEqual") return Value(!Equal(left,right));
                if (!Numeric(left) || !Numeric(right)) return null;
                int order = Compare(left,right);
                return Value(comparator switch { "lessThan" => order < 0, "lessThanEqual" => order <= 0, "greaterThan" => order > 0, "greaterThanEqual" => order >= 0, _ => throw new ArgumentException("Unknown comparator") });
            }
            case "slice": {
                if (input is not JsonArray list) return null; int step = node["step"]!.GetValue<int>();
                if (step == 0) throw new ArgumentException("Zero slice step");
                long Bound(string key, int fallback) { if (node[key] is null) return fallback; long value = node[key]!.GetValue<int>(); if (value < 0) value += list.Count; return Math.Clamp(value, step > 0 ? 0 : -1, step > 0 ? list.Count : list.Count - 1); }
                var result = new List<JsonNode?>(); long stop = Bound("stop",step > 0 ? list.Count : -1);
                for (long i = Bound("start",step > 0 ? 0 : list.Count - 1); step > 0 ? i < stop : i > stop; i += step) result.Add(list[(int)i]);
                return List(result);
            }
            case "function": return Function(Text(node["function"]),Array(node["arguments"]),input);
            default: throw new ArgumentException("Unknown compiled JMESPath node");
        }
    }
    private static List<JsonNode?> Ordered(JsonNode? value)
    {
        var result = Array(value).ToList();
        foreach (var item in result) { if (Numeric(result[0])) _ = Number(item); else _ = Text(item); }
        return result.OrderBy(item => item, Comparer<JsonNode?>.Create(Compare)).ToList();
    }
    private static JsonNode? Reference(JsonArray nodes, int index, JsonNode? input)
    {
        var node = nodes[index]!; if (Text(node["kind"]) != "expressionReference") throw new ArgumentException("JMESPath expected expression reference");
        return Evaluate(node["expression"]!,input);
    }
    // Arithmetic expands only bounded decimal exponents; comparisons never expand them.
    private static (BigInteger Numerator, BigInteger Denominator) Rational(JsonNode? value)
    {
        var (coefficient, exponent) = Number(value).Normalize();
        if (BigInteger.Abs(exponent) > 1_000_000) throw new ArgumentOutOfRangeException(nameof(value), "JMESPath arithmetic exponent exceeds supported precision");
        return exponent.Sign >= 0 ? (coefficient * BigInteger.Pow(10,(int)exponent),BigInteger.One) : (coefficient,BigInteger.Pow(10,(int)-exponent));
    }
    private static JsonNode Quotient(BigInteger numerator, BigInteger denominator, int precision = 34)
    {
        if (numerator.IsZero) return Integer(0);
        var whole = BigInteger.DivRem(numerator,denominator,out var remainder);
        if (remainder.IsZero) return Integer(whole);
        var digits = new StringBuilder(); remainder = BigInteger.Abs(remainder);
        for (int i = 0; i < precision && !remainder.IsZero; i++) { remainder *= 10; digits.Append(BigInteger.DivRem(remainder,denominator,out remainder).ToString(CultureInfo.InvariantCulture)); }
        return JsonNode.Parse((numerator.Sign < 0 && whole.IsZero ? "-" : "") + whole.ToString(CultureInfo.InvariantCulture) + "." + digits)!;
    }
    private static JsonNode? Function(string name, JsonArray nodes, JsonNode? input)
    {
        bool variadic = name is "merge" or "not_null";
        int arity = name is "contains" or "starts_with" or "ends_with" or "join" or "map" or "max_by" or "min_by" or "sort_by" ? 2 : 1;
        if (variadic ? nodes.Count == 0 : nodes.Count != arity) throw new ArgumentException("JMESPath invalid function arity");
        JsonNode? Arg(int index) => Evaluate(nodes[index]!,input);
        switch (name)
        {
            case "abs": { var value = Number(Arg(0)); return JsonNode.Parse(value.ToString().TrimStart('-')); }
            case "ceil": case "floor": { var (n,d) = Rational(Arg(0)); var q = BigInteger.DivRem(n,d,out var remainder); if (!remainder.IsZero && (name == "ceil" ? n.Sign > 0 : n.Sign < 0)) q += name == "ceil" ? 1 : -1; return Integer(q); }
            case "sum": case "avg": {
                var items = Array(Arg(0)); BigInteger n = 0, d = 1;
                foreach (var item in items) { var (next,denom) = Rational(item); n = n * denom + next * d; d *= denom; var divisor = BigInteger.GreatestCommonDivisor(n,d); n /= divisor; d /= divisor; }
                return name == "sum" ? Quotient(n,d,1_000_000) : items.Count == 0 ? null : Quotient(n,d*items.Count);
            }
            case "contains": { var source = Arg(0); var wanted = Arg(1); return Value(source?.GetValueKind() == JsonValueKind.String ? Text(source).Contains(Text(wanted),StringComparison.Ordinal) : Array(source).Any(item => Equal(item,wanted))); }
            case "starts_with": return Value(Text(Arg(0)).StartsWith(Text(Arg(1)),StringComparison.Ordinal));
            case "ends_with": return Value(Text(Arg(0)).EndsWith(Text(Arg(1)),StringComparison.Ordinal));
            case "join": return Value(string.Join(Text(Arg(0)),Array(Arg(1)).Select(Text)));
            case "keys": return List(Object(Arg(0)).Select(entry => Value(entry.Key)));
            case "values": return List(Object(Arg(0)).Select(entry => entry.Value));
            case "length": { var source = Arg(0); return Integer(source is JsonArray a ? a.Count : source is JsonObject o ? o.Count : Text(source).EnumerateRunes().Count()); }
            case "map": return List(Array(Arg(1)).Select(item => Reference(nodes,0,item)));
            case "max": case "min": case "sort": { var items = Ordered(Arg(0)); return name == "sort" ? List(items) : items.Count == 0 ? null : items[name == "max" ? ^1 : 0]; }
            case "max_by": case "min_by": case "sort_by": {
                var items = Array(Arg(0)); var keys = items.Select(item => Reference(nodes,1,item)).ToArray(); _ = Ordered(List(keys));
                if (items.Count == 0) return name == "sort_by" ? new JsonArray() : null;
                if (name == "sort_by") return List(Enumerable.Range(0,items.Count).OrderBy(i => keys[i],Comparer<JsonNode?>.Create(Compare)).Select(i => items[i]));
                int selected = 0; for (int i = 1; i < items.Count; i++) if (name == "max_by" ? Compare(keys[i],keys[selected]) > 0 : Compare(keys[i],keys[selected]) < 0) selected = i;
                return items[selected];
            }
            case "merge": { var result = new JsonObject(); for (int i = 0; i < nodes.Count; i++) foreach (var entry in Object(Arg(i))) result[entry.Key] = entry.Value?.DeepClone(); return result; }
            case "not_null": { for (int i = 0; i < nodes.Count; i++) if (Arg(i) is JsonNode result) return result; return null; }
            case "reverse": { var source = Arg(0); return source is JsonArray list ? List(list.Reverse()) : Value(string.Concat(Text(source).EnumerateRunes().Reverse().Select(rune => rune.ToString()))); }
            case "to_array": { var source = Arg(0); return source is JsonArray ? source : List([source]); }
            case "to_number": { var source = Arg(0); if (Numeric(source)) return source; if (source?.GetValueKind() == JsonValueKind.String) try { return JsonNode.Parse(new BigDecimal(Text(source)).ToString()); } catch (FormatException) { } return null; }
            case "to_string": { var source = Arg(0); return source?.GetValueKind() == JsonValueKind.String ? source : Value(source?.ToJsonString() ?? "null"); }
            case "type": { var source = Arg(0); return Value(source is null ? "null" : source is JsonArray ? "array" : source is JsonObject ? "object" : source.GetValueKind() == JsonValueKind.String ? "string" : source.GetValueKind() is JsonValueKind.True or JsonValueKind.False ? "boolean" : "number"); }
            default: throw new ArgumentException("Unknown compiled JMESPath function");
        }
    }
}
