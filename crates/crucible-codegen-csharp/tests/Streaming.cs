using System.Diagnostics;
using System.Globalization;
using System.Net;
using System.Net.Sockets;
using System.Reflection;
using System.Runtime.ExceptionServices;
using System.Security.Cryptography;
using System.Text;
using Example.Stream;
using Example.Stream.Models;

// Streaming blobs against a loopback HTTP server, under a hard GC heap limit set by the consumer
// project, so a buffered 256 MiB body would fail rather than pass.
static class Program
{
    const long Large = 256L * 1024 * 1024;
    const string Digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    static readonly AwsCredentials Identity = new("AKIDEXAMPLE", "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY");
    static void Check(bool ok, string message) { if (!ok) throw new Exception(message); }
    static async Task<T> Fails<T>(Func<Task> body, string? text = null) where T : Exception
    {
        try { await body(); }
        catch (Exception error)
        {
            for (Exception? cause = error; cause is not null; cause = cause.InnerException)
                if (cause is T match)
                {
                    Check(text is null || match.Message.Contains(text, StringComparison.OrdinalIgnoreCase), $"message '{match.Message}' lacks '{text}'");
                    return match;
                }
            throw new Exception($"expected {typeof(T).Name} but got {error.GetType().Name}: {error.Message}", error);
        }
        throw new Exception("expected " + typeof(T).Name);
    }

    static async Task Main()
    {
        await using var server = new Server();
        var validated = new List<string>();
        var config = new ClientConfig { Endpoint = server.Endpoint, BaseDelay = TimeSpan.Zero, Timeout = TimeSpan.FromMinutes(5), ChecksumValidated = (op, algorithm) => validated.Add(op + ":" + algorithm) };
        using var client = new StreamingClient(config);
        using var sampler = new MemorySampler();

        // A non-seekable stream is one-shot: sent once, even for an idempotent operation, then disposed.
        server.Mode = "503"; server.Attempts = 0;
        var source = new OneShot("one shot");
        var oneShot = ByteStream.FromStream(source);
        Check(!oneShot.Replayable && oneShot.Length is null, "one-shot properties");
        await Fails<ServiceError>(() => client.UploadAsync(new() { Body = oneShot }));
        Check(server.Attempts == 1 && server.Received == "one shot" && server.Header("transfer-encoding") == "chunked", "one-shot dispatched once, chunked");
        Check(source.Disposed, "one-shot stream disposed after its call");
        await Fails<InvalidOperationException>(() => client.UploadAsync(new() { Body = oneShot }), "already opened");

        // A factory runs once per attempt and reproduces the bytes; a known length is sent.
        server.Mode = "503-once"; server.Attempts = 0;
        int factories = 0;
        await client.UploadAsync(new() { Body = ByteStream.FromFactory(() => { factories++; return new MemoryStream(Encoding.UTF8.GetBytes("same bytes")); }, 10) });
        Check(factories == 2 && server.Attempts == 2 && server.Received == "same bytes", "factory replays per attempt");
        Check(server.Header("content-length") == "10" && server.Header("transfer-encoding") is null, "known length sent as Content-Length");

        // A seekable stream replays from its starting position; leaveOpen keeps it usable.
        server.Mode = "503-once"; server.Attempts = 0;
        var seekable = new MemoryStream(Encoding.UTF8.GetBytes("skipsame bytes")) { Position = 4 };
        var replay = ByteStream.FromStream(seekable, leaveOpen: true);
        Check(replay.Replayable && replay.Length == 10, "seekable properties");
        await client.UploadAsync(new() { Body = replay });
        Check(server.Attempts == 2 && server.Received == "same bytes" && seekable.CanRead, "seekable replay, left open");
        server.Mode = "ok"; seekable.Position = 0;
        await client.UploadAsync(new() { Body = ByteStream.FromStream(seekable) });
        Check(server.Received == "skipsame bytes" && !seekable.CanRead, "seekable stream disposed when not left open");

        var file = Path.GetTempFileName();
        try
        {
            await File.WriteAllTextAsync(file, "file bytes");
            await client.UploadAsync(new() { Body = ByteStream.FromFile(file) });
            Check(server.Received == "file bytes" && server.Header("content-length") == "10", "file upload");
        }
        finally { File.Delete(file); }

        // A large unknown-length upload streams in chunks under the heap limit.
        await client.UploadAsync(new() { Body = ByteStream.FromStream(new Generated(Large)) });
        Check(server.ReceivedSize == Large && server.Header("transfer-encoding") == "chunked", "large chunked upload");

        // requiresLength and length bounds are checked before dispatch; a declared length must match the bytes.
        server.Attempts = 0;
        await Fails<ArgumentException>(() => client.SizedAsync(new() { Body = ByteStream.FromStream(new OneShot("abc")) }), "known length");
        await Fails<ArgumentException>(() => client.SizedAsync(new() { Body = ByteStream.FromString("abcd") }), "modeled bounds");
        Check(server.Attempts == 0, "invalid lengths not sent");
        await client.SizedAsync(new() { Body = ByteStream.FromString("abc") });
        Check(server.Header("content-length") == "3" && server.Received == "abc", "sized upload");
        await Fails<ArgumentException>(() => client.SizedAsync(new() { Body = ByteStream.FromStream(new OneShot("ab"), 3) }), "declared length");
        await Fails<ArgumentException>(() => client.SizedAsync(new() { Body = ByteStream.FromStream(new OneShot("abcd"), 3) }), "declared length");
        await Fails<ArgumentException>(() => client.UploadAsync(new() { Body = null! }), "required");

        // A response is returned at its headers; the body is read afterwards, once.
        server.Mode = "ok"; server.Release = Server.Signal();
        var output = await client.DownloadAsync().WaitAsync(TimeSpan.FromSeconds(10));
        Check(output.Count == 3 && output.Body.Length == 3 && !output.Body.Replayable, "headers decoded before the body");
        server.Release.TrySetResult();
        Check(Encoding.UTF8.GetString(await output.Body.ReadAllBytesAsync()) == "abc", "download body");
        await Fails<InvalidOperationException>(() => Task.FromResult(output.Body.OpenStream()), "already opened");

        // The synchronous API reads with blocking calls.
        var sync = client.Download();
        using (var reader = new StreamReader(sync.Body.OpenStream())) Check(reader.ReadToEnd() == "abc", "synchronous download");

        // Cancelling an upload in progress disposes the caller's stream that ignores cancellation.
        var slow = new Slow();
        using (var cancel = new CancellationTokenSource())
        {
            var pending = client.UploadAsync(new() { Body = ByteStream.FromStream(slow) }, cancel.Token);
            await slow.Reading.Task.WaitAsync(TimeSpan.FromSeconds(10));
            cancel.Cancel();
            await slow.Closed.Task.WaitAsync(TimeSpan.FromSeconds(10));
            await Fails<OperationCanceledException>(() => pending);
        }

        // Large downloads are pulled through in chunks under the heap limit.
        server.Mode = "large";
        var large = await client.DownloadAsync();
        Check(large.Body.Length == Large && await large.Body.CopyToAsync(Stream.Null) == Large, "large download");
        sampler.Check();

        // Disposing an unread response, or a stream mid-read, releases the connection.
        server.Mode = "flood"; server.Aborted = Server.Signal();
        (await client.DownloadAsync()).Body.Dispose();
        await server.Aborted.Task.WaitAsync(TimeSpan.FromSeconds(10));
        server.Aborted = Server.Signal();
        var partial = (await client.DownloadAsync()).Body.OpenStream();
        Check(await partial.ReadAsync(new byte[16]) > 0, "partial read");
        await partial.DisposeAsync();
        await server.Aborted.Task.WaitAsync(TimeSpan.FromSeconds(10));
        await Fails<ObjectDisposedException>(() => partial.ReadAsync(new byte[1]).AsTask());

        // Disposing from another thread ends a blocked synchronous read.
        server.Mode = "hang"; server.Release = Server.Signal();
        var blocked = client.Download().Body.OpenStream();
        Exception? readError = null;
        var thread = new Thread(() => { try { blocked.ReadByte(); } catch (Exception error) { readError = error; } });
        thread.Start(); Thread.Sleep(200); blocked.Dispose();
        Check(thread.Join(TimeSpan.FromSeconds(10)) && readError is IOException, "blocked read ended by dispose: " + readError);
        server.Release.TrySetResult();

        // The timeout and the call's cancellation token still bound reading after the call returned.
        using (var timed = new StreamingClient(new() { Endpoint = server.Endpoint, Timeout = TimeSpan.FromMilliseconds(500) }))
        {
            server.Release = Server.Signal();
            var late = (await timed.DownloadAsync()).Body.OpenStream();
            var clock = Stopwatch.StartNew();
            var expired = await Fails<TaskCanceledException>(() => late.ReadAsync(new byte[8]).AsTask(), "timeout");
            Check(expired.InnerException is TimeoutException && clock.Elapsed < TimeSpan.FromSeconds(5), "async deadline after return");
            var lateSync = timed.Download().Body.OpenStream();
            clock.Restart();
            await Fails<TaskCanceledException>(() => Task.FromResult(lateSync.ReadByte()), "timeout");
            Check(clock.Elapsed < TimeSpan.FromSeconds(5), "synchronous deadline after return");
            await late.DisposeAsync(); await lateSync.DisposeAsync();
        }
        using (var cancel = new CancellationTokenSource())
        {
            var canceled = (await client.DownloadAsync(cancel.Token)).Body;
            cancel.CancelAfter(200);
            var error = await Fails<OperationCanceledException>(() => canceled.ReadAllBytesAsync());
            Check(error is not TaskCanceledException { InnerException: TimeoutException }, "caller cancellation is not a timeout");
            await canceled.DisposeAsync();
        }
        server.Release.TrySetResult();

        // Invalid response headers release the unread body.
        server.Mode = "invalid"; server.Aborted = Server.Signal();
        await Fails<Exception>(() => client.DownloadAsync());
        await server.Aborted.Task.WaitAsync(TimeSpan.FromSeconds(10));

        // A streaming response checksum is verified incrementally, at its end.
        server.Mode = "ok"; validated.Clear();
        var fetched = await client.FetchAsync(new());
        Check(validated.Count == 0, "not validated before reading");
        Check(Encoding.UTF8.GetString(await fetched.Body.ReadAllBytesAsync()) == "123456789" && validated.SequenceEqual(["Fetch:CRC32"]), "streaming checksum validated");
        server.Mode = "bad";
        var corrupt = (await client.FetchAsync(new())).Body;
        Check((await Fails<ChecksumException>(() => corrupt.ReadAllBytesAsync())).Algorithm == "CRC32" && validated.Count == 1, "streaming checksum mismatch");

        // Request checksums are never computed by buffering a stream.
        server.Mode = "ok";
        await client.StoreAsync(new() { Body = ByteStream.FromString("abc") });
        Check(server.Header("x-algorithm") is null && server.Header("x-amz-checksum-crc32") is null, "no default checksum for a stream");
        await Fails<ArgumentException>(() => client.StoreAsync(new() { Body = ByteStream.FromString("abc"), Algorithm = Algorithm.CRC32 }), "precomputed");
        await client.StoreAsync(new() { Body = ByteStream.FromString("123456789"), Algorithm = Algorithm.CRC32, Checksum = "y/Q5Jg==" });
        Check(server.Header("x-amz-checksum-crc32") == "y/Q5Jg==" && server.Received == "123456789", "supplied streaming checksum");

        // A content digest is declared, computed from a replayable source in a separate pass, or refused for a one-shot.
        var runtimeType = typeof(StreamingClient).Assembly.GetType("Example.Stream.ClientRuntime")!;
        var digesting = Activator.CreateInstance(runtimeType, BindingFlags.NonPublic | BindingFlags.Instance, null, [config, "", true], null)!;
        Task<Unit> Upload(ByteStream body) => (Task<Unit>)runtimeType.GetMethod("CallAsync", BindingFlags.NonPublic | BindingFlags.Instance)!
            .MakeGenericMethod(typeof(Unit)).Invoke(digesting, ["Upload", new UploadInput { Body = body }, CancellationToken.None])!;
        var unread = new OneShot("abc");
        await Fails<ArgumentException>(() => Upload(ByteStream.FromStream(unread)), "declared SHA-256");
        Check(unread.Reads == 0 && !unread.Disposed, "digest refusal did not open the one-shot stream");
        int opens = 0;
        var factory = ByteStream.FromFactory(() => { opens++; return new MemoryStream(Encoding.UTF8.GetBytes("abc")); });
        await Upload(factory);
        Check(opens == 2 && server.Header("x-amz-content-sha256") == Digest && server.Received == "abc", "replayable digest pass");
        opens = 0;
        await Upload(factory.WithSha256(Digest.ToUpperInvariant()));
        Check(opens == 1 && server.Header("x-amz-content-sha256") == Digest, "declared digest");
        await Upload(ByteStream.FromStream(new OneShot("abc")).WithSha256(Digest));
        Check(server.Header("x-amz-content-sha256") == Digest && server.Received == "abc", "declared one-shot digest");
        ((IDisposable)digesting).Dispose();

        // SigV4 signs a stream with its declared digest, else as an unsigned payload; the server re-derives each signature.
        using (var signed = new StreamingClient(new() { Endpoint = server.Endpoint, Region = "us-east-1", Credentials = _ => ValueTask.FromResult<AwsCredentials?>(Identity) }))
        {
            await signed.UploadAsync(new() { Body = ByteStream.FromStream(new OneShot("unsigned")) });
            Check(server.Header("x-amz-content-sha256") == "UNSIGNED-PAYLOAD" && server.Verified == 1, "unsigned streaming payload");
            await signed.UploadAsync(new() { Body = ByteStream.FromString("abc").WithSha256(Digest) });
            Check(server.Header("x-amz-content-sha256") == Digest && server.Verified == 2, "signed declared digest");
            await signed.UploadAsync(new() { Body = ByteStream.FromStream(new Generated(Large)) });
            Check(server.ReceivedSize == Large && server.Header("x-amz-content-sha256") == "UNSIGNED-PAYLOAD" && server.Verified == 3, "large signed upload");
        }
        var time = DateTimeOffset.Parse("2015-08-30T12:36:00Z", CultureInfo.InvariantCulture);
        using (var streamed = new HttpRequestMessage(HttpMethod.Put, "https://example.amazonaws.com/upload") { Content = new ByteStreamContent(ByteStream.FromString("abc").WithSha256(Digest)) })
        using (var buffered = new HttpRequestMessage(HttpMethod.Put, "https://example.amazonaws.com/upload") { Content = new ByteArrayContent(Encoding.UTF8.GetBytes("abc")) })
        {
            await SigV4Signer.SignAsync(streamed, Identity, new("us-east-1", "service") { Time = time });
            await SigV4Signer.SignAsync(buffered, Identity, new("us-east-1", "service") { Time = time });
            Check(streamed.Headers.GetValues("X-Amz-Content-Sha256").Single() == Digest
                && streamed.Headers.GetValues("Authorization").Single() == buffered.Headers.GetValues("Authorization").Single(), "SigV4 declared digest matches the buffered signature");
        }

        sampler.Check();
        if (server.Failure is not null) ExceptionDispatchInfo.Capture(server.Failure).Throw();
        Console.WriteLine($"C# streaming passed: peak managed heap {sampler.Peak / (1024 * 1024)} MiB, peak working set {Process.GetCurrentProcess().PeakWorkingSet64 / (1024 * 1024)} MiB across 256 MiB transfers");
    }

    /// <summary>Samples the managed heap while large bodies move; buffering one would exceed the bound.</summary>
    sealed class MemorySampler : IDisposable
    {
        const long ManagedLimit = 64L * 1024 * 1024, WorkingSetLimit = 224L * 1024 * 1024;
        readonly CancellationTokenSource stop = new();
        readonly Task loop;
        long peak;
        public long Peak => Interlocked.Read(ref peak);
        public MemorySampler() => loop = Task.Run(async () => {
            while (!stop.IsCancellationRequested)
            {
                var current = GC.GetTotalMemory(false);
                if (current > Interlocked.Read(ref peak)) Interlocked.Exchange(ref peak, current);
                try { await Task.Delay(10, stop.Token); } catch (OperationCanceledException) { }
            }
        });
        public void Check()
        {
            var workingSet = Process.GetCurrentProcess().PeakWorkingSet64;
            Program.Check(Peak < ManagedLimit, $"managed heap peaked at {Peak} bytes");
            Program.Check(workingSet < WorkingSetLimit, $"working set peaked at {workingSet} bytes");
        }
        public void Dispose() { stop.Cancel(); loop.Wait(); stop.Dispose(); }
    }

    /// <summary>A readable, non-seekable stream: one-shot, with an unknown length.</summary>
    sealed class OneShot(string text) : Stream
    {
        readonly MemoryStream inner = new(Encoding.UTF8.GetBytes(text));
        public bool Disposed; public int Reads;
        public override bool CanRead => !Disposed;
        public override bool CanSeek => false;
        public override bool CanWrite => false;
        public override long Length => throw new NotSupportedException();
        public override long Position { get => throw new NotSupportedException(); set => throw new NotSupportedException(); }
        public override void Flush() { }
        public override int Read(byte[] buffer, int offset, int count) { Reads++; ObjectDisposedException.ThrowIf(Disposed, this); return inner.Read(buffer, offset, count); }
        public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
        public override void SetLength(long value) => throw new NotSupportedException();
        public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
        protected override void Dispose(bool disposing) { Disposed = true; base.Dispose(disposing); }
    }

    /// <summary>Produces bytes on demand, so a large body never exists in memory.</summary>
    sealed class Generated(long size) : Stream
    {
        long left = size;
        public override bool CanRead => true;
        public override bool CanSeek => false;
        public override bool CanWrite => false;
        public override long Length => throw new NotSupportedException();
        public override long Position { get => throw new NotSupportedException(); set => throw new NotSupportedException(); }
        public override void Flush() { }
        public override int Read(byte[] buffer, int offset, int count) => Read(buffer.AsSpan(offset, count));
        public override int Read(Span<byte> buffer)
        {
            var count = (int)Math.Min(buffer.Length, left);
            buffer[..count].Fill((byte)'x'); left -= count;
            return count;
        }
        public override ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken cancellationToken = default) => ValueTask.FromResult(Read(buffer.Span));
        public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
        public override void SetLength(long value) => throw new NotSupportedException();
        public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
    }

    /// <summary>Blocks every read, ignoring cancellation, until it is disposed.</summary>
    sealed class Slow : Stream
    {
        public readonly TaskCompletionSource Reading = Server.Signal(), Closed = Server.Signal();
        public override bool CanRead => true;
        public override bool CanSeek => false;
        public override bool CanWrite => false;
        public override long Length => throw new NotSupportedException();
        public override long Position { get => throw new NotSupportedException(); set => throw new NotSupportedException(); }
        public override void Flush() { }
        public override int Read(byte[] buffer, int offset, int count) => ReadAsync(buffer.AsMemory(offset, count)).AsTask().GetAwaiter().GetResult();
        public override async ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken cancellationToken = default)
        {
            Reading.TrySetResult();
            await Closed.Task;
            throw new IOException("closed");
        }
        public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
        public override void SetLength(long value) => throw new NotSupportedException();
        public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
        protected override void Dispose(bool disposing) { Closed.TrySetResult(); base.Dispose(disposing); }
    }

    /// <summary>A minimal HTTP/1.1 loopback server: one request per connection, Content-Length or chunked bodies.</summary>
    sealed class Server : IAsyncDisposable
    {
        public static TaskCompletionSource Signal() => new(TaskCreationOptions.RunContinuationsAsynchronously);
        readonly TcpListener listener = new(IPAddress.Loopback, 0);
        readonly CancellationTokenSource stop = new();
        readonly Task loop;
        public string Endpoint { get; }
        public volatile string Mode = "ok";
        public int Attempts, Verified;
        public long ReceivedSize;
        public string Received = "";
        public Dictionary<string, string> Seen = new(StringComparer.OrdinalIgnoreCase);
        public TaskCompletionSource Release = Signal(), Aborted = Signal();
        public Exception? Failure;
        public string? Header(string name) => Seen.GetValueOrDefault(name);

        public Server()
        {
            listener.Start();
            Endpoint = $"http://127.0.0.1:{((IPEndPoint)listener.LocalEndpoint).Port}";
            loop = Task.Run(async () => {
                var connections = new List<Task>();
                try { while (true) { var socket = await listener.AcceptTcpClientAsync(stop.Token); connections.Add(Task.Run(() => Handle(socket))); } }
                catch (Exception) when (stop.IsCancellationRequested) { }
                await Task.WhenAll(connections);
            });
        }

        async Task Handle(TcpClient socket)
        {
            using var _ = socket;
            var stream = socket.GetStream();
            try
            {
                var head = (await ReadLine(stream)).Split(' ');
                var headers = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
                for (var line = await ReadLine(stream); line.Length != 0; line = await ReadLine(stream))
                {
                    var parts = line.Split(':', 2);
                    headers[parts[0].Trim().ToLowerInvariant()] = parts[1].Trim();
                }
                var mode = Mode;
                switch (head[1])
                {
                    case "/upload": case "/sized": case "/store":
                        Interlocked.Increment(ref Attempts);
                        Seen = headers;
                        var (count, sample) = await ReadBody(stream, headers);
                        ReceivedSize = count; Received = Encoding.UTF8.GetString(sample);
                        if (headers.ContainsKey("authorization")) await Verify(head[0], head[1], headers, sample);
                        var status = mode == "503" || mode == "503-once" && Attempts == 1 ? 503 : 204;
                        await Write(stream, $"HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        break;
                    case "/download":
                        var length = mode switch { "large" => Large, "ok" or "hang" => 3, _ => 1L << 30 };
                        await Write(stream, $"HTTP/1.1 200 OK\r\nx-count: {(mode == "invalid" ? "invalid" : "3")}\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n");
                        var chunk = new byte[65536];
                        if (mode is "ok" or "hang")
                        {
                            try { await Release.Task.WaitAsync(TimeSpan.FromSeconds(10), stop.Token); } catch (TimeoutException) { }
                            if (mode == "ok") await stream.WriteAsync("abc"u8.ToArray());
                        }
                        else
                            for (long sent = 0; sent < length; sent += chunk.Length) await stream.WriteAsync(chunk, stop.Token);
                        break;
                    case "/fetch":
                        await Write(stream, $"HTTP/1.1 200 OK\r\nx-amz-checksum-crc32: {(mode == "bad" ? "AAAAAA==" : "y/Q5Jg==")}\r\nContent-Length: 9\r\nConnection: close\r\n\r\n123456789");
                        break;
                    default:
                        await Write(stream, "HTTP/1.1 404 Missing\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        break;
                }
            }
            catch (Exception error) when (error is IOException or SocketException or ObjectDisposedException or OperationCanceledException) { Aborted.TrySetResult(); }
            catch (Exception error) { Failure ??= error; }
        }

        // Re-derives the signature from the received request; the payload is hashed only when the client declared a digest.
        async Task Verify(string method, string path, Dictionary<string, string> headers, byte[] body)
        {
            var unsigned = headers["x-amz-content-sha256"] == "UNSIGNED-PAYLOAD";
            using var request = new HttpRequestMessage(new HttpMethod(method), Endpoint + path) { Content = new ByteArrayContent(body) };
            foreach (var (key, value) in headers)
            {
                if (key is "authorization" or "host" or "content-length" or "transfer-encoding" or "connection" or "x-amz-date" or "x-amz-content-sha256") continue;
                if (!request.Headers.TryAddWithoutValidation(key, value)) request.Content.Headers.TryAddWithoutValidation(key, value);
            }
            var time = DateTimeOffset.ParseExact(headers["x-amz-date"], "yyyyMMdd'T'HHmmss'Z'", CultureInfo.InvariantCulture, DateTimeStyles.AssumeUniversal);
            await SigV4Signer.SignAsync(request, Identity, new("us-east-1", "service") { Time = time, UnsignedPayload = unsigned });
            if (string.Join(",", request.Headers.GetValues("Authorization")) != headers["authorization"]) throw new Exception("SigV4 signature mismatch for " + path);
            if (!headers["authorization"].Contains("x-amz-content-sha256", StringComparison.Ordinal)) throw new Exception("payload digest not signed");
            Interlocked.Increment(ref Verified);
        }

        async Task<(long Count, byte[] Sample)> ReadBody(NetworkStream stream, Dictionary<string, string> headers)
        {
            var buffer = new byte[65536]; var sample = new MemoryStream(); long total = 0;
            async Task Take(long length)
            {
                while (length > 0)
                {
                    var count = await stream.ReadAsync(buffer.AsMemory(0, (int)Math.Min(buffer.Length, length)), stop.Token);
                    if (count == 0) throw new IOException("request body ended early");
                    if (sample.Length < 1024) sample.Write(buffer, 0, (int)Math.Min(count, 1024 - sample.Length));
                    total += count; length -= count;
                }
            }
            if (headers.GetValueOrDefault("transfer-encoding") == "chunked")
            {
                while (true)
                {
                    var size = long.Parse((await ReadLine(stream)).Split(';')[0], NumberStyles.HexNumber, CultureInfo.InvariantCulture);
                    if (size == 0) { while ((await ReadLine(stream)).Length != 0) { } break; }
                    await Take(size);
                    if ((await ReadLine(stream)).Length != 0) throw new IOException("malformed chunk");
                }
            }
            else await Take(long.Parse(headers.GetValueOrDefault("content-length", "0"), CultureInfo.InvariantCulture));
            return (total, sample.ToArray());
        }

        async Task<string> ReadLine(NetworkStream stream)
        {
            var line = new List<byte>(); var one = new byte[1];
            while (true)
            {
                if (await stream.ReadAsync(one, stop.Token) == 0) throw new IOException("connection closed");
                if (one[0] == '\n') break;
                if (one[0] != '\r') line.Add(one[0]);
            }
            return Encoding.ASCII.GetString(line.ToArray());
        }

        Task Write(NetworkStream stream, string text) => stream.WriteAsync(Encoding.ASCII.GetBytes(text), stop.Token).AsTask();

        public async ValueTask DisposeAsync()
        {
            Release.TrySetResult();
            stop.Cancel(); listener.Stop();
            await loop;
            stop.Dispose();
        }
    }
}
