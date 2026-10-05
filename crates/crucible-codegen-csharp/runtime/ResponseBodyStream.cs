/// <summary>
/// A streaming response body. It owns the response and the call's deadline until the body ends
/// or the stream is disposed, verifies modeled length bounds and the response checksum as bytes
/// pass, and maps a deadline or call cancellation during a read to the call's exceptions.
/// </summary>
internal sealed class ResponseBodyStream : global::System.IO.Stream
{
    private const int Open = 0, Ended = 1, Disposed = 2;
    private readonly HttpResponseMessage response;
    private readonly global::System.IO.Stream inner;
    private readonly CancellationTokenSource deadline;
    private readonly CancellationToken expires;
    private readonly CancellationToken caller;
    private readonly CancellationTokenRegistration expiry;
    private readonly long? minimum, maximum;
    private readonly RunningChecksum? checksum;
    private long total;
    private int state;
    private int released;
    private Exception? failure;

    internal ResponseBodyStream(HttpResponseMessage response, global::System.IO.Stream inner, CancellationTokenSource deadline, CancellationToken caller,
        long? minimum, long? maximum, RunningChecksum? checksum)
    {
        this.response = response; this.inner = inner; this.deadline = deadline; this.caller = caller;
        this.minimum = minimum; this.maximum = maximum; this.checksum = checksum;
        expires = deadline.Token;
        // A blocked synchronous read has no token, so an expired deadline closes the connection under it.
        expiry = expires.Register(static self => ((ResponseBodyStream)self!).response.Dispose(), this);
    }

    public override bool CanRead => Volatile.Read(ref state) != Disposed;
    public override bool CanSeek => false;
    public override bool CanWrite => false;
    public override long Length => throw new NotSupportedException();
    public override long Position { get => throw new NotSupportedException(); set => throw new NotSupportedException(); }
    public override void Flush() { }
    public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
    public override void SetLength(long value) => throw new NotSupportedException();
    public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();

    public override int Read(byte[] buffer, int offset, int count) => Read(buffer.AsSpan(offset, count));
    public override int Read(Span<byte> buffer)
    {
        if (Ready()) return 0;
        if (buffer.IsEmpty) return 0;
        int count;
        try { count = inner.Read(buffer); }
        catch (Exception error) { var mapped = Map(error, default); if (ReferenceEquals(mapped, error)) throw; throw mapped; }
        return Advance(buffer[..count]);
    }

    public override Task<int> ReadAsync(byte[] buffer, int offset, int count, CancellationToken cancellationToken) =>
        ReadAsync(buffer.AsMemory(offset, count), cancellationToken).AsTask();
    public override async ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken cancellationToken = default)
    {
        if (Ready()) return 0;
        if (buffer.IsEmpty) return 0;
        int count;
        using (var linked = cancellationToken.CanBeCanceled ? CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, expires) : null)
        {
            try { count = await inner.ReadAsync(buffer, linked?.Token ?? expires).ConfigureAwait(false); }
            catch (Exception error) { var mapped = Map(error, cancellationToken); if (ReferenceEquals(mapped, error)) throw; throw mapped; }
        }
        return Advance(buffer.Span[..count]);
    }

    // True when the body already ended; throws once it failed or was disposed.
    private bool Ready()
    {
        switch (Volatile.Read(ref state))
        {
            case Disposed: throw new ObjectDisposedException(nameof(ResponseBodyStream));
            case Ended: if (failure is not null) throw failure; return true;
            default: return false;
        }
    }

    private int Advance(ReadOnlySpan<byte> bytes)
    {
        if (bytes.IsEmpty) { Finish(); return 0; }
        total += bytes.Length;
        if (total > maximum) Fail(new IOException("Response stream exceeds its modeled maximum length"));
        checksum?.Append(bytes);
        return bytes.Length;
    }

    private void Finish()
    {
        if (Interlocked.CompareExchange(ref state, Ended, Open) != Open) { Ready(); return; }
        try
        {
            if (total < minimum) throw new IOException("Response stream is shorter than its modeled minimum length");
            checksum?.Verify();
        }
        catch (Exception error) { failure = error; throw; }
        finally { Release(); }
    }

    private void Fail(Exception error)
    {
        failure = error;
        Volatile.Write(ref state, Ended);
        Release();
        throw error;
    }

    private Exception Map(Exception error, CancellationToken token)
    {
        if (Volatile.Read(ref state) == Disposed) return new IOException("The response stream was closed during a read", error);
        if (token.IsCancellationRequested) return error is OperationCanceledException ? error : new OperationCanceledException("The response body read was canceled", error, token);
        if (caller.IsCancellationRequested) return new OperationCanceledException("The call was canceled while its response body was read", error, caller);
        if (expires.IsCancellationRequested) return new TaskCanceledException("The client timeout elapsed while the response body was read", new TimeoutException("Client timeout", error));
        return error;
    }

    private void Release()
    {
        if (Interlocked.Exchange(ref released, 1) != 0) return;
        expiry.Dispose();
        response.Dispose();
        deadline.Dispose();
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing && Interlocked.Exchange(ref state, Disposed) != Disposed) Release();
        base.Dispose(disposing);
    }

    public override ValueTask DisposeAsync() { Dispose(); return ValueTask.CompletedTask; }
}
