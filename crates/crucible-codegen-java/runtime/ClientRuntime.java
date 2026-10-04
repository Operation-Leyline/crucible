import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import com.google.gson.JsonPrimitive;
import com.google.gson.Strictness;
import java.io.IOException;
import java.io.InputStream;
import java.lang.reflect.Field;
import java.math.BigDecimal;
import java.net.URI;
import java.net.URLEncoder;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.net.http.HttpTimeoutException;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.time.Duration;
import java.time.Instant;
import java.time.ZoneOffset;
import java.time.ZonedDateTime;
import java.time.format.DateTimeFormatter;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.HexFormat;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.NoSuchElementException;
import java.util.Set;
import java.util.UUID;
import java.util.concurrent.CancellationException;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CompletionException;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.ScheduledThreadPoolExecutor;
import java.util.concurrent.ThreadLocalRandom;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;

final class ClientRuntime {
  private static final Gson JSON =
      new GsonBuilder()
          .disableHtmlEscaping()
          .serializeNulls()
          .setStrictness(Strictness.STRICT)
          .create();
  private static final ScheduledThreadPoolExecutor TIMER = timer();
  // User hooks and completion callbacks may block, so keep them off the timer and common pool.
  private static final ExecutorService WORKERS =
      Executors.newCachedThreadPool(
          task -> {
            var thread = new Thread(task, "crucible-client-worker");
            thread.setDaemon(true);
            return thread;
          });
  private final HttpClient http;
  private final ClientConfig config;
  private final JsonObject schema;
  private final Map<String, Class<?>> classes;
  private final boolean digest;

  private static ScheduledThreadPoolExecutor timer() {
    var executor =
        new ScheduledThreadPoolExecutor(
            1,
            task -> {
              var thread = new Thread(task, "crucible-deadlines");
              thread.setDaemon(true);
              return thread;
            });
    executor.setRemoveOnCancelPolicy(true);
    return executor;
  }

  ClientRuntime(
      ClientConfig input, String endpoint, boolean digest, Map<String, Class<?>> classes) {
    this(input, endpoint, digest, classes, loadSchema());
  }

  ClientRuntime(
      ClientConfig input,
      String endpoint,
      boolean digest,
      Map<String, Class<?>> classes,
      JsonObject schema) {
    config = new ClientConfig();
    config.endpoint = input.endpoint == null ? endpoint : input.endpoint;
    URI uri = URI.create(config.endpoint);
    if (!("http".equals(uri.getScheme()) || "https".equals(uri.getScheme()))
        || uri.getHost() == null
        || uri.getUserInfo() != null
        || uri.getQuery() != null
        || uri.getFragment() != null)
      throw new IllegalArgumentException(
          "absolute HTTP(S) endpoint without credentials, query, or fragment required");
    config.endpoint = config.endpoint.replaceAll("/+$", "");
    if (input.maxAttempts < 1
        || input.timeout.isNegative()
        || input.timeout.isZero()
        || input.baseDelay.isNegative()
        || input.maxDelay.isNegative())
      throw new IllegalArgumentException("invalid attempt limit, timeout, or retry delay");
    config.maxAttempts = input.maxAttempts;
    config.timeout = input.timeout;
    config.baseDelay = input.baseDelay;
    config.maxDelay = input.maxDelay;
    config.timeout.toNanos();
    config.baseDelay.toNanos();
    config.maxDelay.toNanos();
    config.token = input.token;
    config.headers = new LinkedHashMap<>(input.headers);
    config.middleware = List.copyOf(input.middleware);
    this.http =
        input.httpClient == null
            ? HttpClient.newBuilder().followRedirects(HttpClient.Redirect.NEVER).build()
            : input.httpClient;
    this.schema = schema;
    this.classes = classes;
    this.digest = digest;
  }

  private static JsonObject loadSchema() {
    try (InputStream input = ClientRuntime.class.getResourceAsStream("schema.json")) {
      if (input == null) throw new IllegalStateException("SDK wire schema resource missing");
      return JSON.fromJson(
          new String(input.readAllBytes(), StandardCharsets.UTF_8), JsonObject.class);
    } catch (IOException e) {
      throw new IllegalStateException(e);
    }
  }

  static String text(JsonObject object, String key) {
    JsonElement value = object.get(key);
    return value == null || value.isJsonNull() ? null : value.getAsString();
  }

  private JsonObject shape(String target) {
    return schema.getAsJsonObject("shapes").getAsJsonObject(target);
  }

  private static boolean flag(JsonObject object, String key) {
    return object.has(key) && object.get(key).getAsBoolean();
  }

  private static Object construct(Class<?> type) {
    try {
      return type.getConstructor().newInstance();
    } catch (ReflectiveOperationException e) {
      throw new IllegalStateException(e);
    }
  }

  private Object enumValue(String target, String value) {
    try {
      return classes.get(target).getConstructor(String.class).newInstance(value);
    } catch (ReflectiveOperationException e) {
      throw new IllegalStateException(e);
    }
  }

  JsonElement encode(String target, Object value, String format) {
    if (target == null) return new JsonObject();
    JsonObject shape = shape(target);
    String kind = text(shape, "kind");
    if ("unit".equals(kind)) return new JsonObject();
    if (value == null) return JsonNull.INSTANCE;
    switch (kind) {
      case "timestamp":
        return writeTime((Instant) value, format == null ? text(shape, "format") : format);
      case "enum":
        return new JsonPrimitive(value.toString());
      case "list":
        {
          var result = new JsonArray();
          var member = shape.getAsJsonObject("member");
          for (Object item : (List<?>) value)
            result.add(encode(text(member, "target"), item, text(member, "format")));
          return result;
        }
      case "map":
        {
          var result = new JsonObject();
          var member = shape.getAsJsonObject("value");
          for (var entry : ((Map<?, ?>) value).entrySet())
            result.add(
                entry.getKey().toString(),
                encode(text(member, "target"), entry.getValue(), text(member, "format")));
          return result;
        }
      case "structure":
      case "union":
        {
          var result = new JsonObject();
          try {
            for (var entry : shape.getAsJsonObject("members").entrySet()) {
              var member = entry.getValue().getAsJsonObject();
              Object item = value.getClass().getField(text(member, "attribute")).get(value);
              if (item instanceof OptionalValue<?> optional) {
                if (!optional.present()) continue;
                item = optional.value();
              }
              result.add(
                  entry.getKey(), encode(text(member, "target"), item, text(member, "format")));
            }
            if ("union".equals(kind)) {
              var unknown = (UnknownUnion) value.getClass().getField("unknownVariant").get(value);
              if (unknown != null) {
                if (shape.getAsJsonObject("members").has(unknown.name()))
                  throw new IllegalArgumentException("unknown union name is declared");
                result.add(unknown.name(), unknown.value().deepCopy());
              }
              if (result.size() != 1)
                throw new IllegalArgumentException("a union must contain exactly one member");
            }
          } catch (ReflectiveOperationException e) {
            throw new IllegalStateException(e);
          }
          return result;
        }
      default:
        return JSON.toJsonTree(value);
    }
  }

  @SuppressWarnings("unchecked")
  Object decode(String target, JsonElement value, String format) {
    if (target == null) return new Models.Unit();
    if (value == null || value.isJsonNull()) return null;
    JsonObject shape = shape(target);
    String kind = text(shape, "kind");
    switch (kind) {
      case "unit":
        if (!value.isJsonObject() || value.getAsJsonObject().size() != 0)
          throw new IllegalArgumentException("unit requires empty object");
        return new Models.Unit();
      case "timestamp":
        return readTime(value, format == null ? text(shape, "format") : format);
      case "string":
        if (!value.isJsonPrimitive() || !value.getAsJsonPrimitive().isString())
          throw new IllegalArgumentException("expected string");
        return value.getAsString();
      case "enum":
        if (!value.isJsonPrimitive() || !value.getAsJsonPrimitive().isString())
          throw new IllegalArgumentException("expected enum string");
        return enumValue(target, value.getAsString());
      case "boolean":
        if (!value.isJsonPrimitive() || !value.getAsJsonPrimitive().isBoolean())
          throw new IllegalArgumentException("expected boolean");
        return value.getAsBoolean();
      case "byte":
        return number(value).byteValueExact();
      case "short":
        return number(value).shortValueExact();
      case "integer":
        return number(value).intValueExact();
      case "long":
        return number(value).longValueExact();
      case "float":
        {
          float n = number(value).floatValue();
          if (!Float.isFinite(n)) throw new IllegalArgumentException("float overflow");
          return n;
        }
      case "double":
        {
          double n = number(value).doubleValue();
          if (!Double.isFinite(n)) throw new IllegalArgumentException("double overflow");
          return n;
        }
      case "document":
        return value.deepCopy();
      case "list":
        {
          var result = (List<Object>) construct(classes.get(target));
          var member = shape.getAsJsonObject("member");
          for (JsonElement item : value.getAsJsonArray())
            result.add(decode(text(member, "target"), item, text(member, "format")));
          return result;
        }
      case "map":
        {
          var result = (Map<Object, Object>) construct(classes.get(target));
          var member = shape.getAsJsonObject("value");
          var key = shape.getAsJsonObject("key");
          for (var entry : value.getAsJsonObject().entrySet())
            result.put(
                decode(text(key, "target"), new JsonPrimitive(entry.getKey()), null),
                decode(text(member, "target"), entry.getValue(), text(member, "format")));
          return result;
        }
      case "structure":
      case "union":
        {
          Object result = construct(classes.get(target));
          JsonObject object = value.getAsJsonObject();
          var members = shape.getAsJsonObject("members");
          try {
            if ("union".equals(kind)) {
              if (object.size() != 1)
                throw new IllegalArgumentException("a union must contain exactly one member");
              var entry = object.entrySet().iterator().next();
              if (!members.has(entry.getKey())) {
                result
                    .getClass()
                    .getField("unknownVariant")
                    .set(result, new UnknownUnion(entry.getKey(), entry.getValue().deepCopy()));
                return result;
              }
            }
            for (var entry : members.entrySet()) {
              var member = entry.getValue().getAsJsonObject();
              JsonElement item = object.get(entry.getKey());
              if (item == null && !member.get("default").isJsonNull()) item = member.get("default");
              if (item == null) continue;
              Object decoded = decode(text(member, "target"), item, text(member, "format"));
              Field field = result.getClass().getField(text(member, "attribute"));
              field.set(
                  result,
                  field.getType() == OptionalValue.class ? OptionalValue.of(decoded) : decoded);
            }
          } catch (ReflectiveOperationException e) {
            throw new IllegalStateException(e);
          }
          return result;
        }
      default:
        throw new IllegalArgumentException("unknown wire kind " + kind);
    }
  }

  private static BigDecimal number(JsonElement value) {
    if (!value.isJsonPrimitive() || !value.getAsJsonPrimitive().isNumber())
      throw new IllegalArgumentException("expected number");
    return value.getAsBigDecimal();
  }

  private static JsonElement writeTime(Instant value, String format) {
    if ("epoch-seconds".equals(format))
      return new JsonPrimitive(
          BigDecimal.valueOf(value.getEpochSecond()).add(BigDecimal.valueOf(value.getNano(), 9)));
    if ("http-date".equals(format))
      return new JsonPrimitive(
          DateTimeFormatter.ofPattern("EEE, dd MMM uuuu HH:mm:ss 'GMT'", java.util.Locale.US)
              .format(value.atZone(ZoneOffset.UTC)));
    return new JsonPrimitive(value.toString());
  }

  private static Instant readTime(JsonElement value, String format) {
    if ("epoch-seconds".equals(format)) {
      BigDecimal n = number(value);
      return Instant.ofEpochSecond(
          n.toBigInteger().longValueExact(),
          n.remainder(BigDecimal.ONE).movePointRight(9).longValueExact());
    }
    if ("http-date".equals(format))
      return ZonedDateTime.parse(value.getAsString(), DateTimeFormatter.RFC_1123_DATE_TIME)
          .toInstant();
    return Instant.parse(value.getAsString());
  }

  private static String bound(JsonElement value) {
    if (value.isJsonArray()) {
      List<String> values = new ArrayList<>();
      for (JsonElement item : value.getAsJsonArray()) {
        String text = bound(item);
        values.add(
            text.contains(",")
                    || text.contains("\"")
                    || text.contains("\\")
                    || !text.strip().equals(text)
                ? JSON.toJson(text)
                : text);
      }
      return String.join(", ", values);
    }
    return value.getAsString();
  }

  JsonElement header(JsonObject member, String value) {
    JsonObject shape = shape(text(member, "target"));
    switch (text(shape, "kind")) {
      case "list":
        {
          JsonArray result = new JsonArray();
          if (value.isBlank()) return result;
          List<String> parts = new ArrayList<>();
          int start = 0;
          boolean quoted = false, escaped = false;
          for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            if (escaped) escaped = false;
            else if (c == '\\' && quoted) escaped = true;
            else if (c == '"') quoted = !quoted;
            else if (c == ',' && !quoted) {
              parts.add(value.substring(start, i).strip());
              start = i + 1;
            }
          }
          if (quoted) throw new IllegalArgumentException("unterminated quoted header");
          parts.add(value.substring(start).strip());
          for (String part : parts)
            result.add(
                header(
                    shape.getAsJsonObject("member"),
                    part.startsWith("\"") ? JSON.fromJson(part, String.class) : part));
          return result;
        }
      case "byte":
      case "short":
      case "integer":
      case "long":
      case "float":
      case "double":
      case "boolean":
        return JSON.fromJson(value, JsonElement.class);
      case "timestamp":
        if ("epoch-seconds".equals(text(member, "format")))
          return JSON.fromJson(value, JsonElement.class);
        return new JsonPrimitive(value);
      default:
        return new JsonPrimitive(value);
    }
  }

  private static String escape(String text) {
    return URLEncoder.encode(text, StandardCharsets.UTF_8)
        .replace("+", "%20")
        .replace("%7E", "~")
        .replace("*", "%2A");
  }

  private record Prepared(
      JsonObject operation, URI uri, Map<String, String> headers, byte[] body) {}

  private Prepared prepare(String name, JsonObject values) {
    JsonObject op = schema.getAsJsonObject("operations").getAsJsonObject(name),
        request = op.getAsJsonObject("request");
    String token = text(op, "token");
    if (token != null && (!values.has(token) || values.get(token).isJsonNull()))
      values.addProperty(token, UUID.randomUUID().toString());
    String uri = text(op, "uri");
    for (JsonElement label : request.getAsJsonArray("labels")) {
      String key = label.getAsString();
      JsonElement value = values.get(key);
      if (value == null || value.isJsonNull() || bound(value).isEmpty())
        throw new IllegalArgumentException(key + " is required");
      uri = uri.replace("{" + key + "}", escape(bound(value)));
    }
    List<String> query = new ArrayList<>();
    for (JsonElement item : request.getAsJsonArray("query")) {
      var binding = item.getAsJsonArray();
      JsonElement value = values.get(binding.get(0).getAsString());
      if (value == null || value.isJsonNull()) continue;
      var list = new JsonArray();
      if (value.isJsonArray()) list = value.getAsJsonArray();
      else list.add(value);
      for (var v : list) query.add(escape(binding.get(1).getAsString()) + "=" + escape(bound(v)));
    }
    if (!query.isEmpty()) uri += (uri.contains("?") ? "&" : "?") + String.join("&", query);
    Map<String, String> headers = new LinkedHashMap<>(config.headers);
    for (JsonElement item : request.getAsJsonArray("headers")) {
      var binding = item.getAsJsonArray();
      JsonElement value = values.get(binding.get(0).getAsString());
      if (value != null && !value.isJsonNull())
        headers.put(binding.get(1).getAsString(), bound(value));
    }
    byte[] body = null;
    if (!request.getAsJsonArray("body").isEmpty()) {
      JsonObject object = new JsonObject();
      for (var member : request.getAsJsonArray("body")) {
        String key = member.getAsString();
        if (values.has(key)) object.add(key, values.get(key));
      }
      body = JSON.toJson(object).getBytes(StandardCharsets.UTF_8);
      headers.put("content-type", "application/json");
    }
    return new Prepared(op, URI.create(config.endpoint + uri), headers, body);
  }

  <T> CompletableFuture<T> call(String name, Object input, Class<T> output) {
    try {
      JsonObject op = schema.getAsJsonObject("operations").getAsJsonObject(name);
      return callValues(
          name,
          input == null
              ? new JsonObject()
              : encode(text(op, "input"), input, null).getAsJsonObject(),
          output);
    } catch (RuntimeException e) {
      return CompletableFuture.failedFuture(e);
    }
  }

  private static final class Call<T> extends CompletableFuture<T> {
    final AtomicReference<Future<?>> active = new AtomicReference<>();
    Future<?> retry;

    void track(Future<?> task) {
      active.set(task);
      if (isDone()) task.cancel(true);
    }

    synchronized void schedule(Runnable action, long delay) {
      if (isDone()) return;
      retry =
          TIMER.schedule(
              () -> {
                synchronized (this) {
                  if (isDone()) return;
                }
                WORKERS.execute(action);
              },
              delay,
              TimeUnit.NANOSECONDS);
    }

    synchronized void stop() {
      if (retry != null) retry.cancel(false);
      Future<?> task = active.get();
      if (task != null) task.cancel(true);
    }
  }

  private <T> CompletableFuture<T> callValues(String name, JsonObject values, Class<T> output) {
    Prepared prepared = prepare(name, values);
    var result = new Call<T>();
    long start = System.nanoTime();
    Future<?> deadline =
        TIMER.schedule(
            () ->
                WORKERS.execute(
                    () ->
                        result.completeExceptionally(
                            new HttpTimeoutException("request deadline exceeded"))),
            config.timeout.toNanos(),
            TimeUnit.NANOSECONDS);
    result.whenComplete(
        (value, error) -> {
          deadline.cancel(false);
          result.stop();
        });
    WORKERS.execute(() -> attempt(prepared, output, result, 1, start));
    return result;
  }

  private <T> void attempt(
      Prepared prepared, Class<T> output, Call<T> result, int attempt, long start) {
    if (result.isDone()) return;
    try {
      long remaining = config.timeout.toNanos() - (System.nanoTime() - start);
      if (remaining <= 0) throw new HttpTimeoutException("request deadline exceeded");
      Map<String, String> headers = new LinkedHashMap<>(prepared.headers());
      if (config.token != null && flag(schema, "bearer")) {
        String token = config.token.get();
        if (token != null) headers.put("authorization", "Bearer " + token);
      }
      var request =
          new ClientConfig.Request(
              prepared.uri(),
              text(prepared.operation(), "method"),
              headers,
              prepared.body() == null ? null : prepared.body().clone());
      for (var middleware : config.middleware) middleware.apply(request);
      boolean credentials =
          headers.keySet().stream().anyMatch(k -> k.equalsIgnoreCase("authorization"));
      String host = request.uri.getHost();
      if (credentials
          && !"https".equals(request.uri.getScheme())
          && !Set.of("localhost", "127.0.0.1", "[::1]", "::1").contains(host))
        throw new IllegalArgumentException(
            "refusing credentials over plaintext HTTP outside loopback");
      if (digest && request.body != null && request.body.length > 0)
        headers.put(
            "x-amz-content-sha256",
            HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(request.body)));
      var builder = HttpRequest.newBuilder(request.uri).timeout(Duration.ofNanos(remaining));
      headers.forEach(builder::setHeader);
      builder.method(
          request.method,
          request.body == null
              ? HttpRequest.BodyPublishers.noBody()
              : HttpRequest.BodyPublishers.ofByteArray(request.body));
      boolean hasBody =
          !prepared.operation().getAsJsonObject("response").getAsJsonArray("body").isEmpty();
      if (result.isDone()) return;
      CompletableFuture<HttpResponse<byte[]>> sent =
          http.sendAsync(
              builder.build(),
              info ->
                  info.statusCode() >= 200 && info.statusCode() < 300 && !hasBody
                      ? emptyBody()
                      : HttpResponse.BodySubscribers.ofByteArray());
      result.track(sent);
      sent.whenComplete(
          (response, error) -> {
            if (result.isDone()) return;
            Throwable failure = unwrap(error);
            if (failure == null) {
              try {
                JsonObject decoded = response(prepared.operation(), response);
                result.complete(
                    output.cast(decode(text(prepared.operation(), "output"), decoded, null)));
                return;
              } catch (RuntimeException e) {
                failure = e;
              }
            }
            boolean safe = flag(prepared.operation(), "safe");
            boolean retry;
            if (failure instanceof ServiceError modeled) retry = canRetry(safe, modeled);
            else if (failure instanceof IOException) {
              retry = safe;
              failure = new NetworkError(failure);
            } else retry = false;
            if (!retry || attempt >= config.maxAttempts) {
              result.completeExceptionally(failure);
              return;
            }
            long delay = delay(attempt, response);
            result.schedule(() -> attempt(prepared, output, result, attempt + 1, start), delay);
          });
    } catch (Exception e) {
      result.completeExceptionally(e);
    }
  }

  private static HttpResponse.BodySubscriber<byte[]> emptyBody() {
    return new HttpResponse.BodySubscriber<>() {
      public java.util.concurrent.CompletionStage<byte[]> getBody() {
        return CompletableFuture.completedFuture(new byte[0]);
      }

      public void onSubscribe(java.util.concurrent.Flow.Subscription subscription) {
        subscription.cancel();
      }

      public void onNext(List<java.nio.ByteBuffer> ignored) {}

      public void onError(Throwable ignored) {}

      public void onComplete() {}
    };
  }

  static boolean canRetry(boolean safe, ServiceError error) {
    return error.throttling()
        || safe && (error.retryable() || Set.of(500, 502, 503, 504).contains(error.status()));
  }

  private long delay(int attempt, HttpResponse<?> response) {
    if (response != null) {
      String text = response.headers().firstValue("retry-after").orElse("");
      try {
        BigDecimal seconds = new BigDecimal(text);
        if (seconds.signum() >= 0)
          return seconds.movePointRight(9).min(BigDecimal.valueOf(Long.MAX_VALUE)).longValue();
      } catch (NumberFormatException ignored) {
      }
      try {
        long nanos =
            Duration.between(
                    Instant.now(),
                    ZonedDateTime.parse(text, DateTimeFormatter.RFC_1123_DATE_TIME).toInstant())
                .toNanos();
        if (nanos >= 0) return nanos;
      } catch (RuntimeException ignored) {
      }
    }
    double ceiling =
        Math.min(
            config.maxDelay.toNanos(),
            config.baseDelay.toNanos() * Math.pow(2, Math.min(attempt - 1, 30)));
    return (long) (ThreadLocalRandom.current().nextDouble() * ceiling);
  }

  private JsonObject response(JsonObject op, HttpResponse<byte[]> response) {
    int status = response.statusCode();
    JsonObject values = new JsonObject();
    String body = new String(response.body(), StandardCharsets.UTF_8);
    if (status >= 200 && status < 300) {
      if (!body.isEmpty()) values = JSON.fromJson(body, JsonObject.class);
      if (values == null) throw new IllegalArgumentException("response requires object");
      applyHeaders(
          values,
          text(op, "output"),
          op.getAsJsonObject("response").getAsJsonArray("headers"),
          response);
      return values;
    }
    try {
      JsonElement parsed = JSON.fromJson(body, JsonElement.class);
      if (parsed != null && parsed.isJsonObject()) values = parsed.getAsJsonObject();
    } catch (RuntimeException ignored) {
    }
    JsonElement named = values.get("__type");
    boolean explicit =
        named != null && named.isJsonPrimitive() && named.getAsJsonPrimitive().isString();
    String name = explicit ? named.getAsString() : null, target = null;
    List<String> matches = new ArrayList<>();
    for (JsonElement error : op.getAsJsonArray("errors")) {
      String id = error.getAsString();
      JsonObject shape = shape(id);
      if (explicit && text(shape, "name").equals(name)) target = id;
      else if (!explicit && shape.getAsJsonObject("error").get("status").getAsInt() == status)
        matches.add(id);
    }
    if (!explicit && matches.size() == 1) {
      target = matches.get(0);
      name = text(shape(target), "name");
    }
    JsonObject traits = target == null ? new JsonObject() : shape(target).getAsJsonObject("error");
    if (target != null) applyHeaders(values, target, traits.getAsJsonArray("headers"), response);
    ServiceError error =
        target == null ? new ServiceError() : (ServiceError) decode(target, values, null);
    String message =
        values.has("message") && values.get("message").isJsonPrimitive()
            ? values.get("message").getAsString()
            : "HTTP " + status;
    JsonElement idValue = values.get("requestId");
    String fallback =
        idValue != null && idValue.isJsonPrimitive() && idValue.getAsJsonPrimitive().isString()
            ? idValue.getAsString()
            : null;
    String requestId = response.headers().firstValue("x-request-id").orElse(fallback);
    error.initialize(
        name == null ? "UnknownError" : name,
        message,
        status,
        requestId,
        response.headers(),
        flag(traits, "retryable"),
        flag(traits, "throttling"));
    throw error;
  }

  private void applyHeaders(
      JsonObject values, String target, JsonArray bindings, HttpResponse<?> response) {
    for (JsonElement item : bindings) {
      var binding = item.getAsJsonArray();
      String member = binding.get(0).getAsString();
      List<String> headers = response.headers().allValues(binding.get(1).getAsString());
      if (!headers.isEmpty())
        values.add(
            member,
            header(
                shape(target).getAsJsonObject("members").getAsJsonObject(member),
                String.join(", ", headers)));
    }
  }

  static Throwable unwrap(Throwable error) {
    while ((error instanceof CompletionException || error instanceof ExecutionException)
        && error.getCause() != null) error = error.getCause();
    return error;
  }

  static <T> T await(CompletableFuture<T> future) {
    try {
      return future.get();
    } catch (InterruptedException e) {
      future.cancel(true);
      Thread.currentThread().interrupt();
      throw new CancellationException("interrupted");
    } catch (ExecutionException e) {
      Throwable cause = unwrap(e);
      if (cause instanceof RuntimeException runtime) throw runtime;
      throw new NetworkError(cause);
    }
  }

  <T> Iterator<T> pages(String name, Object input, Class<T> output) {
    JsonObject op = schema.getAsJsonObject("operations").getAsJsonObject(name),
        pagination = op.getAsJsonObject("pagination");
    JsonObject values =
        input == null ? new JsonObject() : encode(text(op, "input"), input, null).getAsJsonObject();
    String key = text(pagination, "input");
    Set<String> seen = new HashSet<>();
    if (values.has(key) && !values.get(key).isJsonNull()) seen.add(values.get(key).getAsString());
    return new Iterator<>() {
      boolean done;

      public boolean hasNext() {
        return !done;
      }

      public T next() {
        if (done) throw new NoSuchElementException();
        T page = await(callValues(name, values, output));
        JsonElement token = encode(text(op, "output"), page, null);
        for (JsonElement part : pagination.getAsJsonArray("output")) {
          token =
              token != null && token.isJsonObject()
                  ? token.getAsJsonObject().get(part.getAsString())
                  : null;
        }
        if (token == null || token.isJsonNull() || token.getAsString().isEmpty()) done = true;
        else {
          String next = token.getAsString();
          if (!seen.add(next)) {
            done = true;
            throw new IllegalStateException("pagination token did not advance");
          }
          values.addProperty(key, next);
        }
        return page;
      }
    };
  }
}
