using System.Buffers;
using System.Net;

/// <summary>
/// HTTP content that sends a <see cref="ByteStream"/> in chunks, opening its source once per send.
/// A known length is sent as Content-Length and must match the bytes produced; otherwise the body
/// is chunked. Middleware sees a streaming request body as this type; reading it consumes a
/// one-shot source.
/// </summary>
public sealed class ByteStreamContent : HttpContent
{
    private readonly long? minimum, maximum;
    private global::System.IO.Stream? active;
    private int disposed;

    /// <summary>Sends the stream as an HTTP body.</summary>
    /// <param name="stream">The stream to send.</param>
    public ByteStreamContent(ByteStream stream) : this(stream, null, null) { }
    internal ByteStreamContent(ByteStream stream, long? minimum, long? maximum)
    {
        Body = stream ?? throw new ArgumentNullException(nameof(stream));
        this.minimum = minimum; this.maximum = maximum;
    }

    /// <summary>The stream this content sends.</summary>
    public ByteStream Body { get; }

    /// <summary>A failure of the caller's stream or its declared length, which is never retried as a network error.</summary>
    internal Exception? SourceFailure { get; private set; }

    /// <inheritdoc/>
    protected override bool TryComputeLength(out long length)
    {
        length = Body.Length ?? 0;
        return Body.Length.HasValue;
    }

    /// <inheritdoc/>
    protected override Task SerializeToStreamAsync(global::System.IO.Stream stream, TransportContext? context) =>
        SerializeToStreamAsync(stream, context, CancellationToken.None);

    /// <inheritdoc/>
    protected override async Task SerializeToStreamAsync(global::System.IO.Stream stream, TransportContext? context, CancellationToken cancellationToken)
    {
        global::System.IO.Stream source; bool owned;
        try { (source, owned) = await Body.OpenAsync(cancellationToken).ConfigureAwait(false); }
        catch (Exception error) when (error is not OperationCanceledException) { SourceFailure = error; throw; }
        if (owned) active = source;
        // A source that ignores cancellation is unblocked by disposing it; a caller's stream left open is not ours to dispose.
        var cancelTarget = owned ? source : Body.CancelTarget;
        using var registration = cancelTarget is null ? default : cancellationToken.Register(static target => ((global::System.IO.Stream)target!).Dispose(), cancelTarget);
        var buffer = ArrayPool<byte>.Shared.Rent(81920);
        try
        {
            long total = 0;
            var declared = Body.Length;
            while (true)
            {
                int count;
                try { count = await source.ReadAsync(buffer, cancellationToken).ConfigureAwait(false); }
                catch (Exception error) when (!cancellationToken.IsCancellationRequested) { SourceFailure = error; throw; }
                if (count == 0) break;
                total += count;
                if (total > declared) Fail("Streaming body exceeded its declared length");
                if (total > maximum) Fail("Streaming body exceeded its modeled maximum length");
                await stream.WriteAsync(buffer.AsMemory(0, count), cancellationToken).ConfigureAwait(false);
            }
            if (total < declared) Fail("Streaming body ended before its declared length");
            if (total < minimum) Fail("Streaming body is shorter than its modeled minimum length");
        }
        finally
        {
            ArrayPool<byte>.Shared.Return(buffer);
            active = null;
            if (owned) await source.DisposeAsync().ConfigureAwait(false);
        }
    }

    private void Fail(string message)
    {
        var error = new ArgumentException(message);
        SourceFailure = error;
        throw error;
    }

    /// <inheritdoc/>
    protected override void Dispose(bool disposing)
    {
        if (disposing && Interlocked.Exchange(ref disposed, 1) == 0 && active is { } open)
        {
            // Disposing a request mid-send stops a source it opened.
            try { open.Dispose(); } catch (ObjectDisposedException) { }
        }
        base.Dispose(disposing);
    }
}
