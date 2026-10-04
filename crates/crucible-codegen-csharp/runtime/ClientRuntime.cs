using System.Collections;
using System.Globalization;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

internal sealed class ClientRuntime : IDisposable
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
        if (!Uri.TryCreate(endpoint, UriKind.Absolute, out var uri) ||
            (uri.Scheme != "http" && uri.Scheme != "https") || uri.Host.Length == 0 ||
            uri.UserInfo.Length != 0 || uri.Query.Length != 0 || uri.Fragment.Length != 0)
            throw new ArgumentException("Absolute HTTP(S) endpoint without credentials, query, or fragment required");
        if (input.MaxAttempts < 1 || input.Timeout <= TimeSpan.Zero || input.Timeout.TotalMilliseconds > uint.MaxValue - 1 ||
            input.BaseDelay < TimeSpan.Zero || input.MaxDelay < TimeSpan.Zero || input.MaxDelay.TotalMilliseconds > int.MaxValue)
            throw new ArgumentException("Invalid attempt limit, timeout, or retry delay");
        config = new ClientConfig {
            Endpoint = endpoint.TrimEnd('/'), Token = input.Token, MaxAttempts = input.MaxAttempts,
            Timeout = input.Timeout, BaseDelay = input.BaseDelay, MaxDelay = input.MaxDelay,
            Headers = new(input.Headers, StringComparer.OrdinalIgnoreCase), Middleware = [.. input.Middleware]
        };
        ownsHttp = input.HttpClient is null;
        http = input.HttpClient ?? new HttpClient(new SocketsHttpHandler { AllowAutoRedirect = false }) { Timeout = System.Threading.Timeout.InfiniteTimeSpan };
        this.digest = digest; this.schema = schema; this.types = types;
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
    private static object New(Type type) => Activator.CreateInstance(type)!;

    internal JsonNode? Encode(string? target, object? value, string? format = null)
    {
        if (target is null) return new JsonObject();
        var shape = Shape(target); var kind = Text(shape["kind"]);
        if (kind == "unit") return new JsonObject();
        if (value is null) return null;
        switch (kind)
        {
            case "timestamp": return WriteTime((DateTimeOffset)value, format ?? Text(shape["format"]));
            case "enum": return JsonValue.Create(value.ToString());
            case "document": return ((JsonNode)value).DeepClone();
            case "list":
                var array = new JsonArray(); var item = shape["member"]!;
                foreach (var element in (IEnumerable)value) array.Add(Encode(Text(item["target"]), element, Text(item["format"])));
                return array;
            case "map":
                var map = new JsonObject(); var member = shape["value"]!;
                foreach (DictionaryEntry entry in (IDictionary)value)
                    map.Add(entry.Key.ToString()!, Encode(Text(member["target"]), entry.Value, Text(member["format"])));
                return map;
            case "structure":
            case "union":
                var result = new JsonObject();
                foreach (var (name, description) in shape["members"]!.AsObject())
                {
                    var v = value.GetType().GetProperty(Text(description!["attribute"])!)!.GetValue(value);
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
            case "boolean": return value.GetValue<bool>();
            case "byte": return checked((sbyte)Integer(value));
            case "short": return checked((short)Integer(value));
            case "integer": return checked((int)Integer(value));
            case "long": return checked((long)Integer(value));
            case "float": var f = value.GetValue<float>(); if (!float.IsFinite(f)) throw new JsonException("Float overflow"); return f;
            case "double": var d = value.GetValue<double>(); if (!double.IsFinite(d)) throw new JsonException("Double overflow"); return d;
            case "timestamp": return ReadTime(value, format ?? Text(shape["format"]));
            case "document": return value.DeepClone();
            case "enum": return Activator.CreateInstance(types[target], value.GetValue<string>());
            case "list":
                var list = (IList)New(types[target]); var member = shape["member"]!;
                foreach (var item in value.AsArray()) list.Add(Decode(Text(member["target"]), item, Text(member["format"])));
                return list;
            case "map":
                var map = (IDictionary)New(types[target]); var val = shape["value"]!; var key = shape["key"]!;
                foreach (var (name, item) in value.AsObject())
                    map.Add(Decode(Text(key["target"]), JsonValue.Create(name))!, Decode(Text(val["target"]), item, Text(val["format"])));
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
                    if (!obj.TryGetPropertyValue(name, out var item)) { item = description!["default"]; if (item is null) continue; }
                    var decoded = Decode(Text(description!["target"]), item, Text(description["format"]));
                    var property = result.GetType().GetProperty(Text(description["attribute"])!)!;
                    if (property.PropertyType.IsGenericType && property.PropertyType.GetGenericTypeDefinition() == typeof(Optional<>))
                        decoded = property.PropertyType.GetMethod("FromWire", BindingFlags.Static | BindingFlags.NonPublic)!.Invoke(null, [decoded]);
                    else if (decoded is null && property.PropertyType.IsValueType) throw new JsonException($"Null for non-nullable member {name}");
                    property.SetValue(result, decoded);
                }
                return result;
            default: throw new JsonException($"Unknown wire kind {kind}");
        }
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
            return text.IndexOfAny([',', '"', '\\']) >= 0 || text.Trim() != text ? JsonSerializer.Serialize(text, JsonOptions) : text;
        }));
    }
    internal JsonNode? Header(JsonNode member, string value)
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
            foreach (var part in parts) result.Add(Header(shape["member"]!, part.StartsWith('"') ? JsonSerializer.Deserialize<string>(part)! : part));
            return result;
        }
        if (kind is "byte" or "short" or "integer" or "long" or "float" or "double" or "boolean" ||
            kind == "timestamp" && Text(member["format"]) == "epoch-seconds") return JsonNode.Parse(value);
        return JsonValue.Create(value);
    }
    private sealed record Prepared(JsonObject Operation, Uri Uri, Dictionary<string, string> Headers, byte[]? Body);
    private Prepared Prepare(string name, JsonObject values)
    {
        var op = schema["operations"]![name]!.AsObject(); var request = op["request"]!;
        var token = Text(op["token"]);
        if (token is not null && values[token] is null) values[token] = Guid.NewGuid().ToString();
        var path = Text(op["uri"])!;
        foreach (var label in request["labels"]!.AsArray())
        {
            var key = Text(label)!; var value = values[key];
            if (value is null || Bound(value).Length == 0) throw new ArgumentException($"{key} is required");
            path = path.Replace("{" + key + "}", Uri.EscapeDataString(Bound(value)), StringComparison.Ordinal);
        }
        var query = new List<string>();
        foreach (var binding in request["query"]!.AsArray())
        {
            var value = values[Text(binding![0])!]; if (value is null) continue;
            IEnumerable<JsonNode?> items = value is JsonArray array ? array : new[] { value };
            foreach (var item in items) query.Add(Uri.EscapeDataString(Text(binding[1])!) + "=" + Uri.EscapeDataString(Scalar(item!)));
        }
        if (query.Count != 0) path += (path.Contains('?') ? "&" : "?") + string.Join("&", query);
        var headers = new Dictionary<string, string>(config.Headers, StringComparer.OrdinalIgnoreCase);
        foreach (var binding in request["headers"]!.AsArray())
        {
            var value = values[Text(binding![0])!]; if (value is not null) headers[Text(binding[1])!] = Bound(value);
        }
        byte[]? body = null;
        if (request["body"]!.AsArray().Count != 0)
        {
            var obj = new JsonObject();
            foreach (var m in request["body"]!.AsArray()) { var key = Text(m)!; if (values.ContainsKey(key)) obj[key] = values[key]?.DeepClone(); }
            body = Encoding.UTF8.GetBytes(obj.ToJsonString(JsonOptions)); headers["Content-Type"] = "application/json";
        }
        return new(op, new Uri(config.Endpoint + path), headers, body);
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
    internal Task<T> CallAsync<T>(string name, object? input, CancellationToken cancellationToken)
    {
        var op = schema["operations"]![name]!;
        var values = Encode(Text(op["input"]), input)?.AsObject() ?? new JsonObject();
        return SendAsync<T>(Prepare(name, values), cancellationToken);
    }
    private async Task<T> SendAsync<T>(Prepared prepared, CancellationToken cancellationToken)
    {
        using var deadline = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        deadline.CancelAfter(config.Timeout); var ct = deadline.Token;
        var op = prepared.Operation; bool safe = Flag(op["safe"]);
        for (int attempt = 0; ; attempt++)
        {
            ct.ThrowIfCancellationRequested();
            using var request = new HttpRequestMessage(new HttpMethod(Text(op["method"])!), prepared.Uri);
            if (prepared.Body is not null) request.Content = new ByteArrayContent(prepared.Body);
            foreach (var (key, value) in prepared.Headers) SetHeader(request, key, value);
            if (Flag(schema["bearer"]) && config.Token is not null)
            {
                var token = await config.Token(ct).AsTask().WaitAsync(ct).ConfigureAwait(false);
                if (token is not null) SetHeader(request, "Authorization", "Bearer " + token);
            }
            foreach (var middleware in config.Middleware) await middleware(request, ct).AsTask().WaitAsync(ct).ConfigureAwait(false);
            if (request.RequestUri is null || !request.RequestUri.IsAbsoluteUri || (request.RequestUri.Scheme != "https" && request.RequestUri.Scheme != "http") || request.RequestUri.UserInfo.Length != 0)
                throw new ArgumentException("Middleware must supply an absolute HTTP(S) URI without credentials");
            if (request.Headers.Contains("Authorization") && !Secure(request.RequestUri)) throw new ArgumentException("Credentials require HTTPS or loopback HTTP");
            if (digest)
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
            Exception? failure = null; TimeSpan delay = TimeSpan.Zero;
            try
            {
                using var response = await http.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, ct).ConfigureAwait(false);
                var headers = response.Headers.Concat(response.Content.Headers).GroupBy(h => h.Key, StringComparer.OrdinalIgnoreCase)
                    .ToDictionary(g => g.Key, g => string.Join(", ", g.SelectMany(h => h.Value)), StringComparer.OrdinalIgnoreCase);
                bool hasBody = op["response"]!["body"]!.AsArray().Count != 0;
                var bytes = response.IsSuccessStatusCode && !hasBody ? [] : await response.Content.ReadAsByteArrayAsync(ct).ConfigureAwait(false);
                if (response.IsSuccessStatusCode)
                {
                    var body = bytes.Length == 0 ? new JsonObject() : JsonNode.Parse(bytes)!.AsObject();
                    var target = Text(op["output"]);
                    if (target is null) return (T)(object)new Unit();
                    foreach (var binding in op["response"]!["headers"]!.AsArray())
                    {
                        var name = Text(binding![0])!;
                        if (headers.TryGetValue(Text(binding[1])!, out var value)) body[name] = Header(Shape(target)["members"]![name]!, value);
                    }
                    return (T)Decode(target, body)!;
                }
                var error = Error(op, (int)response.StatusCode, headers, bytes);
                if (attempt + 1 >= config.MaxAttempts || !(error.Throttling || safe && (error.Retryable || error.StatusCode is 500 or 502 or 503 or 504))) throw error;
                failure = error; delay = Delay(attempt, headers);
            }
            catch (HttpRequestException e) { failure = new NetworkError(e.Message, e); if (!safe || attempt + 1 >= config.MaxAttempts) throw failure; delay = Delay(attempt, null); }
            catch (IOException e) { failure = new NetworkError(e.Message, e); if (!safe || attempt + 1 >= config.MaxAttempts) throw failure; delay = Delay(attempt, null); }
            if (failure is not null) await Task.Delay(delay, ct).ConfigureAwait(false);
        }
    }
    private ServiceError Error(JsonObject op, int status, Dictionary<string, string> headers, byte[] bytes)
    {
        JsonObject body;
        try { body = JsonNode.Parse(bytes) as JsonObject ?? new JsonObject(); } catch (JsonException) { body = new(); }
        static string? StringValue(JsonNode? v) => v is JsonValue j && j.TryGetValue<string>(out var s) ? s : null;
        var name = StringValue(body["__type"]);
        var candidates = op["errors"]!.AsArray().Select(n => Text(n)!).ToList();
        string? target;
        if (name is not null) target = candidates.Find(id => Text(Shape(id)["name"]) == name);
        else { var matches = candidates.Where(id => Shape(id)["error"]!["status"]!.GetValue<int>() == status).ToList(); target = matches.Count == 1 ? matches[0] : null; }
        if (target is not null)
        {
            foreach (var binding in Shape(target)["error"]!["headers"]!.AsArray())
            {
                var member = Text(binding![0])!;
                if (headers.TryGetValue(Text(binding[1])!, out var value)) body[member] = Header(Shape(target)["members"]![member]!, value);
            }
        }
        var error = target is null ? new ServiceError() : (ServiceError)Decode(target, body)!;
        var metadata = target is null ? null : Shape(target)["error"];
        name ??= target is null ? "UnknownError" : Text(Shape(target)["name"])!;
        var requestId = headers.GetValueOrDefault("x-request-id") ?? StringValue(body["requestId"]);
        error.Initialize(name, StringValue(body["message"]) ?? $"HTTP {status}", status, requestId, headers, Flag(metadata?["retryable"]), Flag(metadata?["throttling"]));
        return error;
    }
    private TimeSpan Delay(int attempt, Dictionary<string, string>? headers)
    {
        if (headers is not null && headers.TryGetValue("Retry-After", out var value))
        {
            if (double.TryParse(value, NumberStyles.Float, CultureInfo.InvariantCulture, out var seconds) && double.IsFinite(seconds) && seconds >= 0)
                return TimeSpan.FromMilliseconds(Math.Min(uint.MaxValue - 1d, seconds * 1000));
            if (DateTimeOffset.TryParse(value, CultureInfo.InvariantCulture, DateTimeStyles.AssumeUniversal, out var date))
            {
                var remaining = (date - DateTimeOffset.UtcNow).TotalMilliseconds;
                if (remaining >= 0) return TimeSpan.FromMilliseconds(Math.Min(uint.MaxValue - 1d, remaining));
            }
        }
        return TimeSpan.FromMilliseconds(Random.Shared.NextDouble() * Math.Min(config.MaxDelay.TotalMilliseconds, config.BaseDelay.TotalMilliseconds * Math.Pow(2, Math.Min(attempt, 30))));
    }
    internal async IAsyncEnumerable<T> PagesAsync<T>(string name, object? input, [EnumeratorCancellation] CancellationToken cancellationToken)
    {
        var op = schema["operations"]![name]!; var pagination = op["pagination"]!;
        var values = Encode(Text(op["input"]), input)?.AsObject() ?? new JsonObject(); var seen = new HashSet<string>();
        var inputToken = Text(pagination["input"])!;
        if (values[inputToken] is JsonNode initial) seen.Add(initial.ToJsonString());
        while (true)
        {
            var result = await SendAsync<T>(Prepare(name, values), cancellationToken).ConfigureAwait(false);
            yield return result;
            JsonNode? next = Encode(Text(op["output"]), result);
            foreach (var path in pagination["output"]!.AsArray()) next = (next as JsonObject)?[Text(path)!];
            if (next is null || next.GetValueKind() == JsonValueKind.String && next.GetValue<string>().Length == 0) yield break;
            if (!seen.Add(next.ToJsonString())) throw new InvalidOperationException("Pagination token cycle");
            values[inputToken] = next.DeepClone();
        }
    }
}
