// Exercises @deprecated members on purpose; the consumer builds with -warnaserror.
#pragma warning disable CS0618
using System.Net;
using System.Net.Sockets;
using System.Reflection;
using System.Runtime.ExceptionServices;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Example.Weather;
using Example.Weather.Models;
using Forecast = System.Collections.Generic.Dictionary<Example.Weather.Models.Sky, System.Text.Json.Nodes.JsonNode>;

static class Program
{
    static void Check(bool ok, string message) { if (!ok) throw new Exception(message); }
    static JsonNode J(string text) => JsonNode.Parse(text)!;
    static readonly Type RuntimeType = typeof(WeatherClient).Assembly.GetType("Example.Weather.ClientRuntime")!;
    static object Runtime(ClientConfig config, JsonObject? schema = null, Dictionary<string, Type>? registry = null) =>
        Activator.CreateInstance(RuntimeType, BindingFlags.NonPublic | BindingFlags.Instance, null,
            schema is null ? [config, "", false] : [config, "", false, schema, registry!], null)!;
    static object? Invoke(object runtime, string name, params object?[] args)
    {
        try { return RuntimeType.GetMethod(name, BindingFlags.NonPublic | BindingFlags.Instance)!.Invoke(runtime, args); }
        catch (TargetInvocationException e) { ExceptionDispatchInfo.Capture(e.InnerException!).Throw(); throw; }
    }
    static JsonNode Encode(object runtime, string target, object value) => (JsonNode)Invoke(runtime, "Encode", target, value, null)!;
    static object Decode(object runtime, string target, string json) => Invoke(runtime, "Decode", target, J(json), null)!;
    static Task<Unit> Call(object runtime) => (Task<Unit>)RuntimeType.GetMethod("CallAsync", BindingFlags.NonPublic | BindingFlags.Instance)!
        .MakeGenericMethod(typeof(Unit)).Invoke(runtime, ["Retry", null, CancellationToken.None])!;

    sealed record Request(string Path, Dictionary<string, string> Headers, byte[] Body);
    sealed record Response(int Status, string Body, Dictionary<string, string>? Headers = null, bool Truncated = false, bool Stall = false, byte[]? BinaryBody = null);
    sealed class Server : IAsyncDisposable
    {
        readonly TcpListener listener = new(IPAddress.Loopback, 0);
        readonly CancellationTokenSource stop = new();
        readonly Task loop;
        public string Endpoint { get; }
        public int Attempts;
        public Server(Func<Request, Response> handler)
        {
            listener.Start(); Endpoint = $"http://127.0.0.1:{((IPEndPoint)listener.LocalEndpoint).Port}";
            loop = Task.Run(async () => {
                try
                {
                    while (!stop.IsCancellationRequested)
                    {
                        using var socket = await listener.AcceptTcpClientAsync(stop.Token);
                        await using var stream = socket.GetStream();
                        var head = new List<byte>(); var one = new byte[1];
                        while (true)
                        {
                            if (await stream.ReadAsync(one, stop.Token) == 0) throw new IOException("missing request headers");
                            head.Add(one[0]);
                            if (head.Count >= 4 && head.TakeLast(4).SequenceEqual(new byte[] { 13, 10, 13, 10 })) break;
                        }
                        var lines = Encoding.ASCII.GetString(head.ToArray()).Split("\r\n", StringSplitOptions.RemoveEmptyEntries);
                        var headers = lines.Skip(1).Select(line => line.Split(':', 2)).ToDictionary(p => p[0], p => p[1].Trim(), StringComparer.OrdinalIgnoreCase);
                        var body = new byte[int.Parse(headers.GetValueOrDefault("Content-Length", "0"))];
                        await stream.ReadExactlyAsync(body, stop.Token);
                        Attempts++;
                        var response = handler(new(lines[0].Split(' ')[1], headers, body));
                        var bytes = response.BinaryBody ?? Encoding.UTF8.GetBytes(response.Body);
                        var responseHeaders = $"HTTP/1.1 {response.Status} Test\r\nContent-Length: {bytes.Length + (response.Truncated ? 10 : 0)}\r\nConnection: close\r\n";
                        if (response.Headers is not null) foreach (var (key, value) in response.Headers) responseHeaders += $"{key}: {value}\r\n";
                        await stream.WriteAsync(Encoding.ASCII.GetBytes(responseHeaders + "\r\n"), stop.Token);
                        if (response.Stall) await Task.Delay(5000, stop.Token);
                        await stream.WriteAsync(bytes, stop.Token);
                    }
                }
                // Stopping the listener fails a pending accept in more ways
                // than cancellation (InvalidOperationException, SocketException).
                catch (Exception) when (stop.IsCancellationRequested) { }
            });
        }
        public async ValueTask DisposeAsync() { stop.Cancel(); listener.Stop(); await loop; stop.Dispose(); }
    }
    static async Task RetryCases(string path)
    {
        var cases = J(File.ReadAllText(path)).AsArray();
        cases.Add(J("""{"name":"renamed-error","safe":false,"status":429,"error":"RenamedThrottle","failures":1,"maxAttempts":3,"attempts":2,"success":true}"""));
        cases.Add(J("""{"name":"explicit-empty-error","safe":false,"status":429,"error":"","failures":1,"maxAttempts":3,"attempts":1,"success":false}"""));
        cases.Add(J("""{"name":"explicit-unknown-error","safe":false,"status":429,"error":"ns#Throttled","failures":1,"maxAttempts":3,"attempts":1,"success":false}"""));
        foreach (var test in cases)
        {
            int count = 0;
            await using var server = new Server(_ => new(++count <= test!["failures"]!.GetValue<int>() ? test["status"]!.GetValue<int>() : 200,
                new JsonObject { ["__type"] = test!["error"]?.DeepClone() }.ToJsonString()));
            var schema = J("""{"bearer":false,"shapes":{},"operations":{"Retry":{"method":"POST","uri":"/","input":null,"output":null,"token":null,"request":{"labels":[],"query":[],"headers":[],"body":[]},"response":{"headers":[],"body":[]},"errors":["Ordinary","Transient","Throttled"]}}}""").AsObject();
            var types = new Dictionary<string, Type>();
            foreach (var name in new[] { "Ordinary", "Transient", "Throttled" })
            {
                var shape = J("""{"kind":"structure","members":{},"error":{"headers":[]}}""");
                shape["name"] = name == "Throttled" && test!["name"]!.GetValue<string>() == "renamed-error" ? "RenamedThrottle" : name; shape["error"]!["status"] = name == "Throttled" ? 429 : 400;
                shape["error"]!["retryable"] = name != "Ordinary"; shape["error"]!["throttling"] = name == "Throttled";
                schema["shapes"]![name] = shape; types[name] = typeof(ServiceError);
            }
            schema["operations"]!["Retry"]!["safe"] = test!["safe"]!.DeepClone();
            var runtime = Runtime(new() { Endpoint = server.Endpoint, MaxAttempts = test["maxAttempts"]!.GetValue<int>(), BaseDelay = TimeSpan.Zero }, schema, types);
            bool success = false;
            try { await Call(runtime).WaitAsync(TimeSpan.FromSeconds(5)); success = true; } catch (ServiceError) { }
            finally { ((IDisposable)runtime).Dispose(); }
            Check(count == test["attempts"]!.GetValue<int>() && success == test["success"]!.GetValue<bool>(), test["name"]!.ToString());
        }
    }
    static void Serialization(string path)
    {
        var runtime = Runtime(new() { Endpoint = "https://example.invalid" });
        try
        {
            foreach (var row in J(File.ReadAllText(Path.Combine(Path.GetDirectoryName(path)!, "serialization.json"))).AsArray())
            {
                var test = row!; var id = test["id"]!.GetValue<string>(); var target = test["target"]!.GetValue<string>();
                object value;
                try { value = Decode(runtime, target, test["wire"]!.GetValue<string>()); }
                catch (Exception e) when (e is JsonException or FormatException or OverflowException or ArgumentException)
                { if (test["reject"]?.GetValue<bool>() == true) continue; throw new Exception(id, e); }
                Check(test["reject"]?.GetValue<bool>() != true, id + " accepted invalid wire");
                var expect = test["expectByProfile"]?["dotnet-ticks"] ?? test["expect"]!;
                var sharedEncoded = Encode(runtime, target, value);
                Check(ExactJsonEquals(sharedEncoded, J(expect["json"]!.GetValue<string>())), id + ": " + sharedEncoded);
                if (expect["states"] is JsonObject states) foreach (var (member, state) in states)
                {
                    var optional = (value.GetType().GetProperty(char.ToUpperInvariant(member[0]) + member[1..] + "Presence") ?? value.GetType().GetProperty(char.ToUpperInvariant(member[0]) + member[1..]))!.GetValue(value)!;
                    var set = (bool?)optional.GetType().GetProperty("IsSet")?.GetValue(optional) ?? true;
                    var isNull = (bool?)optional.GetType().GetProperty("IsNull")?.GetValue(optional) ?? false;
                    Check((!set ? "absent" : isNull ? "null" : "value") == state!.GetValue<string>(), id + " native presence");
                }
                if (expect["blobHex"] is JsonNode blobHex) Check(Convert.ToHexString((byte[])value).ToLowerInvariant() == blobHex.GetValue<string>(), id + " bytes");
                if (expect["instant"] is JsonNode instant)
                {
                    var member = instant["member"]!.GetValue<string>();
                    var optional = (value.GetType().GetProperty(char.ToUpperInvariant(member[0]) + member[1..] + "Presence") ?? value.GetType().GetProperty(char.ToUpperInvariant(member[0]) + member[1..]))!.GetValue(value)!;
                    var time = (DateTimeOffset)optional.GetType().GetProperty("Value")!.GetValue(optional)!;
                    long seconds = time.ToUnixTimeSeconds(), nanos = (time - DateTimeOffset.FromUnixTimeSeconds(seconds)).Ticks * 100;
                    Check(seconds.ToString(System.Globalization.CultureInfo.InvariantCulture) == instant["seconds"]!.GetValue<string>() && nanos == instant["nanoseconds"]!.GetValue<int>(), id + " instant");
                }
            }
            byte[] blob = [0xff, 0, 0x80, 0x01];
            Check(Encode(runtime, "example.weather#Thumbnail", blob).GetValue<string>() == "/wCAAQ==", "blob base64 encoding");
            Check(((byte[])Decode(runtime, "example.weather#Thumbnail", "\"/wCAAQ==\"")).SequenceEqual(blob), "blob base64 decoding");
            Check(((byte[])Decode(runtime, "example.weather#Thumbnail", "\"\"")).Length == 0, "empty JSON blob");
            try { Decode(runtime, "example.weather#Thumbnail", "\"invalid!\""); throw new Exception("invalid base64 accepted"); } catch (FormatException) { }
            Check(Decode(runtime, "example.weather#Sky", "\"future\"").ToString() == "future", "unknown enum");
            const string unknown = """{"future":{"n":9223372036854775807}}""";
            Check(JsonNode.DeepEquals(Encode(runtime, "example.weather#Location", Decode(runtime, "example.weather#Location", unknown)), J(unknown)), "unknown union");
            Check(Encode(runtime, "example.weather#Location", new Location { Unknown = new Unit() }).ToJsonString() == "{\"unknown\":{}}", "unit union");
            var city = (CitySummary)Decode(runtime, "example.weather#CitySummary", """{"id":"c","name":"x","createdAt":-0.0000001,"population":9223372036854775807}""");
            Check(city.Population == long.MaxValue && city.CreatedAt == DateTimeOffset.UnixEpoch.AddTicks(-1), "number and timestamp precision");
            Check(((CitySummary)Decode(runtime, "example.weather#CitySummary", """{"id":"c","name":"x"}""")).Population == 0, "absent default");
            try { Decode(runtime, "example.weather#CitySummary", """{"id":"c","name":"x","population":null}"""); throw new Exception("null default accepted"); } catch (JsonException) { }
            var ordinary = new CreateCityInput { Name = "", ExpectedRevision = 0L };
            Check(ordinary.Name == "" && ordinary.ExpectedRevision == 0L && ordinary.NamePresence.IsSet, "ordinary values retain presence");
            ordinary.Name = null;
            Check(ordinary.Name is null && ordinary.NamePresence.IsNull, "nullable assignment is explicit null");
            ordinary.NamePresence = default;
            Check(ordinary.Name is null && !ordinary.NamePresence.IsSet, "advanced absence reset");
            var input = new CreateCityInput { Name = "x", LocationPresence = Optional<Location>.Null };
            var encoded = Encode(runtime, "example.weather#CreateCityInput", input).AsObject();
            Check(encoded.ContainsKey("location") && encoded["location"] is null && !encoded.ContainsKey("clientToken"), "presence");
            var forecast = (Forecast)Decode(runtime, "example.weather#Forecast", """{"future":{"n":9223372036854775807}}""");
            Check(forecast[new Sky("future")]["n"]!.GetValue<long>() == long.MaxValue, "enum keys and document precision");
            foreach (var test in J(File.ReadAllText(path)).AsArray())
            {
                var actual = (JsonNode)Invoke(runtime, "Header", J("""{"target":"example.weather#Tags"}"""), test!["wire"]!.GetValue<string>(), null)!;
                Check(JsonNode.DeepEquals(actual, test["values"]), "header list");
                var headerText = (string)RuntimeType.GetMethod("Bound", BindingFlags.NonPublic | BindingFlags.Static)!.Invoke(null, [test["values"]])!;
                Check(headerText == test["encoded"]!.GetValue<string>(), "header encoding");
                var roundTrip = (JsonNode)Invoke(runtime, "Header", J("""{"target":"example.weather#Tags"}"""), headerText, null)!;
                Check(JsonNode.DeepEquals(roundTrip, test["values"]), "header roundtrip");
            }
            var date = (JsonNode)Invoke(runtime, "Encode", "example.weather#DateTime", DateTimeOffset.Parse("2026-10-01T00:00:00Z"), "http-date")!;
            Check(date.GetValue<string>() == "Thu, 01 Oct 2026 00:00:00 GMT", "HTTP date");
            Check((TimeSpan)Invoke(runtime, "Delay", 10, new Dictionary<string, string> { ["Retry-After"] = "0" })! == TimeSpan.Zero, "zero Retry-After overrides jitter");
            Check((TimeSpan)Invoke(runtime, "Delay", 0, new Dictionary<string, string> { ["Retry-After"] = "123" })! == TimeSpan.FromSeconds(123), "Retry-After exceeds jitter cap");
        }
        finally { ((IDisposable)runtime).Dispose(); }
    }
    static bool ExactJsonEquals(JsonNode? a, JsonNode? b)
    {
        if (a is JsonObject x) return b is JsonObject y && x.Count == y.Count && x.All(pair => y.ContainsKey(pair.Key) && ExactJsonEquals(pair.Value, y[pair.Key]));
        if (a is JsonArray list) return b is JsonArray other && list.Count == other.Count && list.Select((item, i) => ExactJsonEquals(item, other[i])).All(ok => ok);
        if (a?.GetValueKind() == JsonValueKind.Number) return b?.GetValueKind() == JsonValueKind.Number && decimal.Parse(a.ToJsonString(), System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture) == decimal.Parse(b.ToJsonString(), System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture);
        return JsonNode.DeepEquals(a, b);
    }
    static async Task HttpBehavior()
    {
        var sent = new List<byte[]>(); int credentials = 0;
        await using (var server = new Server(request => {
            sent.Add(request.Body);
            Check(request.Headers["x-amz-content-sha256"] == Convert.ToHexString(SHA256.HashData(request.Body)).ToLowerInvariant(), "digest");
            Check(request.Headers["Authorization"] == "Bearer " + sent.Count, "auth refresh");
            Check(request.Headers["x-expected-revision"] == "7", "header binding");
            return sent.Count == 1 ? new(503, "{\"__type\":\"FutureError\"}") : new(200, """{"city":{"id":"c","name":"x"}}""");
        }))
        {
            using var client = new WeatherClient(new() { Endpoint = server.Endpoint, BaseDelay = TimeSpan.Zero, Token = _ => ValueTask.FromResult<string?>((++credentials).ToString()),
                Middleware = [async (request, ct) => { var content = await request.Content!.ReadAsStringAsync(ct); request.Content.Dispose(); request.Content = new StringContent(content + " "); }] });
            var input = new CreateCityInput { Name = "x", ExpectedRevision = 7L };
            await client.CreateCityAsync(input);
            Check(!input.ClientTokenPresence.IsSet && sent[0].SequenceEqual(sent[1]), "token reuse and caller immutability");
        }
        await using (var server = new Server(request => {
            Check(request.Path.Contains("a%2Fb%20%2A%3F%23%C3%A9", StringComparison.OrdinalIgnoreCase), "label encoding " + request.Path);
            return new(200, """{"city":{"id":"x","name":"x"}}""", new() { ["x-request-id"] = "r1" });
        }))
        {
            using var client = new WeatherClient(new() { Endpoint = server.Endpoint });
            var city = await client.GetCityAsync(new() { CityId = "a/b *?#é" });
            Check(city.RequestIdValue == "r1", "response header");
        }
        await using (var server = new Server(_ => new(400, """{"__type":"ValidationError","message":"bad","reason":"other","requestId":"body"}""", new() { ["x-request-id"] = "wire" })))
        {
            using var client = new WeatherClient(new() { Endpoint = server.Endpoint });
            try { await client.GetCityAsync(new() { CityId = "x" }); throw new Exception("typed error missing"); }
            catch (ValidationError e) { Check(e.RequestId == "wire" && e.RequestIdValue == "body" && e.Message == "bad" && e.StatusCode == 400, "error metadata"); }
        }
        await using (var server = new Server(_ => new(400, """{"__type":"FutureError","requestId":{}}""", new() { ["x-request-id"] = "wire" })))
        {
            using var client = new WeatherClient(new() { Endpoint = server.Endpoint });
            try { await client.GetCityAsync(new() { CityId = "x" }); throw new Exception("error missing"); }
            catch (ServiceError e) { Check(e is WeatherException, "unknown service exception base"); Check(e.ErrorName == "FutureError" && e.RequestId == "wire", "malformed metadata"); }
        }
    }
    static async Task Payloads()
    {
        byte[] photo = [0xff, 0xd8, (byte)'{', (byte)'"', (byte)'a', (byte)'"', (byte)':', (byte)'1', (byte)'}', 0, 0x80];
        int calls = 0;
        await using var server = new Server(request => {
            int call = ++calls;
            if (call <= 2)
            {
                Check(request.Body.SequenceEqual(photo), "payload request bytes");
                Check(request.Headers["Content-Type"] == "image/jpeg" && request.Headers["X-Photo-Caption"] == "harbour", "payload request bindings");
                Check(request.Headers["x-amz-content-sha256"] == Convert.ToHexString(SHA256.HashData(photo)).ToLowerInvariant(), "raw payload digest");
                return call == 1 ? new(503, "{\"__type\":\"FutureError\"}") : new(200, """{"photo":{"size":11,"thumbnail":"/9h7ImEiOjE=","caption":"harbour"}}""");
            }
            if (call == 3) return new(200, "", new() { ["Content-Type"] = "image/jpeg", ["X-Photo-Caption"] = "harbour" }, BinaryBody: photo);
            if (call == 4) return new(200, "");
            Check(request.Body.Length == 0 && request.Headers["Content-Type"] == "image/jpeg", "empty payload request retains its modeled media type");
            Check(request.Headers["x-amz-content-sha256"] == Convert.ToHexString(SHA256.HashData([])).ToLowerInvariant(), "empty payload digest");
            return new(200, """{"photo":{"size":0,"thumbnail":""}}""");
        });
        using var client = new WeatherClient(new() { Endpoint = server.Endpoint, BaseDelay = TimeSpan.Zero });
        var stored = await client.PutCityPhotoAsync(new() { CityId = "c-00000001", Caption = "harbour", Photo = photo });
        Check(stored.Photo.Thumbnail.SequenceEqual(photo.Take(8)), "JSON thumbnail decoding");
        var get = new GetCityPhotoInput { CityId = "c-00000001" };
        var read = await client.GetCityPhotoAsync(get);
        Check(read.PhotoPresence.IsSet && read.Photo!.SequenceEqual(photo) && read.Caption == "harbour", "raw response and header binding");
        var empty = await client.GetCityPhotoAsync(get);
        Check(empty.PhotoPresence.IsSet && empty.Photo!.Length == 0, "empty raw response presence");
        await client.PutCityPhotoAsync(new() { CityId = "c-00000001", Photo = Array.Empty<byte>() });
        Check(calls == 5, "payload retry count");
    }
    static async Task MalformedModeledErrors()
    {
        foreach (var body in new[] {
            """{"__type":"ValidationError","message":{},"reason":"other"}""",
            """{"__type":"ValidationError","message":"invalid reason","reason":42}""",
            """{"__type":"ValidationError","message":"invalid list","fieldList":{}}"""
        })
        {
            await using var server = new Server(_ => new(400, body, new() { ["x-request-id"] = "malformed-id", ["x-error-detail"] = "retained" }));
            using var client = new WeatherClient(new() { Endpoint = server.Endpoint, BaseDelay = TimeSpan.Zero });
            try { await client.PingAsync(); throw new Exception("malformed modeled error ignored"); }
            catch (ServiceError error)
            {
                Check(error.GetType() == typeof(WeatherException), "malformed modeled fields did not use generic error");
                Check(error.ErrorName == "ValidationError" && error.StatusCode == 400 && error.RequestId == "malformed-id" && error.Headers["x-error-detail"] == "retained", "malformed error lost HTTP metadata");
                Check(!error.Retryable && !error.Throttling && server.Attempts == 1, "ordinary malformed error retried");
            }
        }

        using var schemaStream = typeof(WeatherClient).Assembly.GetManifestResourceStream("crucible.schema.json")!;
        var schema = JsonNode.Parse(schemaStream)!.AsObject();
        schema["operations"] = J("""{"Retry":{"method":"POST","uri":"/","input":null,"output":null,"token":null,"safe":false,"request":{"labels":[],"query":[],"headers":[],"body":[]},"response":{"headers":[],"body":[]},"errors":["example.weather#ThrottlingError"]}}""");
        var registry = (Dictionary<string, Type>)typeof(WeatherClient).Assembly.GetType("Example.Weather.ModelRegistry")!.GetField("Types", BindingFlags.Static | BindingFlags.NonPublic)!.GetValue(null)!;
        foreach (var (body, header) in new[] {
            ("""{"__type":"ThrottlingError","message":{}}""", "0"),
            ("""{"__type":"ThrottlingError","message":"overflow","retryAfter":2147483648}""", (string?)null),
            ("""{"__type":"ThrottlingError","message":"invalid header"}""", "not-a-number")
        })
        {
            int attempts = 0;
            var headers = new Dictionary<string, string> { ["x-request-id"] = "throttle-id" };
            if (header is not null) headers["Retry-After"] = header;
            await using var server = new Server(_ => ++attempts == 1 ? new(429, body, headers) : new(200, "{}"));
            var runtime = Runtime(new() { Endpoint = server.Endpoint, BaseDelay = TimeSpan.Zero }, schema, registry);
            try { await Call(runtime).WaitAsync(TimeSpan.FromSeconds(5)); }
            finally { ((IDisposable)runtime).Dispose(); }
            Check(attempts == 2, "malformed modeled throttle did not retry unsafe operation");
        }
    }
    public sealed class NumericValues : List<int> { }
    public sealed class NumericListError : ServiceError
    {
        public Optional<NumericValues> Values { get; set; }
    }
    static async Task NumericListErrors()
    {
        var schema = J("""{"bearer":false,"shapes":{"Integer":{"kind":"integer"},"NumericValues":{"kind":"list","member":{"target":"Integer","format":"date-time"}},"NumericListError":{"kind":"structure","name":"NumericListError","members":{"values":{"target":"NumericValues","attribute":"Values","default":null,"format":"date-time"}},"error":{"status":400,"retryable":false,"throttling":false,"headers":[]}}},"operations":{"Retry":{"method":"POST","uri":"/","input":null,"output":null,"token":null,"safe":false,"request":{"labels":[],"query":[],"headers":[],"body":[]},"response":{"headers":[],"body":[]},"errors":["NumericListError"]}}}""").AsObject();
        var registry = new Dictionary<string, Type> { ["NumericValues"] = typeof(NumericValues), ["NumericListError"] = typeof(NumericListError) };
        const string malformed = """{"__type":"NumericListError","values":[null]}""";
        foreach (bool throttling in new[] { false, true })
        {
            int status = throttling ? 429 : 400;
            var traits = schema["shapes"]!["NumericListError"]!["error"]!;
            traits["status"] = status; traits["retryable"] = throttling; traits["throttling"] = throttling;
            await using var server = new Server(_ => new(status, malformed, new() { ["x-request-id"] = "numeric-id", ["x-error-detail"] = "retained" }));
            var runtime = Runtime(new() { Endpoint = server.Endpoint, MaxAttempts = 1 }, schema, registry);
            try
            {
                var valid = (NumericListError)Decode(runtime, "NumericListError", """{"values":[1]}""");
                Check(valid.Values.IsSet && valid.Values.Value.SequenceEqual(new[] { 1 }), "valid modeled numeric list did not decode");
                try { await Call(runtime); throw new Exception("null numeric list error ignored"); }
                catch (ServiceError error)
                {
                    Check(error.GetType() == typeof(WeatherException) && error.ErrorName == "NumericListError" && error.StatusCode == status && error.RequestId == "numeric-id" && error.Headers["x-error-detail"] == "retained", "numeric list failure lost HTTP metadata");
                    Check(error.Retryable == throttling && error.Throttling == throttling && server.Attempts == 1, "numeric list failure lost retry traits or attempt limit");
                }
            }
            finally { ((IDisposable)runtime).Dispose(); }
        }
        int attempts = 0;
        await using var retryServer = new Server(_ => ++attempts == 1 ? new(429, malformed) : new(200, "{}"));
        var retryRuntime = Runtime(new() { Endpoint = retryServer.Endpoint, BaseDelay = TimeSpan.Zero }, schema, registry);
        try { await Call(retryRuntime).WaitAsync(TimeSpan.FromSeconds(5)); }
        finally { ((IDisposable)retryRuntime).Dispose(); }
        Check(attempts == 2, "null numeric list throttle did not retry unsafe operation");
    }
    static async Task FailureBehavior()
    {
        await using (var server = new Server(_ => new(200, "invalid")))
        {
            using var client = new WeatherClient(new() { Endpoint = server.Endpoint, BaseDelay = TimeSpan.Zero });
            try { await client.GetCityAsync(new() { CityId = "x" }); throw new Exception("JSON accepted"); } catch (JsonException) { }
            Check(server.Attempts == 1, "malformed JSON retried");
        }
        int requests = 0;
        await using (var server = new Server(_ => new(200, """{"city":{"id":"x","name":"x"}}""", Truncated: ++requests == 1)))
        {
            using var client = new WeatherClient(new() { Endpoint = server.Endpoint, BaseDelay = TimeSpan.Zero });
            await client.GetCityAsync(new() { CityId = "x" }); Check(requests == 2, "truncated body retry");
        }
        await using (var server = new Server(_ => new(200, "ignored", Stall: true)))
        {
            using var client = new WeatherClient(new() { Endpoint = server.Endpoint });
            await client.PingAsync().WaitAsync(TimeSpan.FromSeconds(2));
        }
        await using (var server = new Server(_ => new(200, "{}")))
        {
            var entered = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
            var release = new TaskCompletionSource<string?>(TaskCreationOptions.RunContinuationsAsynchronously);
            using var client = new WeatherClient(new() { Endpoint = server.Endpoint, Token = _ => { entered.SetResult(); return new(release.Task); } });
            using var cancel = new CancellationTokenSource();
            var call = client.PingAsync(cancel.Token); await entered.Task.WaitAsync(TimeSpan.FromSeconds(2)); cancel.Cancel();
            try { await call.WaitAsync(TimeSpan.FromSeconds(2)); throw new Exception("not canceled"); } catch (OperationCanceledException) { }
            release.SetResult("token"); await Task.Delay(50); Check(server.Attempts == 0, "sent canceled request");
        }
        using (var client = new WeatherClient(new() { Endpoint = "http://example.invalid", Token = _ => ValueTask.FromResult<string?>("secret") }))
        {
            try { await client.PingAsync(); throw new Exception("plaintext credentials allowed"); } catch (ArgumentException) { }
        }
        int pages = 0;
        await using (var server = new Server(_ => { pages++; return new(200, """{"cities":[],"nextToken":"same"}"""); }))
        {
            using var client = new WeatherClient(new() { Endpoint = server.Endpoint });
            var input = new ListCitiesInput();
            try { await foreach (var _ in client.ListCitiesPagesAsync(input)) { } throw new Exception("cycle accepted"); } catch (InvalidOperationException) { }
            Check(pages == 2 && !input.NextTokenPresence.IsSet, "cycle detection and input mutation");
        }
    }
    sealed class NonPumpingContext : SynchronizationContext
    {
        public override void Post(SendOrPostCallback callback, object? state) => throw new Exception("SDK hook captured caller context");
    }
    static async Task Weather()
    {
        using var client = new WeatherClient(new() { Endpoint = Environment.GetEnvironmentVariable("WEATHER_URL"), Token = async _ => { await Task.Delay(1); return "test-token"; } });
        var input = new CreateCityInput { Name = "csharp-http", Location = new Location { Unknown = new Unit() } };
        var previous = SynchronizationContext.Current;
        CreateCityOutput city;
        try { SynchronizationContext.SetSynchronizationContext(new NonPumpingContext()); city = client.CreateCity(input); }
        finally { SynchronizationContext.SetSynchronizationContext(previous); }
        Check((await client.GetCityAsync(new() { CityId = city.City.Id })).City.Name == "csharp-http", "weather get");
        try { await client.CreateCityAsync(input); throw new Exception("missing conflict"); } catch (ConflictError) { }
        bool found = false;
        await foreach (var page in client.ListCitiesPagesAsync(new() { MaxResults = 1 })) found |= page.Cities.Any(c => c.Name == "csharp-http");
        Check(found, "weather pagination");
        byte[] photo = [0xff, (byte)'{', (byte)'"', (byte)'a', (byte)'"', (byte)':', (byte)'1', (byte)'}', 0, 0x80];
        var stored = await client.PutCityPhotoAsync(new() { CityId = city.City.Id, Caption = "aurora", Photo = photo });
        Check(stored.Photo.Size == photo.Length && stored.Photo.Thumbnail.SequenceEqual(photo.Take(8)), "weather payload size and thumbnail");
        var read = await client.GetCityPhotoAsync(new() { CityId = city.City.Id });
        Check(read.PhotoPresence.IsSet && read.Photo!.SequenceEqual(photo) && read.Caption == "aurora", "weather payload round trip");
    }
    static async Task Main(string[] args)
    {
        await BodilessDigests();
        HeaderParsing();
        await Hardening();
        await RetryCases(args[0]); Serialization(args[1]); await HttpBehavior(); await Payloads(); await MalformedModeledErrors(); await NumericListErrors(); await FailureBehavior(); await Weather();
        Console.WriteLine("C# client conformance and weather HTTP passed");
    }
    static async Task BodilessDigests()
    {
        foreach (var method in new[] { "POST", "PUT", "PATCH", "GET", "DELETE" })
        {
            Request? sent = null;
            await using var server = new Server(request => {
                sent = request;
                return new(200, "");
            });
            using var client = new WeatherClient(new() { Endpoint = server.Endpoint,
                Middleware = [(request, _) => { request.Method = new HttpMethod(method); return ValueTask.CompletedTask; }] });
            await client.PingAsync();
            Check(server.Attempts == 1, "one bodiless request");
            Check(sent!.Body.Length == 0, "expected no body");
            var present = sent.Headers.TryGetValue("x-amz-content-sha256", out var digest);
            if (method is "GET" or "DELETE") Check(!present, "bodiless read has no digest");
            else Check(present && digest == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", "empty write digest");
        }
    }
    static void HeaderParsing()
    {
        var first = "Mon, 16 Dec 2019 23:48:18 GMT"; var second = "Tue, 17 Dec 2019 23:48:18 GMT";
        string[] Parse(object runtime, string wire) => ((JsonArray)Invoke(runtime, "Header", J("""{"target":"Times"}"""), wire, null)!).Select(item => item!.GetValue<string>()).ToArray();
        var shapes = """{"Time":{"kind":"timestamp","format":"http-date"},"Times":{"kind":"list","member":{"target":"Time","format":"http-date"}},"Ratio":{"kind":"float"}}""";
        var runtime = Runtime(new() { Endpoint = "https://example.invalid" }, J("{\"shapes\":" + shapes + ",\"operations\":{},\"protocolSettings\":{\"nonFiniteFloatStrings\":true}}").AsObject(), new());
        try
        {
            Check(Parse(runtime, first + ", " + second).SequenceEqual([first, second]), "unquoted HTTP-date list splits on date boundaries");
            Check(Parse(runtime, first).SequenceEqual([first]), "single HTTP-date");
            Check(Parse(runtime, "\"" + first + "\"").SequenceEqual([first]), "quoted HTTP-date");
            foreach (var text in new[] { "NaN", "Infinity", "-Infinity" })
                Check(((JsonNode)Invoke(runtime, "Header", J("""{"target":"Ratio"}"""), text, null)!).GetValue<string>() == text, "non-finite float header " + text);
            Check(((JsonNode)Invoke(runtime, "Header", J("""{"target":"Ratio"}"""), "1.5", null)!).GetValue<double>() == 1.5, "finite float header");
        }
        finally { ((IDisposable)runtime).Dispose(); }
        var strict = Runtime(new() { Endpoint = "https://example.invalid" }, J("{\"shapes\":" + shapes + ",\"operations\":{}}").AsObject(), new());
        try
        {
            try { Invoke(strict, "Header", J("""{"target":"Ratio"}"""), "NaN", null); throw new Exception("NaN accepted without non-finite floats"); }
            catch (JsonException) { }
        }
        finally { ((IDisposable)strict).Dispose(); }
    }
    static async Task Hardening()
    {
        await using var server = new Server(request => {
            Check(!request.Headers.ContainsKey("Authorization"), "Public operation has no bearer header");
            return new(204, "");
        });
        var schema = J("""{"bearer":true,"shapes":{},"operations":{"Retry":{"bearer":false,"safe":true,"method":"GET","uri":"/public","input":null,"output":null,"token":null,"request":{"labels":[],"query":[],"headers":[],"body":[]},"response":{"headers":[],"body":[]},"errors":[]}}}""").AsObject();
        var runtime = Runtime(new() { Endpoint = server.Endpoint, Token = _ => throw new Exception("Public operation requested credentials") }, schema, new());
        try { await Call(runtime); } finally { ((IDisposable)runtime).Dispose(); }
        using var client = new WeatherClient(new() { Endpoint = server.Endpoint });
        foreach (var label in new[] { ".", ".." })
        {
            try { await client.GetCityAsync(new() { CityId = label }); throw new Exception("Dot label accepted"); }
            catch (ArgumentException) { }
        }
        Check(server.Attempts == 1, "Dot labels rejected before dispatch");
        var codec = Runtime(new() { Endpoint = server.Endpoint });
        try {
            try { Decode(codec, "example.weather#Tags", "[null]"); throw new Exception("Dense null accepted"); }
            catch (JsonException) { }
            try { Decode(codec, "example.weather#Forecast", "{\"future\":null}"); throw new Exception("Dense document null accepted"); }
            catch (JsonException) { }
            Check(((Forecast)Decode(codec, "example.weather#Forecast", "{\"future\":{\"nested\":null}}")).ContainsKey(new Sky("future")), "Nested document null remains valid");
        } finally { ((IDisposable)codec).Dispose(); }
    }
}
