using System.Diagnostics;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Text.Json.Nodes;
using Example.Streams;
using Example.Streams.Models;

const string Item = "data: {\"ready\":\"first\"}\n\n";
const string Complete = "event: crucible.complete\ndata: {}\n\n";
const string Media = "text/event-stream; charset=utf-8; crucible-stream=1";
static void Check(bool value, string message) { if (!value) throw new Exception(message); }
static async Task<T> Throws<T>(Func<Task> action) where T : Exception
{
    try { await action(); } catch (T error) { return error; }
    throw new Exception("Expected " + typeof(T).Name);
}
static StreamsClient Client(string endpoint, int limit = 1048576, TimeSpan? timeout = null, HttpClient? http = null, TelemetryHook? telemetry = null) =>
    new(new ClientConfig { Endpoint = endpoint, MaxStreamEventBytes = limit, Timeout = timeout ?? TimeSpan.FromSeconds(5), HttpClient = http, Telemetry = telemetry,
        BaseDelay = TimeSpan.Zero, Token = _ => ValueTask.FromResult<string?>("token"),
        Middleware = [(request, _) => { request.Headers.Add("X-Test", "middleware"); return ValueTask.CompletedTask; }] });
static async Task<int> Drain(ResponseStream<Progress> stream)
{
    var count = 0; await foreach (var value in stream) { count++; }
    return count;
}

var vectors = JsonNode.Parse(File.ReadAllText(Environment.GetEnvironmentVariable("CRUCIBLE_STREAM_VECTORS")!))!["cases"]!.AsArray();
Check(vectors.Count == 25, "shared vectors changed");
foreach (var vector in vectors)
{
    var name = vector!["name"]!.GetValue<string>();
    var bytes = vector["hex"] is JsonNode hex ? Convert.FromHexString(hex.GetValue<string>()) : Encoding.UTF8.GetBytes(vector["wire"]!.GetValue<string>());
    await using var server = new RawServer(async (connection, _, _) => {
        await connection.WriteAsync(Encoding.ASCII.GetBytes($"HTTP/1.1 200 OK\r\nContent-Type: {Media}\r\nContent-Length: {bytes.Length}\r\n\r\n"));
        foreach (var b in bytes) await connection.WriteAsync(new byte[] { b });
    });
    using var client = Client(server.Endpoint, vector["limit"]?.GetValue<int>() ?? 1048576);
    await using var events = await client.WatchAsync();
    Check(events.Metadata.StatusCode == 200, name + " immediate metadata");
    int count = 0; Exception? failure = null;
    try {
        await foreach (var value in events) {
            count++;
            if (name == "unknown-union") Check(value.UnknownVariant?.Name == "future", "unknown union");
            if (name == "item") Check(value.Ready == "first", "native item field");
            if (name == "multiline-cr") Check(value.Ready == "café", "UTF8 chunk seams");
        }
    } catch (Exception error) { failure = error; }
    Check(count == vector["items"]!.GetValue<int>(), name + " item count");
    Check(vector["valid"]!.GetValue<bool>() ? failure is null : failure is StreamProtocolError, name + ": " + failure);
    Check(server.Requests == 1, name + " no replay");
    Console.WriteLine("vector " + name);
}

// A held native socket proves all delimiters deliver before EOF and cancellation releases it before cleanup.
foreach (var delimiter in new[] { "\n", "\r\n", "\r" })
{
    var disconnected = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
    await using var server = new RawServer(async (connection, _, _) => {
        var frame = $"data: {{\"ready\":\"first\"}}{delimiter}{delimiter}";
        await connection.WriteAsync(Encoding.UTF8.GetBytes($"HTTP/1.1 200 OK\r\nContent-Type: {Media}\r\nTransfer-Encoding: chunked\r\n\r\n{Encoding.UTF8.GetByteCount(frame):x}\r\n{frame}\r\n"));
        // Closing the response can reset the socket: a failed read is a disconnect too.
        try { if (await connection.ReadAsync(new byte[1]) == 0) disconnected.TrySetResult(); }
        catch (IOException) { disconnected.TrySetResult(); }
    });
    using var client = Client(server.Endpoint);
    await using var events = await client.WatchAsync();
    var iterator = events.GetAsyncEnumerator();
    Check(await iterator.MoveNextAsync().AsTask().WaitAsync(TimeSpan.FromSeconds(1)) && iterator.Current.Ready == "first", "held delimiter progress");
    events.Cancel();
    await disconnected.Task.WaitAsync(TimeSpan.FromSeconds(1));
}

foreach (var post in new[] { false, true })
{
    int auth = 0;
    await using var server = new RawServer(async (connection, request, attempt) => {
        if (request.Contains("Authorization: Bearer token", StringComparison.OrdinalIgnoreCase) && request.Contains("X-Test: middleware", StringComparison.OrdinalIgnoreCase)) Interlocked.Increment(ref auth);
        var body = attempt == 1 ? new string('x', 4096) : Item + Complete;
        await Reply(connection, body, status: attempt == 1 ? 503 : 200);
    });
    using var client = Client(server.Endpoint, 256);
    if (post) await Throws<ServiceError>(async () => await client.PublishAsync(new PublishInput { Topic = "send" }));
    else { await using var events = await client.WatchAsync(); Check(await Drain(events) == 1, "GET retry"); }
    Check(server.Requests == (post ? 1 : 2) && auth == server.Requests, "eligible retries auth/middleware");
}
foreach (var media in new[] { "application/json", "text/event-stream; charset=latin1; crucible-stream=1", "text/event-stream; charset=utf-8; crucible-stream=2", "text/event-stream; charset=utf-8; crucible-stream=1; crucible-stream=1" })
{
    await using var server = new RawServer((connection, _, _) => Reply(connection, Item + Complete, media));
    using var client = Client(server.Endpoint);
    await Throws<StreamProtocolError>(async () => await client.WatchAsync());
    Check(server.Requests == 1, "invalid 2xx profile must not replay");
}

var cases = new[] {
    ("WorkFailed", "{\"message\":\"late\",\"reason\":\"broken\"}", "broken"),
    ("WorkFailed", "{\"message\":\"late\",\"reason\":null}", (string?)null),
    ("WorkFailed", "{\"message\":\"late\"}", (string?)null),
    ("WorkFailed", "{\"message\":\"late\",\"reason\":\"future\"}", "future"),
    ("HeaderFailure", "{\"message\":\"late\",\"reason\":\"broken\"}", "broken"),
    ("PayloadFailure", "{\"reason\":\"future\"}", "future"),
    ("DefaultReason", "{\"message\":\"late\"}", "broken"),
    ("DefaultEmptyReason", "{\"message\":\"late\"}", ""),
    ("BlobFailure", "{\"value\":\"AAEC/w==\"}", (string?)null),
    ("TimeFailure", "{\"value\":\"2026-10-26T00:00:00Z\"}", (string?)null),
    ("FutureError", "{\"message\":\"late\"}", (string?)null),
};
foreach (var (name, fields, reason) in cases)
{
    await using var server = new RawServer((connection, _, _) => Reply(connection,
        Item + $"event: crucible.error\ndata: {{\"type\":\"{name}\",\"status\":503,\"error\":{fields}}}\n\n"));
    using var client = Client(server.Endpoint);
    await using var events = await client.WatchAsync();
    var iterator = events.GetAsyncEnumerator();
    Check(await iterator.MoveNextAsync(), "item before late error");
    var error = await Throws<ServiceError>(async () => await iterator.MoveNextAsync());
    Check(error.ErrorName == name && error.StatusCode == 503 && error.RequestId == "profile", "late HTTP metadata");
    var actual = error switch {
        WorkFailed value => value.Reason?.Value,
        HeaderFailure value => value.Reason.Value,
        PayloadFailure value => value.Reason.Value,
        DefaultReason value => value.Reason.Value,
        DefaultEmptyReason value => value.Reason.Value,
        _ => null
    };
    Check(actual == reason, "typed document reason " + name);
    if (error is WorkFailed work) Check(work.ReasonPresence.IsSet == fields.Contains("reason", StringComparison.Ordinal), "late presence");
    if (error is BlobFailure blob) Check(blob.Value.SequenceEqual(new byte[] { 0, 1, 2, 255 }), "native blob document payload");
    if (error is TimeFailure time) Check(time.Value == DateTimeOffset.Parse("2026-10-26T00:00:00Z"), "native header-bound document timestamp");
    var property = typeof(ServiceError).GetProperty("DocUrl");
    if (property is not null)
    {
        string? expected = name == "FutureError" ? null : "https://docs.example.test/errors/" + name;
        if (reason is "broken" or "") expected += "/reason-" + reason;
        Check((string?)property.GetValue(error) == expected, "late documentation URL " + name + ": " + property.GetValue(error));
    }
    Check(server.Requests == 1, "late error replay");
}

await using (var server = new RawServer((connection, _, _) => Reply(connection, Complete)))
{
    using var precanceled = new CancellationTokenSource(); precanceled.Cancel();
    var limitError = await Throws<ArgumentException>(() => Task.Run(() => Client(server.Endpoint, 255)));
    Check(limitError.Message.Contains("MaxStreamEventBytes", StringComparison.Ordinal), "the limit error names its setting");
    int tokens = 0;
    using var before = new StreamsClient(new ClientConfig { Endpoint = server.Endpoint, Token = _ => { tokens++; return ValueTask.FromResult<string?>("token"); } });
    await Throws<OperationCanceledException>(async () => await before.WatchAsync(precanceled.Token));
    Check(tokens == 0 && server.Requests == 0, "pre-cancel must precede credentials and HTTP");
    using var client = new StreamsClient(new ClientConfig { Endpoint = server.Endpoint, Timeout = TimeSpan.FromMilliseconds(80), Token = async _ => { await Task.Delay(250); return "late"; } });
    await Throws<OperationCanceledException>(async () => await client.WatchAsync());
    await Task.Delay(280);
    Check(server.Requests == 0, "credentials timeout initiated request");
}

await using (var server = new RawServer(async (connection, _, _) => {
    await connection.WriteAsync(Encoding.UTF8.GetBytes($"HTTP/1.1 200 OK\r\nContent-Type: {Media}\r\nConnection: close\r\n\r\n"));
    await connection.ReadAsync(new byte[1]);
}))
{
    using var client = Client(server.Endpoint);
    await using var events = await client.WatchAsync();
    var iterator = events.GetAsyncEnumerator();
    await Throws<InvalidOperationException>(() => Task.Run(() => events.GetAsyncEnumerator()));
    var read = iterator.MoveNextAsync().AsTask();
    await Throws<InvalidOperationException>(async () => await iterator.MoveNextAsync());
    events.Cancel(); await Throws<OperationCanceledException>(async () => await read);
}

// Bounds count physical comments/unknown fields/CRLF, not only JSON data characters.
foreach (var wire in new[] { ":" + new string('x', 260) + "\n\n", string.Concat(Enumerable.Repeat("x:\r\n", 65)) + Item + Complete, "event: crucible.complete\n\n" + Complete, Item + "event: crucible.error\ndata: {}\n\n", Complete + "x", "data: {\"ready\":\"first\",\"ready\":\"second\"}\n\n" + Complete })
{
    await using var server = new RawServer((connection, _, _) => Reply(connection, wire));
    using var client = Client(server.Endpoint, 256);
    await using var events = await client.WatchAsync();
    await Throws<StreamProtocolError>(async () => await Drain(events));
    Check(server.Requests == 1, "malformed body replay");
}

foreach (var deadline in new[] { false, true })
{
    var disconnected = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
    await using var server = new RawServer(async (connection, _, _) => {
        var frame = Item + Item;
        await connection.WriteAsync(Encoding.UTF8.GetBytes($"HTTP/1.1 200 OK\r\nContent-Type: {Media}\r\nTransfer-Encoding: chunked\r\n\r\n{Encoding.UTF8.GetByteCount(frame):x}\r\n{frame}\r\n"));
        // Closing the response can reset the socket: a failed read is a disconnect too.
        try { if (await connection.ReadAsync(new byte[1]) == 0) disconnected.TrySetResult(); }
        catch (IOException) { disconnected.TrySetResult(); }
    });
    using var cancellation = new CancellationTokenSource();
    using var client = Client(server.Endpoint, timeout: deadline ? TimeSpan.FromMilliseconds(150) : null);
    await using var events = await client.WatchAsync(cancellation.Token);
    var iterator = events.GetAsyncEnumerator();
    if (!deadline) Check(await iterator.MoveNextAsync(), "first of queued events");
    if (deadline) await Task.Delay(200); else cancellation.Cancel();
    await Throws<OperationCanceledException>(async () => await iterator.MoveNextAsync());
    await disconnected.Task.WaitAsync(TimeSpan.FromSeconds(1));
    Check(server.Requests == 1, "queued cancellation/deadline must not replay");
}

await using (var server = new RawServer(async (connection, _, _) => {
    await connection.WriteAsync(Encoding.UTF8.GetBytes($"HTTP/1.1 200 OK\r\nContent-Type: {Media}\r\nContent-Length: 99999\r\n\r\n" + Item));
}))
{
    using var client = Client(server.Endpoint);
    await using var events = await client.WatchAsync();
    var iterator = events.GetAsyncEnumerator(); Check(await iterator.MoveNextAsync(), "item before transport interruption");
    await Throws<NetworkError>(async () => await iterator.MoveNextAsync());
    Check(server.Requests == 1, "interrupted 2xx body must not replay");
}

// A transport failure acquiring a successful body is a NetworkError, and is never retried.
{
    int sends = 0;
    using var http = new HttpClient(new StubHandler(() => {
        Interlocked.Increment(ref sends);
        var response = new HttpResponseMessage(HttpStatusCode.OK) { Content = new FailingContent() };
        response.Content.Headers.TryAddWithoutValidation("Content-Type", Media);
        return response;
    }));
    using var client = Client("http://127.0.0.1:1", http: http);
    var failure = await Throws<NetworkError>(async () => await client.WatchAsync());
    Check(failure.InnerException is IOException && sends == 1, "post-header transport failure");
}

// The operation ends with the stream, not at its headers, with the outcome the consumer saw.
foreach (var (wire, read, expected) in new[] {
    (Item + Complete, true, "ok"),
    (Item + "event: crucible.error\ndata: {\"type\":\"WorkFailed\",\"status\":503,\"error\":{\"message\":\"late\"}}\n\n", true, "WorkFailed"),
    (Complete + "x", true, nameof(StreamProtocolError)),
    (Item + Complete, false, nameof(OperationCanceledException)),
})
{
    await using var server = new RawServer((connection, _, _) => Reply(connection, wire));
    var telemetry = new TelemetryLog();
    using var client = Client(server.Endpoint, telemetry: telemetry);
    var events = await client.WatchAsync();
    Check(telemetry.Attempts.SequenceEqual(new[] { "200" }) && telemetry.Operations.Count == 0, "opening ends only the attempt");
    if (read) { try { await Drain(events); } catch (Exception error) when (error is ServiceError or StreamProtocolError) { } }
    await events.DisposeAsync();
    Check(telemetry.Operations.SequenceEqual(new[] { expected }), "stream outcome " + expected + ": " + string.Join(",", telemetry.Operations));
}

// Like Go and Python, an opening error one byte past the limit is not decoded, and telemetry keeps the bytes read.
foreach (var (size, decoded) in new[] { (256, true), (257, false) })
{
    const string document = "{\"__type\":\"WorkFailed\",\"message\":\"opening\"}";
    var body = document + new string(' ', size - document.Length);
    await using var server = new RawServer((connection, _, _) => Reply(connection, body, "application/json", 503));
    var telemetry = new TelemetryLog();
    using var client = Client(server.Endpoint, 256, telemetry: telemetry);
    var error = await Throws<ServiceError>(async () => await client.PublishAsync(new PublishInput { Topic = "send" }));
    Check((error is WorkFailed work && work.MessageValue == "opening") == decoded, "opening error at limit " + size);
    Check(telemetry.Bytes.SequenceEqual(new long?[] { size }), "opening error bytes " + size);
}

// Buffered calls keep the default drain policy: an unread body arriving after
// the headers returns the SDK-owned connection to the pool.
{
    var listener = new TcpListener(IPAddress.Loopback, 0); listener.Start();
    int connections = 0;
    using var stop = new CancellationTokenSource();
    var accept = Task.Run(async () => {
        var workers = new List<Task>();
        try {
            while (true) {
                var socket = await listener.AcceptTcpClientAsync(stop.Token);
                Interlocked.Increment(ref connections);
                workers.Add(Task.Run(async () => {
                    using (socket) {
                        try {
                            var stream = socket.GetStream(); var one = new byte[1]; var head = new List<byte>();
                            while (true) {
                                head.Clear();
                                while (!(head.Count >= 4 && head[^4] == 13 && head[^3] == 10 && head[^2] == 13 && head[^1] == 10)) {
                                    if (await stream.ReadAsync(one, stop.Token) == 0) return;
                                    head.Add(one[0]);
                                }
                                await stream.WriteAsync(Encoding.ASCII.GetBytes("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n"), stop.Token);
                                await stream.FlushAsync(stop.Token);
                                await Task.Delay(50, stop.Token);
                                await stream.WriteAsync(Encoding.ASCII.GetBytes("{}"), stop.Token);
                            }
                        } catch (Exception error) when (error is IOException or OperationCanceledException or ObjectDisposedException) { }
                    }
                }));
            }
        } catch (Exception error) when (error is OperationCanceledException or SocketException or ObjectDisposedException) { }
        await Task.WhenAll(workers);
    });
    try {
        using var client = Client("http://" + listener.LocalEndpoint);
        await client.PingAsync();
        await Task.Delay(300);
        await client.PingAsync();
        Check(Volatile.Read(ref connections) == 1, "buffered calls reuse their connection: " + connections);
    } finally { stop.Cancel(); listener.Stop(); await accept.WaitAsync(TimeSpan.FromSeconds(2)); }
}

foreach (var mode in new[] { "ok", "typed", "hold", "pending" })
{
    using var fixture = Process.Start(new ProcessStartInfo(Environment.GetEnvironmentVariable("CRUCIBLE_STREAM_FIXTURE")!, mode) { RedirectStandardOutput = true, UseShellExecute = false, CreateNoWindow = true })!;
    try
    {
        var urls = (await fixture.StandardOutput.ReadLineAsync().WaitAsync(TimeSpan.FromSeconds(5)))!.Split(' ');
        using var pool = new HttpClient { Timeout = Timeout.InfiniteTimeSpan };
        using var client = Client(urls[0], http: pool);
        Check((await client.ReadAsync()).Value == "unary", "ordinary unary call");
        var opening = await Throws<WorkFailed>(async () => await client.PublishAsync(new PublishInput { Topic = "open-fail" }));
        Check(opening.StatusCode == 503 && opening.MessageValue == "opening failed", "Rust modeled opening error");
        for (int action = 0; action < (mode is "hold" or "pending" ? 5 : 1); action++)
        {
            var dropsBefore = JsonNode.Parse(await pool.GetStringAsync(urls[1]))![1]!.GetValue<int>();
            using var cancel = new CancellationTokenSource();
            await using var events = action == 4 ? await client.PublishAsync(new PublishInput { Topic = mode }, cancel.Token) : await client.WatchAsync(cancel.Token);
            Check(events.Metadata.StatusCode == 200, "opening metadata");
            if (mode == "ok") Check(await Drain(events) == 1, "Rust complete");
            else if (mode == "typed") await Throws<WorkFailed>(async () => await Drain(events));
            else if (action == 0) await events.DisposeAsync();
            else
            {
                var iterator = events.GetAsyncEnumerator();
                if (mode == "hold") Check(await iterator.MoveNextAsync(), "Rust progressive delivery");
                if (action == 1) { cancel.Cancel(); await Throws<OperationCanceledException>(async () => await iterator.MoveNextAsync()); }
                else if (action == 2) { var read = iterator.MoveNextAsync().AsTask(); cancel.Cancel(); await Throws<OperationCanceledException>(async () => await read); }
                else if (action == 3) { await events.DisposeAsync(); }
                else { events.Cancel(); }
            }
            var until = DateTime.UtcNow.AddSeconds(3); int drops;
            do { drops = JsonNode.Parse(await pool.GetStringAsync(urls[1]))![1]!.GetValue<int>(); if (drops > dropsBefore) break; await Task.Delay(10); } while (DateTime.UtcNow < until);
            Check(drops == dropsBefore + 1, "producer release BEFORE fixture cleanup " + mode + "/" + action);
            Check((await client.ReadAsync()).Value == "unary", "caller HTTP pool remains usable");
        }
    }
    finally { fixture.Kill(true); await fixture.WaitForExitAsync(); }
}
Console.WriteLine("C# .NET " + Environment.Version + " response stream conformance passed");

static async Task Reply(NetworkStream connection, string body, string media = Media, int status = 200)
{
    var bytes = Encoding.UTF8.GetBytes(body);
    await connection.WriteAsync(Encoding.ASCII.GetBytes($"HTTP/1.1 {status} Result\r\nContent-Type: {media}\r\nx-request-id: profile\r\nX-Reason: header-spoof\r\nConnection: close\r\nContent-Length: {bytes.Length}\r\n\r\n"));
    await connection.WriteAsync(bytes);
}

sealed class TelemetryLog : TelemetryHook
{
    readonly object gate = new();
    readonly List<string> attempts = [], operations = [];
    readonly List<long?> bytes = [];
    public List<string> Attempts { get { lock (gate) return [.. attempts]; } }
    public List<string> Operations { get { lock (gate) return [.. operations]; } }
    public List<long?> Bytes { get { lock (gate) return [.. bytes]; } }
    public override void OnAttemptFinished(AttemptFinished e) { lock (gate) { attempts.Add(e.Status + (e.Error is null ? "" : " " + e.Error)); bytes.Add(e.ResponseBytes); } }
    public override void OnOperationFinished(OperationFinished e) { lock (gate) operations.Add(e.Error ?? "ok"); }
}

sealed class StubHandler(Func<HttpResponseMessage> respond) : HttpMessageHandler
{
    protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken) => Task.FromResult(respond());
}

sealed class FailingContent : HttpContent
{
    protected override Task SerializeToStreamAsync(Stream stream, TransportContext? context) => Task.FromException(new IOException("body unavailable"));
    protected override bool TryComputeLength(out long length) { length = 0; return false; }
    protected override Task<Stream> CreateContentReadStreamAsync() => Task.FromException<Stream>(new IOException("body unavailable"));
}

sealed class RawServer : IAsyncDisposable
{
    readonly TcpListener listener = new(IPAddress.Loopback, 0);
    readonly CancellationTokenSource stop = new();
    readonly Task loop;
    readonly List<Task> workers = [];
    readonly List<TcpClient> sockets = [];
    int requests;
    public int Requests => Volatile.Read(ref requests);
    public string Endpoint { get; }
    public RawServer(Func<NetworkStream, string, int, Task> action)
    {
        listener.Start(); Endpoint = "http://" + listener.LocalEndpoint;
        loop = Task.Run(async () => {
            try { while (!stop.IsCancellationRequested) {
                var socket = await listener.AcceptTcpClientAsync(stop.Token);
                sockets.Add(socket);
                workers.Add(Task.Run(async () => {
                    using (socket) {
                        try {
                            var stream = socket.GetStream(); var bytes = new List<byte>(); var one = new byte[1];
                            while (await stream.ReadAsync(one, stop.Token) != 0) { bytes.Add(one[0]); if (bytes.Count >= 4 && Encoding.ASCII.GetString(bytes.TakeLast(4).ToArray()) == "\r\n\r\n") break; }
                            var request = Encoding.ASCII.GetString(bytes.ToArray());
                            var content = request.Split("\r\n").FirstOrDefault(line => line.StartsWith("Content-Length:", StringComparison.OrdinalIgnoreCase));
                            if (content is not null) { int remaining = int.Parse(content.Split(':')[1]); while (remaining-- > 0) await stream.ReadExactlyAsync(one, stop.Token); }
                            await action(stream, request, Interlocked.Increment(ref requests));
                        } catch (Exception error) when (error is IOException or SocketException or OperationCanceledException or ObjectDisposedException) { }
                    }
                }));
            }} catch (OperationCanceledException) { } catch (SocketException) when (stop.IsCancellationRequested) { }
        });
    }
    public async ValueTask DisposeAsync() { stop.Cancel(); listener.Stop(); await loop; foreach (var socket in sockets) socket.Dispose(); await Task.WhenAll(workers).WaitAsync(TimeSpan.FromSeconds(2)); stop.Dispose(); }
}
