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

  static final class BlockingZero extends InputStream {
    final CountDownLatch reading = new CountDownLatch(1), released = new CountDownLatch(1), closed = new CountDownLatch(1);
    final AtomicInteger closes = new AtomicInteger();
    public int read() throws IOException {
      reading.countDown();
      try { check(released.await(10, TimeUnit.SECONDS), "zero source released"); }
      catch (InterruptedException interrupted) { throw new IOException(interrupted); }
      return -1;
    }
    public void close() { closes.incrementAndGet(); released.countDown(); closed.countDown(); }
  }

  static void zeroLengthInputs(StreamingClient client, ClientConfig config) throws Exception {
    attempts.set(0);
    var closes = new AtomicInteger();
    var invalid = ByteStream.fromInputStream(new java.io.ByteArrayInputStream(new byte[]{1}) {
      public void close() { closes.incrementAndGet(); }
    }, 0);
    fails(IllegalArgumentException.class, "declared length", () -> client.upload(b -> b.body(invalid)));
    invalid.close();
    check(attempts.get() == 0 && closes.get() == 1, "nonempty zero-length source rejected and released before dispatch");
    var opens = new AtomicInteger();
    fails(IllegalArgumentException.class, "declared length", () -> client.upload(b -> b.body(ByteStream.fromInputStreamSupplier(() -> {
      opens.incrementAndGet(); return new java.io.ByteArrayInputStream(new byte[]{1});
    }, 0))));
    check(attempts.get() == 0 && opens.get() == 1, "invalid zero-length factory never retried or dispatched");
    fails(java.io.UncheckedIOException.class, "streaming request body failed", () -> client.upload(b -> b.body(ByteStream.fromInputStreamSupplier(() -> {
      opens.incrementAndGet(); return new InputStream() {
        public int read() throws IOException { throw new IOException("source failed"); }
        public void close() { closes.incrementAndGet(); }
      };
    }, 0))));
    check(attempts.get() == 0 && opens.get() == 2 && closes.get() == 2, "zero-length source failure is final and closes its factory IO");

    var timed = new ClientConfig(); timed.endpoint = config.endpoint; timed.timeout = Duration.ofMillis(300);
    var stalled = new BlockingZero();
    var late = new StreamingClient(timed).uploadAsync(UploadInput.builder().body(ByteStream.fromInputStream(stalled, 0)).build());
    check(stalled.reading.await(5, TimeUnit.SECONDS), "zero-length deadline preflight started");
    fails(HttpTimeoutException.class, null, () -> late.get(5, TimeUnit.SECONDS));
    check(stalled.closed.await(5, TimeUnit.SECONDS) && stalled.closes.get() == 1 && attempts.get() == 0,
        "deadline closes stalled zero-length preflight without dispatch");
    var cancelled = new BlockingZero();
    var pending = client.uploadAsync(UploadInput.builder().body(ByteStream.fromInputStream(cancelled, 0)).build());
    check(cancelled.reading.await(5, TimeUnit.SECONDS), "zero-length cancellation preflight started");
    pending.cancel(true);
    check(cancelled.closed.await(5, TimeUnit.SECONDS) && cancelled.closes.get() == 1 && attempts.get() == 0,
        "cancellation closes zero-length preflight without dispatch");

    mode.set("503-once"); opens.set(0); closes.set(0);
    client.upload(b -> b.body(ByteStream.fromInputStreamSupplier(() -> {
      opens.incrementAndGet(); return new java.io.ByteArrayInputStream(new byte[0]) {
        public void close() { closes.incrementAndGet(); }
      };
    }, 0)));
    check(attempts.get() == 2 && opens.get() == 2 && closes.get() == 2 && receivedSize.get() == 0,
        "empty zero-length factories validate and close once per retry");
    mode.set("ok");
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

  static InputStream trackedInput(AtomicInteger closes) {
    return new InputStream() {
      public int read() { return -1; }
      public void close() { closes.incrementAndGet(); }
    };
  }

  static final class WaiterHttp extends java.net.http.HttpClient {
    final java.util.concurrent.CompletableFuture<java.net.http.HttpResponse<Object>> sent = new java.util.concurrent.CompletableFuture<>();
    final CountDownLatch entered = new CountDownLatch(1);
    volatile HttpRequest request;
    public java.util.Optional<java.net.CookieHandler> cookieHandler() { return java.util.Optional.empty(); }
    public java.util.Optional<Duration> connectTimeout() { return java.util.Optional.empty(); }
    public Redirect followRedirects() { return Redirect.NEVER; }
    public java.util.Optional<java.net.ProxySelector> proxy() { return java.util.Optional.empty(); }
    public javax.net.ssl.SSLContext sslContext() {
      try { return javax.net.ssl.SSLContext.getDefault(); }
      catch (java.security.NoSuchAlgorithmException failure) { throw new IllegalStateException(failure); }
    }
    public javax.net.ssl.SSLParameters sslParameters() { return new javax.net.ssl.SSLParameters(); }
    public java.util.Optional<java.net.Authenticator> authenticator() { return java.util.Optional.empty(); }
    public Version version() { return Version.HTTP_1_1; }
    public java.util.Optional<java.util.concurrent.Executor> executor() { return java.util.Optional.empty(); }
    public <T> java.net.http.HttpResponse<T> send(HttpRequest request, java.net.http.HttpResponse.BodyHandler<T> handler) { throw new UnsupportedOperationException(); }
    @SuppressWarnings("unchecked")
    public <T> java.util.concurrent.CompletableFuture<java.net.http.HttpResponse<T>> sendAsync(HttpRequest request, java.net.http.HttpResponse.BodyHandler<T> handler) {
      this.request = request; entered.countDown();
      return (java.util.concurrent.CompletableFuture<java.net.http.HttpResponse<T>>) (java.util.concurrent.CompletableFuture<?>) sent;
    }
    public <T> java.util.concurrent.CompletableFuture<java.net.http.HttpResponse<T>> sendAsync(HttpRequest request, java.net.http.HttpResponse.BodyHandler<T> handler, java.net.http.HttpResponse.PushPromiseHandler<T> push) { return sendAsync(request, handler); }
    java.net.http.HttpResponse<Object> response(Flow.Publisher<List<ByteBuffer>> body) {
      return new java.net.http.HttpResponse<>() {
        public int statusCode() { return 200; }
        public HttpRequest request() { return request; }
        public java.util.Optional<java.net.http.HttpResponse<Object>> previousResponse() { return java.util.Optional.empty(); }
        public java.net.http.HttpHeaders headers() { return java.net.http.HttpHeaders.of(Map.of(), (name, value) -> true); }
        public Object body() { return body; }
        public java.util.Optional<javax.net.ssl.SSLSession> sslSession() { return java.util.Optional.empty(); }
        public URI uri() { return request.uri(); }
        public Version version() { return Version.HTTP_1_1; }
      };
    }
  }

  static void cancelledWaiterResponseOwnership() throws Exception {
    com.google.gson.JsonObject schema;
    try (var resource = StreamingTest.class.getResourceAsStream("schema.json")) {
      schema = new com.google.gson.Gson().fromJson(new String(resource.readAllBytes(), StandardCharsets.UTF_8), com.google.gson.JsonObject.class);
    }
    schema.getAsJsonObject("operations").getAsJsonObject("Download").add("waiters", com.google.gson.JsonParser.parseString(
        "{\"Ready\":{\"minDelay\":0,\"maxDelay\":0,\"acceptors\":[{\"state\":\"success\",\"matcher\":{\"kind\":\"success\",\"expected\":true}}]}}"));
    var http = new WaiterHttp(); var config = new ClientConfig();
    config.endpoint = "http://localhost"; config.httpClient = http; config.timeout = Duration.ofSeconds(60);
    var runtime = new ClientRuntime(config, "", false, ModelRegistry.classes(), schema);
    var waiter = runtime.waitUntil("Download", "Ready", null, DownloadOutput.class,
        new WaiterOptions(Duration.ofSeconds(60), Duration.ZERO, Duration.ZERO));
    check(http.entered.await(5, TimeUnit.SECONDS), "streaming waiter operation started");
    var activeField = waiter.getClass().getDeclaredField("active"); activeField.setAccessible(true);
    @SuppressWarnings("unchecked") var active = (AtomicReference<java.util.concurrent.Future<?>>) activeField.get(waiter);
    java.util.concurrent.CompletableFuture<?> operation = null;
    long until = System.nanoTime() + TimeUnit.SECONDS.toNanos(5);
    while (System.nanoTime() < until) {
      var found = active.get();
      if (found instanceof java.util.concurrent.CompletableFuture<?> future && future.getNumberOfDependents() >= 2) { operation = future; break; }
      Thread.sleep(1);
    }
    check(operation != null, "waiter callback registered on operation");
    var ready = new CountDownLatch(1); var releaseCompletion = new CountDownLatch(1); var cancelled = new CountDownLatch(1);
    var owned = new AtomicReference<DownloadOutput>(); var cancellations = new AtomicInteger();
    // The later synchronous dependent runs first, holding the completed operation
    // before the waiter's asynchronous dependent can observe its response.
    operation.whenComplete((value, error) -> {
      check(error == null, "streaming waiter operation succeeded");
      owned.set((DownloadOutput) value); ready.countDown();
      try { check(releaseCompletion.await(5, TimeUnit.SECONDS), "operation completion released"); }
      catch (InterruptedException interrupted) { throw new RuntimeException(interrupted); }
    });
    Flow.Publisher<List<ByteBuffer>> body = subscriber -> subscriber.onSubscribe(new Flow.Subscription() {
      public void request(long count) {}
      public void cancel() { cancellations.incrementAndGet(); cancelled.countDown(); }
    });
    var completion = new Thread(() -> http.sent.complete(http.response(body)));
    completion.start();
    try {
      check(ready.await(5, TimeUnit.SECONDS), "completed streaming response held before waiter callback");
      check(operation.isDone() && owned.get().body() != null, "completed operation owns unread response");
      check(waiter.cancel(true) && !operation.cancel(true), "waiter cancelled after operation completed");
      releaseCompletion.countDown();
      check(cancelled.await(5, TimeUnit.SECONDS), "cancelled waiter callback releases its late response");
      owned.get().body().close();
      check(cancellations.get() == 1, "late waiter response released exactly once");
    } finally {
      releaseCompletion.countDown(); waiter.cancel(true);
      completion.join(5000);
      if (owned.get() != null) owned.get().body().close();
    }
    check(!completion.isAlive(), "controlled operation completion finished");
  }

  public static final class MalformedInput {
    public ByteStream body;
  }

  static final class PendingSource {
    final AtomicInteger closes = new AtomicInteger(), opens = new AtomicInteger(), reads = new AtomicInteger();
    final CountDownLatch closed = new CountDownLatch(1);
    final ByteStream stream;
    final boolean replayable;
    PendingSource(boolean replayable) {
      this.replayable = replayable;
      java.util.function.Supplier<InputStream> factory = () -> {
        opens.incrementAndGet();
        return new InputStream() {
          public int read() { reads.incrementAndGet(); return -1; }
          public void close() { closes.incrementAndGet(); closed.countDown(); }
        };
      };
      if (replayable) stream = ByteStream.fromInputStreamSupplier(factory);
      else { stream = ByteStream.fromInputStream(factory.get()); opens.set(0); }
    }
    void checkReleased(String message) throws InterruptedException {
      if (!replayable) check(closed.await(5, TimeUnit.SECONDS), message + " completed");
      check(closes.get() == (replayable ? 0 : 1) && opens.get() == 0 && reads.get() == 0, message);
      stream.close();
      check(closes.get() == (replayable ? 0 : 1), message + " is idempotent");
    }
  }

  static void unopenedCallOwnership(ClientConfig original) throws Exception {
    attempts.set(0);
    for (boolean replayable : new boolean[]{false, true}) {
      var source = new PendingSource(replayable);
      var client = new StreamingClient(original);
      fails(IllegalArgumentException.class, "known length", () -> client.sized(b -> b.body(source.stream)));
      source.checkReleased("requiresLength failure releases only supplied one-shot input");

      var malformed = new PendingSource(replayable);
      var input = new MalformedInput(); input.body = malformed.stream;
      var runtime = new ClientRuntime(original, "", false, ModelRegistry.classes());
      fails(IllegalStateException.class, null, () -> ClientRuntime.await(runtime.call("Store", input, Unit.class)));
      malformed.checkReleased("malformed input cleanup precedes encoding");

      var digest = new PendingSource(replayable);
      var digesting = new ClientRuntime(original, "", true, ModelRegistry.classes());
      fails(IllegalArgumentException.class, "precomputed", () -> ClientRuntime.await(digesting.call("Upload", UploadInput.builder().body(digest.stream).build(), Unit.class)));
      digest.checkReleased("precomputed digest failure releases only supplied input");

      var middleware = new PendingSource(replayable);
      var failing = new ClientConfig(); failing.endpoint = original.endpoint;
      failing.middleware.add(request -> { throw new IllegalStateException("middleware failed"); });
      fails(IllegalStateException.class, "middleware failed", () -> new StreamingClient(failing).upload(b -> b.body(middleware.stream)));
      middleware.checkReleased("failed middleware cleanup");

      var token = new PendingSource(replayable);
      var auth = new ClientConfig(); auth.endpoint = original.endpoint;
      auth.token = () -> { throw new IllegalStateException("token failed"); };
      com.google.gson.JsonObject schema;
      try (var resource = StreamingTest.class.getResourceAsStream("schema.json")) {
        schema = new com.google.gson.Gson().fromJson(new String(resource.readAllBytes(), StandardCharsets.UTF_8), com.google.gson.JsonObject.class);
      }
      schema.getAsJsonObject("operations").getAsJsonObject("Upload").addProperty("bearer", true);
      var authenticated = new ClientRuntime(auth, "", false, ModelRegistry.classes(), schema);
      fails(IllegalStateException.class, "token failed", () -> ClientRuntime.await(authenticated.call("Upload", UploadInput.builder().body(token.stream).build(), Unit.class)));
      token.checkReleased("failed token provider cleanup");

      for (boolean timeout : new boolean[]{false, true}) {
        var pending = new PendingSource(replayable);
        var entered = new CountDownLatch(1); var releaseHook = new CountDownLatch(1); var finished = new CountDownLatch(1);
        var delayed = new ClientConfig(); delayed.endpoint = original.endpoint;
        if (timeout) delayed.timeout = Duration.ofMillis(300);
        delayed.middleware.add(request -> {
          entered.countDown();
          try { check(releaseHook.await(5, TimeUnit.SECONDS), "middleware released"); }
          catch (InterruptedException interrupted) { throw new RuntimeException(interrupted); }
          finally { finished.countDown(); }
        });
        var future = new StreamingClient(delayed).uploadAsync(UploadInput.builder().body(pending.stream).build());
        try {
          check(entered.await(5, TimeUnit.SECONDS), "middleware entered before source open");
          if (timeout) fails(HttpTimeoutException.class, null, () -> future.get(5, TimeUnit.SECONDS));
          else future.cancel(true);
        } finally { releaseHook.countDown(); }
        check(finished.await(5, TimeUnit.SECONDS), "middleware returned after terminal call");
        pending.checkReleased(timeout ? "deadline before source open cleanup" : "cancellation before source open cleanup");
      }
    }
    check(attempts.get() == 0, "pre-dispatch failures and cancellation never reach HTTP");
  }

  static void unreadInputOwnership() throws Exception {
    var closes = new AtomicInteger();
    var source = ByteStream.fromInputStream(trackedInput(closes));
    source.inputStream().close();
    check(closes.get() == 1, "closing an unread input releases the supplied stream");
    source.close();
    check(closes.get() == 1, "one-shot input release is idempotent");

    closes.set(0);
    var published = ByteStream.fromInputStream(trackedInput(closes));
    published.publisher().subscribe(new Flow.Subscriber<ByteBuffer>() {
      public void onSubscribe(Flow.Subscription subscription) { subscription.cancel(); }
      public void onNext(ByteBuffer bytes) { throw new AssertionError("cancelled source emitted bytes"); }
      public void onError(Throwable error) { throw new AssertionError(error); }
      public void onComplete() { throw new AssertionError("cancelled source completed"); }
    });
    check(closes.get() == 1, "cancellation before demand releases the supplied stream");
    published.close();
    check(closes.get() == 1, "publisher cancellation and discard share ownership");

    var opens = new AtomicInteger();
    var lazy = ByteStream.fromInputStreamSupplier(() -> {
      opens.incrementAndGet();
      return InputStream.nullInputStream();
    });
    lazy.inputStream().close();
    lazy.close();
    check(opens.get() == 0, "closing an unread factory stream never opens it");
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
    cancelledWaiterResponseOwnership();
    unreadInputOwnership();
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
      unopenedCallOwnership(config);

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
      var emptyCloses = new AtomicInteger();
      var empty = ByteStream.fromInputStream(trackedInput(emptyCloses), 0);
      client.upload(b -> b.body(empty));
      check(receivedSize.get() == 0 && emptyCloses.get() == 1, "zero-length upload releases its unread source");
      empty.close();
      check(emptyCloses.get() == 1, "zero-length upload release is idempotent");
      zeroLengthInputs(client, config);
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
