using System.Collections;
using System.Globalization;
using System.Numerics;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.ExceptionServices;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

internal sealed partial class ClientRuntime : IDisposable
{
    private readonly ClientConfig config;
    private readonly HttpClient http;
    private readonly bool ownsHttp;
    private readonly bool digest;
    private readonly JsonObject schema;
    private readonly IReadOnlyDictionary<string, Type> types;
    private static readonly JsonSerializerOptions JsonOptions = new() { Encoder = System.Text.Encodings.Web.JavaScriptEncoder.UnsafeRelaxedJsonEscaping };

    internal ClientRuntime(ClientConfig input, string endpoint, bool digest)
        : this(input, endpoint, digest, LoadSchema(), ModelRegistry.Types) { }

    internal ClientRuntime(ClientConfig input, string endpoint, bool digest, JsonObject schema, IReadOnlyDictionary<string, Type> types)
    {
        endpoint = input.Endpoint ?? endpoint;
        if (!Enum.IsDefined(input.RequestChecksumCalculation) || !Enum.IsDefined(input.ResponseChecksumValidation)) throw new ArgumentException("Invalid checksum policy");
        if (!(endpoint.Length == 0 && schema["endpointRules"] is not null) && (!Uri.TryCreate(endpoint, UriKind.Absolute, out var uri) ||
            (uri.Scheme != "http" && uri.Scheme != "https") || uri.Host.Length == 0 ||
            uri.UserInfo.Length != 0 || uri.Query.Length != 0 || uri.Fragment.Length != 0))
            throw new ArgumentException("Absolute HTTP(S) endpoint without credentials, query, or fragment required");
        if (input.SigV4aSigningRegionSet is { } regions && (regions.Count == 0 || regions.Any(region => string.IsNullOrEmpty(region) || region.IndexOfAny([',', '/', '\r', '\n']) >= 0)))
            throw new ArgumentException("The SigV4a signing region set must be a nonempty list of regions");
        if (input.MaxAttempts < 1 || input.Timeout <= TimeSpan.Zero || input.Timeout.TotalMilliseconds > uint.MaxValue - 1 ||
            input.BaseDelay < TimeSpan.Zero || input.MaxDelay < TimeSpan.Zero || input.MaxDelay.TotalMilliseconds > int.MaxValue)
            throw new ArgumentException("Invalid attempt limit, timeout, or retry delay");
        config = new ClientConfig {
            RequestChecksumCalculation = input.RequestChecksumCalculation, ResponseChecksumValidation = input.ResponseChecksumValidation, ChecksumValidated = input.ChecksumValidated,
            Endpoint = endpoint.TrimEnd('/'), Token = input.Token, MaxAttempts = input.MaxAttempts,
            BasicCredentials = input.BasicCredentials, ApiKey = input.ApiKey, Credentials = input.Credentials, Region = input.Region,
            DigestCredentials = input.DigestCredentials, SigV4aSigningRegionSet = input.SigV4aSigningRegionSet?.ToArray(),
            EndpointParameters = input.EndpointParameters.ToDictionary(entry => entry.Key,entry => entry.Value?.DeepClone()),
            EndpointBuiltIns = input.EndpointBuiltIns.ToDictionary(entry => entry.Key,entry => entry.Value?.DeepClone()), DisableHostPrefix = input.DisableHostPrefix,
            Timeout = input.Timeout, BaseDelay = input.BaseDelay, MaxDelay = input.MaxDelay,
            Headers = new(input.Headers, StringComparer.OrdinalIgnoreCase), Middleware = [.. input.Middleware],
            RetryMode = input.RetryMode, EndpointDiscovery = input.EndpointDiscovery, Telemetry = input.Telemetry,
            TimeProvider = input.TimeProvider ?? throw new ArgumentException("A TimeProvider is required")
        };
        ownsHttp = input.HttpClient is null;
        http = input.HttpClient ?? new HttpClient(new SocketsHttpHandler { AllowAutoRedirect = false }) { Timeout = System.Threading.Timeout.InfiniteTimeSpan };
        this.digest = digest; this.schema = schema; this.types = types;
        InitializePolicy();
    }
    public void Dispose() { if (ownsHttp) http.Dispose(); }
    private static JsonObject LoadSchema()
    {
        using var stream = typeof(ClientRuntime).Assembly.GetManifestResourceStream("crucible.schema.json")
            ?? throw new InvalidOperationException("SDK schema resource missing");
        return JsonNode.Parse(stream)!.AsObject();
    }
    private static string? Text(JsonNode? value) => value is null ? null : value.GetValue<string>();
    private static bool Flag(JsonNode? value) => value?.GetValue<bool>() == true;
    private JsonObject Shape(string id) => schema["shapes"]![id]!.AsObject();
    private string ErrorCode(string id) => Text(Shape(id)["error"]?["wireCode"]) ?? Text(Shape(id)["name"])!;
    // RPC v2 CBOR carries non-finite floats natively; the wire values bridge them as these strings.
    private bool ProtocolFlag(string name) => Flag(schema["protocolSettings"]?[name]) || name == "nonFiniteFloatStrings" && Text(schema["protocol"]) == "rpcv2Cbor";
    private bool AwsJson => Text(schema["protocol"]) is "awsRestJson1" or "awsJson1_0" or "awsJson1_1";
    private static double SpecialFloat(string value) => value switch
    {
        "NaN" => double.NaN, "Infinity" => double.PositiveInfinity, "-Infinity" => double.NegativeInfinity,
        _ => throw new JsonException("Invalid floating point string")
    };
    private string WireName(string? target, string name) => target is null ? name : Text(Shape(target)["members"]![name]?["jsonName"]) ?? name;
    private static PropertyInfo ModelProperty(Type type, string name) {
        var presence = type.GetProperty(name + "Presence");
        return presence?.PropertyType.IsGenericType == true && presence.PropertyType.GetGenericTypeDefinition() == typeof(Optional<>) ? presence : type.GetProperty(name)!;
    }
    private static object New(Type type) => Activator.CreateInstance(type)!;
    private bool Streaming(JsonNode member) => Flag(Shape(Text(member["target"])!)["streaming"]);
    private static long? LengthBound(JsonNode member, string key) => member["length"]?[key]?.GetValue<long>();
    private void CheckElement(JsonNode member, object? value)
    {
        if (value is null)
            throw new JsonException("Dense collections cannot contain null elements");
    }

    internal JsonNode? Encode(string? target, object? value, string? format = null)
    {
        if (target is null) return new JsonObject();
        var shape = Shape(target); var kind = Text(shape["kind"]);
        if (kind == "unit") return new JsonObject();
        if (value is null) return null;
        switch (kind)
        {
            case "float":
            case "double":
                var number = Convert.ToDouble(value, CultureInfo.InvariantCulture);
                if (ProtocolFlag("nonFiniteFloatStrings") && !double.IsFinite(number))
                    return JsonValue.Create(double.IsNaN(number) ? "NaN" : number > 0 ? "Infinity" : "-Infinity");
                return JsonSerializer.SerializeToNode(value, value.GetType(), JsonOptions);
            case "blob": return JsonValue.Create(Convert.ToBase64String((byte[])value));
            case "timestamp": return WriteTime((DateTimeOffset)value, format ?? Text(shape["format"]));
            case "enum": return JsonValue.Create(value.ToString());
            case "intEnum": return JsonValue.Create((int)value.GetType().GetProperty("Value")!.GetValue(value)!);
            // RPC v2 JSON carries arbitrary-precision numbers as strings.
            case "bigInteger": return ProtocolFlag("arbitraryPrecisionStrings") ? JsonValue.Create(((BigInteger)value).ToString(CultureInfo.InvariantCulture)) : JsonNode.Parse(((BigInteger)value).ToString(CultureInfo.InvariantCulture));
            case "bigDecimal": return ProtocolFlag("arbitraryPrecisionStrings") ? JsonValue.Create(((BigDecimal)value).ToString()) : JsonNode.Parse(((BigDecimal)value).ToString());
            case "document": return ((JsonNode)value).DeepClone();
            case "list":
                var array = new JsonArray(); var item = shape["member"]!;
                foreach (var element in (IEnumerable)value) {
                    if (!Flag(shape["sparse"])) CheckElement(item, element);
                    var encoded = Encode(Text(item["target"]), element, Text(item["format"]));
                    if (Flag(shape["set"]) && array.Any(existing => WireEquals(existing, encoded))) throw new JsonException("Duplicate element in set");
                    array.Add(encoded);
                }
                return array;
            case "map":
                var map = new JsonObject(); var member = shape["value"]!;
                foreach (DictionaryEntry entry in (IDictionary)value)
                { if (!Flag(shape["sparse"])) CheckElement(member, entry.Value); map.Add(entry.Key.ToString()!, Encode(Text(member["target"]), entry.Value, Text(member["format"]))); }
                return map;
            case "structure":
            case "union":
                var result = new JsonObject();
                foreach (var (name, description) in shape["members"]!.AsObject())
                {
                    // A stream travels beside the JSON values, as the HTTP payload itself.
                    if (Streaming(description!)) continue;
                    var v = ModelProperty(value.GetType(), Text(description!["attribute"])!).GetValue(value);
                    if (v is IOptional optional) { if (!optional.IsSet) continue; v = optional.Untyped; }
                    if (kind == "structure" && v is null && ProtocolFlag("omitNullStructureMembers") && Text(Shape(Text(description["target"])!)["kind"]) != "document") continue;
                    result.Add(WireName(target, name), Encode(Text(description["target"]), v, Text(description["format"])));
                }
                if (kind == "union")
                {
                    var unknown = (UnknownUnion?)value.GetType().GetProperty("UnknownVariant")!.GetValue(value);
                    if (unknown is not null)
                    {
                        if (shape["members"]!.AsObject().Any(entry => WireName(target, entry.Key) == unknown.Name)) throw new ArgumentException("Unknown union name is declared");
                        result.Add(unknown.Name, unknown.Value?.DeepClone());
                    }
                    if (result.Count != 1) throw new ArgumentException("A union must contain exactly one member");
                }
                return result;
            default: return JsonSerializer.SerializeToNode(value, value.GetType(), JsonOptions);
        }
    }

    internal object? Decode(string? target, JsonNode? value, string? format = null)
    {
        if (target is null) return new Unit();
        if (value is null) return null;
        var shape = Shape(target); var kind = Text(shape["kind"]);
        switch (kind)
        {
            case "unit": if (value.AsObject().Count != 0) throw new JsonException("Unit requires an empty object"); return new Unit();
            case "string": return value.GetValue<string>();
            case "blob": return Convert.FromBase64String(value.GetValue<string>());
            case "boolean": return value.GetValue<bool>();
            case "byte": return checked((sbyte)Integer(value));
            case "short": return checked((short)Integer(value));
            case "integer": return checked((int)Integer(value));
            case "long": return checked((long)Integer(value));
            case "float":
                if (ProtocolFlag("nonFiniteFloatStrings") && value.GetValueKind() == JsonValueKind.String) return (float)SpecialFloat(value.GetValue<string>());
                var f = value.GetValue<float>(); if (!float.IsFinite(f)) throw new JsonException("Float overflow"); return f;
            case "double":
                if (ProtocolFlag("nonFiniteFloatStrings") && value.GetValueKind() == JsonValueKind.String) return SpecialFloat(value.GetValue<string>());
                var d = value.GetValue<double>(); if (!double.IsFinite(d)) throw new JsonException("Double overflow"); return d;
            case "timestamp": return ReadTime(value, format ?? Text(shape["format"]));
            case "document": return value.DeepClone();
            case "enum": return Activator.CreateInstance(types[target], value.GetValue<string>());
            case "intEnum": return Activator.CreateInstance(types[target], checked((int)Integer(value)));
            case "bigInteger":
                if (ProtocolFlag("arbitraryPrecisionStrings"))
                {
                    var digits = value.GetValueKind() == JsonValueKind.String ? value.GetValue<string>() : throw new JsonException("Expected arbitrary precision integer string");
                    if (!System.Text.RegularExpressions.Regex.IsMatch(digits, @"\A-?(?:0|[1-9][0-9]*)\z")) throw new JsonException("Invalid arbitrary precision integer");
                    return BigInteger.Parse(digits, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture);
                }
                if (value.GetValueKind() != JsonValueKind.Number) throw new JsonException("Expected arbitrary precision integer");
                return BigInteger.Parse(value.ToJsonString(), NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture);
            case "bigDecimal":
                if (ProtocolFlag("arbitraryPrecisionStrings"))
                    return new BigDecimal(value.GetValueKind() == JsonValueKind.String ? value.GetValue<string>() : throw new JsonException("Expected arbitrary precision decimal string"));
                if (value.GetValueKind() != JsonValueKind.Number) throw new JsonException("Expected arbitrary precision decimal");
                return new BigDecimal(value.ToJsonString());
            case "list":
                var list = (IList)New(types[target]); var member = shape["member"]!;
                var seen = new List<JsonNode?>();
                foreach (var item in value.AsArray()) {
                    if (!Flag(shape["sparse"])) CheckElement(member, item);
                    if (Flag(shape["set"]) && seen.Any(existing => WireEquals(existing, item))) throw new JsonException("Duplicate element in set");
                    seen.Add(item); list.Add(Decode(Text(member["target"]), item, Text(member["format"])));
                }
                return list;
            case "map":
                var map = (IDictionary)New(types[target]); var val = shape["value"]!; var key = shape["key"]!;
                foreach (var (name, item) in value.AsObject())
                { if (!Flag(shape["sparse"])) CheckElement(val, item); map.Add(Decode(Text(key["target"]), JsonValue.Create(name))!, Decode(Text(val["target"]), item, Text(val["format"]))); }
                return map;
            case "structure":
            case "union":
                var result = New(types[target]); var obj = value.AsObject(); var members = shape["members"]!.AsObject();
                if (kind == "union")
                {
                    if (ProtocolFlag("ignoreUnknownUnionType") && !members.Any(entry => WireName(target, entry.Key) == "__type"))
                    { obj = (JsonObject)obj.DeepClone(); obj.Remove("__type"); }
                    if (obj.Count != 1) throw new JsonException("A union must contain exactly one member");
                    var variant = obj.First();
                    if (!members.Any(entry => WireName(target, entry.Key) == variant.Key))
                    {
                        result.GetType().GetProperty("UnknownVariant")!.SetValue(result, new UnknownUnion(variant.Key, variant.Value?.DeepClone()));
                        return result;
                    }
                }
                foreach (var (name, description) in members)
                {
                    // A response stream is attached after decoding, from the body itself.
                    if (Streaming(description!)) continue;
                    var property = ModelProperty(result.GetType(), Text(description!["attribute"])!);
                    bool optional = property.PropertyType.IsGenericType && property.PropertyType.GetGenericTypeDefinition() == typeof(Optional<>);
                    bool present = obj.TryGetPropertyValue(WireName(target, name), out var item);
                    if (kind == "structure" && item is null && ProtocolFlag("omitNullStructureMembers") && Text(Shape(Text(description["target"])!)["kind"]) != "document") present = false;
                    if (!present && description!["default"] is JsonNode defaultValue) { item = defaultValue; present = true; }
                    // The generated Optional type carries the IR's client nullability.
                    if (!optional && (!present || item is null && Text(Shape(Text(description!["target"])!)["kind"]) != "document"))
                        throw new JsonException($"Missing or null non-optional member {name}");
                    if (!present) continue;
                    var decoded = Decode(Text(description!["target"]), item, Text(description["format"]));
                    if (optional)
                        decoded = property.PropertyType.GetMethod("FromWire", BindingFlags.Static | BindingFlags.NonPublic)!.Invoke(null, [decoded]);
                    else if (decoded is null && property.PropertyType.IsValueType) throw new JsonException($"Null for non-nullable member {name}");
                    property.SetValue(result, decoded);
                }
                return result;
            default: throw new JsonException($"Unknown wire kind {kind}");
        }
    }
    private static bool WireEquals(JsonNode? left, JsonNode? right)
    {
        if (left?.GetValueKind() == JsonValueKind.Number && right?.GetValueKind() == JsonValueKind.Number)
            return new BigDecimal(left.ToJsonString()) == new BigDecimal(right.ToJsonString());
        if (left is JsonArray a && right is JsonArray b) return a.Count == b.Count && a.Zip(b).All(pair => WireEquals(pair.First, pair.Second));
        if (left is JsonObject x && right is JsonObject y) return x.Count == y.Count && x.All(pair => y.ContainsKey(pair.Key) && WireEquals(pair.Value, y[pair.Key]));
        return JsonNode.DeepEquals(left, right);
    }
    private static decimal Integer(JsonNode value)
    {
        var number = value.GetValue<decimal>();
        if (decimal.Truncate(number) != number) throw new JsonException("Expected integer");
        return number;
    }
    private static JsonNode WriteTime(DateTimeOffset value, string? format) => format switch
    {
        "epoch-seconds" => JsonValue.Create((value.UtcTicks - DateTimeOffset.UnixEpoch.UtcTicks) / (decimal)TimeSpan.TicksPerSecond)!,
        "http-date" => JsonValue.Create(value.UtcDateTime.ToString("R", CultureInfo.InvariantCulture))!,
        _ => JsonValue.Create(value.UtcDateTime.ToString("yyyy-MM-dd'T'HH:mm:ss.FFFFFFF'Z'", CultureInfo.InvariantCulture))!
    };
    private static DateTimeOffset ReadTime(JsonNode value, string? format)
    {
        if (format == "epoch-seconds") return DateTimeOffset.UnixEpoch.AddTicks(checked((long)decimal.Truncate(value.GetValue<decimal>() * TimeSpan.TicksPerSecond)));
        return DateTimeOffset.Parse(value.GetValue<string>(), CultureInfo.InvariantCulture, DateTimeStyles.AssumeUniversal).ToUniversalTime();
    }
    private static string Scalar(JsonNode value) => value.GetValueKind() == JsonValueKind.String ? value.GetValue<string>() : value.ToJsonString(JsonOptions);
    internal static string Bound(JsonNode value)
    {
        if (value is not JsonArray list) return Scalar(value);
        return string.Join(", ", list.Select(item => {
            var text = Scalar(item!);
            return text.Length == 0 || text.IndexOfAny([',', '"', '\\']) >= 0 || text.Trim() != text ? JsonSerializer.Serialize(text, JsonOptions) : text;
        }));
    }
    private JsonNode? BoundMember(JsonNode member, JsonNode? value, string? format)
    {
        if (value is null || format is null) return value?.DeepClone();
        var shape = Shape(Text(member["target"])!);
        if (Text(shape["kind"]) == "timestamp") return WriteTime(ReadTime(value, Text(member["format"])), format);
        if (Text(shape["kind"]) == "list") return new JsonArray(value.AsArray().Select(item => BoundMember(shape["member"]!, item, format)).ToArray());
        return value.DeepClone();
    }
    internal JsonNode? Header(JsonNode member, string value, string? format = null)
    {
        var shape = Shape(Text(member["target"])!); var kind = Text(shape["kind"]);
        if (kind == "list")
        {
            var result = new JsonArray(); if (string.IsNullOrWhiteSpace(value)) return result;
            var parts = new List<string>(); int start = 0; bool quoted = false, escaped = false;
            for (int i = 0; i < value.Length; i++)
            {
                var c = value[i];
                if (escaped) escaped = false;
                else if (c == '\\' && quoted) escaped = true;
                else if (c == '"') quoted = !quoted;
                else if (c == ',' && !quoted) { parts.Add(value[start..i].Trim()); start = i + 1; }
            }
            if (quoted) throw new JsonException("Unterminated quoted header");
            parts.Add(value[start..].Trim());
            foreach (var part in parts) result.Add(Header(shape["member"]!, part.StartsWith('"') ? JsonSerializer.Deserialize<string>(part)! : part, format));
            return result;
        }
        if (kind == "timestamp") {
            format ??= Text(member["format"]);
            var raw = format == "epoch-seconds" ? JsonNode.Parse(value)! : JsonValue.Create(value)!;
            return WriteTime(ReadTime(raw, format), Text(member["format"]));
        }
        if (kind is "byte" or "short" or "integer" or "long" or "float" or "double" or "boolean" or "intEnum" or "bigInteger" or "bigDecimal") return JsonNode.Parse(value);
        return JsonValue.Create(value);
    }
    private sealed record Prepared(JsonObject Operation, Uri Uri, Dictionary<string, string> Headers, byte[]? Body, string Name, ChecksumSettings Checksums,
        ByteStream? Stream = null, long? MinLength = null, long? MaxLength = null)
    {
        /// <summary>The endpoint the URI was built from, and where it came from: configured or rules.</summary>
        public string EndpointUrl { get; init; } = "";
        public string EndpointSource { get; init; } = "configured";
        /// <summary>The path and query appended to the endpoint, so a discovered endpoint can replace it.</summary>
        public string Target { get; init; } = "";
        /// <summary>The path with sensitive labels redacted, for telemetry.</summary>
        public string DisplayPath { get; init; } = "";
        public HashSet<string> SensitiveHeaders { get; init; } = [];
        public HashSet<string> SensitiveQuery { get; init; } = [];
        public DiscoveryRequest? Discovery { get; init; }
    }
    private static readonly UTF8Encoding StrictUtf8 = new(false, true);
    private static JsonNode? MediaValue(JsonNode? value, bool decode)
    {
        if (value is JsonArray list) return new JsonArray(list.Select(item => MediaValue(item, decode)).ToArray());
        var text = value!.GetValue<string>();
        return JsonValue.Create(decode ? StrictUtf8.GetString(Convert.FromBase64String(text)) : Convert.ToBase64String(StrictUtf8.GetBytes(text)));
    }
    private static JsonNode? PayloadValue(string? kind, byte[] bytes) => kind switch
    {
        "string" => JsonValue.Create(StrictUtf8.GetString(bytes)),
        "document" => bytes.Length == 0 ? null : JsonNode.Parse(bytes),
        _ => JsonValue.Create(Convert.ToBase64String(bytes))
    };
    private void ResponseBindings(string target, JsonNode bindings, int status, Dictionary<string, string> headers, JsonObject body)
    {
        foreach (var binding in bindings["prefixHeaders"]?.AsArray() ?? new JsonArray())
        {
            var prefix = Text(binding![1])!;
            var entries = new JsonObject();
            foreach (var (name, value) in headers)
                if (name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)) entries[name[prefix.Length..]] = Text(bindings["headerEncodings"]?[Text(binding[0])!]) == "base64" ? MediaValue(JsonValue.Create(value),true) : JsonValue.Create(value);
            body[WireName(target, Text(binding[0])!)] = entries;
        }
        if (Text(bindings["responseCode"]) is string code) body[WireName(target, code)] = (decimal)status;
        foreach (var binding in bindings["headers"]!.AsArray())
        {
            var name = Text(binding![0])!;
            if (headers.TryGetValue(Text(binding[1])!, out var value))
            {
                var decoded = Header(Shape(target)["members"]![name]!, value, Text(bindings["timestampFormats"]?[name]));
                body[WireName(target, name)] = Text(bindings["headerEncodings"]?[name]) == "base64" ? MediaValue(decoded, true) : decoded;
            }
        }
    }
    private Prepared Prepare(string name, JsonObject values, ByteStream? stream = null)
    {
        var op = schema["operations"]![name]!.AsObject(); var request = op["request"]!;
        var queryInput = schema["endpointRules"] is not null || op["endpoint"] is not null ? QueryModel(Text(op["input"]),values) : null;
        var resolved = new JsonObject { ["url"] = config.Endpoint, ["headers"] = new JsonObject(), ["properties"] = new JsonObject() };
        if (schema["endpointRules"] is JsonNode rules)
        {
            var builtins = new Dictionary<string,JsonNode?>(config.EndpointBuiltIns);
            if (!builtins.ContainsKey("AWS::Region")) builtins["AWS::Region"] = JsonValue.Create(config.Region);
            if (!builtins.ContainsKey("SDK::Endpoint") && !string.IsNullOrEmpty(config.Endpoint)) builtins["SDK::Endpoint"] = JsonValue.Create(config.Endpoint);
            resolved = Endpoints.Resolve(rules,op["endpointParameters"],queryInput,config.EndpointParameters,builtins);
            op = (JsonObject)op.DeepClone(); op["resolvedEndpoint"] = resolved;
        }
        var endpoint = Text(resolved["url"])!;
        if (!config.DisableHostPrefix && op["endpoint"] is JsonNode host) endpoint = Endpoints.HostPrefix(endpoint,host,queryInput);
        string Wire(string key) => WireName(Text(op["input"]), key);
        var checksums = Checksums.Prepare(op["checksum"], values, Wire, config.RequestChecksumCalculation, config.ResponseChecksumValidation, stream is not null);
        var token = Text(op["token"]);
        if (token is not null && values[Wire(token)] is null) values[Wire(token)] = Guid.NewGuid().ToString();
        var boundValues = (JsonObject)values.DeepClone();
        foreach (var (member, format) in request["timestampFormats"]?.AsObject() ?? new JsonObject())
            boundValues[Wire(member)] = BoundMember(Shape(Text(op["input"])!)["members"]![member]!, values[Wire(member)], Text(format));
        var path = Text(op["uri"])!; var display = path;
        // Telemetry shows where sensitive members went without their values.
        var inputMembers = op["input"] is null ? null : Shape(Text(op["input"])!)["members"]?.AsObject();
        bool Sensitive(string member) => Flag(inputMembers?[member]?["sensitive"]);
        var sensitiveHeaders = new HashSet<string>(credentialHeaders, StringComparer.OrdinalIgnoreCase);
        var sensitiveQuery = new HashSet<string>(credentialQuery, StringComparer.OrdinalIgnoreCase);
        foreach (var label in request["labels"]!.AsArray())
        {
            var key = Text(label)!; var value = boundValues[Wire(key)];
            if (value is null || Bound(value).Length == 0) throw new ArgumentException($"{key} is required");
            var labelText = Bound(value);
            var greedy = op["greedyLabels"]?.AsArray().Any(item => Text(item) == key) == true;
            var parts = greedy ? labelText.Split('/') : new[] { labelText };
            if (parts.Any(part => part is "." or "..")) throw new ArgumentException("Dot-segment HTTP label: " + key);
            var placeholder = "{" + key + (greedy ? "+" : "") + "}"; var escaped = string.Join("/", parts.Select(Uri.EscapeDataString));
            path = path.Replace(placeholder, escaped, StringComparison.Ordinal);
            display = display.Replace(placeholder, Sensitive(key) ? Redacted : escaped, StringComparison.Ordinal);
        }
        var query = new List<string>();
        foreach (var binding in request["query"]!.AsArray())
        {
            if (Sensitive(Text(binding![0])!)) sensitiveQuery.Add(Text(binding[1])!);
            var value = boundValues[Wire(Text(binding[0])!)]; if (value is null) continue;
            IEnumerable<JsonNode?> items = value is JsonArray array ? array : new[] { value };
            foreach (var item in items) query.Add(Uri.EscapeDataString(Text(binding[1])!) + "=" + Uri.EscapeDataString(Scalar(item!)));
        }
        if (Text(request["queryParams"]) is string queryMember && values[Wire(queryMember)] is JsonObject queryMap)
        {
            var reserved = request["query"]!.AsArray().Select(binding => Text(binding![1])!).ToHashSet(StringComparer.Ordinal);
            if (path.Contains('?')) foreach (var pair in path.Split('?', 2)[1].Split('&')) reserved.Add(Uri.UnescapeDataString(pair.Split('=', 2)[0]));
            foreach (var (key, value) in queryMap.OrderBy(entry => entry.Key, StringComparer.Ordinal))
            {
                if (reserved.Contains(key) || value is null) continue;
                if (Sensitive(queryMember)) sensitiveQuery.Add(key);
                IEnumerable<JsonNode?> items = value is JsonArray array ? array : new[] { value };
                foreach (var item in items) query.Add(Uri.EscapeDataString(key) + "=" + Uri.EscapeDataString(Scalar(item!)));
            }
        }
        if (query.Count != 0) path += (path.Contains('?') ? "&" : "?") + string.Join("&", query);
        var headers = new Dictionary<string, string>(config.Headers, StringComparer.OrdinalIgnoreCase);
        foreach (var (key,entries) in resolved["headers"]!.AsObject()) headers[key] = string.Join(", ",entries!.AsArray().Select(item => Text(item)));
        var reservedHeaders = request["headers"]!.AsArray().Select(binding => Text(binding![1])!).ToHashSet(StringComparer.OrdinalIgnoreCase);
        foreach (var binding in request["prefixHeaders"]?.AsArray() ?? new JsonArray())
        {
            if (values[Wire(Text(binding![0])!)] is not JsonObject map) continue;
            foreach (var (key, value) in map)
            {
                var header = Text(binding[1]) + key;
                if (Sensitive(Text(binding[0])!)) sensitiveHeaders.Add(header);
                if (!reservedHeaders.Contains(header) && value is not null) headers[header] = Bound(Text(request["headerEncodings"]?[Text(binding[0])!]) == "base64" ? MediaValue(value,false)! : value);
            }
        }
        foreach (var binding in request["headers"]!.AsArray())
        {
            var key = Text(binding![0])!;
            if (Sensitive(key)) sensitiveHeaders.Add(Text(binding[1])!);
            var value = boundValues[Wire(key)];
            if (value is not null) headers[Text(binding[1])!] = Bound(Text(request["headerEncodings"]?[key]) == "base64" ? MediaValue(value, false)! : value);
        }
        byte[]? body = null;
        if (request["body"]!.AsArray().Count != 0)
        {
            var obj = new JsonObject();
            foreach (var m in request["body"]!.AsArray()) { var key = Wire(Text(m)!); if (values.ContainsKey(key)) obj[key] = values[key]?.DeepClone(); }
            body = Encoding.UTF8.GetBytes(obj.ToJsonString(JsonOptions)); headers["Content-Type"] = "application/json";
        }
        if (request["payload"] is JsonArray payload && values[Wire(Text(payload[0])!)] is JsonNode encoded)
        {
            body = Text(request["payloadKind"]) switch
            {
                "string" => StrictUtf8.GetBytes(encoded.GetValue<string>()),
                "document" => Encoding.UTF8.GetBytes(encoded.ToJsonString(JsonOptions)),
                _ => Convert.FromBase64String(encoded.GetValue<string>())
            };
            headers["Content-Type"] = Text(payload[1])!;
        }
        long? minimum = null, maximum = null;
        if (stream is not null)
        {
            var payloadMember = Shape(Text(op["input"])!)["members"]![Text(request["payload"]![0])!]!;
            minimum = LengthBound(payloadMember, "min"); maximum = LengthBound(payloadMember, "max");
            if (Flag(request["requiresLength"]) && stream.Length is null) throw new ArgumentException(name + ": streaming payload requires a known length");
            if (stream.Length < minimum || stream.Length > maximum) throw new ArgumentException(name + ": streaming payload length is outside its modeled bounds");
            headers["Content-Type"] = Text(request["payload"]![1]) ?? "application/octet-stream";
            if (stream.Sha256 is not null) headers["x-amz-content-sha256"] = stream.Sha256;
        }
        if (Text(schema["protocol"]) is "awsJson1_0" or "awsJson1_1")
        {
            body ??= Encoding.UTF8.GetBytes("{}");
            headers["Content-Type"] = "application/x-amz-json-" + (Text(schema["protocol"]) == "awsJson1_0" ? "1.0" : "1.1");
            headers["X-Amz-Target"] = Text(schema["service"])!.Split('#')[^1] + "." + name;
        }
        if (Rpc is { } rpc)
        {
            // RPC v2: the lowered URI is /service/{Service}/operation/{Operation}; an operation without input sends no body.
            headers["Smithy-Protocol"] = rpc.Marker; headers["Accept"] = rpc.Media; headers.Remove("Content-Type"); body = null;
            if (Text(op["input"]) is string inputTarget)
            {
                var document = new JsonObject();
                foreach (var m in request["body"]!.AsArray()) { var key = Wire(Text(m)!); if (values.ContainsKey(key)) document[key] = values[key]?.DeepClone(); }
                body = RpcRequestBody(rpc, inputTarget, document); headers["Content-Type"] = rpc.Media;
            }
        }
        return new(op, new Uri(endpoint.TrimEnd('/') + path), headers, body, name, checksums, stream, minimum, maximum)
        {
            EndpointUrl = endpoint.TrimEnd('/'), EndpointSource = schema["endpointRules"] is null ? "configured" : "rules", Target = path,
            DisplayPath = display.Split('?', 2)[0], SensitiveHeaders = sensitiveHeaders, SensitiveQuery = sensitiveQuery,
            Discovery = DiscoveryFor(name, op, key => values[Wire(key)])
        };
    }
    private static bool Secure(Uri uri) => uri.Scheme == "https" || uri.Scheme == "http" &&
        (uri.Host.Equals("localhost", StringComparison.OrdinalIgnoreCase) || System.Net.IPAddress.TryParse(uri.Host.Trim('[', ']'), out var ip) && System.Net.IPAddress.IsLoopback(ip));
    private static void SetHeader(HttpRequestMessage request, string key, string value)
    {
        // Validation prevents header injection even for user-supplied defaults.
        if (value.Contains('\r') || value.Contains('\n')) throw new ArgumentException("Invalid header value");
        bool contentHeader = false;
        try { request.Headers.Remove(key); } catch (InvalidOperationException) { contentHeader = true; }
        if (contentHeader || !request.Headers.TryAddWithoutValidation(key, value))
        {
            request.Content ??= new ByteArrayContent([]);
            request.Content.Headers.Remove(key); request.Content.Headers.Add(key, value);
        }
    }
    internal Task<T> CallAsync<T>(string name, object? input, CancellationToken cancellationToken) => TracedAsync(name, async trace =>
    {
        var op = schema["operations"]![name]!.AsObject();
        var stream = RequestStream(op, input);
        try
        {
            var values = Encode(Text(op["input"]), input)?.AsObject() ?? new JsonObject();
            return await SendAsync<T>(Prepare(name, values, stream), trace, null, cancellationToken).ConfigureAwait(false);
        }
        finally { stream?.Release(); }
    });
    /// <summary>The streaming payload of <paramref name="input"/>, which encoding leaves out of the JSON values.</summary>
    private ByteStream? RequestStream(JsonObject op, object? input)
    {
        var request = op["request"]!;
        if (!Flag(request["streaming"]) || request["payload"] is not JsonArray payload) return null;
        var member = Shape(Text(op["input"])!)["members"]![Text(payload[0])!]!;
        object? value = input is null ? null : ModelProperty(input.GetType(), Text(member["attribute"])!).GetValue(input);
        if (value is IOptional optional) value = optional.IsSet ? optional.Untyped : null;
        if (value is ByteStream stream) return stream;
        if (member["default"] is JsonNode fallback) return ByteStream.FromBytes(Convert.FromBase64String(fallback.GetValue<string>()));
        throw new ArgumentException(Text(payload[0]) + " is required");
    }
    /// <summary>Releases the response streams of an output the caller never receives.</summary>
    internal static void DisposeStreams(object? value)
    {
        if (value is null) return;
        foreach (var property in value.GetType().GetProperties())
            if (property.PropertyType == typeof(ByteStream) && property.GetIndexParameters().Length == 0) (property.GetValue(value) as ByteStream)?.Dispose();
    }
    // A stream is never buffered to hash it: a replayable source is read once more in chunks, a one-shot needs a declared digest.
    private static async Task StreamDigestAsync(HttpRequestMessage request, ByteStreamContent content, CancellationToken token)
    {
        if (request.Headers.Contains("x-amz-content-sha256")) return;
        if (!content.Body.Replayable) throw new ArgumentException("A one-shot streaming body needs a declared SHA-256 (ByteStream.WithSha256) for the content digest");
        var (source, owned) = await content.Body.OpenAsync(token).ConfigureAwait(false);
        var buffer = System.Buffers.ArrayPool<byte>.Shared.Rent(81920);
        try
        {
            using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
            int count;
            while ((count = await source.ReadAsync(buffer, token).ConfigureAwait(false)) != 0) hash.AppendData(buffer, 0, count);
            SetHeader(request, "x-amz-content-sha256", Convert.ToHexString(hash.GetHashAndReset()).ToLowerInvariant());
        }
        finally
        {
            System.Buffers.ArrayPool<byte>.Shared.Return(buffer);
            if (owned) await source.DisposeAsync().ConfigureAwait(false);
        }
    }
    private async ValueTask<(bool Credentials, Func<ValueTask>? Sign)> AuthenticateAsync(JsonObject op, HttpRequestMessage request, CancellationToken token)
    {
        var ids = op["auth"]?.AsArray().Select(id => Text(id)!).ToList() ?? (Flag(op["bearer"]) ? ["smithy.api#httpBearerAuth"] : new List<string>());
        foreach (var id in ids)
        {
            var scheme = schema["authSchemes"]?[id];
            var kind = id == "smithy.api#httpBearerAuth" ? "bearer" : Text(scheme?["kind"]);
            if (op["resolvedEndpoint"]?["properties"]?["authSchemes"] is JsonArray allowed)
            {
                var selected = allowed.FirstOrDefault(item => string.Equals(Text(item?["name"]),kind,StringComparison.OrdinalIgnoreCase));
                if (selected is null) continue;
                var effective = scheme?.DeepClone().AsObject() ?? new JsonObject { ["kind"] = kind };
                foreach (var (key,value) in selected.AsObject()) if (key != "name") effective[key] = value?.DeepClone();
                scheme = effective;
            }
            var authPrefix = kind switch { "basic" => "Basic", "bearer" => "Bearer", "digest" => "Digest", "sigV4" => "AWS4-HMAC-SHA256", "sigV4a" => "AWS4-ECDSA-P256-SHA256", "apiKey" => Text(scheme?["scheme"]) ?? "", _ => "" };
            var header = kind == "apiKey" ? Text(scheme!["name"])! : "Authorization";
            if (kind == "apiKey" && Text(scheme!["location"]) == "query")
            {
                if (request.RequestUri!.Query.TrimStart('?').Split('&', StringSplitOptions.RemoveEmptyEntries).Any(pair => {
                    var parts = pair.Split('=',2); return parts.Length == 2 && parts[1].Length != 0 && Uri.UnescapeDataString(parts[0].Replace("+", " ", StringComparison.Ordinal)) == header;
                })) return (true,null);
            }
            else if (request.Headers.TryGetValues(header,out var existing))
            {
                var value = string.Join(",",existing);
                if (authPrefix.Length == 0 ? value.Length != 0 : value.StartsWith(authPrefix+" ",StringComparison.OrdinalIgnoreCase) && value.Length > authPrefix.Length+1) return (true,null);
            }
            switch (kind)
            {
                case "bearer":
                    if (config.Token is not null && await config.Token(token).AsTask().WaitAsync(token).ConfigureAwait(false) is string bearer && bearer.Length != 0)
                    { SetHeader(request, "Authorization", "Bearer " + bearer); return (true, null); }
                    break;
                case "basic":
                    if (config.BasicCredentials is not null && await config.BasicCredentials(token).AsTask().WaitAsync(token).ConfigureAwait(false) is BasicCredentials basic)
                    {
                        if (basic.Username.Contains(':')) throw new ArgumentException("Basic username cannot contain a colon");
                        SetHeader(request, "Authorization", "Basic " + Convert.ToBase64String(StrictUtf8.GetBytes(basic.Username + ":" + basic.Password)));
                        return (true, null);
                    }
                    break;
                case "apiKey":
                    if (config.ApiKey is not null && await config.ApiKey(token).AsTask().WaitAsync(token).ConfigureAwait(false) is string key && key.Length != 0)
                    {
                        var name = Text(scheme!["name"])!;
                        if (Text(scheme["location"]) == "query")
                        {
                            var uri = new UriBuilder(request.RequestUri!);
                            var query = uri.Query.TrimStart('?').Split('&', StringSplitOptions.RemoveEmptyEntries)
                                .Where(pair => Uri.UnescapeDataString(pair.Split('=', 2)[0].Replace("+", " ", StringComparison.Ordinal)) != name).ToList();
                            query.Add(Uri.EscapeDataString(name) + "=" + Uri.EscapeDataString(key)); uri.Query = string.Join("&", query); request.RequestUri = uri.Uri;
                        }
                        else { var prefix = Text(scheme["scheme"]); SetHeader(request, name, (string.IsNullOrEmpty(prefix) ? "" : prefix + " ") + key); }
                        return (true, null);
                    }
                    break;
                case "sigV4":
                    if (config.Credentials is not null && await config.Credentials(token).AsTask().WaitAsync(token).ConfigureAwait(false) is AwsCredentials aws)
                    {
                        var options = new SigningOptions(Text(scheme?["signingRegion"]) ?? config.Region ?? "", Text(scheme!["signingName"])!) {
                            UnsignedPayload = Flag(op["unsignedPayload"]), DisableDoubleEncoding = scheme["disableDoubleEncoding"]?.GetValue<bool>(), DisableNormalizePath = scheme["disableNormalizePath"]?.GetValue<bool>() };
                        return (true, () => SigV4Signer.SignAsync(request, aws, options, token));
                    }
                    break;
                case "sigV4a":
                    if (config.Credentials is not null && await config.Credentials(token).AsTask().WaitAsync(token).ConfigureAwait(false) is AwsCredentials signer)
                    {
                        // An endpoint rule's region set wins, then the configured set, then the client region.
                        var regions = scheme?["signingRegionSet"] is JsonArray set ? set.Select(item => Text(item)!).ToList()
                            : config.SigV4aSigningRegionSet?.ToList() ?? (config.Region is null ? null : [config.Region]);
                        if (regions is null || regions.Count == 0) throw new InvalidOperationException("SigV4a requires a signing region set or region");
                        var options = new SigV4aSigningOptions(string.Join(",", regions), Text(scheme!["signingName"])!) {
                            UnsignedPayload = Flag(op["unsignedPayload"]), DisableDoubleEncoding = scheme["disableDoubleEncoding"]?.GetValue<bool>(), DisableNormalizePath = scheme["disableNormalizePath"]?.GetValue<bool>() };
                        return (true, async () => await SigV4aSigner.SignAsync(request, signer, options, token).ConfigureAwait(false));
                    }
                    break;
                case "digest":
                    if (config.DigestCredentials is not null && await config.DigestCredentials(token).AsTask().WaitAsync(token).ConfigureAwait(false) is BasicCredentials digestUser)
                    {
                        // Until a server challenge arrives the request carries no credentials; SendAsync answers the 401.
                        if (!digestSession.Ready) return (true, null);
                        return (true, async () => SetHeader(request, "Authorization", digestSession.Authorization(digestUser, request.Method.Method, request.RequestUri!,
                            request.Content is null ? [] : request.Content is ByteStreamContent ? null : await request.Content.ReadAsByteArrayAsync(token).ConfigureAwait(false))));
                    }
                    break;
            }
        }
        if (ids.Count != 0 && !(ids.Count == 1 && ids[0] == "smithy.api#httpBearerAuth" && Flag(op["bearer"])) && !Flag(op["optionalAuth"]))
            throw new InvalidOperationException("No identity available for modeled authentication schemes");
        return (false, null);
    }
    private async Task<T> SendAsync<T>(Prepared prepared, CallTrace trace, TimeSpan? timeout, CancellationToken cancellationToken)
    {
        // A streamed response takes over the deadline, so it still bounds reading the body.
        var deadline = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        bool handedOff = false;
        try
        {
            // The deadline always uses real time; the configured TimeProvider only drives policy sleeps and readings.
            var limit = timeout ?? config.Timeout; var started = System.Diagnostics.Stopwatch.GetTimestamp();
            deadline.CancelAfter(limit); var ct = deadline.Token;
            var op = prepared.Operation;
            var stream = prepared.Stream;
            // A one-shot stream cannot be sent twice.
            int maxAttempts = stream is { Replayable: false } ? 1 : config.MaxAttempts;
            int? lastCost = null; bool challenged = false;
            for (int attempt = 1; ; attempt++)
            {
                var answering = challenged; challenged = false;
                ct.ThrowIfCancellationRequested();
                var discovered = await DiscoverAsync(prepared, limit - System.Diagnostics.Stopwatch.GetElapsedTime(started), ct).ConfigureAwait(false);
                // Host prefixes are not applied to a discovered address; endpoint-rule headers and auth properties still are.
                var (uri, endpoint, source) = discovered is null ? (prepared.Uri, prepared.EndpointUrl, prepared.EndpointSource)
                    : (new Uri(discovered.Url + prepared.Target), discovered.Url, "discovered");
                var wait = retry.SendDelay();
                if (wait > TimeSpan.Zero)
                {
                    try { await Task.Delay(wait, config.TimeProvider, ct).ConfigureAwait(false); }
                    catch (OperationCanceledException) { retry.CancelSend(); throw; }
                }
                using var request = new HttpRequestMessage(new HttpMethod(Text(op["method"])!), uri);
                if (stream is not null) request.Content = new ByteStreamContent(stream, prepared.MinLength, prepared.MaxLength);
                else if (prepared.Body is not null) request.Content = new ByteArrayContent(prepared.Body);
                foreach (var (key, value) in prepared.Headers) SetHeader(request, key, value);
                var authentication = await AuthenticateAsync(op, request, ct).ConfigureAwait(false);
                foreach (var middleware in config.Middleware) await middleware(request, ct).AsTask().WaitAsync(ct).ConfigureAwait(false);
                if (request.RequestUri is null || !request.RequestUri.IsAbsoluteUri || (request.RequestUri.Scheme != "https" && request.RequestUri.Scheme != "http") || request.RequestUri.UserInfo.Length != 0)
                    throw new ArgumentException("Middleware must supply an absolute HTTP(S) URI without credentials");
                if ((authentication.Credentials || request.Headers.Contains("Authorization")) && !Secure(request.RequestUri)) throw new ArgumentException("Credentials require HTTPS or loopback HTTP");
                if (digest && request.Content is ByteStreamContent streaming) await StreamDigestAsync(request, streaming, ct).ConfigureAwait(false);
                else if (digest && (request.Content is not null || request.Method == HttpMethod.Post
                    || request.Method == HttpMethod.Put || request.Method == HttpMethod.Patch))
                {
                    var bytes = request.Content is null ? [] : await request.Content.ReadAsByteArrayAsync(ct).ConfigureAwait(false);
                    if (request.Content is not null)
                    {
                        var previous = request.Content; var buffered = new ByteArrayContent(bytes);
                        foreach (var header in previous.Headers) if (!header.Key.Equals("Content-Length", StringComparison.OrdinalIgnoreCase)) buffered.Headers.TryAddWithoutValidation(header.Key, header.Value);
                        request.Content = buffered; previous.Dispose();
                    }
                    SetHeader(request, "x-amz-content-sha256", Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant());
                }
                ct.ThrowIfCancellationRequested();
                await Checksums.RequestAsync(prepared.Checksums, request, ct).ConfigureAwait(false);
                if (authentication.Sign is not null) await authentication.Sign().ConfigureAwait(false);
                trace.AttemptStarted(prepared, request, endpoint, source);
                Exception? failure = null; bool timedOut = false;
                HttpResponseMessage? response = null; int? status = null; long? size = null;
                Dictionary<string, string>? headers = null;
                try
                {
                    response = await http.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, ct).ConfigureAwait(false);
                    status = (int)response.StatusCode;
                    headers = response.Headers.Concat(response.Content.Headers).GroupBy(h => h.Key, StringComparer.OrdinalIgnoreCase)
                        .ToDictionary(g => g.Key, g => string.Join(", ", g.SelectMany(h => h.Value)), StringComparer.OrdinalIgnoreCase);
                    // A Digest 401 is answered once per attempt: when no credentials went out, or the nonce was stale.
                    if (status == 401 && !answering && DigestChallenge(prepared, request, headers) is { } challenge)
                    {
                        digestSession.Accept(challenge);
                        retry.AfterAttempt(false);
                        trace.AttemptFinished(status, null, null, RequestIdOf(null, headers));
                        challenged = true; attempt--;
                        continue;
                    }
                    var payload = op["response"]!["payload"] as JsonArray;
                    if (response.IsSuccessStatusCode && payload is not null && Flag(op["response"]!["streaming"]))
                    {
                        var length = response.Content.Headers.ContentLength;
                        var output = await StreamedAsync<T>(prepared, response, headers, deadline, cancellationToken, ct).ConfigureAwait(false);
                        handedOff = true; response = null;
                        Succeeded(lastCost);
                        trace.AttemptFinished(status, null, length, RequestIdOf(null, headers));
                        return output;
                    }
                    bool hasBody = op["response"]!["body"]!.AsArray().Count != 0 || payload is not null || prepared.Checksums.Response.Count != 0
                        || Rpc is not null && op["output"] is not null;
                    var bytes = response.IsSuccessStatusCode && !hasBody ? [] : await response.Content.ReadAsByteArrayAsync(ct).ConfigureAwait(false);
                    size = bytes.Length;
                    if (response.IsSuccessStatusCode)
                    {
                        var algorithm = Checksums.Response(prepared.Checksums, headers, bytes);
                        if (algorithm is not null) config.ChecksumValidated?.Invoke(prepared.Name, algorithm);
                        var output = Output<T>(op, payload, bytes, (int)response.StatusCode, headers);
                        Succeeded(lastCost);
                        trace.AttemptFinished(status, null, size, RequestIdOf(null, headers));
                        return output;
                    }
                    failure = Error(op, (int)response.StatusCode, headers, bytes);
                }
                // The caller's stream failed or broke its declared length: not a transport fault, never retried.
                catch (Exception) when (request.Content is ByteStreamContent { SourceFailure: { } broken })
                {
                    Interrupted(status, broken, size, headers, trace);
                    ExceptionDispatchInfo.Capture(broken).Throw();
                }
                catch (Exception e) when (e is HttpRequestException or IOException || TimedOut(e, ct))
                { failure = new NetworkError(e.Message, e); timedOut = TimedOut(e, ct); }
                catch (Exception e) { Interrupted(status, e, size, headers, trace); throw; }
                finally { response?.Dispose(); }
                var decision = AfterFailure(prepared, failure!, timedOut, attempt, maxAttempts, discovered, headers);
                trace.AttemptFinished(status, failure, size, RequestIdOf(failure, headers), decision);
                if (decision is not (_, var delay, var cost)) throw failure!;
                lastCost = cost;
                await Task.Delay(delay, config.TimeProvider, ct).ConfigureAwait(false);
            }
        }
        finally { if (!handedOff) deadline.Dispose(); }
    }
    private void Succeeded(int? lastCost)
    {
        retry.AfterAttempt(false);
        retry.Succeeded(lastCost);
    }
    /// <summary>An attempt ended by an error that is never retried; a response that arrived was still measured.</summary>
    private void Interrupted(int? status, Exception error, long? size, Dictionary<string, string>? headers, CallTrace trace)
    {
        if (status is not null) retry.AfterAttempt(false);
        trace.AttemptFinished(status, error, size, RequestIdOf(error, headers));
    }
    /// <summary>Decodes a buffered successful response; payloads occupy the whole body and are decoded by their modeled kind.</summary>
    private T Output<T>(JsonObject op, JsonArray? payload, byte[] bytes, int status, Dictionary<string, string> headers)
    {
        if (Rpc is { } rpc) return RpcOutput<T>(rpc, op, bytes, headers);
        var body = payload is not null || bytes.Length == 0 ? new JsonObject() : JsonNode.Parse(bytes)!.AsObject();
        var target = Text(op["output"]);
        if (target is null) return (T)(object)new Unit();
        if (payload is not null) body[WireName(target, Text(payload[0])!)] = PayloadValue(Text(op["response"]!["payloadKind"]), bytes);
        ResponseBindings(target, op["response"]!, status, headers, body);
        return (T)Decode(target, body)!;
    }
    /// <summary>Returns a successful streaming response at its headers; the caller reads the body.</summary>
    private async Task<T> StreamedAsync<T>(Prepared prepared, HttpResponseMessage response, Dictionary<string, string> headers,
        CancellationTokenSource deadline, CancellationToken caller, CancellationToken ct)
    {
        var op = prepared.Operation; var target = Text(op["output"])!;
        var member = Shape(target)["members"]![Text(op["response"]!["payload"]![0])!]!;
        var body = new JsonObject();
        ResponseBindings(target, op["response"]!, (int)response.StatusCode, headers, body);
        var result = Decode(target, body)!;
        var inner = await response.Content.ReadAsStreamAsync(ct).ConfigureAwait(false);
        var reader = new ResponseBodyStream(response, inner, deadline, caller, LengthBound(member, "min"), LengthBound(member, "max"),
            Checksums.Running(prepared.Checksums, headers, prepared.Name, config.ChecksumValidated));
        var stream = ByteStream.Response(reader, response.Content.Headers.ContentLength);
        var property = ModelProperty(result.GetType(), Text(member["attribute"])!);
        property.SetValue(result, property.PropertyType == typeof(Optional<ByteStream>) ? Optional<ByteStream>.Of(stream) : (object)stream);
        return (T)result;
    }
    private ServiceError Error(JsonObject op, int status, Dictionary<string, string> headers, byte[] bytes)
    {
        if (Rpc is { } rpc) return RpcError(rpc, op, status, headers, bytes);
        JsonObject body;
        try { body = JsonNode.Parse(bytes) as JsonObject ?? new JsonObject(); } catch (JsonException) { body = new(); }
        static string? StringValue(JsonNode? v) => v is JsonValue j && j.TryGetValue<string>(out var s) ? s : null;
        var name = headers.GetValueOrDefault("x-crucible-error") ?? StringValue(body["__type"]);
        if (AwsJson)
        {
            name = headers.GetValueOrDefault("x-amzn-errortype") ?? StringValue(body["code"]) ?? StringValue(body["__type"]);
            if (name is not null) name = name.Split(':', 2)[0].Split('#')[^1];
        }
        var candidates = op["errors"]!.AsArray().Select(n => Text(n)!).ToList();
        string? target;
        if (name is not null) target = candidates.Find(id => ErrorCode(id) == name);
        else { var matches = candidates.Where(id => Shape(id)["error"]!["status"]!.GetValue<int>() == status).ToList(); target = matches.Count == 1 ? matches[0] : null; }
        ServiceError error = new ServiceException();
        var metadata = target is null ? null : Shape(target)["error"];
        if (target is not null)
        {
            try
            {
                if (metadata!["payload"] is JsonArray payload)
                    body = new JsonObject { [WireName(target, Text(payload[0])!)] = PayloadValue(Text(metadata["payloadKind"]), bytes) };
                ResponseBindings(target, metadata!, status, headers, body);
                error = (ServiceError)Decode(target, body)!;
            }
            catch (Exception e) when (e is JsonException or InvalidOperationException or FormatException or OverflowException or ArgumentException)
            {
                // Malformed wire members must not erase HTTP metadata or modeled retry policy.
            }
        }
        name ??= target is null ? "UnknownError" : ErrorCode(target);
        var requestId = headers.GetValueOrDefault("x-request-id") ?? StringValue(body["requestId"]);
        if (AwsJson) requestId = headers.GetValueOrDefault("x-amzn-requestid") ?? headers.GetValueOrDefault("x-amz-request-id") ?? requestId;
        error.Initialize(name, StringValue(body["message"]) ?? (AwsJson ? StringValue(body["Message"]) : null) ?? $"HTTP {status}", status, requestId, headers, Flag(metadata?["retryable"]), Flag(metadata?["throttling"]));
        return error;
    }
    /// <summary>The delay after the given (1-based) failed attempt: a valid Retry-After, or full-jitter backoff.</summary>
    private TimeSpan Delay(int attempt, Dictionary<string, string>? headers)
    {
        if (headers is not null && headers.TryGetValue("Retry-After", out var value))
        {
            if (double.TryParse(value, NumberStyles.Float, CultureInfo.InvariantCulture, out var seconds) && double.IsFinite(seconds) && seconds >= 0)
                return TimeSpan.FromMilliseconds(Math.Min(uint.MaxValue - 1d, seconds * 1000));
            if (DateTimeOffset.TryParse(value, CultureInfo.InvariantCulture, DateTimeStyles.AssumeUniversal, out var date))
            {
                var remaining = (date - config.TimeProvider.GetUtcNow()).TotalMilliseconds;
                if (remaining >= 0) return TimeSpan.FromMilliseconds(Math.Min(uint.MaxValue - 1d, remaining));
            }
        }
        return retry.Backoff(attempt);
    }
    internal async IAsyncEnumerable<T> PagesAsync<T>(string name, object? input, [EnumeratorCancellation] CancellationToken cancellationToken)
    {
        var op = schema["operations"]![name]!; var pagination = op["pagination"]!;
        var values = Encode(Text(op["input"]), input)?.AsObject() ?? new JsonObject(); var seen = new HashSet<string>();
        var inputToken = WireName(Text(op["input"]), Text(pagination["input"])!);
        if (values[inputToken] is JsonNode initial) seen.Add(initial.ToJsonString());
        var stream = RequestStream(op.AsObject(), input);
        try
        {
            while (true)
            {
                var result = await TracedAsync(name, trace => SendAsync<T>(Prepare(name, values, stream), trace, null, cancellationToken)).ConfigureAwait(false);
                yield return result;
                JsonNode? next = Encode(Text(op["output"]), result);
                var target = Text(op["output"]);
                foreach (var path in pagination["output"]!.AsArray())
                {
                    var member = Text(path)!;
                    next = (next as JsonObject)?[WireName(target, member)];
                    target = Text(Shape(target!)["members"]![member]!["target"]);
                }
                if (next is null || next.GetValueKind() == JsonValueKind.String && next.GetValue<string>().Length == 0) yield break;
                if (!seen.Add(next.ToJsonString())) throw new InvalidOperationException("Pagination token cycle");
                values[inputToken] = next.DeepClone();
            }
        }
        finally { stream?.Release(); }
    }
}
