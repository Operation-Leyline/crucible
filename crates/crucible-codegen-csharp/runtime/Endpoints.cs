using System.Net;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;

internal static class Endpoints
{
    private static string Text(JsonNode? value) => value?.GetValue<string>() ?? throw new ArgumentException("Endpoint string required");
    private static bool Boolean(JsonNode? value) => value?.GetValue<bool>() ?? throw new ArgumentException("Endpoint boolean required");
    private static readonly Regex Label = new("\\A[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?\\z",RegexOptions.NonBacktracking|RegexOptions.CultureInvariant);
    internal static bool HostLabel(string value, bool subdomains = false) => (subdomains ? value.Split('.') : [value]).All(label => Label.IsMatch(label));
    internal static JsonObject? ParseUrl(string value)
    {
        if (value.Any(ch => ch <= 32 || ch == 127) || value.Contains('?') || value.Contains('#') ||
            !Uri.TryCreate(value,UriKind.Absolute,out var uri) || uri.Scheme is not ("http" or "https") || uri.UserInfo.Length != 0 || uri.Host.Length == 0) return null;
        bool ip = IPAddress.TryParse(uri.Host.Trim('[',']'),out _);
        if (!ip && !HostLabel(uri.Host.TrimEnd('.'),true)) return null;
        var path = uri.AbsolutePath;
        return new() { ["scheme"] = uri.Scheme, ["authority"] = uri.Authority, ["path"] = path, ["normalizedPath"] = path.EndsWith('/') ? path : path + "/", ["isIp"] = ip };
    }
    private static JsonNode? Attribute(JsonNode? value, JsonNode path)
    {
        JsonArray steps;
        if (path is JsonArray array) steps = array;
        else
        {
            steps = [];
            foreach (var part in Text(path).Split('.'))
            {
                int marker = part.IndexOf('['); var key = marker < 0 ? part : part[..marker];
                if (key.Length != 0) steps.Add(new JsonObject { ["kind"] = "member", ["name"] = key });
                if (marker >= 0) steps.Add(new JsonObject { ["kind"] = "index", ["index"] = int.Parse(part[(marker+1)..^1],System.Globalization.CultureInfo.InvariantCulture) });
            }
        }
        foreach (var step in steps)
        {
            if (Text(step!["kind"]) == "member") value = value is JsonObject obj ? obj[Text(step["name"])] : null;
            else if (value is JsonArray list) { long index = step["index"]!.GetValue<int>(); if (index < 0) index += list.Count; value = index >= 0 && index < list.Count ? list[(int)index] : null; }
            else value = null;
            if (value is null) break;
        }
        return value;
    }
    private static JsonNode? Function(string name, JsonNode?[] args) => name switch
    {
        "isSet" => JsonValue.Create(args[0] is not null),
        "not" => JsonValue.Create(!Boolean(args[0])),
        "booleanEquals" => JsonValue.Create(args.All(value => value?.GetValueKind() is JsonValueKind.True or JsonValueKind.False) && Boolean(args[0]) == Boolean(args[1])),
        "stringEquals" => JsonValue.Create(args.All(value => value?.GetValueKind() == JsonValueKind.String) && Text(args[0]) == Text(args[1])),
        "getAttr" => Attribute(args[0],args[1]!),
        "substring" => Substring(args),
        "isValidHostLabel" => JsonValue.Create(HostLabel(Text(args[0]),Boolean(args[1]))),
        "parseUrl" => ParseUrl(Text(args[0])),
        "uriEncode" => JsonValue.Create(Uri.EscapeDataString(Text(args[0]))),
        _ => throw new ArgumentException("Unknown endpoint function")
    };
    private static JsonNode? Substring(JsonNode?[] args)
    {
        var text = Text(args[0]); long start = args[1]!.GetValue<long>(), end = args[2]!.GetValue<long>();
        if (text.Any(ch => ch > 127) || start < 0 || end < start || end > text.Length) return null;
        return JsonValue.Create(Boolean(args[3]) ? text[(int)(text.Length-end)..(int)(text.Length-start)] : text[(int)start..(int)end]);
    }
    private static JsonNode? Expression(JsonNode node, Dictionary<string,JsonNode?> values)
    {
        switch (Text(node["kind"]))
        {
            case "reference": return values.GetValueOrDefault(Text(node["name"]));
            case "boolean": case "integer": return node["value"];
            case "array": return new JsonArray(node["values"]!.AsArray().Select(item => Expression(item!,values)?.DeepClone()).ToArray());
            case "object": { var result = new JsonObject(); foreach (var (key,item) in node["values"]!.AsObject()) result[key] = Expression(item!,values)?.DeepClone(); return result; }
            case "call": return Function(Text(node["function"]),node["arguments"]!.AsArray().Select(item => Expression(item!,values)).ToArray());
            case "string": {
                var text = new StringBuilder();
                foreach (var part in node["parts"]!.AsArray())
                {
                    var value = Text(part!["kind"]) == "literal" ? part["value"] : Attribute(values.GetValueOrDefault(Text(part["name"])),part["path"]!);
                    if (value is null || value is JsonArray or JsonObject) throw new ArgumentException("Endpoint template references missing or non-scalar value");
                    text.Append(value.GetValueKind() == JsonValueKind.String ? Text(value) : value.ToJsonString());
                }
                return JsonValue.Create(text.ToString());
            }
            default: throw new ArgumentException("Unknown endpoint expression");
        }
    }
    private static JsonObject? Rules(JsonArray nodes, Dictionary<string,JsonNode?> parameters)
    {
        foreach (var rule in nodes)
        {
            var scope = new Dictionary<string,JsonNode?>(parameters); bool matched = true;
            foreach (var condition in rule!["conditions"]!.AsArray())
            {
                var value = Function(Text(condition!["function"]),condition["arguments"]!.AsArray().Select(item => Expression(item!,scope)).ToArray());
                if (value is null || value.GetValueKind() == JsonValueKind.False) { matched = false; break; }
                if (condition["assign"] is JsonNode assign) scope[Text(assign)] = value;
            }
            if (!matched) continue;
            var result = rule["result"]!;
            if (Text(result["kind"]) == "tree") { var nested = Rules(result["rules"]!.AsArray(),scope); if (nested is not null) return nested; }
            else if (Text(result["kind"]) == "error") throw new ArgumentException(Text(Expression(result["message"]!,scope)));
            else
            {
                var url = Text(Expression(result["url"]!,scope)); if (ParseUrl(url) is null) throw new ArgumentException("Endpoint rule returned invalid URL");
                var headers = new JsonObject(); var properties = new JsonObject();
                foreach (var (key,entries) in result["headers"]!.AsObject()) headers[key] = new JsonArray(entries!.AsArray().Select(item => Expression(item!,scope)?.DeepClone()).ToArray());
                foreach (var (key,item) in result["properties"]!.AsObject()) properties[key] = Expression(item!,scope)?.DeepClone();
                return new() { ["url"] = url, ["headers"] = headers, ["properties"] = properties };
            }
        }
        return null;
    }
    internal static JsonObject Resolve(JsonNode rules, JsonNode? sources, JsonNode? input, IReadOnlyDictionary<string,JsonNode?> client, IReadOnlyDictionary<string,JsonNode?> builtins)
    {
        var parameters = new Dictionary<string,JsonNode?>();
        foreach (var (name,parameter) in rules["parameters"]!.AsObject())
        {
            JsonNode? value = null;
            foreach (var source in sources?[name]?.AsArray() ?? new JsonArray())
            {
                value = Text(source!["kind"]) switch {
                    "static" or "default" => source["value"],
                    "inputMember" => input is JsonObject obj ? obj[Text(source["member"])] : null,
                    "inputPath" => Query.Evaluate(source["path"]!["expression"]!,input),
                    "client" => client.GetValueOrDefault(Text(source["name"])),
                    "builtIn" => builtins.GetValueOrDefault(Text(source["name"])),
                    _ => throw new ArgumentException("Unknown endpoint parameter source")
                };
                if (value is not null) break;
            }
            if (value is null && parameter!["required"]!.GetValue<bool>()) throw new ArgumentException("Required endpoint parameter missing: " + name);
            if (value is not null)
            {
                bool valid = Text(parameter!["kind"]) switch { "string" => value.GetValueKind() == JsonValueKind.String, "boolean" => value.GetValueKind() is JsonValueKind.True or JsonValueKind.False, "stringArray" => value is JsonArray list && list.All(item => item?.GetValueKind() == JsonValueKind.String), _ => false };
                if (!valid) throw new ArgumentException("Wrong endpoint parameter type: " + name);
            }
            parameters[name] = value;
        }
        return Rules(rules["rules"]!.AsArray(),parameters) ?? throw new ArgumentException("No endpoint rule matched");
    }
    internal static string HostPrefix(string endpoint, JsonNode description, JsonNode? input)
    {
        var prefix = new StringBuilder();
        foreach (var part in description["parts"]!.AsArray())
        {
            if (Text(part!["kind"]) == "literal") prefix.Append(Text(part["value"]));
            else { var value = input is JsonObject obj ? obj[Text(part["member"])] : null; if (value?.GetValueKind() != JsonValueKind.String || !HostLabel(Text(value))) throw new ArgumentException("Invalid endpoint host label"); prefix.Append(Text(value)); }
        }
        var uri = new UriBuilder(endpoint); var host = prefix + uri.Host;
        if (host.Length > 253 || !HostLabel(host,true)) throw new ArgumentException("Invalid expanded endpoint hostname");
        uri.Host = host; return uri.Uri.AbsoluteUri.TrimEnd('/');
    }
}
