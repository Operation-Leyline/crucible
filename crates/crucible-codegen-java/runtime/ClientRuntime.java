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
import java.time.Duration;
import java.time.Instant;
import java.time.ZoneOffset;
import java.time.ZonedDateTime;
import java.time.format.DateTimeFormatter;
import java.util.ArrayList;
import java.util.Base64;
import java.util.HashSet;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.NoSuchElementException;
import java.util.Set;
import java.util.concurrent.CancellationException;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CompletionException;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.Flow;
import java.util.concurrent.ConcurrentLinkedQueue;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicBoolean;
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
  static final ExecutorService WORKERS =
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

  private void checkCollectionValue(JsonObject container, JsonObject member, Object value) {
    if (!flag(container, "sparse") && (value == null || value instanceof JsonElement item && item.isJsonNull()))
      throw new IllegalArgumentException("Null value in non-sparse collection");
  }

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

  ClientRuntime(ClientConfig input, String endpoint, Map<String, Class<?>> classes) {
    this(input, endpoint, classes, loadSchema());
  }

  ClientRuntime(
      ClientConfig input, String endpoint, Map<String, Class<?>> classes, JsonObject schema) {
    config = new ClientConfig();
    config.endpoint = input.endpoint == null ? endpoint : input.endpoint;
    if (!validEndpoint(config.endpoint))
      throw new IllegalArgumentException("absolute HTTP(S) endpoint without credentials, query, or fragment required");
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
    config.idempotencyToken = java.util.Objects.requireNonNull(input.idempotencyToken);
    config.headers = new LinkedHashMap<>(input.headers);
    config.middleware = List.copyOf(input.middleware);
    this.http =
        input.httpClient == null
            ? HttpClient.newBuilder().followRedirects(HttpClient.Redirect.NEVER).build()
            : input.httpClient;
    this.schema = schema;
    this.classes = classes;
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

  private static final java.util.regex.Pattern HOST_LABEL =
      java.util.regex.Pattern.compile("[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?");

  // A base URL is checked once at construction so no request is ever built against a malformed one.
  private static boolean validEndpoint(String value) {
    try {
      var url = URI.create(value);
      if (!("http".equals(url.getScheme()) || "https".equals(url.getScheme()))
          || url.getHost() == null
          || url.getRawUserInfo() != null
          || url.getRawQuery() != null
          || url.getRawFragment() != null
          || url.getPort() > 65535
          || url.getPort() == 0
          || value.chars().anyMatch(c -> c <= 32 || c == 127)) return false;
      String host = url.getHost();
      if (host.startsWith("[")) return true;
      if (host.endsWith(".")) host = host.substring(0, host.length() - 1);
      if (host.isEmpty()) return false;
      for (String part : host.split("\\.", -1)) if (!HOST_LABEL.matcher(part).matches()) return false;
      return true;
    } catch (IllegalArgumentException invalid) {
      return false;
    }
  }

  private boolean declaredName(String target, String name) {
    return shape(target).getAsJsonObject("members").has(name);
  }

  // Request binding names are the model's member names, which are also the JSON names.
  private JsonObject inputValues(JsonObject op, Object input) {
    if (input == null) return new JsonObject();
    String target = text(op, "input");
    JsonObject wire = encode(target, input, null).getAsJsonObject(), values = new JsonObject();
    for (String name : shape(target).getAsJsonObject("members").keySet())
      if (wire.has(name)) values.add(name, wire.get(name));
    return values;
  }

  private JsonElement paginationToken(JsonObject op, Object page) {
    String target = text(op, "output");
    JsonElement token = encode(target, page, null);
    for (JsonElement item : op.getAsJsonObject("pagination").getAsJsonArray("output")) {
      String name = item.getAsString();
      token = token != null && token.isJsonObject() ? token.getAsJsonObject().get(name) : null;
      target = text(shape(target).getAsJsonObject("members").getAsJsonObject(name), "target");
    }
    return token;
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
      case "float": case "double": {
        double number=((Number)value).doubleValue();
        if(!Double.isFinite(number)) throw new IllegalArgumentException("Nonfinite number");
        return new JsonPrimitive((Number)value);
      }
      case "timestamp":
        return writeTime((Instant) value, format == null ? text(shape, "format") : format);
      case "enum":
        return new JsonPrimitive(value.toString());
      case "intEnum":
        return new JsonPrimitive(Integer.parseInt(value.toString()));
      case "blob":
        // Standard, padded base64 in JSON; a payload decodes it back to the bytes.
        if (!(value instanceof byte[] bytes))
          throw new IllegalArgumentException("a blob is a byte[]");
        return new JsonPrimitive(Base64.getEncoder().encodeToString(bytes));
      case "list":
        {
          var result = new JsonArray();
          var member = shape.getAsJsonObject("member");
          var unique = new HashSet<String>();
          for (Object item : (java.util.Collection<?>) value) {
            checkCollectionValue(shape, member, item);
            var encoded = encode(text(member, "target"), item, text(member, "format"));
            if (flag(shape, "set") && !unique.add(collectionKey(encoded))) throw new IllegalArgumentException("Duplicate set member");
            result.add(encoded);
          }
          return result;
        }
      case "map":
        {
          var result = new JsonObject();
          var member = shape.getAsJsonObject("value");
          for (var entry : ((Map<?, ?>) value).entrySet()) {
            checkCollectionValue(shape, member, entry.getValue());
            result.add(
                entry.getKey().toString(),
                encode(text(member, "target"), entry.getValue(), text(member, "format")));
          }
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
                if (declaredName(target, unknown.name()))
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
      case "intEnum":
        try { return classes.get(target).getConstructor(int.class).newInstance(number(value).intValueExact()); }
        catch (ReflectiveOperationException error) { throw new IllegalStateException(error); }
      case "bigInteger": return number(value).toBigIntegerExact();
      case "bigDecimal": return number(value);
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
      case "blob":
        if (!value.isJsonPrimitive() || !value.getAsJsonPrimitive().isString())
          throw new IllegalArgumentException("expected base64 text");
        return Base64.getDecoder().decode(value.getAsString());
      case "list":
        {
            var result = (java.util.Collection<Object>) construct(classes.get(target));
            var member = shape.getAsJsonObject("member");
            var seen = new HashSet<String>();
            for (JsonElement item : value.getAsJsonArray()) {
              checkCollectionValue(shape, member, item);
              if (flag(shape, "set") && !seen.add(collectionKey(item))) throw new IllegalArgumentException("Duplicate set member");
            Object decoded = decode(text(member, "target"), item, text(member, "format"));
            if (!result.add(decoded) && flag(shape, "set")) throw new IllegalArgumentException("Duplicate set member");
          }
          return result;
        }
      case "map":
        {
          var result = (Map<Object, Object>) construct(classes.get(target));
          var member = shape.getAsJsonObject("value");
          var key = shape.getAsJsonObject("key");
          for (var entry : value.getAsJsonObject().entrySet()) {
            checkCollectionValue(shape, member, entry.getValue());
            result.put(
                decode(text(key, "target"), new JsonPrimitive(entry.getKey()), null),
                decode(text(member, "target"), entry.getValue(), text(member, "format")));
          }
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
              if (!declaredName(target, entry.getKey())) {
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
              Field field = result.getClass().getField(text(member, "attribute"));
              // OptionalValue is emitted from the IR's client nullability.
              boolean optional = field.getType() == OptionalValue.class;
              if (!optional
                  && (item == null
                      || (item.isJsonNull()
                          && !"document".equals(text(shape(text(member, "target")), "kind")))))
                throw new IllegalArgumentException(
                    entry.getKey() + ": missing or null non-optional member");
              if (item == null) continue;
              Object decoded = decode(text(member, "target"), item, text(member, "format"));
              field.set(result, optional ? OptionalValue.of(decoded) : decoded);
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

  private static String collectionKey(JsonElement value) {
    if (value.isJsonNull()) return "null";
    if (value.isJsonArray()) {
      var keys=new ArrayList<String>(); for (var item:value.getAsJsonArray()) keys.add(collectionKey(item));
      return "["+String.join(",",keys)+"]";
    }
    if (value.isJsonObject()) {
      var keys=new java.util.TreeMap<String,String>(); value.getAsJsonObject().entrySet().forEach(entry -> keys.put(entry.getKey(),collectionKey(entry.getValue())));
      var fields=new ArrayList<String>(); keys.forEach((key,item)->fields.add(JSON.toJson(key)+":"+item));
      return "{"+String.join(",",fields)+"}";
    }
    return value.getAsJsonPrimitive().isNumber() ? number(value).stripTrailingZeros().toString() : JSON.toJson(value);
  }

  private static JsonElement writeTime(Instant value, String format) {
    if ("epoch-seconds".equals(format)) {
      BigDecimal seconds = BigDecimal.valueOf(value.getEpochSecond())
          .add(BigDecimal.valueOf(value.getNano(), 9)).stripTrailingZeros();
      // Bindings use this number's text, which must stay decimal rather than scientific notation.
      return new JsonPrimitive(seconds.scale() < 0 ? seconds.setScale(0) : seconds);
    }
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

  static String bound(JsonElement value) {
    if (value.isJsonArray()) {
      List<String> values = new ArrayList<>();
      for (JsonElement item : value.getAsJsonArray()) {
        String text = bound(item);
        values.add(
            text.isEmpty()
                    || text.contains(",")
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

  private JsonElement boundaryTime(JsonObject member,JsonElement value,String format,boolean encode) {
    if(format==null || value==null || value.isJsonNull()) return value;
    var target=shape(text(member,"target"));
    switch(text(target,"kind")) {
      case "timestamp": { String original=text(member,"format")==null ? text(target,"format") : text(member,"format"); return encode ? writeTime(readTime(value,original),format) : writeTime(readTime(value,format),original); }
      case "list": { var result=new JsonArray(); for(var item:value.getAsJsonArray()) result.add(boundaryTime(target.getAsJsonObject("member"),item,format,encode)); return result; }
      case "map": { var result=new JsonObject(); for(var entry:value.getAsJsonObject().entrySet()) result.add(entry.getKey(),boundaryTime(target.getAsJsonObject("value"),entry.getValue(),format,encode)); return result; }
      default: return value;
    }
  }
  private String headerText(JsonObject member,JsonElement value,String format) {
    var target=shape(text(member,"target"));
    if("http-date".equals(format) && "list".equals(text(target,"kind")) && "timestamp".equals(text(shape(text(target.getAsJsonObject("member"),"target")),"kind"))) return String.join(", ",value.getAsJsonArray().asList().stream().map(JsonElement::getAsString).toList());
    return bound(value);
  }
  JsonElement header(JsonObject member, String value) { return header(member,value,null); }
  private JsonElement header(JsonObject member, String value, String format) {
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
          var child=shape.getAsJsonObject("member");
          boolean dates="http-date".equals(format==null ? text(child,"format") : format) && "timestamp".equals(text(shape(text(child,"target")),"kind"));
          for(int index=0;index<parts.size();index++) {
            String part=parts.get(index);
            if(dates && part.matches("[A-Za-z]{3}")) { if(++index>=parts.size()) throw new IllegalArgumentException("Incomplete HTTP date"); part+=", "+parts.get(index); }
            result.add(header(child,part.startsWith("\"") ? JSON.fromJson(part,String.class) : part,format));
          }
          return result;
        }
      case "byte":
      case "short":
      case "integer":
      case "long":
      case "intEnum": case "bigInteger": case "bigDecimal":
      case "float": case "double":
        return JSON.fromJson(value,JsonElement.class);
      case "boolean":
        return JSON.fromJson(value, JsonElement.class);
      case "timestamp":
        if ("epoch-seconds".equals(format==null ? text(member, "format") : format))
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
      String name, JsonObject operation, URI uri, Map<String, String> headers, byte[] body) {}

  private Prepared prepare(String name, JsonObject originalValues) {
    JsonObject values=originalValues.deepCopy();
    JsonObject op = schema.getAsJsonObject("operations").getAsJsonObject(name),
        request = op.getAsJsonObject("request");
    JsonObject formats=request.has("timestampFormats") ? request.getAsJsonObject("timestampFormats") : new JsonObject();
    for(var entry:formats.entrySet()) if(values.has(entry.getKey())) values.add(entry.getKey(),boundaryTime(shape(text(op,"input")).getAsJsonObject("members").getAsJsonObject(entry.getKey()),values.get(entry.getKey()),entry.getValue().getAsString(),true));
    String token = text(op, "token");
    if (token != null && (!values.has(token) || values.get(token).isJsonNull()))
      values.addProperty(token, java.util.Objects.requireNonNull(config.idempotencyToken.get()));
    String uri = text(op, "uri");
    for (JsonElement label : request.getAsJsonArray("labels")) {
      String key = label.getAsString();
      JsonElement value = values.get(key);
      if (value == null || value.isJsonNull() || bound(value).isEmpty())
        throw new IllegalArgumentException(key + " is required");
      String labelValue = bound(value);
      boolean greedy = op.has("greedyLabels") && op.getAsJsonArray("greedyLabels").contains(new JsonPrimitive(key));
      var segments = greedy ? labelValue.split("/", -1) : new String[] {labelValue};
      var encoded = new ArrayList<String>();
      for (String segment : segments) {
        if (segment.equals(".") || segment.equals("..")) throw new IllegalArgumentException(key + " cannot contain a dot segment");
        encoded.add(escape(segment));
      }
      uri = uri.replace("{" + key + (greedy ? "+" : "") + "}", String.join("/", encoded));
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
    // A Content-Type bound to an input member outranks the payload's media
    // type, which outranks client header defaults.
    boolean modelledContentType = false;
    for (JsonElement item : request.getAsJsonArray("headers")) {
      var binding = item.getAsJsonArray();
      JsonElement value = values.get(binding.get(0).getAsString());
      if (value == null || value.isJsonNull()) continue;
      String wire = binding.get(1).getAsString();
      // Replaces a default of the same name in any case, so only one is sent.
      headers.keySet().removeIf(key -> key.equalsIgnoreCase(wire));
      if (wire.equalsIgnoreCase("content-type")) modelledContentType = true;
      headers.put(wire, headerText(shape(text(op,"input")).getAsJsonObject("members").getAsJsonObject(binding.get(0).getAsString()),headerEncoding(request, binding.get(0).getAsString(), value, true),text(formats,binding.get(0).getAsString())));
    }
    byte[] body = null;
    if (!request.getAsJsonArray("body").isEmpty()) {
      JsonObject object = new JsonObject();
      for (var member : request.getAsJsonArray("body")) {
        String key = member.getAsString();
        if (values.has(key)) object.add(key, values.get(key));
      }
      body = JSON.toJson(object).getBytes(StandardCharsets.UTF_8);
      headers.keySet().removeIf(key -> key.equalsIgnoreCase("content-type"));
      headers.put("content-type", "application/json");
    }
    JsonElement payload = request.get("payload");
    if (payload != null && payload.isJsonArray()) {
      // The bytes go as they are.
      JsonElement value = values.get(payload.getAsJsonArray().get(0).getAsString());
      if (value != null && (!value.isJsonNull() || "document".equals(text(request, "payloadKind")))) {
        String kind = text(request, "payloadKind");
        byte[] bytes = "string".equals(kind) ? value.getAsString().getBytes(StandardCharsets.UTF_8)
            : "document".equals(kind) ? JSON.toJson(value).getBytes(StandardCharsets.UTF_8)
            : Base64.getDecoder().decode(value.getAsString());
        if (bytes.length > 0 || "string".equals(kind)) {
          body = bytes;
          if (!modelledContentType) {
            headers.keySet().removeIf(key -> key.equalsIgnoreCase("content-type"));
            headers.put("content-type", payload.getAsJsonArray().get(1).getAsString());
          }
        }
      }
    }
    // The release this client was generated for (docs/api-versions-clients.md).
    String apiVersion = text(schema, "apiVersion");
    if (apiVersion != null) headers.put("crucible-version", apiVersion);
    return new Prepared(name, op, URI.create(config.endpoint + uri), headers, body);
  }

  <T> CompletableFuture<T> call(String name, Object input, Class<T> output) {
    try {
      JsonObject op = schema.getAsJsonObject("operations").getAsJsonObject(name);
      return callValues(name, inputValues(op, input), output);
    } catch (RuntimeException e) {
      return CompletableFuture.failedFuture(e);
    }
  }

  private static final class Call<T> extends CompletableFuture<T> {
    final AtomicReference<Future<?>> active = new AtomicReference<>();
    Future<?> retry;
    volatile Future<?> deadline;

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
    result.deadline =
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
          result.deadline.cancel(false);
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
      var request =
          new ClientConfig.Request(
              prepared.uri(),
              text(prepared.operation(), "method"),
              headers,
              prepared.body() == null ? null : prepared.body().clone());
      for (var middleware : config.middleware) middleware.apply(request);
      boolean credentials = Auth.apply(config, prepared.operation(), request)
          || headers.keySet().stream().anyMatch(k -> k.equalsIgnoreCase("authorization"));
      String host = request.uri.getHost();
      if (credentials
          && !"https".equals(request.uri.getScheme())
          && !Set.of("localhost", "127.0.0.1", "[::1]", "::1").contains(host))
        throw new IllegalArgumentException(
            "refusing credentials over plaintext HTTP outside loopback");
      var builder = HttpRequest.newBuilder(request.uri).timeout(Duration.ofNanos(remaining));
      headers.forEach(builder::setHeader);
      builder.method(
          request.method,
          request.body == null
              ? HttpRequest.BodyPublishers.noBody()
              : HttpRequest.BodyPublishers.ofByteArray(request.body));
      JsonObject bindings = prepared.operation().getAsJsonObject("response");
      boolean hasBody =
          !bindings.getAsJsonArray("body").isEmpty() || payloadMember(bindings) != null;
      if (result.isDone()) return;
      CompletableFuture<HttpResponse<byte[]>> sent =
          http.sendAsync(
              builder.build(),
              info -> {
                boolean ok = info.statusCode() >= 200 && info.statusCode() < 300;
                return ok && !hasBody ? emptyBody() : HttpResponse.BodySubscribers.ofByteArray();
              });
      result.track(sent);
      sent.whenComplete(
          (response, error) -> {
            if (result.isDone()) return;
            Throwable failure = unwrap(error);
            if (failure == null) {
              try {
                JsonObject decoded =
                    response(prepared.name(), prepared.operation(), response, response.body());
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

  /** The member that is the whole body, raw, or null. */
  private static String payloadMember(JsonObject bindings) {
    JsonElement payload = bindings.get("payload");
    return payload != null && payload.isJsonArray()
        ? payload.getAsJsonArray().get(0).getAsString()
        : null;
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

  private JsonObject response(String operation, JsonObject op, HttpResponse<?> response, byte[] bytes) {
    int status = response.statusCode();
    JsonObject values = new JsonObject();
    String body = new String(bytes, StandardCharsets.UTF_8);
    if (status >= 200 && status < 300) {
      String payload = payloadMember(op.getAsJsonObject("response"));
      // A payload is the body as received, whatever it holds.
      if (payload != null)
        values.add(payload, payloadValue(op.getAsJsonObject("response"), bytes));
      else if (!body.isEmpty()) values = JSON.fromJson(body, JsonObject.class);
      if (values == null) throw new IllegalArgumentException("response requires object");
      applyHeaders(
          values,
          text(op, "output"),
          op.getAsJsonObject("response"),
          response);
      return values;
    }
    // BEGIN ERROR DOCUMENTATION
    boolean documentationBodyAbsent = body.chars().allMatch(c -> c == ' ' || c == '\t' || c == '\r' || c == '\n');
    boolean documentationBodyValid = documentationBodyAbsent;
    // END ERROR DOCUMENTATION
    try {
      JsonElement parsed = JSON.fromJson(body, JsonElement.class);
      // BEGIN ERROR DOCUMENTATION
      documentationBodyValid = documentationBodyAbsent || (parsed != null && parsed.isJsonObject());
      // END ERROR DOCUMENTATION
      if (parsed != null && parsed.isJsonObject()) values = parsed.getAsJsonObject();
    } catch (RuntimeException ignored) {
    }
    JsonElement named = values.get("__type");
    if (named==null) named=response.headers().firstValue("x-crucible-error").map(JsonPrimitive::new).orElse(null);
    boolean explicit =
        named != null && named.isJsonPrimitive() && named.getAsJsonPrimitive().isString();
    String name = explicit ? named.getAsString() : null, target = null;
    List<String> matches = new ArrayList<>();
    for (JsonElement error : op.getAsJsonArray("errors")) {
      String id = error.getAsString();
      JsonObject shape = shape(id);
      String wireCode=text(shape.getAsJsonObject("error"), "wireCode");
      if (explicit && (wireCode==null ? text(shape,"name") : wireCode).equals(name)) target = id;
      else if (!explicit && shape.getAsJsonObject("error").get("status").getAsInt() == status)
        matches.add(id);
    }
    if (!explicit && matches.size() == 1) {
      target = matches.get(0);
      name = text(shape(target), "name");
    }
    JsonObject traits = target == null ? new JsonObject() : shape(target).getAsJsonObject("error");
    // BEGIN ERROR DOCUMENTATION
    JsonObject documentation = traits.getAsJsonObject("documentation");
    String documentationUrl = documentation == null ? null : text(documentation, "url");
    // END ERROR DOCUMENTATION
    ServiceError error = new ServiceException();
    if (target != null) {
      try {
        JsonObject typedValues = values.deepCopy();
        String payload = payloadMember(traits);
        // BEGIN ERROR DOCUMENTATION
        if (payload == null && !documentationBodyValid) throw new IllegalArgumentException("modeled error body requires object");
        // END ERROR DOCUMENTATION
        if (payload != null) typedValues.add(payload, payloadValue(traits, bytes));
        applyHeaders(typedValues, target, traits, response);
        error = (ServiceError) decode(target, typedValues, null);
        // BEGIN ERROR DOCUMENTATION
        if (documentation != null) {
          String member = text(documentation, "reasonMember");
          JsonElement reason = member == null ? null : typedValues.get(member);
          if (member != null && !typedValues.has(member)) reason = documentation.get("reasonDefault");
          JsonObject pages = documentation.getAsJsonObject("reasonUrls");
          if (reason != null && reason.isJsonPrimitive() && reason.getAsJsonPrimitive().isString()
              && pages.has(reason.getAsString())) {
            documentationUrl = pages.get(reason.getAsString()).getAsString();
          }
        }
        // END ERROR DOCUMENTATION
      } catch (RuntimeException ignored) {
        // Malformed modeled members must not hide HTTP metadata or the retry policy.
      }
    }
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
        bytes,
        flag(traits, "retryable"),
        flag(traits, "throttling"));
    // BEGIN ERROR DOCUMENTATION
    error.initializeDocumentation(documentationUrl);
    // END ERROR DOCUMENTATION
    throw error;
  }

  private void applyHeaders(
      JsonObject values, String target, JsonObject bindings, HttpResponse<?> response) {
    if (target == null) return;
    for (JsonElement item : bindings.getAsJsonArray("headers")) {
      var binding = item.getAsJsonArray();
      String member = binding.get(0).getAsString();
      List<String> headers = response.headers().allValues(binding.get(1).getAsString());
      if (!headers.isEmpty()) {
        var description=shape(target).getAsJsonObject("members").getAsJsonObject(member);
        String format=bindings.has("timestampFormats") ? text(bindings.getAsJsonObject("timestampFormats"),member) : null;
        values.add(member,boundaryTime(description,headerEncoding(bindings,member,header(description,String.join(", ",headers),format),false),format,false));
      }
    }
  }

  private static JsonElement payloadValue(JsonObject binding, byte[] bytes) {
    String kind = text(binding, "payloadKind");
    if ("string".equals(kind)) return new JsonPrimitive(new String(bytes, StandardCharsets.UTF_8));
    if ("document".equals(kind)) return JSON.fromJson(new String(bytes, StandardCharsets.UTF_8), JsonElement.class);
    return new JsonPrimitive(Base64.getEncoder().encodeToString(bytes));
  }

  private static JsonElement headerEncoding(JsonObject binding, String member, JsonElement value, boolean encode) {
    JsonObject encodings = binding.getAsJsonObject("headerEncodings");
    if (encodings == null || !"base64".equals(text(encodings, member))) return value;
    return mediaHeader(value, encode);
  }

  private static JsonElement mediaHeader(JsonElement value, boolean encode) {
    if (value.isJsonObject()) {
      var result = new JsonObject();
      for (var entry : value.getAsJsonObject().entrySet()) result.add(entry.getKey(), mediaHeader(entry.getValue(), encode));
      return result;
    }
    if (value.isJsonArray()) {
      var result = new JsonArray();
      for (JsonElement item : value.getAsJsonArray()) result.add(mediaHeader(item, encode));
      return result;
    }
    return new JsonPrimitive(encode ? Base64.getEncoder().encodeToString(value.getAsString().getBytes(StandardCharsets.UTF_8))
        : new String(Base64.getDecoder().decode(value.getAsString()), StandardCharsets.UTF_8));
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

  <T> Flow.Publisher<T> pagesAsync(String name, Object input, Class<T> output) {
    JsonObject op = schema.getAsJsonObject("operations").getAsJsonObject(name);
    JsonObject snapshot = inputValues(op, input);
    return subscriber -> {
      java.util.Objects.requireNonNull(subscriber, "subscriber");
      new PageSubscription<T>(subscriber, name, output, op, snapshot.deepCopy()).start();
    };
  }

  /** Every subscriber owns its cursor and demand. Queue callbacks to avoid recursive request(1). */
  private final class PageSubscription<T> implements Flow.Subscription {
    private final Flow.Subscriber<? super T> subscriber;
    private final String name;
    private final Class<T> output;
    private final JsonObject op, pagination, values;
    private final Set<String> seen = new HashSet<>();
    private final ConcurrentLinkedQueue<Runnable> events = new ConcurrentLinkedQueue<>();
    private final AtomicInteger scheduled = new AtomicInteger();
    private final AtomicBoolean cancelled = new AtomicBoolean();
    private final AtomicReference<CompletableFuture<T>> active = new AtomicReference<>();
    private long demand;
    private boolean pending, terminal;

    PageSubscription(Flow.Subscriber<? super T> subscriber, String name, Class<T> output,
        JsonObject op, JsonObject values) {
      this.subscriber = subscriber; this.name = name; this.output = output;
      this.op = op; this.pagination = op.getAsJsonObject("pagination"); this.values = values;
    }

    void start() {
      enqueue(() -> {
        try {
          subscriber.onSubscribe(this);
          JsonElement token = values.get(text(pagination, "input"));
          if (token != null && !token.isJsonNull() && !token.getAsString().isEmpty()) seen.add(token.getAsString());
        } catch (Throwable error) { cancel(); }
      });
    }

    private void enqueue(Runnable event) {
      events.add(event);
      if (scheduled.getAndIncrement() == 0) WORKERS.execute(() -> {
        int consumed = 1;
        do {
          Runnable next;
          while ((next = events.poll()) != null) next.run();
          consumed = scheduled.addAndGet(-consumed);
        } while (consumed != 0);
      });
    }

    @Override public void request(long count) {
      enqueue(() -> {
        if (cancelled.get() || terminal) return;
        if (count <= 0) { fail(new IllegalArgumentException("page demand must be positive")); return; }
        demand = demand > Long.MAX_VALUE - count ? Long.MAX_VALUE : demand + count;
        fetch();
      });
    }

    @Override public void cancel() {
      cancelled.set(true);
      CompletableFuture<T> call = active.getAndSet(null);
      if (call != null) call.cancel(true);
    }

    private void fail(Throwable error) {
      if (terminal || cancelled.get()) return;
      terminal = true;
      CompletableFuture<T> call = active.getAndSet(null);
      if (call != null) call.cancel(true);
      try { subscriber.onError(unwrap(error)); } catch (Throwable ignored) { cancel(); }
    }

    private void fetch() {
      if (pending || terminal || cancelled.get() || demand == 0) return;
      pending = true;
      CompletableFuture<T> call;
      try { call = callValues(name, values, output); }
      catch (Throwable error) { pending = false; fail(error); return; }
      active.set(call);
      if (cancelled.get()) call.cancel(true);
      call.whenComplete((page, error) -> enqueue(() -> {
        active.compareAndSet(call, null);
        pending = false;
        if (terminal || cancelled.get()) return;
        if (error != null) { fail(error); return; }
        boolean complete;
        try {
          JsonElement token = paginationToken(op, page);
          complete = token == null || token.isJsonNull() || token.getAsString().isEmpty();
          if (!complete) {
            String cursor = token.getAsString();
            if (!seen.add(cursor)) throw new IllegalStateException("pagination token did not advance");
            values.addProperty(text(pagination, "input"), cursor);
          }
        } catch (Throwable failure) { fail(failure); return; }
        if (demand != Long.MAX_VALUE) demand--;
        try { subscriber.onNext(page); } catch (Throwable failure) { cancel(); return; }
        if (cancelled.get()) return;
        if (complete) {
          terminal = true;
          try { subscriber.onComplete(); } catch (Throwable ignored) { cancel(); }
        } else fetch();
      }));
    }
  }

  <T> Iterator<T> pages(String name, Object input, Class<T> output) {
    JsonObject op = schema.getAsJsonObject("operations").getAsJsonObject(name),
        pagination = op.getAsJsonObject("pagination");
    JsonObject values = inputValues(op, input);
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
        JsonElement token = paginationToken(op, page);
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
