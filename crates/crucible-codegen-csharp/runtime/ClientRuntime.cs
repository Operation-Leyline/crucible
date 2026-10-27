using System.Collections;
using System.Globalization;
using System.Numerics;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;

internal sealed partial class ClientRuntime : IDisposable
{
    private readonly ClientConfig config;
    private readonly HttpClient http;
    // Streams only: the SDK-owned one does not drain abandoned responses.
    private readonly HttpClient streamHttp;
    private readonly bool ownsHttp;
    private readonly JsonObject schema;
    private readonly IReadOnlyDictionary<string, Type> types;
    private static readonly JsonSerializerOptions JsonOptions = new() { Encoder = System.Text.Encodings.Web.JavaScriptEncoder.UnsafeRelaxedJsonEscaping };

    internal ClientRuntime(ClientConfig input, string endpoint)
        : this(input, endpoint, LoadSchema(), ModelRegistry.Types) { }

    internal ClientRuntime(ClientConfig input, string endpoint, JsonObject schema, IReadOnlyDictionary<string, Type> types)
    {
        endpoint = input.Endpoint ?? endpoint;
        if (!Uri.TryCreate(endpoint, UriKind.Absolute, out var uri) ||
            (uri.Scheme != "http" && uri.Scheme != "https") || uri.Host.Length == 0 ||
            uri.UserInfo.Length != 0 || uri.Query.Length != 0 || uri.Fragment.Length != 0)
            throw new ArgumentException("Absolute HTTP(S) endpoint without credentials, query, or fragment required");
        if (input.MaxStreamEventBytes < 256)
            throw new ArgumentException("MaxStreamEventBytes must be at least 256");
        if (input.MaxAttempts < 1 || input.Timeout <= TimeSpan.Zero || input.Timeout.TotalMilliseconds > uint.MaxValue - 1 ||
            input.BaseDelay < TimeSpan.Zero || input.MaxDelay < TimeSpan.Zero || input.MaxDelay.TotalMilliseconds > int.MaxValue)
            throw new ArgumentException("Invalid attempt limit, timeout, or retry delay");
        config = new ClientConfig {
            Endpoint = endpoint.TrimEnd('/'), Token = input.Token, MaxAttempts = input.MaxAttempts,
            Timeout = input.Timeout, MaxStreamEventBytes = input.MaxStreamEventBytes, BaseDelay = input.BaseDelay, MaxDelay = input.MaxDelay,
            Headers = new(input.Headers, StringComparer.OrdinalIgnoreCase), Middleware = [.. input.Middleware],
            RetryMode = input.RetryMode, Telemetry = input.Telemetry,
            TimeProvider = input.TimeProvider ?? throw new ArgumentException("A TimeProvider is required")
        };
        ownsHttp = input.HttpClient is null;
        http = input.HttpClient ?? new HttpClient(new SocketsHttpHandler { AllowAutoRedirect = false }) { Timeout = System.Threading.Timeout.InfiniteTimeSpan };
        // An abandoned stream must release its connection without waiting for an idle
        // producer, but buffered calls that leave a small body unread must keep draining
        // it back into the pool. So SDK-owned streams get their own non-draining pool;
        // a supplied client keeps its caller-selected drain policy for both.
        streamHttp = input.HttpClient ?? new HttpClient(new SocketsHttpHandler { AllowAutoRedirect = false, MaxResponseDrainSize = 0 }) { Timeout = System.Threading.Timeout.InfiniteTimeSpan };
        this.schema = schema; this.types = types;
        InitializePolicy();
    }
    public void Dispose() { if (ownsHttp) { http.Dispose(); streamHttp.Dispose(); } }
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
    private static PropertyInfo ModelProperty(Type type, string name) {
        var presence = type.GetProperty(name + "Presence");
        return presence?.PropertyType.IsGenericType == true && presence.PropertyType.GetGenericTypeDefinition() == typeof(Optional<>) ? presence : type.GetProperty(name)!;
    }
    private static object New(Type type) => Activator.CreateInstance(type)!;
    private static void CheckElement(JsonNode collection, object? value)
    {
        if (value is null && !Flag(collection["sparse"]))
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
                return JsonSerializer.SerializeToNode(value, value.GetType(), JsonOptions);
            case "blob": return JsonValue.Create(Convert.ToBase64String((byte[])value));
            case "timestamp": return WriteTime((DateTimeOffset)value, format ?? Text(shape["format"]));
            case "enum": return JsonValue.Create(value.ToString());
            case "intEnum": return JsonValue.Create((int)value.GetType().GetProperty("Value")!.GetValue(value)!);
            case "bigInteger": return JsonNode.Parse(((BigInteger)value).ToString(CultureInfo.InvariantCulture));
            case "bigDecimal": return JsonNode.Parse(((BigDecimal)value).ToString());
            case "document": return ((JsonNode)value).DeepClone();
            case "list":
                var array = new JsonArray(); var item = shape["member"]!;
                foreach (var element in (IEnumerable)value) {
                    CheckElement(shape, element);
                    var encoded = Encode(Text(item["target"]), element, Text(item["format"]));
                    if (Flag(shape["set"]) && array.Any(existing => WireEquals(existing, encoded))) throw new JsonException("Duplicate element in set");
                    array.Add(encoded);
                }
                return array;
            case "map":
                var map = new JsonObject(); var member = shape["value"]!;
                foreach (DictionaryEntry entry in (IDictionary)value)
                { CheckElement(shape, entry.Value); map.Add(entry.Key.ToString()!, Encode(Text(member["target"]), entry.Value, Text(member["format"]))); }
                return map;
            case "structure":
            case "union":
                var result = new JsonObject();
                foreach (var (name, description) in shape["members"]!.AsObject())
                {
                    var v = ModelProperty(value.GetType(), Text(description!["attribute"])!).GetValue(value);
                    if (v is IOptional optional) { if (!optional.IsSet) continue; v = optional.Untyped; }
                    result.Add(name, Encode(Text(description["target"]), v, Text(description["format"])));
                }
                if (kind == "union")
                {
                    var unknown = (UnknownUnion?)value.GetType().GetProperty("UnknownVariant")!.GetValue(value);
                    if (unknown is not null)
                    {
                        if (shape["members"]!.AsObject().ContainsKey(unknown.Name)) throw new ArgumentException("Unknown union name is declared");
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
                var f = value.GetValue<float>(); if (!float.IsFinite(f)) throw new JsonException("Float overflow"); return f;
            case "double":
                var d = value.GetValue<double>(); if (!double.IsFinite(d)) throw new JsonException("Double overflow"); return d;
            case "timestamp": return ReadTime(value, format ?? Text(shape["format"]));
            case "document": return value.DeepClone();
            case "enum": return Activator.CreateInstance(types[target], value.GetValue<string>());
            case "intEnum": return Activator.CreateInstance(types[target], checked((int)Integer(value)));
            case "bigInteger":
                if (value.GetValueKind() != JsonValueKind.Number) throw new JsonException("Expected arbitrary precision integer");
                return BigInteger.Parse(value.ToJsonString(), NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture);
            case "bigDecimal":
                if (value.GetValueKind() != JsonValueKind.Number) throw new JsonException("Expected arbitrary precision decimal");
                return new BigDecimal(value.ToJsonString());
            case "list":
                var list = (IList)New(types[target]); var member = shape["member"]!;
                var seen = new List<JsonNode?>();
                foreach (var item in value.AsArray()) {
                    CheckElement(shape, item);
                    if (Flag(shape["set"]) && seen.Any(existing => WireEquals(existing, item))) throw new JsonException("Duplicate element in set");
                    seen.Add(item); list.Add(Decode(Text(member["target"]), item, Text(member["format"])));
                }
                return list;
            case "map":
                var map = (IDictionary)New(types[target]); var val = shape["value"]!; var key = shape["key"]!;
                foreach (var (name, item) in value.AsObject())
                { CheckElement(shape, item); map.Add(Decode(Text(key["target"]), JsonValue.Create(name))!, Decode(Text(val["target"]), item, Text(val["format"]))); }
                return map;
            case "structure":
            case "union":
                var result = New(types[target]); var obj = value.AsObject(); var members = shape["members"]!.AsObject();
                if (kind == "union")
                {
                    if (obj.Count != 1) throw new JsonException("A union must contain exactly one member");
                    var variant = obj.First();
                    if (!members.ContainsKey(variant.Key))
                    {
                        result.GetType().GetProperty("UnknownVariant")!.SetValue(result, new UnknownUnion(variant.Key, variant.Value?.DeepClone()));
                        return result;
                    }
                }
                foreach (var (name, description) in members)
                {
                    var property = ModelProperty(result.GetType(), Text(description!["attribute"])!);
                    bool optional = property.PropertyType.IsGenericType && property.PropertyType.GetGenericTypeDefinition() == typeof(Optional<>);
                    bool present = obj.TryGetPropertyValue(name, out var item);
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
    private static readonly Regex HttpDateSeparator = new(@",\s*(?=(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun),)", RegexOptions.Compiled);
    internal JsonNode? Header(JsonNode member, string value, string? format = null)
    {
        var shape = Shape(Text(member["target"])!); var kind = Text(shape["kind"]);
        if (kind == "list")
        {
            var result = new JsonArray(); if (string.IsNullOrWhiteSpace(value)) return result;
            var parts = new List<string>(); int start = 0;
            var elementFormat = format ?? Text(shape["member"]!["format"]);
            if (Text(Shape(Text(shape["member"]!["target"])!)["kind"]) == "timestamp" && elementFormat == "http-date" && !value.TrimStart().StartsWith('"'))
            {
                // Every IMF-fixdate holds a comma and the protocols write these lists unquoted, so only a comma
                // that is followed by the next date's weekday separates two values.
                foreach (Match found in HttpDateSeparator.Matches(value)) { parts.Add(value[start..found.Index].Trim()); start = found.Index + 1; }
            }
            else
            {
                bool quoted = false, escaped = false;
                for (int i = 0; i < value.Length; i++)
                {
                    var c = value[i];
                    if (escaped) escaped = false;
                    else if (c == '\\' && quoted) escaped = true;
                    else if (c == '"') quoted = !quoted;
                    else if (c == ',' && !quoted) { parts.Add(value[start..i].Trim()); start = i + 1; }
                }
                if (quoted) throw new JsonException("Unterminated quoted header");
            }
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
    private sealed record Prepared(JsonObject Operation, Uri Uri, Dictionary<string, string> Headers, byte[]? Body, string Name)
    {
        /// <summary>The path with sensitive labels redacted, for telemetry.</summary>
        public string DisplayPath { get; init; } = "";
        public HashSet<string> SensitiveHeaders { get; init; } = [];
        public HashSet<string> SensitiveQuery { get; init; } = [];
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
    private void ResponseBindings(string target, JsonNode bindings, Dictionary<string, string> headers, JsonObject body)
    {
        foreach (var binding in bindings["headers"]!.AsArray())
        {
            var name = Text(binding![0])!;
            if (headers.TryGetValue(Text(binding[1])!, out var value))
            {
                var decoded = Header(Shape(target)["members"]![name]!, value, Text(bindings["timestampFormats"]?[name]));
                body[name] = Text(bindings["headerEncodings"]?[name]) == "base64" ? MediaValue(decoded, true) : decoded;
            }
        }
    }
    private Prepared Prepare(string name, JsonObject values)
    {
        var op = schema["operations"]![name]!.AsObject(); var request = op["request"]!;
        var token = Text(op["token"]);
        if (token is not null && values[token] is null) values[token] = Guid.NewGuid().ToString();
        var boundValues = (JsonObject)values.DeepClone();
        foreach (var (member, format) in request["timestampFormats"]?.AsObject() ?? new JsonObject())
            boundValues[member] = BoundMember(Shape(Text(op["input"])!)["members"]![member]!, values[member], Text(format));
        var path = Text(op["uri"])!; var display = path;
        // Telemetry shows where sensitive members went without their values.
        var inputMembers = op["input"] is null ? null : Shape(Text(op["input"])!)["members"]?.AsObject();
        bool Sensitive(string member) => Flag(inputMembers?[member]?["sensitive"]);
        var sensitiveHeaders = new HashSet<string>(credentialHeaders, StringComparer.OrdinalIgnoreCase);
        var sensitiveQuery = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var label in request["labels"]!.AsArray())
        {
            var key = Text(label)!; var value = boundValues[key];
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
            var value = boundValues[Text(binding[0])!]; if (value is null) continue;
            IEnumerable<JsonNode?> items = value is JsonArray array ? array : new[] { value };
            foreach (var item in items) query.Add(Uri.EscapeDataString(Text(binding[1])!) + "=" + Uri.EscapeDataString(Scalar(item!)));
        }
        if (query.Count != 0) path += (path.Contains('?') ? "&" : "?") + string.Join("&", query);
        var headers = new Dictionary<string, string>(config.Headers, StringComparer.OrdinalIgnoreCase);
        // A Content-Type bound to an input member outranks the payload's media
        // type, which outranks client header defaults.
        var modelledContentType = false;
        foreach (var binding in request["headers"]!.AsArray())
        {
            var key = Text(binding![0])!;
            if (Sensitive(key)) sensitiveHeaders.Add(Text(binding[1])!);
            var value = boundValues[key];
            if (value is null) continue;
            headers[Text(binding[1])!] = Bound(Text(request["headerEncodings"]?[key]) == "base64" ? MediaValue(value, false)! : value);
            modelledContentType |= string.Equals(Text(binding[1]), "Content-Type", StringComparison.OrdinalIgnoreCase);
        }
        byte[]? body = null;
        if (request["body"]!.AsArray().Count != 0)
        {
            var obj = new JsonObject();
            foreach (var m in request["body"]!.AsArray()) { var key = Text(m)!; if (values.ContainsKey(key)) obj[key] = values[key]?.DeepClone(); }
            body = Encoding.UTF8.GetBytes(obj.ToJsonString(JsonOptions)); headers["Content-Type"] = "application/json";
        }
        if (request["payload"] is JsonArray payload && values[Text(payload[0])!] is JsonNode encoded)
        {
            body = Text(request["payloadKind"]) switch
            {
                "string" => StrictUtf8.GetBytes(encoded.GetValue<string>()),
                "document" => Encoding.UTF8.GetBytes(encoded.ToJsonString(JsonOptions)),
                _ => Convert.FromBase64String(encoded.GetValue<string>())
            };
            if (!modelledContentType) headers["Content-Type"] = Text(payload[1])!;
        }
        // The release this client was generated for (docs/api-versions-clients.md).
        if (Text(schema["apiVersion"]) is string apiVersion) headers["Crucible-Version"] = apiVersion;
        return new(op, new Uri(config.Endpoint!.TrimEnd('/') + path), headers, body, name)
        {
            DisplayPath = display.Split('?', 2)[0], SensitiveHeaders = sensitiveHeaders, SensitiveQuery = sensitiveQuery
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
            // Sent as given, as other clients send it: a modelled Content-Type
            // that is empty or not a media type is the caller's to choose.
            request.Content.Headers.Remove(key);
            if (!request.Content.Headers.TryAddWithoutValidation(key, value)) throw new ArgumentException("Invalid header " + key);
        }
    }
    internal Task<T> CallAsync<T>(string name, object? input, CancellationToken cancellationToken) => TracedAsync(name, trace =>
    {
        var op = schema["operations"]![name]!.AsObject();
        var values = Encode(Text(op["input"]), input)?.AsObject() ?? new JsonObject();
        return SendAsync<T>(Prepare(name, values), trace, cancellationToken);
    });
    /// <summary>Adds the bearer token to a request of an operation that takes one; false when the request carries no credentials.</summary>
    private async ValueTask<bool> AuthenticateAsync(JsonObject op, HttpRequestMessage request, CancellationToken token)
    {
        if (!Flag(op["bearer"])) return false;
        if (request.Headers.TryGetValues("Authorization", out var existing))
        {
            var value = string.Join(",", existing);
            if (value.StartsWith("Bearer ", StringComparison.OrdinalIgnoreCase) && value.Length > "Bearer ".Length) return true;
        }
        if (config.Token is not null && await config.Token(token).AsTask().WaitAsync(token).ConfigureAwait(false) is string bearer && bearer.Length != 0)
        { SetHeader(request, "Authorization", "Bearer " + bearer); return true; }
        return false;
    }
    private async Task<T> SendAsync<T>(Prepared prepared, CallTrace trace, CancellationToken cancellationToken)
    {
        using var deadline = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        // The deadline always uses real time; the configured TimeProvider only drives policy sleeps and readings.
        deadline.CancelAfter(config.Timeout); var ct = deadline.Token;
        var op = prepared.Operation;
        int? lastCost = null;
        for (int attempt = 1; ; attempt++)
        {
            ct.ThrowIfCancellationRequested();
            var wait = retry.SendDelay();
            if (wait > TimeSpan.Zero)
            {
                try { await Task.Delay(wait, config.TimeProvider, ct).ConfigureAwait(false); }
                catch (OperationCanceledException) { retry.CancelSend(); throw; }
            }
            using var request = new HttpRequestMessage(new HttpMethod(Text(op["method"])!), prepared.Uri);
            if (prepared.Body is not null) request.Content = new ByteArrayContent(prepared.Body);
            foreach (var (key, value) in prepared.Headers) SetHeader(request, key, value);
            var authenticated = await AuthenticateAsync(op, request, ct).ConfigureAwait(false);
            foreach (var middleware in config.Middleware) await middleware(request, ct).AsTask().WaitAsync(ct).ConfigureAwait(false);
            if (request.RequestUri is null || !request.RequestUri.IsAbsoluteUri || (request.RequestUri.Scheme != "https" && request.RequestUri.Scheme != "http") || request.RequestUri.UserInfo.Length != 0)
                throw new ArgumentException("Middleware must supply an absolute HTTP(S) URI without credentials");
            if ((authenticated || request.Headers.Contains("Authorization")) && !Secure(request.RequestUri)) throw new ArgumentException("Credentials require HTTPS or loopback HTTP");
            ct.ThrowIfCancellationRequested();
            trace.AttemptStarted(prepared, request, config.Endpoint!);
            Exception? failure = null; bool timedOut = false;
            HttpResponseMessage? response = null; int? status = null; long? size = null;
            Dictionary<string, string>? headers = null;
            try
            {
                response = await http.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, ct).ConfigureAwait(false);
                status = (int)response.StatusCode;
                headers = response.Headers.Concat(response.Content.Headers).GroupBy(h => h.Key, StringComparer.OrdinalIgnoreCase)
                    .ToDictionary(g => g.Key, g => string.Join(", ", g.SelectMany(h => h.Value)), StringComparer.OrdinalIgnoreCase);
                var payload = op["response"]!["payload"] as JsonArray;
                bool hasBody = op["response"]!["body"]!.AsArray().Count != 0 || payload is not null;
                var bytes = response.IsSuccessStatusCode && !hasBody ? [] : await response.Content.ReadAsByteArrayAsync(ct).ConfigureAwait(false);
                size = bytes.Length;
                if (response.IsSuccessStatusCode)
                {
                    var output = Output<T>(op, payload, bytes, headers);
                    Succeeded(lastCost);
                    trace.AttemptFinished(status, null, size, RequestIdOf(null, headers));
                    return output;
                }
                failure = Error(op, (int)response.StatusCode, headers, bytes);
            }
            catch (Exception e) when (e is HttpRequestException or IOException || TimedOut(e, ct))
            { failure = new NetworkError(e.Message, e); timedOut = TimedOut(e, ct); }
            catch (Exception e) { Interrupted(status, e, size, headers, trace); throw; }
            finally { response?.Dispose(); }
            var decision = AfterFailure(prepared, failure!, timedOut, attempt, config.MaxAttempts, headers);
            trace.AttemptFinished(status, failure, size, RequestIdOf(failure, headers), decision);
            if (decision is not (_, var delay, var cost)) throw failure!;
            lastCost = cost;
            await Task.Delay(delay, config.TimeProvider, ct).ConfigureAwait(false);
        }
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
    private T Output<T>(JsonObject op, JsonArray? payload, byte[] bytes, Dictionary<string, string> headers)
    {
        var body = payload is not null || bytes.Length == 0 ? new JsonObject() : JsonNode.Parse(bytes)!.AsObject();
        var target = Text(op["output"]);
        if (target is null) return (T)(object)new Unit();
        if (payload is not null) body[Text(payload[0])!] = PayloadValue(Text(op["response"]!["payloadKind"]), bytes);
        ResponseBindings(target, op["response"]!, headers, body);
        return (T)Decode(target, body)!;
    }
    private ServiceError Error(JsonObject op, int status, Dictionary<string, string> headers, byte[] bytes)
    {
        JsonObject body;
        // BEGIN ERROR DOCUMENTATION
        var documentationBodyAbsent = bytes.All(value => value is 0x20 or 0x09 or 0x0d or 0x0a);
        var documentationBodyValid = documentationBodyAbsent;
        // END ERROR DOCUMENTATION
        try
        {
            var parsed = JsonNode.Parse(bytes) as JsonObject;
            // BEGIN ERROR DOCUMENTATION
            documentationBodyValid = documentationBodyAbsent || parsed is not null;
            // END ERROR DOCUMENTATION
            body = parsed ?? new JsonObject();
        }
        catch (JsonException) { body = new(); }
        static string? StringValue(JsonNode? v) => v is JsonValue j && j.TryGetValue<string>(out var s) ? s : null;
        var name = headers.GetValueOrDefault("x-crucible-error") ?? StringValue(body["__type"]);
        var candidates = op["errors"]!.AsArray().Select(n => Text(n)!).ToList();
        string? target;
        if (name is not null) target = candidates.Find(id => ErrorCode(id) == name);
        else { var matches = candidates.Where(id => Shape(id)["error"]!["status"]!.GetValue<int>() == status).ToList(); target = matches.Count == 1 ? matches[0] : null; }
        ServiceError error = new ServiceException();
        var metadata = target is null ? null : Shape(target)["error"];
        // BEGIN ERROR DOCUMENTATION
        var documentation = metadata?["documentation"] as JsonObject;
        var documentationUrl = Text(documentation?["url"]);
        // END ERROR DOCUMENTATION
        if (target is not null)
        {
            try
            {
                // BEGIN ERROR DOCUMENTATION
                if (metadata!["payload"] is not JsonArray && !documentationBodyValid)
                    throw new JsonException("modeled error body requires object");
                // END ERROR DOCUMENTATION
                if (metadata!["payload"] is JsonArray payload
                    // BEGIN ERROR DOCUMENTATION
                    && !(Text(metadata["payloadKind"]) == "document" && documentationBodyAbsent)
                    // END ERROR DOCUMENTATION
                )
                    body = new JsonObject { [Text(payload[0])!] = PayloadValue(Text(metadata["payloadKind"]), bytes) };
                ResponseBindings(target, metadata!, headers, body);
                error = (ServiceError)Decode(target, body)!;
                // BEGIN ERROR DOCUMENTATION
                var member = Text(documentation?["reasonMember"]);
                var reason = member is null ? null : StringValue(body.ContainsKey(member) ? body[member] : documentation?["reasonDefault"]);
                if (reason is not null && documentation?["reasonUrls"] is JsonObject pages
                    && pages.TryGetPropertyValue(reason, out var page)) documentationUrl = Text(page);
                // END ERROR DOCUMENTATION
            }
            catch (Exception e) when (e is JsonException or InvalidOperationException or FormatException or OverflowException or ArgumentException)
            {
                // Malformed wire members must not erase HTTP metadata or modeled retry policy.
            }
        }
        name ??= target is null ? "UnknownError" : ErrorCode(target);
        var requestId = headers.GetValueOrDefault("x-request-id") ?? StringValue(body["requestId"]);
        error.Initialize(name, StringValue(body["message"]) ?? $"HTTP {status}", status, requestId, headers, Flag(metadata?["retryable"]), Flag(metadata?["throttling"]));
        // BEGIN ERROR DOCUMENTATION
        error.DocUrl = documentationUrl;
        // END ERROR DOCUMENTATION
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
        var inputToken = Text(pagination["input"])!;
        if (values[inputToken] is JsonNode initial) seen.Add(initial.ToJsonString());
        while (true)
        {
            var result = await TracedAsync(name, trace => SendAsync<T>(Prepare(name, values), trace, cancellationToken)).ConfigureAwait(false);
            yield return result;
            JsonNode? next = Encode(Text(op["output"]), result);
            var target = Text(op["output"]);
            foreach (var path in pagination["output"]!.AsArray())
            {
                var member = Text(path)!;
                next = (next as JsonObject)?[member];
                target = Text(Shape(target!)["members"]![member]!["target"]);
            }
            if (next is null || next.GetValueKind() == JsonValueKind.String && next.GetValue<string>().Length == 0) yield break;
            if (!seen.Add(next.ToJsonString())) throw new InvalidOperationException("Pagination token cycle");
            values[inputToken] = next.DeepClone();
        }
    }
}
