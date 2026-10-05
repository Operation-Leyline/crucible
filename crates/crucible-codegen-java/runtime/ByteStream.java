import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.io.UncheckedIOException;
import java.net.http.HttpRequest;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Objects;
import java.util.Optional;
import java.util.OptionalLong;
import java.util.concurrent.Flow;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.function.Supplier;

/**
 * A streaming blob, read or written without buffering the whole body.
 *
 * <p>A request stream is replayable when it can reproduce the same bytes for every attempt:
 * bytes, a file, or an {@code InputStream} factory. An {@code InputStream} or a publisher is
 * one-shot: it is opened once and the operation is never retried. A response stream is one-shot
 * and keeps the call's overall timeout until it is read to the end or closed; close an unread
 * response stream to release its connection.
 */
public final class ByteStream implements AutoCloseable {
  private final Supplier<Flow.Publisher<java.nio.ByteBuffer>> source;
  private final Runnable discard;
  private final boolean replayable;
  private final Long length;
  private final String sha256;
  private final AtomicBoolean opened;

  private ByteStream(Supplier<Flow.Publisher<ByteBuffer>> source, Runnable discard, boolean replayable,
      Long length, String sha256, AtomicBoolean opened) {
    if (length != null && length < 0) throw new IllegalArgumentException("stream length must be nonnegative");
    this.source = source; this.discard = discard; this.replayable = replayable;
    this.length = length; this.sha256 = sha256; this.opened = opened;
  }

  private static ByteStream replay(Supplier<Flow.Publisher<ByteBuffer>> source, Long length) {
    return new ByteStream(source, () -> {}, true, length, null, new AtomicBoolean());
  }

  /** Replayable bytes; the array is copied. */
  public static ByteStream fromBytes(byte[] bytes) {
    byte[] copy = bytes.clone();
    return replay(() -> HttpRequest.BodyPublishers.ofByteArray(copy), (long) copy.length);
  }

  /** Replayable UTF-8 text. */
  public static ByteStream fromString(String text) {
    return fromBytes(text.getBytes(StandardCharsets.UTF_8));
  }

  /** A replayable file, reopened for every attempt. Its size is read now. */
  public static ByteStream fromFile(Path path) {
    Objects.requireNonNull(path, "path");
    long size;
    try { size = Files.size(path); } catch (IOException error) { throw new UncheckedIOException(error); }
    return replay(() -> {
      try { return HttpRequest.BodyPublishers.ofFile(path); }
      catch (java.io.FileNotFoundException error) { throw new UncheckedIOException(error); }
    }, size);
  }

  /** A one-shot stream of unknown length. The client closes it. */
  public static ByteStream fromInputStream(InputStream input) {
    return oneShot(input, null);
  }

  /** A one-shot stream of a known length. The client closes it. */
  public static ByteStream fromInputStream(InputStream input, long length) {
    return oneShot(input, length);
  }

  private static ByteStream oneShot(InputStream input, Long length) {
    Objects.requireNonNull(input, "input");
    var closed = new AtomicBoolean();
    Runnable release = () -> {
      if (closed.compareAndSet(false, true)) {
        try { input.close(); } catch (IOException ignored) { }
      }
    };
    return new ByteStream(() -> new InputStreamPublisher(() -> input, release), release,
        false, length, null, new AtomicBoolean());
  }

  /** Replayable: the factory runs once per attempt and must return the same bytes each time. */
  public static ByteStream fromInputStreamSupplier(Supplier<? extends InputStream> factory) {
    Objects.requireNonNull(factory, "factory");
    return replay(() -> new InputStreamPublisher(factory), null);
  }

  /** Replayable, with a known length. */
  public static ByteStream fromInputStreamSupplier(Supplier<? extends InputStream> factory, long length) {
    Objects.requireNonNull(factory, "factory");
    return replay(() -> new InputStreamPublisher(factory), length);
  }

  /** A one-shot reactive source of unknown length; it is subscribed once. */
  public static ByteStream fromPublisher(Flow.Publisher<ByteBuffer> publisher) {
    Objects.requireNonNull(publisher, "publisher");
    return new ByteStream(() -> publisher, () -> {}, false, null, null, new AtomicBoolean());
  }

  /** A one-shot reactive source of a known length. */
  public static ByteStream fromPublisher(Flow.Publisher<ByteBuffer> publisher, long length) {
    Objects.requireNonNull(publisher, "publisher");
    return new ByteStream(() -> publisher, () -> {}, false, length, null, new AtomicBoolean());
  }

  /** A response body. */
  static ByteStream response(StreamBody body) {
    return new ByteStream(() -> body, () -> body.abort(new java.util.concurrent.CancellationException("stream closed")),
        false, null, null, new AtomicBoolean());
  }

  /**
   * The same stream, declaring the hex SHA-256 of its bytes. SigV4 and the content digest
   * need it, since a stream is never buffered to compute one.
   */
  public ByteStream withSha256(String hex) {
    if (hex == null || !hex.matches("[0-9a-fA-F]{64}")) throw new IllegalArgumentException("sha256 must contain 64 hexadecimal digits");
    return new ByteStream(source, discard, replayable, length, hex.toLowerCase(java.util.Locale.ROOT), opened);
  }

  public boolean replayable() { return replayable; }
  public OptionalLong length() { return length == null ? OptionalLong.empty() : OptionalLong.of(length); }
  public Optional<String> sha256() { return Optional.ofNullable(sha256); }

  /** Opens the source: once for a one-shot stream, once per call for a replayable one. */
  Flow.Publisher<ByteBuffer> open() {
    if (opened.getAndSet(true) && !replayable) throw new IllegalStateException("one-shot ByteStream already opened");
    return source.get();
  }

  /** Reads the bytes as they arrive, with demand-driven backpressure. */
  public Flow.Publisher<ByteBuffer> publisher() { return open(); }

  /** Reads the bytes as a blocking stream; close it to release the body early. */
  public InputStream inputStream() {
    var input = new StreamInput();
    open().subscribe(input);
    return input;
  }

  /** Reads the remaining bytes into memory. */
  public byte[] readAllBytes() throws IOException {
    try (var input = inputStream(); var output = new ByteArrayOutputStream()) {
      input.transferTo(output);
      return output.toByteArray();
    }
  }

  /** Copies the bytes to {@code output} without buffering them all; returns the count. */
  public long transferTo(OutputStream output) throws IOException {
    try (var input = inputStream()) { return input.transferTo(output); }
  }

  /** Releases an unread stream: cancels a response body, or closes a one-shot input. */
  @Override public void close() {
    opened.set(true);
    discard.run();
  }

  @Override public String toString() {
    return "ByteStream[replayable=" + replayable + ", length=" + (length == null ? "unknown" : length) + "]";
  }
}
