package example.stream;

import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import example.stream.model.Algorithm;
import example.stream.model.DownloadOutput;
import example.stream.model.Unit;
import example.stream.model.UploadInput;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.net.URI;
import java.net.http.HttpRequest;
import java.net.http.HttpTimeoutException;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.time.Duration;
import java.time.Instant;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.Executors;
import java.util.concurrent.Flow;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicLong;
import java.util.concurrent.atomic.AtomicReference;

/** Streaming blobs against a loopback HTTP server; run with a small heap to prove bounded memory. */
public final class StreamingTest {
  static final long LARGE = 256L * 1024 * 1024;
  static final AtomicReference<String> mode = new AtomicReference<>("ok");
  static final AtomicInteger attempts = new AtomicInteger();
  static final AtomicReference<String> received = new AtomicReference<>();
  static final AtomicLong receivedSize = new AtomicLong();
  static final AtomicReference<Map<String, String>> seen = new AtomicReference<>();
  static final AtomicReference<Throwable> failure = new AtomicReference<>();
  static volatile CountDownLatch release = new CountDownLatch(1);
  static volatile CountDownLatch aborted = new CountDownLatch(1);

  static void check(boolean ok, String message) { if (!ok) throw new AssertionError(message); }

  interface Body { void run() throws Exception; }
  static <T extends Throwable> T fails(Class<T> type, String text, Body body) {
    try { body.run(); } catch (Throwable error) {
      Throwable cause = error;
      while (cause != null && !type.isInstance(cause)) cause = cause.getCause();
      if (cause == null) throw new AssertionError("expected " + type.getSimpleName() + " but got " + error, error);
      check(text == null || String.valueOf(cause.getMessage()).contains(text), "message " + cause.getMessage() + " lacks " + text);
      return type.cast(cause);
    }
    throw new AssertionError("expected " + type.getSimpleName());
  }

  static InputStream generated(long size) {
    return new InputStream() {
      long left = size;
      public int read() { if (left == 0) return -1; left--; return 'x'; }
      public int read(byte[] b, int off, int len) {
        if (left == 0) return -1;
        int n = (int) Math.min(len, left); java.util.Arrays.fill(b, off, off + n, (byte) 'x'); left -= n; return n;
      }
    };
  }

  static void checksumCompletionRace() throws Exception {
    var verifying = new CountDownLatch(1);
    var finish = new CountDownLatch(1);
    var terminal = new CountDownLatch(1);
    var upstream = new AtomicReference<Flow.Subscriber<? super List<ByteBuffer>>>();
    var demand = new AtomicReference<Flow.Subscription>();
    var error = new AtomicReference<Throwable>();
    Checksums.Hasher hasher = new Checksums.Hasher() {
      public void update(ByteBuffer bytes) {}
      public byte[] finish() {
        verifying.countDown();
        try { check(finish.await(5, TimeUnit.SECONDS), "checksum verifier released"); }
        catch (InterruptedException interrupted) { throw new RuntimeException(interrupted); }
        return new byte[]{1};
      }
    };
    var checksum = new Checksums.Running("Fetch", "CRC32", new byte[]{0}, hasher, null);
    var body = StreamBody.response(subscriber -> {
      upstream.set(subscriber);
      subscriber.onSubscribe(new Flow.Subscription() {
        public void request(long n) {}
        public void cancel() {}
      });
    }, null, null, checksum, () -> {});
    body.subscribe(new Flow.Subscriber<ByteBuffer>() {
      public void onSubscribe(Flow.Subscription subscription) { demand.set(subscription); }
      public void onNext(ByteBuffer bytes) {}
      public void onError(Throwable failure) { error.set(failure); terminal.countDown(); }
      public void onComplete() { terminal.countDown(); }
    });
    var ending = new Thread(() -> upstream.get().onComplete());
    ending.start();
    try {
      check(verifying.await(5, TimeUnit.SECONDS), "checksum verification started");
      demand.get().request(1);
      check(terminal.getCount() == 1, "EOF waits for checksum verification despite concurrent demand");
    } finally {
      finish.countDown();
      ending.join(5000);
    }
    check(!ending.isAlive() && terminal.getCount() == 0 && error.get() instanceof ChecksumMismatch,
        "concurrent demand receives checksum mismatch");
  }

  static void handle(HttpExchange exchange) throws IOException, InterruptedException {
    String path = exchange.getRequestURI().getPath();
    var headers = new LinkedHashMap<String, String>();
    exchange.getRequestHeaders().forEach((k, v) -> headers.put(k.toLowerCase(java.util.Locale.ROOT), String.join(",", v)));
    seen.set(headers);
    switch (path) {
      case "/upload", "/sized", "/store" -> {
        attempts.incrementAndGet();
        long count = 0; var small = new ByteArrayOutputStream(); byte[] buffer = new byte[65536]; int n;
        try (InputStream in = exchange.getRequestBody()) {
          while ((n = in.read(buffer)) >= 0) { count += n; if (small.size() < 1024) small.write(buffer, 0, Math.min(n, 1024)); }
        } catch (IOException ignored) { aborted.countDown(); }
        receivedSize.set(count);
        received.set(small.toString(StandardCharsets.UTF_8));
        if (mode.get().equals("503") || mode.get().equals("503-once") && attempts.get() == 1) exchange.sendResponseHeaders(503, -1);
        else exchange.sendResponseHeaders(204, -1);
      }
      case "/download" -> {
        String current = mode.get();
        exchange.getResponseHeaders().add("x-count", current.equals("invalid") ? "invalid" : "3");
        exchange.sendResponseHeaders(200, current.equals("large") ? LARGE : 0);
        OutputStream out = exchange.getResponseBody();
        out.flush();
        try {
          switch (current) {
            case "ok" -> { release.await(10, TimeUnit.SECONDS); out.write("abc".getBytes(StandardCharsets.UTF_8)); }
            case "hang" -> release.await(10, TimeUnit.SECONDS);
            case "large" -> { byte[] chunk = new byte[65536]; for (long sent = 0; sent < LARGE; sent += chunk.length) out.write(chunk); }
            default -> { byte[] chunk = new byte[65536]; for (int i = 0; i < 16384; i++) { out.write(chunk); out.flush(); } }
          }
        } catch (IOException closed) { aborted.countDown(); }
      }
      case "/fetch" -> {
        byte[] body = "123456789".getBytes(StandardCharsets.UTF_8);
        exchange.getResponseHeaders().add("x-amz-checksum-crc32", mode.get().equals("bad") ? "AAAAAA==" : "y/Q5Jg==");
        exchange.sendResponseHeaders(200, body.length);
        exchange.getResponseBody().write(body);
      }
      default -> exchange.sendResponseHeaders(404, -1);
    }
  }

  public static void main(String[] args) throws Exception {
    checksumCompletionRace();
    var server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
    server.setExecutor(Executors.newCachedThreadPool());
    server.createContext("/", exchange -> {
      try { handle(exchange); } catch (Throwable error) { failure.compareAndSet(null, error); }
      finally { exchange.close(); }
    });
    server.start();
    try {
      var config = new ClientConfig();
      config.endpoint = "http://127.0.0.1:" + server.getAddress().getPort();
      config.baseDelay = Duration.ZERO;
      var validated = new ArrayList<String>();
      config.checksumValidated = (operation, algorithm) -> validated.add(operation + ":" + algorithm);
      var client = new StreamingClient(config);

      // One-shot bodies are sent once, even for an idempotent operation that could be retried.
      mode.set("503"); attempts.set(0);
      var oneShot = ByteStream.fromInputStream(new java.io.ByteArrayInputStream("one shot".getBytes(StandardCharsets.UTF_8)));
      check(!oneShot.replayable() && oneShot.length().isEmpty(), "one-shot properties");
      fails(ServiceError.class, null, () -> client.upload(b -> b.body(oneShot)));
      check(attempts.get() == 1 && received.get().equals("one shot"), "one-shot dispatched once");
      fails(IllegalStateException.class, "already opened", () -> client.upload(b -> b.body(oneShot)));

      // A replay factory runs once per attempt and reproduces the bytes; a known length is sent.
      mode.set("503-once"); attempts.set(0);
      var factories = new AtomicInteger();
      client.upload(b -> b.body(ByteStream.fromInputStreamSupplier(() -> {
        factories.incrementAndGet(); return new java.io.ByteArrayInputStream("same bytes".getBytes(StandardCharsets.UTF_8)); }, 10)));
      check(factories.get() == 2 && attempts.get() == 2 && received.get().equals("same bytes"), "replay per attempt");
      check("10".equals(seen.get().get("content-length")), "known length sent as Content-Length");
      mode.set("ok");
      var file = Files.createTempFile("crucible-stream", ".bin");
      try {
        Files.writeString(file, "file bytes");
        client.upload(b -> b.body(ByteStream.fromFile(file)));
        check(received.get().equals("file bytes"), "file upload");
      } finally { Files.deleteIfExists(file); }

      // Large unknown-length upload streams in chunks under a small heap.
      client.upload(b -> b.body(ByteStream.fromInputStream(generated(LARGE))));
      check(receivedSize.get() == LARGE && "chunked".equalsIgnoreCase(seen.get().get("transfer-encoding")), "large chunked upload");

      // requiresLength rejects an unknown length before sending, and actual bytes must match it.
      attempts.set(0);
      fails(IllegalArgumentException.class, "known length", () -> client.sized(b -> b.body(ByteStream.fromInputStream(InputStream.nullInputStream()))));
      check(attempts.get() == 0, "unknown length not sent");
      client.sized(b -> b.body(ByteStream.fromString("abc")));
      check("3".equals(seen.get().get("content-length")) && received.get().equals("abc"), "sized upload");
      fails(IllegalArgumentException.class, "declared length", () -> client.sized(b -> b.body(ByteStream.fromInputStream(new java.io.ByteArrayInputStream("ab".getBytes(StandardCharsets.UTF_8)), 3))));
      fails(IllegalArgumentException.class, "declared length", () -> client.sized(b -> b.body(ByteStream.fromInputStream(new java.io.ByteArrayInputStream("abcd".getBytes(StandardCharsets.UTF_8)), 3))));

      // A response is returned at its headers; the body is read afterwards.
      mode.set("ok"); release = new CountDownLatch(1);
      DownloadOutput output = client.download();
      check(output.count() == 3, "header decoded before the body");
      release.countDown();
      check(new String(output.body().readAllBytes(), StandardCharsets.UTF_8).equals("abc"), "download body");
      fails(IllegalStateException.class, "already opened", () -> output.body().inputStream());

      // Asynchronous calls read with demand-driven publishers.
      release = new CountDownLatch(1); release.countDown();
      var asyncOutput = client.downloadAsync().get(10, TimeUnit.SECONDS);
      var chunks = new ByteArrayOutputStream(); var done = new CountDownLatch(1); var demandOk = new AtomicBoolean(true);
      asyncOutput.body().publisher().subscribe(new Flow.Subscriber<ByteBuffer>() {
        Flow.Subscription subscription; int outstanding;
        public void onSubscribe(Flow.Subscription value) { subscription = value; outstanding = 1; value.request(1); }
        public void onNext(ByteBuffer item) {
          if (--outstanding < 0) demandOk.set(false);
          byte[] bytes = new byte[item.remaining()]; item.get(bytes); chunks.writeBytes(bytes);
          outstanding++; subscription.request(1);
        }
        public void onError(Throwable error) { failure.compareAndSet(null, error); done.countDown(); }
        public void onComplete() { done.countDown(); }
      });
      check(done.await(10, TimeUnit.SECONDS) && demandOk.get() && chunks.toString(StandardCharsets.UTF_8).equals("abc"), "async publisher download");
      client.uploadAsync(UploadInput.builder().body(ByteStream.fromPublisher(HttpRequest.BodyPublishers.ofString("from publisher"), 14)).build()).get(10, TimeUnit.SECONDS);
      check(received.get().equals("from publisher"), "async publisher upload");

      // Cancelling an asynchronous upload closes the caller's stream.
      var closed = new CountDownLatch(1); var reading = new CountDownLatch(1);
      var slow = new InputStream() {
        final CountDownLatch never = new CountDownLatch(1);
        public int read() throws IOException {
          reading.countDown();
          try { never.await(); } catch (InterruptedException interrupted) { throw new IOException(interrupted); }
          throw new IOException("closed");
        }
        public void close() { closed.countDown(); never.countDown(); }
      };
      var pending = client.uploadAsync(UploadInput.builder().body(ByteStream.fromInputStream(slow)).build());
      check(reading.await(10, TimeUnit.SECONDS), "upload started");
      pending.cancel(true);
      check(closed.await(10, TimeUnit.SECONDS), "cancelled upload closed its stream");

      // Large downloads are pulled through under a small heap.
      mode.set("large");
      var counted = new AtomicLong();
      long copied = client.download().body().transferTo(new OutputStream() {
        public void write(int b) { counted.incrementAndGet(); }
        public void write(byte[] b, int off, int len) { counted.addAndGet(len); }
      });
      check(copied == LARGE && counted.get() == LARGE, "large download");

      // Closing an unread response, or closing mid-read, releases the connection.
      mode.set("flood"); aborted = new CountDownLatch(1);
      client.download().body().close();
      check(aborted.await(10, TimeUnit.SECONDS), "unread response released");
      aborted = new CountDownLatch(1);
      var input = client.download().body().inputStream();
      check(input.read(new byte[16]) > 0, "partial read");
      input.close();
      check(aborted.await(10, TimeUnit.SECONDS), "closed response released");
      fails(IOException.class, "closed", () -> input.read());

      // Closing from another thread interrupts a blocked read.
      mode.set("hang"); release = new CountDownLatch(1);
      var blocked = client.download().body().inputStream();
      var readError = new AtomicReference<Throwable>(); var reader = new Thread(() -> {
        try { blocked.read(); } catch (Throwable error) { readError.set(error); } });
      reader.start(); Thread.sleep(200); blocked.close(); reader.join(10000);
      check(readError.get() instanceof IOException, "blocked read cancelled: " + readError.get());
      release.countDown();

      // The overall timeout still bounds reading after the operation returned.
      var timed = new ClientConfig(); timed.endpoint = config.endpoint; timed.timeout = Duration.ofMillis(500);
      release = new CountDownLatch(1);
      var late = new StreamingClient(timed).download().body().inputStream();
      long started = System.nanoTime();
      fails(HttpTimeoutException.class, null, () -> late.read());
      check(System.nanoTime() - started < TimeUnit.SECONDS.toNanos(5), "deadline after return");
      release.countDown();

      // Invalid response headers release the unread body.
      mode.set("invalid"); aborted = new CountDownLatch(1);
      fails(RuntimeException.class, null, () -> client.download());
      check(aborted.await(10, TimeUnit.SECONDS), "invalid response body released");

      // A streaming response checksum is verified at its end.
      mode.set("ok"); validated.clear();
      var fetched = client.fetch();
      check(validated.isEmpty(), "not validated before reading");
      check(new String(fetched.body().readAllBytes(), StandardCharsets.UTF_8).equals("123456789") && validated.equals(List.of("Fetch:CRC32")), "streaming checksum validated");
      mode.set("bad");
      var corrupt = client.fetch().body();
      check(fails(ChecksumMismatch.class, null, corrupt::readAllBytes).algorithm.equals("CRC32"), "streaming checksum mismatch");

      // Request checksums are never computed by buffering a stream.
      mode.set("ok");
      client.store(b -> b.body(ByteStream.fromString("abc")));
      check(!seen.get().containsKey("x-algorithm") && !seen.get().containsKey("x-amz-checksum-crc32"), "no default checksum for a stream");
      fails(IllegalArgumentException.class, "precomputed", () -> client.store(b -> b.body(ByteStream.fromString("abc")).algorithm(Algorithm.CRC32)));
      client.store(b -> b.body(ByteStream.fromString("123456789")).algorithm(Algorithm.CRC32).checksum("y/Q5Jg=="));
      check("y/Q5Jg==".equals(seen.get().get("x-amz-checksum-crc32")), "supplied streaming checksum");

      // A content digest is declared, never computed from a stream.
      String digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
      var digesting = new ClientRuntime(config, "", true, ModelRegistry.classes());
      var opens = new AtomicInteger();
      var unsized = ByteStream.fromInputStreamSupplier(() -> { opens.incrementAndGet(); return new java.io.ByteArrayInputStream("abc".getBytes(StandardCharsets.UTF_8)); });
      fails(IllegalArgumentException.class, "precomputed", () -> ClientRuntime.await(digesting.call("Upload", UploadInput.builder().body(unsized).build(), Unit.class)));
      check(opens.get() == 0, "digest failure did not open the stream");
      ClientRuntime.await(digesting.call("Upload", UploadInput.builder().body(unsized.withSha256(digest.toUpperCase())).build(), Unit.class));
      check(opens.get() == 1 && digest.equals(seen.get().get("x-amz-content-sha256")) && received.get().equals("abc"), "declared digest");

      // SigV4 signs a stream with its declared digest.
      var credentials = new AwsCredentials("AKIDEXAMPLE", "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY", null, null);
      var unsigned = new ClientConfig.Request(URI.create("https://example.amazonaws.com/upload"), "PUT", new LinkedHashMap<>(), null, ByteStream.fromString("abc"));
      fails(IllegalArgumentException.class, "precomputed", () -> Auth.sign(unsigned, credentials, "service", "us-east-1", Instant.parse("2015-08-30T12:36:00Z"), false));
      var declared = new ClientConfig.Request(URI.create("https://example.amazonaws.com/upload"), "PUT", new LinkedHashMap<>(Map.of("x-amz-content-sha256", digest)), null, ByteStream.fromString("abc"));
      Auth.sign(declared, credentials, "service", "us-east-1", Instant.parse("2015-08-30T12:36:00Z"), false);
      check(digest.equals(declared.headers.get("x-amz-content-sha256")) && declared.headers.get("authorization").contains("x-amz-content-sha256"), "SigV4 streaming digest");

      if (failure.get() != null) throw new AssertionError(failure.get());
    } finally {
      release.countDown();
      server.stop(0);
      ((java.util.concurrent.ExecutorService) server.getExecutor()).shutdownNow();
    }
    System.out.println("Java streaming passed: one-shot/replay, requiresLength, bounded 256 MiB transfers, cancellation, deadlines, checksums, digests");
  }
}
