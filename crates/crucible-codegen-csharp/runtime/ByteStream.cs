using System.Text;

/// <summary>
/// A streaming blob, sent or received as the whole HTTP body without buffering it.
/// </summary>
/// <remarks>
/// A request stream is replayable when it can reproduce the same bytes for every attempt:
/// bytes, a file, a seekable System.IO.Stream (replayed from its starting position), or a
/// factory. A non-seekable stream is one-shot: it is read once and its call is never retried.
/// A response stream is one-shot, returned as soon as the response headers arrive, and keeps the
/// call's timeout and cancellation token until it is read to the end or disposed. Dispose an
/// unread response stream to release its connection.
/// </remarks>
public sealed class ByteStream : IAsyncDisposable, IDisposable
{
    // Shared by the copies WithSha256 makes, so a one-shot source is opened once in total.
    private sealed class State
    {
        internal int Opened;
        internal bool Disposed;
        internal global::System.IO.Stream? Caller;
        internal bool LeaveOpen;
        internal long Start;
        internal ResponseBodyStream? Response;
    }
    private readonly Func<CancellationToken, ValueTask<global::System.IO.Stream>>? factory;
    private readonly State state;

    /// <summary>The number of bytes the stream produces, when known; sent as Content-Length.</summary>
    public long? Length { get; }
    /// <summary>Whether a request can send this stream again on a retry.</summary>
    public bool Replayable { get; }
    /// <summary>The declared lowercase hex SHA-256 of the bytes, used for content digests and SigV4.</summary>
    public string? Sha256 { get; }

    private ByteStream(Func<CancellationToken, ValueTask<global::System.IO.Stream>>? factory, State state, bool replayable, long? length, string? sha256)
    {
        if (length < 0) throw new ArgumentOutOfRangeException(nameof(length), "Stream length must be nonnegative");
        this.factory = factory; this.state = state; Replayable = replayable; Length = length; Sha256 = sha256;
    }

    /// <summary>Replayable bytes; the array is copied.</summary>
    public static ByteStream FromBytes(byte[] bytes)
    {
        ArgumentNullException.ThrowIfNull(bytes);
        var copy = (byte[])bytes.Clone();
        return new(_ => ValueTask.FromResult<global::System.IO.Stream>(new MemoryStream(copy, false)), new State(), true, copy.Length, null);
    }

    /// <summary>Replayable UTF-8 text.</summary>
    public static ByteStream FromString(string text) => FromBytes(Encoding.UTF8.GetBytes(text ?? throw new ArgumentNullException(nameof(text))));

    /// <summary>A replayable file, reopened for every attempt. Its size is read now.</summary>
    public static ByteStream FromFile(string path)
    {
        ArgumentNullException.ThrowIfNull(path);
        var info = new FileInfo(path);
        if (!info.Exists) throw new FileNotFoundException("Streaming file not found", path);
        return new(_ => ValueTask.FromResult<global::System.IO.Stream>(new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read, 81920,
            FileOptions.Asynchronous | FileOptions.SequentialScan)), new State(), true, info.Length, null);
    }

    /// <summary>
    /// A caller's stream. A seekable stream is replayable from its current position and its length
    /// defaults to the bytes remaining; any other stream is one-shot. The client disposes the stream
    /// when a call that read it ends, unless <paramref name="leaveOpen"/> is set.
    /// </summary>
    public static ByteStream FromStream(global::System.IO.Stream stream, long? length = null, bool leaveOpen = false)
    {
        ArgumentNullException.ThrowIfNull(stream);
        if (!stream.CanRead) throw new ArgumentException("Streaming input must be readable", nameof(stream));
        var state = new State { Caller = stream, LeaveOpen = leaveOpen };
        if (!stream.CanSeek) return new(_ => ValueTask.FromResult(stream), state, false, length, null);
        state.Start = stream.Position;
        return new(_ => { stream.Position = state.Start; return ValueTask.FromResult(stream); }, state, true, length ?? stream.Length - state.Start, null);
    }

    /// <summary>Replayable: the factory runs once per attempt, must return the same bytes each time, and the client disposes each stream.</summary>
    public static ByteStream FromFactory(Func<global::System.IO.Stream> factory, long? length = null)
    {
        ArgumentNullException.ThrowIfNull(factory);
        return new(_ => ValueTask.FromResult(factory()), new State(), true, length, null);
    }

    /// <summary>Replayable: the asynchronous factory runs once per attempt and must return the same bytes each time.</summary>
    public static ByteStream FromFactory(Func<CancellationToken, ValueTask<global::System.IO.Stream>> factory, long? length = null)
    {
        ArgumentNullException.ThrowIfNull(factory);
        return new(factory, new State(), true, length, null);
    }

    internal static ByteStream Response(ResponseBodyStream body, long? length) =>
        new(null, new State { Response = body }, false, length, null);

    /// <summary>
    /// The same stream, declaring the hex SHA-256 of its bytes. A content digest and SigV4 use it,
    /// since a stream is never buffered to compute one.
    /// </summary>
    public ByteStream WithSha256(string hex)
    {
        if (hex is null || hex.Length != 64 || !hex.All(Uri.IsHexDigit)) throw new ArgumentException("SHA-256 must contain 64 hexadecimal digits", nameof(hex));
        if (factory is null) throw new InvalidOperationException("A response stream has no declared digest");
        return new(factory, state, Replayable, Length, hex.ToLowerInvariant());
    }

    /// <summary>Opens the source: once for a one-shot stream, once per attempt for a replayable one.</summary>
    internal async ValueTask<(global::System.IO.Stream Source, bool Owned)> OpenAsync(CancellationToken token)
    {
        if (state.Response is { } response) { Claim(); return (response, true); }
        lock (state)
        {
            if (state.Opened != 0 && !Replayable) throw new InvalidOperationException("One-shot ByteStream already opened");
            if (state.Disposed) throw new ObjectDisposedException(nameof(ByteStream));
            state.Opened++;
        }
        var stream = await factory!(token).ConfigureAwait(false) ?? throw new InvalidOperationException("Stream factory returned null");
        return (stream, state.Caller is null);
    }

    private void Claim()
    {
        lock (state)
        {
            if (state.Opened++ != 0) throw new InvalidOperationException("Response ByteStream already opened");
            if (state.Disposed) throw new ObjectDisposedException(nameof(ByteStream));
        }
    }

    /// <summary>Whether this call may dispose a caller's stream when a send is canceled.</summary>
    internal global::System.IO.Stream? CancelTarget => state.LeaveOpen ? null : state.Caller;

    /// <summary>Ends a call: a caller's stream that the call read is disposed unless left open.</summary>
    internal void Release()
    {
        global::System.IO.Stream? dispose = null;
        lock (state)
        {
            if (state.Caller is null || state.Opened == 0 || state.LeaveOpen || state.Disposed) return;
            state.Disposed = true; dispose = state.Caller;
        }
        dispose.Dispose();
    }

    /// <summary>
    /// Opens the bytes once as a readable stream. For a response, disposing it releases the
    /// connection; reads observe the call's timeout and cancellation token.
    /// </summary>
    public global::System.IO.Stream OpenStream() => OpenAsync(CancellationToken.None).AsTask().GetAwaiter().GetResult().Source;

    /// <summary>Reads the remaining bytes into memory.</summary>
    public async Task<byte[]> ReadAllBytesAsync(CancellationToken cancellationToken = default)
    {
        var (stream, owned) = await OpenAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            using var memory = new MemoryStream();
            await stream.CopyToAsync(memory, cancellationToken).ConfigureAwait(false);
            return memory.ToArray();
        }
        finally { if (owned) await stream.DisposeAsync().ConfigureAwait(false); }
    }

    /// <summary>Copies the bytes to <paramref name="destination"/> in chunks; returns the count.</summary>
    public async Task<long> CopyToAsync(global::System.IO.Stream destination, CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(destination);
        var (stream, owned) = await OpenAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            var buffer = System.Buffers.ArrayPool<byte>.Shared.Rent(81920);
            try
            {
                long total = 0; int count;
                while ((count = await stream.ReadAsync(buffer, cancellationToken).ConfigureAwait(false)) != 0)
                {
                    await destination.WriteAsync(buffer.AsMemory(0, count), cancellationToken).ConfigureAwait(false);
                    total += count;
                }
                return total;
            }
            finally { System.Buffers.ArrayPool<byte>.Shared.Return(buffer); }
        }
        finally { if (owned) await stream.DisposeAsync().ConfigureAwait(false); }
    }

    /// <summary>Releases an unread stream: cancels a response body, or disposes a caller's stream unless left open.</summary>
    public void Dispose()
    {
        global::System.IO.Stream? dispose;
        lock (state)
        {
            if (state.Disposed) return;
            state.Disposed = true;
            dispose = state.Response ?? (state.LeaveOpen ? null : state.Caller);
        }
        dispose?.Dispose();
    }

    /// <inheritdoc/>
    public ValueTask DisposeAsync() { Dispose(); return ValueTask.CompletedTask; }

    /// <inheritdoc/>
    public override string ToString() => $"ByteStream[Replayable={Replayable}, Length={(Length is long length ? length.ToString(System.Globalization.CultureInfo.InvariantCulture) : "unknown")}]";
}
