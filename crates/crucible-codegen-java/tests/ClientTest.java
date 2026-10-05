package example.weather;

import example.weather.model.CitySummary;
import example.weather.model.ConflictError;
import example.weather.model.Coordinates;
import example.weather.model.CreateCityInput;
import example.weather.model.GetCityInput;
import example.weather.model.GetCityPhotoInput;
import example.weather.model.ListCitiesInput;
import example.weather.model.Location;
import example.weather.model.PutCityPhotoInput;
import example.weather.model.Sky;
import example.weather.model.Unit;
import example.weather.model.ValidationError;
import example.weather.model.ValidationReason;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.time.Duration;
import java.time.Instant;
import java.util.ArrayList;
import java.util.HexFormat;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;

public final class ClientTest {
  interface Handler {
    void run(HttpExchange exchange) throws Exception;
  }

  static final class Server implements AutoCloseable {
    final HttpServer server;
    volatile Throwable failure;

    Server(Handler handler) throws Exception {
      server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
      server.createContext(
          "/",
          exchange -> {
            try {
              handler.run(exchange);
            } catch (Throwable error) {
              failure = error;
              exchange.close();
            }
          });
      server.start();
    }

    ClientConfig config() {
      var config = new ClientConfig();
      config.endpoint = "http://127.0.0.1:" + server.getAddress().getPort();
      config.baseDelay = Duration.ZERO;
      return config;
    }

    public void close() {
      server.stop(0);
      if (failure != null) throw new AssertionError(failure);
    }
  }

  static void check(boolean condition, String message) {
    if (!condition) throw new AssertionError(message);
  }

  static void response(HttpExchange exchange, int status, String body) throws Exception {
    byte[] bytes = body.getBytes(StandardCharsets.UTF_8);
    exchange.sendResponseHeaders(status, bytes.length);
    exchange.getResponseBody().write(bytes);
    exchange.close();
  }

  static void emptyResponse(HttpExchange exchange) throws Exception {
    // Unit calls complete at headers and cancel their response subscription.
    // Sending a body here would race that cancellation and fixture shutdown.
    exchange.sendResponseHeaders(200, -1);
    exchange.close();
  }

  static void retries(Path cases) throws Exception {
    for (var entry : JsonParser.parseString(Files.readString(cases)).getAsJsonArray()) {
      var test = entry.getAsJsonObject();
      var attempts = new AtomicInteger();
      try (var server =
          new Server(
              exchange -> {
                int count = attempts.incrementAndGet();
                var body = new JsonObject();
                if (!test.get("error").isJsonNull()) body.add("__type", test.get("error"));
                if (count <= test.get("failures").getAsInt())
                  response(exchange, test.get("status").getAsInt(), body.toString());
                else emptyResponse(exchange);
              })) {
        JsonObject schema =
            JsonParser.parseString(
                    "{\"bearer\":false,\"shapes\":{},\"operations\":{\"Retry\":{\"method\":\"POST\",\"uri\":\"/\",\"input\":null,\"output\":null,\"token\":null,\"request\":{\"labels\":[],\"query\":[],\"headers\":[],\"body\":[]},\"response\":{\"headers\":[],\"body\":[]},\"errors\":[\"Ordinary\",\"Transient\",\"Throttled\"]}}}")
                .getAsJsonObject();
        for (String name : List.of("Ordinary", "Transient", "Throttled")) {
          JsonObject shape =
              JsonParser.parseString(
                      "{\"kind\":\"structure\",\"members\":{},\"error\":{\"headers\":[]}}")
                  .getAsJsonObject();
          shape.addProperty("name", name);
          var error = shape.getAsJsonObject("error");
          error.addProperty("status", name.equals("Throttled") ? 429 : 400);
          error.addProperty("retryable", !name.equals("Ordinary"));
          error.addProperty("throttling", name.equals("Throttled"));
          schema.getAsJsonObject("shapes").add(name, shape);
        }
        schema.getAsJsonObject("operations").getAsJsonObject("Retry").add("safe", test.get("safe"));
        var config = server.config();
        config.maxAttempts = test.get("maxAttempts").getAsInt();
        var runtime =
            new ClientRuntime(
                config,
                "",
                false,
                Map.of(
                    "Ordinary",
                    ServiceError.class,
                    "Transient",
                    ServiceError.class,
                    "Throttled",
                    ServiceError.class),
                schema);
        boolean success = false;
        try {
          runtime.call("Retry", null, Unit.class).get(5, TimeUnit.SECONDS);
          success = true;
        } catch (java.util.concurrent.ExecutionException e) {
          check(e.getCause() instanceof ServiceError, "wrong error " + e.getCause());
        }
        check(
            attempts.get() == test.get("attempts").getAsInt()
                && success == test.get("success").getAsBoolean(),
            test.get("name").getAsString());
      }
    }
  }

  static void sharedSerialization(Path path) throws Exception {
    var config = new ClientConfig();
    config.endpoint = "https://example.invalid";
    var runtime = new ClientRuntime(config, "", false, ModelRegistry.classes());
    for (var row : JsonParser.parseString(Files.readString(path)).getAsJsonArray()) {
      var test = row.getAsJsonObject();
      String id = test.get("id").getAsString(), target = test.get("target").getAsString();
      Object nativeValue;
      try {
        nativeValue =
            runtime.decode(target, JsonParser.parseString(test.get("wire").getAsString()), null);
      } catch (IllegalArgumentException | ArithmeticException e) {
        if (test.has("reject") && test.get("reject").getAsBoolean()) continue;
        throw new AssertionError(id, e);
      }
      check(
          !test.has("reject") || !test.get("reject").getAsBoolean(), id + " accepted invalid wire");
      var expected = test.getAsJsonObject("expect");
      var actual = runtime.encode(target, nativeValue, null);
      // Gson's numeric equals compares through double for lazily parsed numbers.
      check(
          exactJsonEquals(actual, JsonParser.parseString(expected.get("json").getAsString())),
          id + ": " + actual);
      if (expected.has("states"))
        for (var state : expected.getAsJsonObject("states").entrySet()) {
          var field = nativeValue.getClass().getField(state.getKey()).get(nativeValue);
          String value =
              field instanceof OptionalValue<?> optional
                  ? (!optional.present() ? "absent" : optional.value() == null ? "null" : "value")
                  : "value";
          check(value.equals(state.getValue().getAsString()), id + " native presence");
        }
      if (expected.has("blobHex"))
        check(
            HexFormat.of()
                .formatHex((byte[]) nativeValue)
                .equals(expected.get("blobHex").getAsString()),
            id + " bytes");
      if (expected.has("instant")) {
        var instant = expected.getAsJsonObject("instant");
        var optional =
            (OptionalValue<?>)
                nativeValue
                    .getClass()
                    .getField(instant.get("member").getAsString())
                    .get(nativeValue);
        var value = (Instant) optional.value();
        check(
            Long.toString(value.getEpochSecond()).equals(instant.get("seconds").getAsString())
                && value.getNano() == instant.get("nanoseconds").getAsInt(),
            id + " instant");
      }
    }
  }

  static boolean exactJsonEquals(com.google.gson.JsonElement a, com.google.gson.JsonElement b) {
    if (a.isJsonPrimitive() && a.getAsJsonPrimitive().isNumber())
      return b.isJsonPrimitive()
          && b.getAsJsonPrimitive().isNumber()
          && a.getAsBigDecimal().compareTo(b.getAsBigDecimal()) == 0;
    if (a.isJsonObject()) {
      if (!b.isJsonObject() || a.getAsJsonObject().size() != b.getAsJsonObject().size())
        return false;
      for (var entry : a.getAsJsonObject().entrySet())
        if (!b.getAsJsonObject().has(entry.getKey())
            || !exactJsonEquals(entry.getValue(), b.getAsJsonObject().get(entry.getKey())))
          return false;
      return true;
    }
    if (a.isJsonArray()) {
      if (!b.isJsonArray() || a.getAsJsonArray().size() != b.getAsJsonArray().size()) return false;
      for (int i = 0; i < a.getAsJsonArray().size(); i++)
        if (!exactJsonEquals(a.getAsJsonArray().get(i), b.getAsJsonArray().get(i))) return false;
      return true;
    }
    return a.equals(b);
  }

  static void serialization(Path headers) throws Exception {
    sharedSerialization(headers.resolveSibling("serialization.json"));
    ClientConfig config = new ClientConfig();
    config.endpoint = "https://example.invalid";
    var runtime = new ClientRuntime(config, "", false, ModelRegistry.classes());
    var sky = runtime.decode("example.weather#Sky", JsonParser.parseString("\"future\""), null);
    check(sky.toString().equals("future"), "unknown enum");
    var wire = JsonParser.parseString("{\"future\":{\"n\":9223372036854775807}}");
    var union = runtime.decode("example.weather#Location", wire, null);
    check(runtime.encode("example.weather#Location", union, null).equals(wire), "unknown union");
    var unit = new Location();
    unit.unknown = OptionalValue.of(new Unit());
    check(
        runtime
            .encode("example.weather#Location", unit, null)
            .toString()
            .equals("{\"unknown\":{}}"),
        "unit union");
    var city =
        (CitySummary)
            runtime.decode(
                "example.weather#CitySummary",
                JsonParser.parseString(
                    "{\"id\":\"c\",\"name\":\"x\",\"createdAt\":-0.000000001,\"population\":9223372036854775807}"),
                null);
    check(
        city.population == Long.MAX_VALUE
            && city.createdAt.value().equals(Instant.ofEpochSecond(-1, 999999999)),
        "numeric precision");
    var absent =
        (CitySummary)
            runtime.decode(
                "example.weather#CitySummary",
                JsonParser.parseString("{\"id\":\"c\",\"name\":\"x\"}"),
                null);
    check(absent.population == 0, "default missing");
    var input = new CreateCityInput();
    input.name = OptionalValue.of("x");
    input.location = OptionalValue.of(null);
    var encoded = runtime.encode("example.weather#CreateCityInput", input, null).getAsJsonObject();
    check(encoded.get("location").isJsonNull() && !encoded.has("clientToken"), "presence lost");
    var member = JsonParser.parseString("{\"target\":\"example.weather#Tags\"}").getAsJsonObject();
    for (var entry : JsonParser.parseString(Files.readString(headers)).getAsJsonArray()) {
      var test = entry.getAsJsonObject();
      check(
          runtime.header(member, test.get("wire").getAsString()).equals(test.get("values")),
          "header lists");
      var headerText = ClientRuntime.bound(test.get("values"));
      check(headerText.equals(test.get("encoded").getAsString()), "header encoding");
      check(runtime.header(member, headerText).equals(test.get("values")), "header roundtrip");
    }
    check(
        runtime
            .encode("example.weather#DateTime", Instant.parse("2026-10-01T00:00:00Z"), "http-date")
            .getAsString()
            .equals("Thu, 01 Oct 2026 00:00:00 GMT"),
        "HTTP-date format");
  }

  static void httpBehavior() throws Exception {
    var sent = new ArrayList<byte[]>();
    var credentials = new AtomicInteger();
    try (var server =
        new Server(
            exchange -> {
              byte[] bytes = exchange.getRequestBody().readAllBytes();
              sent.add(bytes);
              check(
                  exchange
                      .getRequestHeaders()
                      .getFirst("x-amz-content-sha256")
                      .equals(
                          HexFormat.of()
                              .formatHex(MessageDigest.getInstance("SHA-256").digest(bytes))),
                  "digest");
              check(
                  exchange
                      .getRequestHeaders()
                      .getFirst("authorization")
                      .equals("Bearer " + sent.size()),
                  "auth refresh");
              check(
                  exchange.getRequestHeaders().getFirst("x-expected-revision").equals("7"),
                  "bound header");
              response(
                  exchange,
                  sent.size() == 1 ? 503 : 200,
                  sent.size() == 1
                      ? "{\"__type\":\"Unknown\"}"
                      : "{\"city\":{\"id\":\"c\",\"name\":\"é\"}}");
            })) {
      var config = server.config();
      config.token = () -> Integer.toString(credentials.incrementAndGet());
      var client = new WeatherClient(config);
      var input = new CreateCityInput();
      input.name = OptionalValue.of("é");
      input.expectedRevision = OptionalValue.of(7L);
      var city = client.createCityAsync(input).get(5, TimeUnit.SECONDS);
      check(city.city.name.equals("é"), "typed output");
      check(
          !input.clientToken.present() && java.util.Arrays.equals(sent.get(0), sent.get(1)),
          "token mutated or retry body changed");
    }
    try (var server =
        new Server(
            exchange -> {
              check(
                  exchange.getRequestURI().getRawPath().contains("a%2Fb%20c~%C3%A9%2A"),
                  "label encoding");
              exchange.getResponseHeaders().set("x-request-id", "req-1");
              response(
                  exchange,
                  200,
                  "{\"city\":{\"id\":\"c\",\"name\":\"x\"},\"forecast\":{\"future\":123}}");
            })) {
      var client = new WeatherClient(server.config());
      var input = new GetCityInput();
      input.cityId = OptionalValue.of("a/b c~é*");
      var output = client.getCity(input);
      check(
          output.requestId.value().equals("req-1")
              && output.forecast.value().containsKey(new Sky("future")),
          "response bindings");
    }
    try (var server =
        new Server(
            exchange -> {
              exchange.getResponseHeaders().set("x-request-id", "req");
              response(
                  exchange,
                  400,
                  "{\"__type\":\"ValidationError\",\"message\":\"bad\",\"reason\":\"fieldValidationFailed\",\"requestId\":\"body-id\"}");
            })) {
      var client = new WeatherClient(server.config());
      try {
        client.ping();
        throw new AssertionError("error ignored");
      } catch (ValidationError error) {
        check(
            error.reason.equals(ValidationReason.FIELD_VALIDATION_FAILED)
                && error.requestId().equals("req"),
            "typed error metadata");
      }
    }
    AtomicInteger calls = new AtomicInteger();
    try (var server =
        new Server(
            exchange -> {
              exchange.getResponseHeaders().set("x-request-id", "req");
              response(exchange, 400, "{\"__type\":\"FutureError\",\"requestId\":{}}");
            })) {
      try {
        new WeatherClient(server.config()).ping();
        throw new AssertionError("unknown error ignored");
      } catch (ServiceError error) {
        check(
            error.errorName().equals("FutureError") && error.requestId().equals("req"),
            "unknown error metadata");
      }
    }
    try (var server =
        new Server(
            exchange -> {
              calls.incrementAndGet();
              response(exchange, 200, "{\"cities\":[],\"nextToken\":\"same\"}");
            })) {
      var client = new WeatherClient(server.config());
      var input = new ListCitiesInput();
      var pages = client.listCitiesPages(input);
      pages.next();
      try {
        pages.next();
        throw new AssertionError("cycle accepted");
      } catch (IllegalStateException expected) {
      }
      check(!input.nextToken.present() && calls.get() == 2, "paginator ownership");
    }
  }

  static void malformedAndInterruptedBodies() throws Exception {
    AtomicInteger calls = new AtomicInteger();
    try (var server =
        new Server(
            exchange -> {
              calls.incrementAndGet();
              response(exchange, 200, "invalid JSON");
            })) {
      try {
        new WeatherClient(server.config()).listCities();
        throw new AssertionError("invalid JSON accepted");
      } catch (com.google.gson.JsonParseException expected) {
      }
      check(calls.get() == 1, "JSON failure retried");
    }
    calls.set(0);
    try (var server =
        new Server(
            exchange -> {
              if (calls.incrementAndGet() == 1) {
                exchange.sendResponseHeaders(200, 100);
                exchange.getResponseBody().write('{');
                exchange.close();
              } else response(exchange, 200, "{\"cities\":[]}");
            })) {
      check(
          new WeatherClient(server.config()).listCities().cities.isEmpty() && calls.get() == 2,
          "body interruption retry");
    }
  }

  static void cancellation() throws Exception {
    var entered = new CountDownLatch(1);
    var release = new CountDownLatch(1);
    var calls = new AtomicInteger();
    try (var server =
        new Server(
            exchange -> {
              calls.incrementAndGet();
              response(exchange, 200, "{}");
            })) {
      var config = server.config();
      config.token =
          () -> {
            entered.countDown();
            try {
              release.await();
            } catch (InterruptedException e) {
              Thread.currentThread().interrupt();
            }
            return "token";
          };
      var future = new WeatherClient(config).pingAsync();
      check(entered.await(2, TimeUnit.SECONDS), "provider not entered");
      long start = System.nanoTime();
      future.cancel(true);
      check(
          Duration.ofNanos(System.nanoTime() - start).toMillis() < 100,
          "cancel blocked on provider");
      release.countDown();
      Thread.sleep(100);
      check(calls.get() == 0, "cancelled call sent");
    } finally {
      release.countDown();
    }
    var config = new ClientConfig();
    config.endpoint = "http://example.invalid";
    config.token = () -> "secret";
    try {
      new WeatherClient(config).ping();
      throw new AssertionError("plaintext credentials accepted");
    } catch (IllegalArgumentException expected) {
    }
  }

  static void payloads() throws Exception {
    // Not UTF-8, and JSON-looking in places: neither may be reinterpreted.
    byte[] photo = {(byte) 0xff, (byte) 0xd8, '{', '"', 'a', '"', ':', '1', '}', 0, (byte) 0x80};
    var calls = new AtomicInteger();
    try (var server =
        new Server(
            exchange -> {
              int call = calls.incrementAndGet();
              if (call == 1) {
                byte[] bytes = exchange.getRequestBody().readAllBytes();
                var headers = exchange.getRequestHeaders();
                check(java.util.Arrays.equals(bytes, photo), "payload not sent raw");
                check("image/jpeg".equals(headers.getFirst("content-type")), "media type");
                check("harbour".equals(headers.getFirst("x-photo-caption")), "header member");
                check(
                    HexFormat.of()
                        .formatHex(MessageDigest.getInstance("SHA-256").digest(photo))
                        .equals(headers.getFirst("x-amz-content-sha256")),
                    "digest over the exact bytes");
                response(
                    exchange,
                    200,
                    "{\"photo\":{\"size\":11,\"thumbnail\":\"/9h7ImEiOjF9\",\"caption\":\"harbour\"}}");
              } else if (call == 2) {
                exchange.getResponseHeaders().set("content-type", "image/jpeg");
                exchange.getResponseHeaders().set("x-photo-caption", "harbour");
                exchange.sendResponseHeaders(200, photo.length);
                exchange.getResponseBody().write(photo);
                exchange.close();
              } else {
                exchange.sendResponseHeaders(200, -1);
                exchange.close();
              }
            })) {
      var client = new WeatherClient(server.config());
      var put = new PutCityPhotoInput();
      put.cityId = OptionalValue.of("c-00000001");
      put.caption = OptionalValue.of("harbour");
      put.photo = OptionalValue.of(photo);
      var stored = client.putCityPhoto(put);
      check(
          java.util.Arrays.equals(stored.photo.thumbnail, java.util.Arrays.copyOf(photo, 9)),
          "base64 thumbnail");
      var get = new GetCityPhotoInput();
      get.cityId = OptionalValue.of("c-00000001");
      var read = client.getCityPhoto(get);
      check(java.util.Arrays.equals(read.photo.value(), photo), "payload read raw");
      check("harbour".equals(read.caption.value()), "payload header member");
      var empty = client.getCityPhoto(get);
      check(empty.photo.present() && empty.photo.value().length == 0, "empty payload");
    }
  }

  static void ergonomics() {
    check(CitySummary.builder().id("city").name("Oslo").build().population() == 0L,
        "builder omitted modeled default");
    check(CitySummary.builder().id("city").name("Oslo").population(null).build().population() == null,
        "explicit null was replaced by modeled default");
    var builder = CreateCityInput.builder().name("first").location(null);
    var first = builder.build();
    var second = builder.name("second").build();
    check(first.name().equals("first") && second.name().equals("second"), "builder reused live model");
    check(first.location() == null && first.location.present() && !first.clientToken.present(), "builder presence");
    var list = ListCitiesInput.builder().tags(List.of("native", "list")).build();
    check(list.tags().equals(List.of("native", "list")), "native collection");
    var location = Location.builder().coordinates(Coordinates.builder().latitude(1.0).longitude(2.0).build()).unknown(new Unit()).build();
    check(!location.coordinates.present() && location.unknown() != null, "union builder retained prior arm");
    var unknown = Location.builder().unknown(new Unit()).unknownVariant(new UnknownUnion("future", JsonParser.parseString("{}"))).build();
    check(!unknown.unknown.present() && unknown.unknownVariant.name().equals("future"), "unknown builder retained known arm");
  }

  static void weather() throws Exception {
    String endpoint = System.getenv("WEATHER_URL");
    if (endpoint == null) throw new AssertionError("WEATHER_URL required");
    var config = new ClientConfig();
    config.endpoint = endpoint;
    var client = new WeatherClient(config);
    var location = Location.builder()
        .unknown(new Unit())
        .build();
    var input = CreateCityInput.builder().name("java-http").location(location).build();
    var city = client.createCity(input);
    var get = GetCityInput.builder()
        .cityId(city.city().id())
        .build();
    check(client.getCityAsync(builder -> builder.cityId(get.cityId())).get().city().name().equals("java-http"), "weather get");
    try {
      client.createCity(input);
      throw new AssertionError("missing conflict");
    } catch (ConflictError expected) {
    }
    boolean found = false;
    var list = ListCitiesInput.builder()
        .maxResults(1)
        .build();
    var pages = client.listCitiesPages(list);
    while (pages.hasNext())
      for (var row : pages.next().cities()) found |= row.name().equals("java-http");
    check(found, "weather pagination");
    byte[] photo = "\u00ff{\"looks\": \"like json\"}".getBytes(StandardCharsets.ISO_8859_1);
    var put = PutCityPhotoInput.builder()
        .cityId(city.city().id())
        .caption("aurora")
        .photo(photo)
        .build();
    var stored = client.putCityPhoto(put);
    check(stored.photo().size() == photo.length, "weather payload size");
    check(
        java.util.Arrays.equals(stored.photo().thumbnail(), java.util.Arrays.copyOf(photo, 8)),
        "weather thumbnail");
    var read = GetCityPhotoInput.builder()
        .cityId(city.city().id())
        .build();
    var got = client.getCityPhoto(read);
    check(java.util.Arrays.equals(got.photo(), photo), "weather payload round trip");
    check("aurora".equals(got.caption()), "weather payload caption");
  }

  static void independentDeadlines() throws Exception {
    var callbackEntered = new CountDownLatch(1);
    var releaseCallback = new CountDownLatch(1);
    var releaseProviders = new CountDownLatch(1);
    var config = new ClientConfig();
    config.endpoint = "https://example.invalid";
    config.timeout = Duration.ofMillis(250);
    config.token =
        () -> {
          try {
            releaseProviders.await();
          } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
          }
          return "token";
        };
    try {
      var first = new WeatherClient(config).pingAsync();
      var callback =
          first.whenComplete(
              (value, error) -> {
                callbackEntered.countDown();
                try {
                  check(releaseCallback.await(5, TimeUnit.SECONDS), "callback was not released");
                } catch (InterruptedException e) {
                  Thread.currentThread().interrupt();
                  throw new AssertionError(e);
                }
              });
      check(callbackEntered.await(2, TimeUnit.SECONDS), "first deadline did not complete");
      var second = new WeatherClient(config).pingAsync();
      try {
        second.get(2, TimeUnit.SECONDS);
        throw new AssertionError("independent deadline did not fail");
      } catch (java.util.concurrent.ExecutionException e) {
        check(
            e.getCause() instanceof java.net.http.HttpTimeoutException,
            "wrong independent deadline failure: " + e.getCause());
      }
      check(!callback.isDone(), "blocking callback unexpectedly returned");
    } finally {
      releaseCallback.countDown();
      releaseProviders.countDown();
    }
  }

  static void unitSuccessDoesNotWaitForBody() throws Exception {
    var releaseBody = new CountDownLatch(1);
    var handlerFinished = new CountDownLatch(1);
    var calls = new AtomicInteger();
    try (var server =
        new Server(
            exchange -> {
              try {
                calls.incrementAndGet();
                exchange.sendResponseHeaders(200, 100);
                exchange.getResponseBody().flush();
                // Keep the declared body unavailable until the Unit call has
                // completed. No sleep or socket-write exception is required.
                check(releaseBody.await(5, TimeUnit.SECONDS), "body was not released");
                exchange.close();
              } finally {
                handlerFinished.countDown();
              }
            })) {
      try {
        new WeatherClient(server.config()).pingAsync().get(2, TimeUnit.SECONDS);
        check(releaseBody.getCount() == 1 && calls.get() == 1, "Unit call read the body or retried");
      } finally {
        releaseBody.countDown();
        check(handlerFinished.await(2, TimeUnit.SECONDS), "response handler did not finish");
      }
    }
  }

  static void malformedErrorMetadataAndRetry(Path fixtures) throws Exception {
    for (var entry : JsonParser.parseString(Files.readString(fixtures)).getAsJsonArray()) {
      var test = entry.getAsJsonObject();
      for (int limit : new int[] {1, 2}) {
        var attempts = new AtomicInteger();
        try (var server =
            new Server(
                exchange -> {
                  int count = attempts.incrementAndGet();
                  if (count == 1) {
                    for (var header : test.getAsJsonObject("headers").entrySet())
                      exchange
                          .getResponseHeaders()
                          .set(header.getKey(), header.getValue().getAsString());
                  }
                  if (count == 1)
                    response(exchange, test.get("status").getAsInt(), test.get("body").getAsString());
                  else emptyResponse(exchange);
                })) {
          var config = server.config();
          config.maxAttempts = limit;
          try {
            new WeatherClient(config).ping();
            check(limit == 2, "terminal malformed error must fail");
          } catch (ServiceError error) {
            check(limit == 1, "malformed error prevented retry");
            check(error.errorName().equals(test.get("error").getAsString()), "error name");
            check(error.getMessage().equals(test.get("message").getAsString()), "error message");
            check(error.status() == test.get("status").getAsInt(), "wire status");
            check(error.requestId().equals(test.get("requestId").getAsString()), "request ID");
            check(
                new String(error.body(), StandardCharsets.UTF_8)
                    .equals(test.get("body").getAsString()),
                "raw body");
            var copy = error.body();
            copy[0] = 0;
            check(error.body()[0] != 0, "body getter must return a copy");
            check(error.retryable() == test.get("retryable").getAsBoolean(), "retryable trait");
            check(error.throttling() == test.get("throttling").getAsBoolean(), "throttling trait");
            for (var header : test.getAsJsonObject("headers").entrySet())
              check(
                  error
                      .headers()
                      .firstValue(header.getKey())
                      .orElseThrow()
                      .equals(header.getValue().getAsString()),
                  "header " + header.getKey());
          }
          check(attempts.get() == limit, "malformed error attempt count");
        }
      }
    }
  }

  static void hardening() throws Exception {
    var calls = new AtomicInteger();
    try (var server = new Server(exchange -> {
      calls.incrementAndGet();
      check(exchange.getRequestHeaders().getFirst("authorization") == null, "anonymous credentials");
      check(exchange.getRequestHeaders().getFirst("x-amz-content-sha256").equals(
          HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(new byte[0]))), "empty digest");
      emptyResponse(exchange);
    })) {
      var config = server.config();
      config.token = () -> { throw new AssertionError("Anonymous credential provider invoked"); };
      var client = new WeatherClient(config);
      var runtimeField = WeatherClient.class.getDeclaredField("runtime"); runtimeField.setAccessible(true);
      var runtime = (ClientRuntime) runtimeField.get(client);
      var schemaField = ClientRuntime.class.getDeclaredField("schema"); schemaField.setAccessible(true);
      var schema = (JsonObject) schemaField.get(runtime);
      var op = schema.getAsJsonObject("operations").getAsJsonObject("Ping");
      op.addProperty("bearer", false);
      for (var method : List.of("POST", "PUT", "PATCH")) { op.addProperty("method", method); client.ping(); }
      for (var label : List.of(".", "..")) {
        try { client.getCity(GetCityInput.builder().cityId(label).build()); throw new AssertionError("Dot label accepted"); }
        catch (IllegalArgumentException error) { check(error.getMessage().contains("dot segment"), "wrong label error"); }
      }
      check(calls.get() == 3, "invalid request dispatched");
      try { runtime.decode("example.weather#Tags", JsonParser.parseString("[null]"), null); throw new AssertionError("Dense null accepted"); }
      catch (IllegalArgumentException expected) { }
      try { runtime.encode("example.weather#Tags", java.util.Arrays.asList((String) null), null); throw new AssertionError("Dense null encoded"); }
      catch (IllegalArgumentException expected) { }
      var document = JsonParser.parseString("{\"key\":{\"nil\":null}}");
      check(runtime.encode("example.weather#Forecast", runtime.decode("example.weather#Forecast", document, null), null).equals(document), "document null");
      document = JsonParser.parseString("{\"key\":null}");
      try { runtime.decode("example.weather#Forecast", document, null); throw new AssertionError("Dense document null accepted"); }
      catch (IllegalArgumentException expected) { }
      try { runtime.encode("example.weather#Forecast", java.util.Map.of("key",com.google.gson.JsonNull.INSTANCE), null); throw new AssertionError("Dense document null encoded"); }
      catch (IllegalArgumentException expected) { }
      schema.getAsJsonObject("shapes").getAsJsonObject("example.weather#Forecast").getAsJsonObject("value").addProperty("target", "smithy.api#String");
      try { runtime.decode("example.weather#Forecast", document, null); throw new AssertionError("Dense map null accepted"); }
      catch (IllegalArgumentException expected) { }
      var nullMap = new java.util.HashMap<String, String>(); nullMap.put("key", null);
      try { runtime.encode("example.weather#Forecast", nullMap, null); throw new AssertionError("Dense map null encoded"); }
      catch (IllegalArgumentException expected) { }
    }
  }
  static final class PageSubscriber implements java.util.concurrent.Flow.Subscriber<example.weather.model.ListCitiesOutput> {
    final java.util.concurrent.CompletableFuture<java.util.concurrent.Flow.Subscription> subscription = new java.util.concurrent.CompletableFuture<>();
    final java.util.concurrent.BlockingQueue<example.weather.model.ListCitiesOutput> pages = new java.util.concurrent.LinkedBlockingQueue<>();
    final java.util.concurrent.CompletableFuture<Void> complete = new java.util.concurrent.CompletableFuture<>();
    public void onSubscribe(java.util.concurrent.Flow.Subscription value) { subscription.complete(value); }
    public void onNext(example.weather.model.ListCitiesOutput page) { pages.add(page); }
    public void onError(Throwable error) { complete.completeExceptionally(error); }
    public void onComplete() { complete.complete(null); }
  }

  static void asynchronousPagination() throws Exception {
    var calls = new AtomicInteger();
    try (var server = new Server(exchange -> {
      calls.incrementAndGet();
      boolean last = exchange.getRequestURI().getRawQuery() != null && exchange.getRequestURI().getRawQuery().contains("nextToken=second");
      response(exchange, 200, last ? "{\"cities\":[]}" : "{\"cities\":[],\"nextToken\":\"second\"}");
    })) {
      var client = new WeatherClient(server.config());
      var input = ListCitiesInput.builder().maxResults(3).build();
      var publisher = client.listCitiesPagesAsync(input);
      input.nextToken = OptionalValue.of("changed-after-publisher-creation");
      var first = new PageSubscriber(); publisher.subscribe(first);
      var subscription = first.subscription.get(5, TimeUnit.SECONDS);
      check(calls.get() == 0, "publisher fetched before demand");
      subscription.request(1);
      check(first.pages.poll(5, TimeUnit.SECONDS) != null && calls.get() == 1 && !first.complete.isDone(), "one demand must yield one page");
      subscription.request(1);
      first.complete.get(5, TimeUnit.SECONDS);
      check(first.pages.size() == 1 && calls.get() == 2, "final page and completion");
      var second = new PageSubscriber(); publisher.subscribe(second);
      second.subscription.get(5, TimeUnit.SECONDS).request(Long.MAX_VALUE);
      second.complete.get(5, TimeUnit.SECONDS);
      check(second.pages.size() == 2 && calls.get() == 4, "subscribers shared pagination state");
      var invalid = new PageSubscriber(); client.listCitiesPagesAsync().subscribe(invalid);
      invalid.subscription.get(5, TimeUnit.SECONDS).request(0);
      try { invalid.complete.get(5, TimeUnit.SECONDS); throw new AssertionError("zero demand accepted"); }
      catch (java.util.concurrent.ExecutionException error) { check(error.getCause() instanceof IllegalArgumentException, "wrong demand failure"); }
      check(calls.get() == 4, "invalid demand dispatched");
    }
    try (var server = new Server(exchange -> response(exchange, 200, "{\"cities\":[],\"nextToken\":\"same\"}"))) {
      var repeated = new PageSubscriber(); new WeatherClient(server.config()).listCitiesPagesAsync(b -> b.maxResults(1)).subscribe(repeated);
      repeated.subscription.get(5, TimeUnit.SECONDS).request(3);
      try { repeated.complete.get(5, TimeUnit.SECONDS); throw new AssertionError("repeated cursor accepted"); }
      catch (java.util.concurrent.ExecutionException error) { check(error.getCause() instanceof IllegalStateException, "wrong cursor failure"); }
      check(repeated.pages.size() == 1, "repeated cursor page emitted");
    }
    var entered = new CountDownLatch(1); var release = new CountDownLatch(1); var hookFinished = new CountDownLatch(1);
    var requests = new AtomicInteger();
    try (var server = new Server(exchange -> { requests.incrementAndGet(); response(exchange, 200, "{\"cities\":[]}"); })) {
      var config = server.config();
      config.token = () -> { entered.countDown(); try { release.await(); } catch (InterruptedException error) { Thread.currentThread().interrupt(); } return "token"; };
      config.middleware = List.of(request -> hookFinished.countDown());
      var cancelled = new PageSubscriber(); new WeatherClient(config).listCitiesPagesAsync().subscribe(cancelled);
      var subscription = cancelled.subscription.get(5, TimeUnit.SECONDS); subscription.request(1);
      check(entered.await(5, TimeUnit.SECONDS), "active call did not start");
      subscription.cancel(); release.countDown();
      check(hookFinished.await(5, TimeUnit.SECONDS), "credential hook did not release");
      check(requests.get() == 0 && cancelled.pages.isEmpty() && !cancelled.complete.isDone(), "cancelled publisher dispatched or signaled");
    }
  }

  public static void main(String[] args) throws Exception {
    asynchronousPagination();
    hardening();
    retries(Path.of(args[0]));
    malformedErrorMetadataAndRetry(Path.of(args[0]).resolveSibling("malformed-errors.json"));
    serialization(Path.of(args[1]));
    httpBehavior();
    payloads();
    malformedAndInterruptedBodies();
    cancellation();
    independentDeadlines();
    unitSuccessDoesNotWaitForBody();
    ergonomics();
    weather();
    System.out.println("Java client conformance and weather HTTP passed");
  }
}
