// Runs the official Smithy RPC v2 protocol tests (lowered into the package's embedded schema) against the
// generated client: every client-applicable request and response case, for CBOR or JSON.
using System.Globalization;
using System.Net;
using System.Numerics;
using System.Reflection;
using System.Runtime.ExceptionServices;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Example.Rpc;

static class Program
{
    const BindingFlags Hidden = BindingFlags.NonPublic | BindingFlags.Instance;
    static readonly Assembly Package = typeof(ClientConfig).Assembly;
    static readonly Type RuntimeType = Package.GetType("Example.Rpc.ClientRuntime")!;
    static readonly IReadOnlyDictionary<string, Type> Types =
        (IReadOnlyDictionary<string, Type>)Package.GetType("Example.Rpc.ModelRegistry")!.GetField("Types", BindingFlags.NonPublic | BindingFlags.Static)!.GetValue(null)!;
    static JsonObject schema = null!;
    static bool bigStrings;

    static void Check(bool ok, string message) { if (!ok) throw new Exception(message); }
    static string? Text(JsonNode? node) => node?.GetValue<string>();
    static JsonObject Shape(string id) => schema["shapes"]![id]!.AsObject();
    static object? Invoke(object target, string name, params object?[] args)
    {
        try { return RuntimeType.GetMethod(name, Hidden)!.Invoke(target, args); }
        catch (TargetInvocationException e) { ExceptionDispatchInfo.Capture(e.InnerException!).Throw(); throw; }
    }

    sealed class Handler(Func<HttpRequestMessage, byte[], HttpResponseMessage> respond) : HttpMessageHandler
    {
        public HttpRequestMessage? Request; public byte[] Body = [];
        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken token)
        {
            Request = request; Body = request.Content is null ? [] : await request.Content.ReadAsByteArrayAsync(token);
            return respond(request, Body);
        }
    }

    /// <summary>Smithy test params (node values) to the wire values the runtime decodes.</summary>
    static JsonNode? Node(string target, JsonNode? value)
    {
        if (value is null) return null;
        var shape = Shape(target);
        switch (Text(shape["kind"]))
        {
            case "structure":
            case "union":
                var result = new JsonObject();
                foreach (var (name, item) in value.AsObject()) result[name] = Node(Text(shape["members"]![name]!["target"])!, item);
                return result;
            case "list": return new JsonArray(value.AsArray().Select(item => Node(Text(shape["member"]!["target"])!, item)).ToArray());
            case "map":
                var map = new JsonObject();
                foreach (var (key, item) in value.AsObject()) map[key] = Node(Text(shape["value"]!["target"])!, item);
                return map;
            case "blob": return Convert.ToBase64String(Encoding.UTF8.GetBytes(value.GetValue<string>()));
            case "timestamp":
                if (value.GetValueKind() != JsonValueKind.String) return value.DeepClone();
                var instant = DateTimeOffset.Parse(value.GetValue<string>(), CultureInfo.InvariantCulture, DateTimeStyles.AssumeUniversal);
                return JsonValue.Create((instant.UtcTicks - DateTimeOffset.UnixEpoch.UtcTicks) / (decimal)TimeSpan.TicksPerSecond);
            case "bigInteger":
            case "bigDecimal":
                return bigStrings ? JsonValue.Create(value.ToJsonString()) : value.DeepClone();
            default: return value.DeepClone();
        }
    }

    /// <summary>Wire values compared exactly, numbers by value (and float members at single precision).</summary>
    static bool Equivalent(JsonNode? left, JsonNode? right)
    {
        if (left is null || right is null) return left is null && right is null;
        if (left is JsonObject a && right is JsonObject b) return a.Count == b.Count && a.All(pair => b.ContainsKey(pair.Key) && Equivalent(pair.Value, b[pair.Key]));
        if (left is JsonArray x && right is JsonArray y) return x.Count == y.Count && x.Zip(y).All(pair => Equivalent(pair.First, pair.Second));
        if (left.GetValueKind() == JsonValueKind.Number && right.GetValueKind() == JsonValueKind.Number)
        {
            string l = left.ToJsonString(), r = right.ToJsonString();
            if (decimal.TryParse(l, NumberStyles.Float, CultureInfo.InvariantCulture, out var dl) && decimal.TryParse(r, NumberStyles.Float, CultureInfo.InvariantCulture, out var dr) && dl == dr) return true;
            double fl = double.Parse(l, CultureInfo.InvariantCulture), fr = double.Parse(r, CultureInfo.InvariantCulture);
            return fl == fr || (float)fl == (float)fr;
        }
        return JsonNode.DeepEquals(left, right);
    }

    /// <summary>Decoded CBOR items compared by value: maps by key, floats and integers numerically.</summary>
    static bool Same(object? left, object? right)
    {
        static double? Numeric(object? v) => v switch { long l => l, double d => d, BigInteger b => (double)b, _ => null };
        if (Numeric(left) is double l && Numeric(right) is double r) return double.IsNaN(l) ? double.IsNaN(r) : l == r;
        return (left, right) switch
        {
            (null, null) => true,
            (byte[] a, byte[] b) => a.AsSpan().SequenceEqual(b),
            (CborTag a, CborTag b) => a.Tag == b.Tag && Same(a.Value, b.Value),
            (CborMap a, CborMap b) => a.Count == b.Count && a.All(pair => b.TryGetValue(pair.Key, out var other) && Same(pair.Value, other)),
            (List<object?> a, List<object?> b) => a.Count == b.Count && a.Zip(b).All(pair => Same(pair.First, pair.Second)),
            _ => Equals(left, right),
        };
    }
    static string Show(object? item) => item switch
    {
        null => "null",
        byte[] bytes => "h'" + Convert.ToHexString(bytes) + "'",
        CborTag tag => tag.Tag + "(" + Show(tag.Value) + ")",
        CborMap map => "{" + string.Join(", ", map.Select(pair => pair.Key + ": " + Show(pair.Value))) + "}",
        List<object?> list => "[" + string.Join(", ", list.Select(Show)) + "]",
        double d => d.ToString("R", CultureInfo.InvariantCulture),
        _ => Convert.ToString(item, CultureInfo.InvariantCulture)!,
    };

    static async Task Main()
    {
        using (var stream = Package.GetManifestResourceStream("crucible.schema.json")!) schema = JsonNode.Parse(stream)!.AsObject();
        var protocol = Text(schema["protocol"])!;
        var cbor = protocol == "rpcv2Cbor";
        bigStrings = schema["protocolSettings"]?["arbitraryPrecisionStrings"]?.GetValue<bool>() == true;
        int ran = 0;
        foreach (var test in schema["protocolTests"]!.AsArray())
        {
            var id = Text(test!["id"])!;
            if (Text(test["protocol"]) != "smithy.protocols#" + protocol || Text(test["appliesTo"]) == "server") continue;
            var @case = test["case"]!; var message = @case["message"]!;
            var name = Text(test["operation"])!.Split('#')[^1];
            var op = schema["operations"]![name]!;
            void Fail(string why) => throw new Exception(id + ": " + why);
            var expectedHeaders = (message["headers"] as JsonObject ?? new JsonObject()).ToDictionary(pair => pair.Key, pair => Text(pair.Value)!, StringComparer.OrdinalIgnoreCase);
            if (Text(@case["kind"]) == "request")
            {
                var handler = new Handler((_, _) => new HttpResponseMessage(HttpStatusCode.InternalServerError));
                using var http = new HttpClient(handler);
                var runtime = Activator.CreateInstance(RuntimeType, Hidden, null, [new ClientConfig { Endpoint = "https://example.test", HttpClient = http, MaxAttempts = 1 }, "", false], null)!;
                object? input = Text(test["shape"]) is string shape ? Invoke(runtime, "Decode", shape, Node(shape, test["params"]), null) : null;
                try { await (Task<object>)RuntimeType.GetMethod("CallAsync", Hidden)!.MakeGenericMethod(typeof(object)).Invoke(runtime, [name, input, CancellationToken.None])!; }
                catch (ServiceError) { /* Only the request is under test. */ }
                var request = handler.Request ?? throw new Exception(id + ": no request was sent");
                if (request.Method.Method != Text(@case["method"])) Fail("method " + request.Method);
                if (request.RequestUri!.AbsolutePath != Text(@case["uri"]) || request.RequestUri.Query.Length != 0) Fail("URI " + request.RequestUri);
                var sent = request.Headers.Concat(request.Content?.Headers ?? Enumerable.Empty<KeyValuePair<string, IEnumerable<string>>>())
                    .ToDictionary(pair => pair.Key, pair => string.Join(", ", pair.Value), StringComparer.OrdinalIgnoreCase);
                foreach (var (header, value) in expectedHeaders) if (sent.GetValueOrDefault(header) != value) Fail($"header {header}: {sent.GetValueOrDefault(header) ?? "missing"}");
                foreach (var header in message["forbidHeaders"]?.AsArray() ?? []) if (sent.ContainsKey(Text(header)!)) Fail("forbidden header " + header);
                foreach (var header in message["requireHeaders"]?.AsArray() ?? [])
                    if (Text(header)!.Equals("Content-Length", StringComparison.OrdinalIgnoreCase) ? request.Content?.Headers.ContentLength is null : !sent.ContainsKey(Text(header)!)) Fail("required header " + header);
                var expected = Text(message["body"]);
                if (string.IsNullOrEmpty(expected)) { if (handler.Body.Length != 0) Fail("unexpected body"); }
                else if (cbor)
                {
                    object? want = Cbor.Decode(Convert.FromBase64String(expected)), got = Cbor.Decode(handler.Body);
                    if (!Same(want, got)) Fail("body\n  want " + Show(want) + "\n  got  " + Show(got));
                }
                else
                {
                    JsonNode? want = JsonNode.Parse(expected), got = JsonNode.Parse(handler.Body);
                    if (!Equivalent(want, got)) Fail("body\n  want " + want?.ToJsonString() + "\n  got  " + got?.ToJsonString());
                }
            }
            else
            {
                var body = Text(message["body"]) ?? "";
                var bytes = cbor ? Convert.FromBase64String(body) : Encoding.UTF8.GetBytes(body);
                var handler = new Handler((_, _) =>
                {
                    var response = new HttpResponseMessage((HttpStatusCode)@case["code"]!.GetValue<int>()) { Content = new ByteArrayContent(bytes) };
                    foreach (var (header, value) in expectedHeaders)
                        if (!response.Headers.TryAddWithoutValidation(header, value)) response.Content.Headers.TryAddWithoutValidation(header, value);
                    return response;
                });
                using var http = new HttpClient(handler);
                var runtime = Activator.CreateInstance(RuntimeType, Hidden, null, [new ClientConfig { Endpoint = "https://example.test", HttpClient = http, MaxAttempts = 1 }, "", false], null)!;
                object? input = Text(op["input"]) is string inputShape ? Activator.CreateInstance(Types[inputShape]) : null;
                var shape = Text(test["shape"]);
                object? output;
                try
                {
                    output = await (Task<object>)RuntimeType.GetMethod("CallAsync", Hidden)!.MakeGenericMethod(typeof(object)).Invoke(runtime, [name, input, CancellationToken.None])!;
                    if (test["error"]?.GetValue<bool>() == true) Fail("expected an error");
                }
                catch (ServiceError error)
                {
                    if (test["error"]?.GetValue<bool>() != true) Fail($"unexpected error {error.ErrorName}: {error.Message}");
                    if (error.GetType() != Types[shape!]) Fail("error class " + error.GetType().Name);
                    output = error;
                }
                if (shape is not null && test["params"] is JsonNode parameters)
                {
                    var want = Node(shape, parameters);
                    var got = (JsonNode?)Invoke(runtime, "Encode", shape, output, null);
                    if (!Equivalent(want, got)) Fail("output\n  want " + want?.ToJsonString() + "\n  got  " + got?.ToJsonString());
                }
            }
            ran++;
        }
        Check(ran >= 40, $"only {ran} protocol tests ran");
        Console.WriteLine($"C# {protocol}: {ran} official Smithy protocol tests passed");
    }
}
