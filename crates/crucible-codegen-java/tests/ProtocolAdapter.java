package example.xml;

import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.google.gson.JsonPrimitive;
import java.io.InputStream;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.math.BigDecimal;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpHeaders;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.util.ArrayList;
import java.util.Base64;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.concurrent.ExecutionException;
import javax.net.ssl.SSLSession;

/**
 * Native adapter for tools/protocol_conformance.py: one Smithy HTTP vector on stdin, one
 * snapshot on stdout. Requests are captured by middleware after the runtime prepared them;
 * responses go through the runtime's own response decoding.
 */
public final class ProtocolAdapter {
  private static final class Captured extends RuntimeException {
    Captured() { super(null, null, false, false); }
  }

  private static JsonObject schema;
  private static ClientRuntime runtime;

  public static void main(String[] args) throws Exception {
    JsonObject vector = JsonParser.parseString(new String(System.in.readAllBytes(), StandardCharsets.UTF_8)).getAsJsonObject();
    try (InputStream input = ClientRuntime.class.getResourceAsStream("schema.json")) {
      schema = JsonParser.parseString(new String(input.readAllBytes(), StandardCharsets.UTF_8)).getAsJsonObject();
    }
    JsonObject snapshot;
    if ((vector.has("vendorParams") && !vector.get("vendorParams").isJsonNull())
        || (vector.has("authScheme") && !vector.get("authScheme").isJsonNull())) {
      snapshot = new JsonObject();
      snapshot.addProperty("unsupported", "Java adapter needs vendor/auth fixture configuration");
    } else snapshot = run(vector);
    System.out.print(new GsonBuilder().disableHtmlEscaping().serializeNulls().create().toJson(snapshot));
  }

  private static String text(JsonObject object, String key) {
    JsonElement value = object.get(key);
    return value == null || value.isJsonNull() ? null : value.getAsString();
  }

  private static JsonObject shape(String target) {
    return schema.getAsJsonObject("shapes").getAsJsonObject(target);
  }

  private static JsonObject run(JsonObject vector) throws Exception {
    JsonObject testCase = vector.getAsJsonObject("case");
    String operation = vector.get("operation").getAsString();
    operation = operation.substring(operation.indexOf('#') + 1);
    JsonObject op = schema.getAsJsonObject("operations").getAsJsonObject(operation);
    String target = text(vector, "shape");
    var config = new ClientConfig();
    String host = text(testCase, "host");
    config.endpoint = "https://" + (host == null ? "example.test" : host);
    config.maxAttempts = 1;
    JsonObject snapshot = new JsonObject();
    config.middleware.add(request -> {
      snapshot.addProperty("method", request.method);
      snapshot.addProperty("uri", request.uri.getRawPath());
      snapshot.addProperty("host", request.uri.getHost());
      var query = new JsonArray();
      if (request.uri.getRawQuery() != null) for (String pair : request.uri.getRawQuery().split("&")) query.add(pair);
      snapshot.add("queryParams", query);
      var headers = new JsonObject();
      request.headers.forEach(headers::addProperty);
      snapshot.add("headers", headers);
      snapshot.addProperty("body", request.body == null ? "" : new String(request.body, StandardCharsets.UTF_8));
      throw new Captured();
    });
    runtime = new ClientRuntime(config, "", false, ModelRegistry.classes());
    if ("request".equals(text(testCase, "kind"))) {
      Object input = target == null ? null : runtime.decode(target, wire(target, vector.get("params"), null), null);
      try {
        runtime.call(operation, input, Object.class).get();
      } catch (ExecutionException error) {
        if (error.getCause() instanceof Captured) return snapshot;
        throw error;
      }
      throw new AssertionError("native client did not send a request");
    }
    JsonObject message = testCase.getAsJsonObject("message");
    Map<String, List<String>> headers = new LinkedHashMap<>();
    for (var entry : message.getAsJsonObject("headers").entrySet()) headers.put(entry.getKey(), List.of(entry.getValue().getAsString()));
    byte[] body = text(message, "body") == null ? new byte[0] : text(message, "body").getBytes(StandardCharsets.UTF_8);
    var response = new FakeResponse(testCase.get("code").getAsInt(), HttpHeaders.of(headers, (a, b) -> true), body);
    Method decode = ClientRuntime.class.getDeclaredMethod("response", String.class, JsonObject.class, HttpResponse.class, byte[].class);
    decode.setAccessible(true);
    JsonObject result = new JsonObject();
    try {
      JsonObject values = (JsonObject) decode.invoke(runtime, operation, op, response, body);
      String output = text(op, "output");
      result.add("params", output == null ? new JsonObject() : params(output, runtime.encode(output, runtime.decode(output, values, null), null), null));
      result.addProperty("error", false);
    } catch (InvocationTargetException failure) {
      if (!(failure.getCause() instanceof ServiceError error)) throw failure;
      boolean modeled = error.getClass() != ServiceError.class;
      result.add("params", modeled && target != null ? params(target, runtime.encode(target, error, null), null) : JsonNull.INSTANCE);
      result.addProperty("error", true);
    }
    return result;
  }

  /** Smithy test parameters to the runtime's wire values: epoch timestamps and text blobs. */
  private static JsonElement wire(String target, JsonElement node, String format) {
    if (node == null || node.isJsonNull()) return node;
    JsonObject shape = shape(target);
    switch (text(shape, "kind")) {
      case "structure", "union": {
        JsonObject result = new JsonObject();
        for (var entry : node.getAsJsonObject().entrySet()) {
          JsonObject member = shape.getAsJsonObject("members").getAsJsonObject(entry.getKey());
          if (member == null) { result.add(entry.getKey(), entry.getValue()); continue; }
          String name = text(member, "jsonName") == null ? entry.getKey() : text(member, "jsonName");
          result.add(name, wire(text(member, "target"), entry.getValue(), text(member, "format")));
        }
        return result;
      }
      case "list": {
        JsonArray result = new JsonArray();
        JsonObject member = shape.getAsJsonObject("member");
        for (JsonElement item : node.getAsJsonArray()) result.add(wire(text(member, "target"), item, text(member, "format")));
        return result;
      }
      case "map": {
        JsonObject result = new JsonObject();
        JsonObject member = shape.getAsJsonObject("value");
        for (var entry : node.getAsJsonObject().entrySet()) result.add(entry.getKey(), wire(text(member, "target"), entry.getValue(), text(member, "format")));
        return result;
      }
      case "timestamp":
        return runtime.encode(target, runtime.decode(target, node, "epoch-seconds"), format);
      case "blob":
        return new JsonPrimitive(Base64.getEncoder().encodeToString(node.getAsString().getBytes(StandardCharsets.UTF_8)));
      default:
        return node;
    }
  }

  /** The inverse of {@link #wire}, leaving out members that are absent. */
  private static JsonElement params(String target, JsonElement value, String format) {
    if (value == null || value.isJsonNull()) return JsonNull.INSTANCE;
    JsonObject shape = shape(target);
    switch (text(shape, "kind")) {
      case "structure", "union": {
        JsonObject result = new JsonObject();
        for (var entry : shape.getAsJsonObject("members").entrySet()) {
          JsonObject member = entry.getValue().getAsJsonObject();
          String name = text(member, "jsonName") == null ? entry.getKey() : text(member, "jsonName");
          JsonElement item = value.getAsJsonObject().get(name);
          if (item != null && !item.isJsonNull()) result.add(entry.getKey(), params(text(member, "target"), item, text(member, "format")));
        }
        return result;
      }
      case "list": {
        JsonArray result = new JsonArray();
        JsonObject member = shape.getAsJsonObject("member");
        for (JsonElement item : value.getAsJsonArray()) result.add(params(text(member, "target"), item, text(member, "format")));
        return result;
      }
      case "map": {
        JsonObject result = new JsonObject();
        JsonObject member = shape.getAsJsonObject("value");
        for (var entry : value.getAsJsonObject().entrySet()) result.add(entry.getKey(), params(text(member, "target"), entry.getValue(), text(member, "format")));
        return result;
      }
      case "timestamp": {
        Instant instant = (Instant) runtime.decode(target, value, format);
        return new JsonPrimitive(BigDecimal.valueOf(instant.getEpochSecond()).add(BigDecimal.valueOf(instant.getNano(), 9)).stripTrailingZeros());
      }
      case "blob":
        return new JsonPrimitive(new String(Base64.getDecoder().decode(value.getAsString()), StandardCharsets.UTF_8));
      default:
        return value;
    }
  }

  private record FakeResponse(int statusCode, HttpHeaders headers, byte[] body) implements HttpResponse<byte[]> {
    public HttpRequest request() { return HttpRequest.newBuilder(URI.create("https://example.test")).build(); }
    public Optional<HttpResponse<byte[]>> previousResponse() { return Optional.empty(); }
    public Optional<SSLSession> sslSession() { return Optional.empty(); }
    public URI uri() { return URI.create("https://example.test"); }
    public HttpClient.Version version() { return HttpClient.Version.HTTP_1_1; }
  }
}
