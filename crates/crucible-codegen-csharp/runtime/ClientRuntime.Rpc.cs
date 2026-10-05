using System.Globalization;
using System.Numerics;
using System.Text.Json;
using System.Text.Json.Nodes;

// Smithy RPC v2 (CBOR and JSON): framing, and the schema-directed bridge between the JSON-shaped wire values
// Encode and Decode use and CBOR items, so blobs become byte strings, timestamps tag 1 epochs, and big numbers tags 2-4.
internal sealed partial class ClientRuntime
{
    private const int RpcDepth = 256;
    /// <summary>The last HTTP Digest challenge, answered preemptively with increasing nonce counts.</summary>
    private readonly DigestAuthentication digestSession = new();

    /// <summary>Whether RPC v2 uses CBOR, its media type, and its Smithy-Protocol value; null for other protocols.</summary>
    private (bool Cbor, string Media, string Marker)? Rpc => Text(schema["protocol"]) switch
    {
        "rpcv2Cbor" => (true, "application/cbor", "rpc-v2-cbor"),
        "rpcv2Json" => (false, "application/json", "rpc-v2-json"),
        _ => null,
    };

    /// <summary>The strongest Digest challenge a 401 offers, when this request may answer it.</summary>
    private IReadOnlyDictionary<string, string>? DigestChallenge(Prepared prepared, HttpRequestMessage request, Dictionary<string, string> headers)
    {
        if (config.DigestCredentials is null || prepared.Headers.ContainsKey("Authorization") || prepared.Stream is { Replayable: false }) return null;
        if (prepared.Operation["auth"]?.AsArray().Any(id => Text(id) == "smithy.api#httpDigestAuth") != true) return null;
        if (DigestAuthentication.Select(headers.GetValueOrDefault("WWW-Authenticate") ?? "") is not { } challenge) return null;
        // An answer that was already sent is retried only for a stale nonce.
        var answered = request.Headers.TryGetValues("Authorization", out var sent) && sent.Any(value => value.StartsWith("Digest ", StringComparison.OrdinalIgnoreCase));
        return !answered || string.Equals(challenge.GetValueOrDefault("stale"), "true", StringComparison.OrdinalIgnoreCase) ? challenge : null;
    }

    /// <summary>The request body of an RPC v2 call: an encoded map of the input members, or none without input.</summary>
    private byte[]? RpcRequestBody((bool Cbor, string Media, string Marker) rpc, string input, JsonObject document) =>
        rpc.Cbor ? Cbor.Encode(FromWire(input, document, 0)) : System.Text.Encoding.UTF8.GetBytes(document.ToJsonString(JsonOptions));

    /// <summary>An RPC v2 body as a wire object (for <paramref name="target"/>, or model-free); null when it is not a map.</summary>
    private JsonObject? RpcBody((bool Cbor, string Media, string Marker) rpc, byte[] bytes, string? target)
    {
        if (bytes.Length == 0) return new JsonObject();
        if (!rpc.Cbor) return JsonNode.Parse(bytes) as JsonObject;
        if (Cbor.Decode(bytes) is not CborMap map) return null;
        return (target is null ? Wire(map, 0) : ToWire(target, map, 0)) as JsonObject;
    }

    private T RpcOutput<T>((bool Cbor, string Media, string Marker) rpc, JsonObject op, byte[] bytes, Dictionary<string, string> headers)
    {
        // A response without the request's Smithy-Protocol value is malformed.
        if (headers.GetValueOrDefault("Smithy-Protocol") != rpc.Marker) throw new FormatException("Malformed RPC v2 response: missing or different Smithy-Protocol header");
        var target = Text(op["output"]);
        if (target is null) return (T)(object)new Unit();
        var body = RpcBody(rpc, bytes, target) ?? throw new FormatException("Malformed RPC v2 response body");
        return (T)Decode(target, body)!;
    }

    /// <summary>RPC v2 errors are identified only by the body's <c>__type</c>; X-Amzn-ErrorType and <c>code</c> are ignored.</summary>
    private ServiceError RpcError((bool Cbor, string Media, string Marker) rpc, JsonObject op, int status, Dictionary<string, string> headers, byte[] bytes)
    {
        JsonObject body = new(); CborMap? map = null;
        // A malformed response (another protocol, or an undecodable body) keeps only its status.
        if (headers.GetValueOrDefault("Smithy-Protocol") == rpc.Marker && bytes.Length != 0)
        {
            try
            {
                if (!rpc.Cbor) body = JsonNode.Parse(bytes) as JsonObject ?? new();
                else if (Cbor.Decode(bytes) is CborMap decoded) { map = decoded; body = Wire(decoded, 0) as JsonObject ?? new(); }
            }
            catch (Exception e) when (e is FormatException or JsonException) { }
        }
        static string? StringValue(JsonNode? v) => v is JsonValue j && j.TryGetValue<string>(out var s) ? s : null;
        var type = StringValue(body["__type"]);
        // __type is the error's absolute shape ID, or a bare name.
        var matches = type is null ? [] : op["errors"]!.AsArray().Select(n => Text(n)!)
            .Where(id => type.Contains('#') ? id == type : Text(Shape(id)["name"]) == type).ToList();
        var target = matches.Count == 1 ? matches[0] : null;
        ServiceError error = new();
        var metadata = target is null ? null : Shape(target)["error"];
        if (target is not null)
        {
            try { error = (ServiceError)Decode(target, map is null ? body : ToWire(target, map, 0))!; }
            catch (Exception e) when (e is JsonException or InvalidOperationException or FormatException or OverflowException or ArgumentException) { }
        }
        var name = type?.Split('#')[^1] ?? "UnknownError";
        var message = StringValue(body["message"]) ?? StringValue(body["Message"]) ?? $"HTTP {status}";
        error.Initialize(name, message, status, headers.GetValueOrDefault("x-request-id") ?? StringValue(body["requestId"]), headers,
            Flag(metadata?["retryable"]), Flag(metadata?["throttling"]));
        return error;
    }

    private static JsonNode Number(string text) => JsonNode.Parse(text)!;
    private static string NumberText(JsonNode value) => value.ToJsonString();
    private static JsonNode Floating(double value) => double.IsNaN(value) ? JsonValue.Create("NaN")
        : double.IsInfinity(value) ? JsonValue.Create(value > 0 ? "Infinity" : "-Infinity") : Number(value.ToString("R", CultureInfo.InvariantCulture));

    /// <summary>An integer CBOR item for exact digits: a 64-bit integer, or a tag 2/3 bignum beyond it.</summary>
    private static object Integer(BigInteger value)
    {
        if (value >= long.MinValue && value <= long.MaxValue) return (long)value;
        // Tag 3 carries -1 - n, so its magnitude is one less than the value's.
        var magnitude = value.Sign < 0 ? -1 - value : value;
        return new CborTag(value.Sign < 0 ? 3UL : 2UL, magnitude.ToByteArray(isUnsigned: true, isBigEndian: true));
    }

    private static BigInteger? Bignum(object? item) => item switch
    {
        long integer => integer,
        BigInteger big => big,
        CborTag { Tag: 2 or 3, Value: byte[] bytes } tag => tag.Tag == 2 ? new BigInteger(bytes, isUnsigned: true, isBigEndian: true)
            : -1 - new BigInteger(bytes, isUnsigned: true, isBigEndian: true),
        _ => null,
    };

    private static double FloatingValue(JsonNode value, bool single)
    {
        if (value is JsonValue text && text.TryGetValue<string>(out var name))
            return name switch { "NaN" => double.NaN, "Infinity" => double.PositiveInfinity, "-Infinity" => double.NegativeInfinity, _ => throw new FormatException("Invalid floating point string") };
        // A float member's value is the float itself, so its exact single-precision encoding is kept.
        if (single && value.AsValue().TryGetValue<float>(out var f)) return f;
        if (value.AsValue().TryGetValue<double>(out var d)) return d;
        return double.Parse(NumberText(value), CultureInfo.InvariantCulture);
    }

    /// <summary>Converts a wire value for <paramref name="target"/> into a CBOR item.</summary>
    private object? FromWire(string target, JsonNode? value, int depth)
    {
        if (depth >= RpcDepth) throw new FormatException("CBOR nesting limit");
        if (value is null) return null;
        var shape = Shape(target);
        switch (Text(shape["kind"]))
        {
            case "structure":
            case "union":
                var members = shape["members"]!.AsObject(); var map = new CborMap();
                foreach (var (name, item) in value.AsObject())
                    map.Add(name, members[name] is JsonObject member ? FromWire(Text(member["target"])!, item, depth + 1) : UnknownItem(item, depth + 1));
                return map;
            case "list":
                return value.AsArray().Select(item => FromWire(Text(shape["member"]!["target"])!, item, depth + 1)).ToList();
            case "map":
                var entries = new CborMap();
                foreach (var (key, item) in value.AsObject()) entries.Add(key, FromWire(Text(shape["value"]!["target"])!, item, depth + 1));
                return entries;
            case "blob": return Convert.FromBase64String(value.GetValue<string>());
            case "timestamp":
                var seconds = NumberText(value);
                return new CborTag(1, long.TryParse(seconds, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out var whole) ? (object)whole
                    : double.Parse(seconds, CultureInfo.InvariantCulture));
            case "float": return FloatingValue(value, true);
            case "double": return FloatingValue(value, false);
            case "byte": case "short": case "integer": case "long": case "intEnum":
                return long.Parse(NumberText(value), CultureInfo.InvariantCulture);
            case "bigInteger": return Integer(BigInteger.Parse(value.GetValueKind() == JsonValueKind.String ? value.GetValue<string>() : NumberText(value), NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture));
            case "bigDecimal":
                var (coefficient, exponent) = new BigDecimal(value.GetValueKind() == JsonValueKind.String ? value.GetValue<string>() : NumberText(value)).Normalize();
                return new CborTag(4, new List<object?> { (long)exponent, Integer(coefficient) });
            case "document": throw new NotSupportedException("Documents are not supported by RPC v2 CBOR");
            case "boolean": return value.GetValue<bool>();
            default: return value.GetValue<string>();
        }
    }

    /// <summary>Model-free wire to CBOR for unknown union variants: exact integers stay integers.</summary>
    private static object? UnknownItem(JsonNode? value, int depth)
    {
        if (depth >= RpcDepth) throw new FormatException("CBOR nesting limit");
        switch (value)
        {
            case null: return null;
            case JsonObject obj:
                var map = new CborMap();
                foreach (var (key, item) in obj) map.Add(key, UnknownItem(item, depth + 1));
                return map;
            case JsonArray array: return array.Select(item => UnknownItem(item, depth + 1)).ToList();
            default:
                return value.GetValueKind() switch
                {
                    JsonValueKind.String => value.GetValue<string>(),
                    JsonValueKind.True => true,
                    JsonValueKind.False => false,
                    _ => long.TryParse(NumberText(value), NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out var integer) ? (object)integer
                        : double.Parse(NumberText(value), CultureInfo.InvariantCulture),
                };
        }
    }

    /// <summary>Converts a decoded CBOR item into the wire value for <paramref name="target"/>.</summary>
    private JsonNode? ToWire(string target, object? item, int depth)
    {
        if (depth >= RpcDepth) throw new FormatException("CBOR nesting limit");
        if (item is null) return null;
        var shape = Shape(target); var kind = Text(shape["kind"]);
        switch (kind)
        {
            case "structure":
            case "union":
            case "unit":
                if (item is not CborMap map) throw new FormatException("Expected a CBOR map");
                var members = shape["members"]?.AsObject(); var result = new JsonObject();
                foreach (var (name, value) in map)
                {
                    if (members?[name] is JsonObject member) result[name] = ToWire(Text(member["target"])!, value, depth + 1);
                    // Unknown structure members are skipped; an unknown union variant keeps its value.
                    else if (kind == "union") result[name] = Wire(value, depth + 1);
                }
                return result;
            case "list":
                if (item is not List<object?> list) throw new FormatException("Expected a CBOR array");
                return new JsonArray(list.Select(value => ToWire(Text(shape["member"]!["target"])!, value, depth + 1)).ToArray());
            case "map":
                if (item is not CborMap entries) throw new FormatException("Expected a CBOR map");
                var wire = new JsonObject();
                foreach (var (key, value) in entries) wire[key] = ToWire(Text(shape["value"]!["target"])!, value, depth + 1);
                return wire;
            case "blob":
                if (item is not byte[] bytes) throw new FormatException("Expected a CBOR byte string");
                return JsonValue.Create(Convert.ToBase64String(bytes));
            case "timestamp":
                if (item is not CborTag { Tag: 1 } epoch) throw new FormatException("Expected a CBOR tag 1 epoch timestamp");
                return epoch.Value switch
                {
                    long seconds => Number(seconds.ToString(CultureInfo.InvariantCulture)),
                    double seconds when double.IsFinite(seconds) => Number(seconds.ToString("R", CultureInfo.InvariantCulture)),
                    _ => throw new FormatException("Invalid epoch timestamp"),
                };
            case "float":
            case "double":
                return item switch
                {
                    long integer => Number(integer.ToString(CultureInfo.InvariantCulture)),
                    double number => Floating(number),
                    _ => throw new FormatException("Expected a CBOR float"),
                };
            case "byte": case "short": case "integer": case "long": case "intEnum":
                return item is long or BigInteger ? Number(Convert.ToString(item, CultureInfo.InvariantCulture)!) : throw new FormatException("Expected a CBOR integer");
            case "bigInteger":
                return Bignum(item) is BigInteger digits ? Number(digits.ToString(CultureInfo.InvariantCulture)) : throw new FormatException("Expected a CBOR integer or bignum");
            case "bigDecimal":
                if (item is CborTag { Tag: 4, Value: List<object?> { Count: 2 } parts })
                {
                    if (parts[0] is not long exponent || Bignum(parts[1]) is not BigInteger mantissa) throw new FormatException("Invalid CBOR decimal fraction");
                    return Number(mantissa.ToString(CultureInfo.InvariantCulture) + "e" + exponent.ToString(CultureInfo.InvariantCulture));
                }
                return Bignum(item) is BigInteger integral ? Number(integral.ToString(CultureInfo.InvariantCulture)) : throw new FormatException("Expected a CBOR decimal fraction");
            case "string":
            case "enum":
                return item is string text ? JsonValue.Create(text) : throw new FormatException("Expected CBOR text");
            case "boolean":
                return item is bool flag ? JsonValue.Create(flag) : throw new FormatException("Expected a CBOR boolean");
            default:
                return Wire(item, depth + 1);
        }
    }

    /// <summary>Model-free CBOR to wire conversion: byte strings become base64 text, tags their content.</summary>
    private static JsonNode? Wire(object? item, int depth)
    {
        if (depth >= RpcDepth) throw new FormatException("CBOR nesting limit");
        switch (item)
        {
            case null: return null;
            case long or BigInteger: return Number(Convert.ToString(item, CultureInfo.InvariantCulture)!);
            case double number: return Floating(number);
            case byte[] bytes: return JsonValue.Create(Convert.ToBase64String(bytes));
            case CborTag tag: return Wire(tag.Value, depth + 1);
            case CborMap map:
                var result = new JsonObject();
                foreach (var (key, value) in map) result[key] = Wire(value, depth + 1);
                return result;
            case List<object?> list: return new JsonArray(list.Select(value => Wire(value, depth + 1)).ToArray());
            case string text: return JsonValue.Create(text);
            case bool flag: return JsonValue.Create(flag);
            default: throw new FormatException("Unexpected CBOR item");
        }
    }
}
