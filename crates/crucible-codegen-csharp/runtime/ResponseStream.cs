using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

/// <summary>A single-consumer asynchronous sequence owning its HTTP response. Dispose it even when abandoning it before enumeration.</summary>
/// <typeparam name="T">The generated event type.</typeparam>
public sealed class ResponseStream<T> : IAsyncEnumerable<T>, IAsyncEnumerator<T>, IAsyncDisposable
{
    private readonly HttpResponseMessage response;
    private readonly Stream body;
    private readonly CancellationTokenSource lifetime;
    private readonly CancellationToken token;
    private CancellationTokenRegistration lifetimeRegistration;
    private CancellationTokenRegistration enumerationRegistration;
    private readonly Func<JsonNode, T> decode;
    private readonly Func<JsonObject, Exception> decodeError;
    private readonly Action<Exception?> finish;
    private readonly int limit;
    private readonly byte[] buffer = new byte[4096];
    private readonly List<byte> line = [];
    private readonly List<string> data = [];
    private static readonly UTF8Encoding Utf8 = new(false, true);
    private int offset, available, physical, reading, enumerated, disposed, reported;
    private bool firstLine = true, skipLf, terminal, finished, betweenFrames;
    private string eventName = "";
    private Exception? terminalError;
    // The first failure ends the stream: every later read rethrows it, as Go keeps Err.
    private Exception? failure;
    /// <summary>The opening HTTP metadata, available before the first read.</summary>
    public ResponseMetadata Metadata { get; }
    /// <summary>The last event returned by MoveNextAsync.</summary>
    public T Current { get; private set; } = default!;

    internal ResponseStream(HttpResponseMessage response, Stream body, CancellationTokenSource lifetime,
        ResponseMetadata metadata, int limit, Func<JsonNode, T> decode, Func<JsonObject, Exception> decodeError, Action<Exception?> finish)
    {
        this.response = response; this.body = body; this.lifetime = lifetime; token = lifetime.Token;
        Metadata = metadata; this.limit = limit; this.decode = decode; this.decodeError = decodeError; this.finish = finish;
    }
    // Called once the opening attempt is recorded, so the operation never ends before it.
    internal void Activate()
    {
        // Disposing only the response releases the active lease while preserving a caller-owned pool.
        // Cancellation or the deadline also ends the operation, even while the stream is idle.
        lifetimeRegistration = token.Register(static value => {
            var stream = (ResponseStream<T>)value!;
            stream.response.Dispose(); stream.Report(new OperationCanceledException(stream.token));
        }, this);
    }
    // Reports the operation's outcome to telemetry exactly once: null after a complete
    // terminal and EOF, otherwise the failure the consumer observes or cancellation.
    private void Report(Exception? error)
    {
        if (Interlocked.Exchange(ref reported, 1) == 0) finish(error);
    }
    /// <summary>Claims this stream's only consumer; the token remains active while idle.</summary>
    /// <param name="cancellationToken">Additional cancellation for enumeration.</param>
    /// <returns>This stream's enumerator.</returns>
    public IAsyncEnumerator<T> GetAsyncEnumerator(CancellationToken cancellationToken = default)
    {
        if (Interlocked.Exchange(ref enumerated, 1) != 0) throw new InvalidOperationException("A response stream has one consumer");
        enumerationRegistration = cancellationToken.Register(Cancel);
        return this;
    }
    /// <summary>Cancels this stream and immediately releases its response. Idempotent.</summary>
    public void Cancel()
    {
        if (Interlocked.Exchange(ref disposed, 1) != 0) return;
        lifetime.Cancel(); response.Dispose();
    }
    /// <summary>Releases the response, including before the first read.</summary>
    /// <returns>A completed disposal operation.</returns>
    public ValueTask DisposeAsync()
    {
        Cancel(); enumerationRegistration.Dispose(); lifetimeRegistration.Dispose(); lifetime.Dispose();
        Report(new OperationCanceledException(token));
        return ValueTask.CompletedTask;
    }
    /// <summary>Reads one event on demand. Cancellation takes precedence over buffered events.</summary>
    /// <returns>True for an item; false only after a complete terminal and EOF.</returns>
    public async ValueTask<bool> MoveNextAsync()
    {
        if (Interlocked.Exchange(ref reading, 1) != 0) throw new InvalidOperationException("Concurrent stream reads are forbidden");
        if (failure is { } failed)
        {
            // Never resume parsing the buffer of a released response, whose reads look like EOF.
            Volatile.Write(ref reading, 0);
            System.Runtime.ExceptionServices.ExceptionDispatchInfo.Throw(failed);
        }
        try
        {
            if (finished) return false;
            token.ThrowIfCancellationRequested();
            while (true)
            {
                token.ThrowIfCancellationRequested();
                var next = await ByteAsync().ConfigureAwait(false);
                token.ThrowIfCancellationRequested();
                if (next < 0)
                {
                    // Decode a partial line too, so a truncated UTF-8 scalar cannot escape validation.
                    if (line.Count != 0) _ = Utf8.GetString(line.ToArray());
                    if (!terminal) throw new StreamProtocolError("Stream ended without a terminal event");
                    finished = true;
                    response.Dispose();
                    if (terminalError is not null) throw terminalError;
                    Report(null);
                    return false;
                }
                var b = (byte)next;
                if (skipLf)
                {
                    skipLf = false;
                    if (b == 10)
                    {
                        if (++physical > limit) throw new StreamProtocolError("Stream event exceeds byte limit");
                        if (betweenFrames) { physical = 0; betweenFrames = false; }
                        continue;
                    }
                }
                if (betweenFrames) { physical = 0; betweenFrames = false; }
                if (terminal) throw new StreamProtocolError("Bytes after terminal event");
                if (++physical > limit) throw new StreamProtocolError("Stream event exceeds byte limit");
                if (b is 10 or 13)
                {
                    skipLf = b == 13;
                    var text = Utf8.GetString(line.ToArray()); line.Clear();
                    if (firstLine) { if (text.StartsWith('\ufeff')) text = text[1..]; firstLine = false; }
                    if (text.Length == 0)
                    {
                        if (skipLf) betweenFrames = true;
                        else physical = 0;
                        if (data.Count == 0)
                        {
                            if (eventName is not "" and not "message") throw new StreamProtocolError("Control event without data");
                            eventName = ""; continue;
                        }
                        var value = ParseEvent(string.Join("\n", data)); data.Clear();
                        var kind = eventName; eventName = "";
                        if (kind is "" or "message")
                        {
                            if (value is null) throw new StreamProtocolError("Null stream item");
                            Current = decode(value);
                            token.ThrowIfCancellationRequested();
                            return true;
                        }
                        if (kind == "crucible.complete")
                        {
                            if (value is not JsonObject complete || complete.Count != 0) throw new StreamProtocolError("Invalid complete terminal");
                            terminal = true;
                        }
                        else if (kind == "crucible.error")
                        {
                            if (value is not JsonObject error) throw new StreamProtocolError("Invalid error terminal");
                            terminalError = decodeError(error); terminal = true;
                        }
                        else throw new StreamProtocolError("Unknown stream control");
                    }
                    else if (!text.StartsWith(':'))
                    {
                        var colon = text.IndexOf(':');
                        var field = colon < 0 ? text : text[..colon];
                        var value = colon < 0 ? "" : text[(colon + 1)..];
                        if (value.StartsWith(' ')) value = value[1..];
                        if (field == "data") data.Add(value);
                        else if (field == "event") eventName = value;
                    }
                }
                else line.Add(b);
            }
        }
        catch (Exception error)
        {
            response.Dispose();
            // Cancellation takes precedence; other failures surface as the stream's error
            // types. The caught exception is not chained into StreamProtocolError: decoder
            // messages can quote response bytes, which protocol errors never disclose.
            var thrown = token.IsCancellationRequested ? new OperationCanceledException(token)
                : error is StreamProtocolError or ServiceError or OperationCanceledException ? error
                : error is HttpRequestException or IOException ? new NetworkError(error.Message, error)
                : new StreamProtocolError("Invalid stream encoding or modeled value");
            failure = thrown;
            Report(thrown);
            if (ReferenceEquals(thrown, error)) throw;
            throw thrown;
        }
        finally { Volatile.Write(ref reading, 0); }
    }
    private async ValueTask<int> ByteAsync()
    {
        if (offset == available)
        {
            available = await body.ReadAsync(buffer.AsMemory(), token).ConfigureAwait(false); offset = 0;
            if (available == 0) return -1;
        }
        return buffer[offset++];
    }
    private static JsonNode? ParseEvent(string text)
    {
        // JsonNode defers duplicate-key checks until object access. Walk the whole
        // document so duplicates in unknown variants and document fields also fail.
        using var document = JsonDocument.Parse(text);
        static void Check(JsonElement element)
        {
            if (element.ValueKind == JsonValueKind.Object)
            {
                var names = new HashSet<string>(StringComparer.Ordinal);
                foreach (var property in element.EnumerateObject())
                {
                    if (!names.Add(property.Name)) throw new StreamProtocolError("Duplicate JSON member");
                    Check(property.Value);
                }
            }
            else if (element.ValueKind == JsonValueKind.Array)
                foreach (var value in element.EnumerateArray()) Check(value);
        }
        Check(document.RootElement);
        return JsonNode.Parse(text);
    }
}
