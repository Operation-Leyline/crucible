package example.weather;

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
                response(
                    exchange,
                    count <= test.get("failures").getAsInt() ? test.get("status").getAsInt() : 200,
                    body.toString());
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
          runtime.call("Retry", null, Models.Unit.class).get(5, TimeUnit.SECONDS);
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

  static void serialization(Path headers) throws Exception {
    ClientConfig config = new ClientConfig();
    config.endpoint = "https://example.invalid";
    var runtime = new ClientRuntime(config, "", false, Models.classes());
    var sky = runtime.decode("example.weather#Sky", JsonParser.parseString("\"future\""), null);
    check(sky.toString().equals("future"), "unknown enum");
    var wire = JsonParser.parseString("{\"future\":{\"n\":9223372036854775807}}");
    var union = runtime.decode("example.weather#Location", wire, null);
    check(runtime.encode("example.weather#Location", union, null).equals(wire), "unknown union");
    var unit = new Models.Location();
    unit.unknown = OptionalValue.of(new Models.Unit());
    check(
        runtime
            .encode("example.weather#Location", unit, null)
            .toString()
            .equals("{\"unknown\":{}}"),
        "unit union");
    var city =
        (Models.CitySummary)
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
        (Models.CitySummary)
            runtime.decode(
                "example.weather#CitySummary",
                JsonParser.parseString("{\"id\":\"c\",\"name\":\"x\"}"),
                null);
    check(absent.population == 0, "default missing");
    var input = new Models.CreateCityInput();
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
      var client = new Client(config);
      var input = new Models.CreateCityInput();
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
      var client = new Client(server.config());
      var input = new Models.GetCityInput();
      input.cityId = OptionalValue.of("a/b c~é*");
      var output = client.getCity(input);
      check(
          output.requestId.value().equals("req-1")
              && output.forecast.value().containsKey(new Models.Sky("future")),
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
      var client = new Client(server.config());
      try {
        client.ping();
        throw new AssertionError("error ignored");
      } catch (Models.ValidationError error) {
        check(
            error.reason.equals(Models.ValidationReason.FIELD_VALIDATION_FAILED)
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
        new Client(server.config()).ping();
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
      var client = new Client(server.config());
      var input = new Models.ListCitiesInput();
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
        new Client(server.config()).listCities(null);
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
          new Client(server.config()).listCities(null).cities.isEmpty() && calls.get() == 2,
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
      var future = new Client(config).pingAsync();
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
      new Client(config).ping();
      throw new AssertionError("plaintext credentials accepted");
    } catch (IllegalArgumentException expected) {
    }
  }

  static void weather() throws Exception {
    String endpoint = System.getenv("WEATHER_URL");
    if (endpoint == null) throw new AssertionError("WEATHER_URL required");
    var config = new ClientConfig();
    config.endpoint = endpoint;
    var client = new Client(config);
    var input = new Models.CreateCityInput();
    input.name = OptionalValue.of("java-http");
    var location = new Models.Location();
    location.unknown = OptionalValue.of(new Models.Unit());
    input.location = OptionalValue.of(location);
    var city = client.createCity(input);
    var get = new Models.GetCityInput();
    get.cityId = OptionalValue.of(city.city.id);
    check(client.getCityAsync(get).get().city.name.equals("java-http"), "weather get");
    try {
      client.createCity(input);
      throw new AssertionError("missing conflict");
    } catch (Models.ConflictError expected) {
    }
    boolean found = false;
    var list = new Models.ListCitiesInput();
    list.maxResults = OptionalValue.of(1);
    var pages = client.listCitiesPages(list);
    while (pages.hasNext())
      for (var row : pages.next().cities) found |= row.name.equals("java-http");
    check(found, "weather pagination");
  }

  public static void main(String[] args) throws Exception {
    retries(Path.of(args[0]));
    serialization(Path.of(args[1]));
    httpBehavior();
    malformedAndInterruptedBodies();
    cancellation();
    weather();
    System.out.println("Java client conformance and weather HTTP passed");
  }
}
