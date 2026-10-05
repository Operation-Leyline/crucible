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
import java.util.Base64;
import java.util.HashSet;
import java.util.HexFormat;
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
  private final JsonObject endpointParameters;
  private final JsonObject endpointBuiltins;
  private final Map<String, Class<?>> classes;
  private final boolean digest;
  // The XML family's codec, or null for the JSON protocols.
  private final XmlProtocol xml;

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
    if (config.endpoint.isEmpty() ? !schema.has("endpointRules") : Endpoints.parseUrl(config.endpoint)==null)
      throw new IllegalArgumentException("absolute HTTP(S) endpoint without credentials, query, or fragment required");
    endpointParameters = JSON.toJsonTree(input.endpointParameters).getAsJsonObject();
    endpointBuiltins = new JsonObject(); endpointBuiltins.addProperty("AWS::Region",input.region);
    endpointBuiltins.addProperty("SDK::Endpoint",config.endpoint.isEmpty() ? null : config.endpoint);
    JSON.toJsonTree(input.endpointBuiltins).getAsJsonObject().entrySet().forEach(entry -> endpointBuiltins.add(entry.getKey(),entry.getValue()));
    config.disableHostPrefix=input.disableHostPrefix;
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
    config.basicAuth = input.basicAuth;
    config.apiKey = input.apiKey;
    config.credentials = input.credentials;
    config.region = input.region;
    config.clock = input.clock;
    config.requestChecksumCalculation=java.util.Objects.requireNonNull(input.requestChecksumCalculation);
    config.responseChecksumValidation=java.util.Objects.requireNonNull(input.responseChecksumValidation);
    config.checksumValidated=input.checksumValidated;
    config.headers = new LinkedHashMap<>(input.headers);
    config.middleware = List.copyOf(input.middleware);
    this.http =
        input.httpClient == null
            ? HttpClient.newBuilder().followRedirects(HttpClient.Redirect.NEVER).build()
            : input.httpClient;
    this.schema = schema;
    this.classes = classes;
    this.digest = digest;
    this.xml = XmlProtocol.handles(text(schema, "protocol")) ? new XmlProtocol(schema) : null;
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

  private String wireName(String target, String name) {
    String wire = text(shape(target).getAsJsonObject("members").getAsJsonObject(name), "jsonName");
    return wire == null ? name : wire;
  }

  private boolean declaredWireName(String target, String wire) {
    return shape(target).getAsJsonObject("members").keySet().stream().anyMatch(name -> wireName(target, name).equals(wire));
  }

  // Request binding names are canonical model names; nested values already use JSON wire names.
  private JsonObject inputValues(JsonObject op, Object input) {
    if (input == null) return new JsonObject();
    String target = text(op, "input");
    JsonObject wire = encode(target, input, null).getAsJsonObject(), values = new JsonObject();
    for (String name : shape(target).getAsJsonObject("members").keySet()) {
      String key = wireName(target, name);
      if (wire.has(key)) values.add(name, wire.get(key));
    }
    return values;
  }

  private JsonElement paginationToken(JsonObject op, Object page) {
    String target = text(op, "output");
    JsonElement token = encode(target, page, null);
    for (JsonElement item : op.getAsJsonObject("pagination").getAsJsonArray("output")) {
      String name = item.getAsString();
      token = token != null && token.isJsonObject() ? token.getAsJsonObject().get(wireName(target, name)) : null;
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

  private boolean policy(String name) { var settings=schema.getAsJsonObject("protocolSettings"); return settings!=null && flag(settings,name); }
  private boolean awsJson() { return Set.of("awsRestJson1","awsJson1_0","awsJson1_1").contains(String.valueOf(text(schema,"protocol"))); }

  JsonElement encode(String target, Object value, String format) {
    if (target == null) return new JsonObject();
    JsonObject shape = shape(target);
    String kind = text(shape, "kind");
    if ("unit".equals(kind)) return new JsonObject();
    if (value == null) return JsonNull.INSTANCE;
    switch (kind) {
      case "float": case "double": {
        double number=((Number)value).doubleValue();
        if(!Double.isFinite(number)) { if(!policy("nonFiniteFloatStrings")) throw new IllegalArgumentException("Nonfinite number"); return new JsonPrimitive(value.toString()); }
        return new JsonPrimitive((Number)value);
      }
      case "timestamp":
        return writeTime((Instant) value, format == null ? text(shape, "format") : format);
      case "enum":
        return new JsonPrimitive(value.toString());
      case "intEnum":
        return new JsonPrimitive(Integer.parseInt(value.toString()));
      case "blob":
        // A stream travels beside the JSON values, as the payload itself.
        if (flag(shape, "streaming")) {
          if (!(value instanceof ByteStream)) throw new IllegalArgumentException("a streaming blob is a ByteStream");
          return JsonNull.INSTANCE;
        }
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
              if(item==null && policy("omitNullStructureMembers") && "structure".equals(kind) && !"document".equals(text(shape(text(member,"target")),"kind"))) continue;
              result.add(
                  wireName(target, entry.getKey()), encode(text(member, "target"), item, text(member, "format")));
            }
            if ("union".equals(kind)) {
              var unknown = (UnknownUnion) value.getClass().getField("unknownVariant").get(value);
              if (unknown != null) {
                if (declaredWireName(target, unknown.name()))
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
          if(policy("nonFiniteFloatStrings") && value.isJsonPrimitive() && value.getAsJsonPrimitive().isString() && Set.of("NaN","Infinity","-Infinity").contains(value.getAsString())) return Float.valueOf(value.getAsString());
          float n = number(value).floatValue();
          if (!Float.isFinite(n)) throw new IllegalArgumentException("float overflow");
          return n;
        }
      case "double":
        {
          if(policy("nonFiniteFloatStrings") && value.isJsonPrimitive() && value.getAsJsonPrimitive().isString() && Set.of("NaN","Infinity","-Infinity").contains(value.getAsString())) return Double.valueOf(value.getAsString());
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
              if(policy("ignoreUnknownUnionType") && !declaredWireName(target,"__type")) { object=object.deepCopy(); object.remove("__type"); }
              if (object.size() != 1 || policy("ignoreUnknownUnionType") && object.entrySet().iterator().next().getValue().isJsonNull())
                throw new IllegalArgumentException("a union must contain exactly one member");
              var entry = object.entrySet().iterator().next();
              if (!declaredWireName(target, entry.getKey())) {
                result
                    .getClass()
                    .getField("unknownVariant")
                    .set(result, new UnknownUnion(entry.getKey(), entry.getValue().deepCopy()));
                return result;
              }
            }
            for (var entry : members.entrySet()) {
              var member = entry.getValue().getAsJsonObject();
              // A response stream is attached after decoding, from the body itself.
              if (flag(shape(text(member, "target")), "streaming")) continue;
              JsonElement item = object.get(wireName(target, entry.getKey()));
              if(item!=null && item.isJsonNull() && policy("omitNullStructureMembers") && "structure".equals(kind) && !"document".equals(text(shape(text(member,"target")),"kind"))) item=null;
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
      case "float": case "double":
        if(policy("nonFiniteFloatStrings") && Set.of("NaN","Infinity","-Infinity").contains(value)) return new JsonPrimitive(value);
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
      String name, JsonObject operation, URI uri, Map<String, String> headers, byte[] body, Checksums.Plan checksum,
      ByteStream stream) {}

  /** The streaming payload of {@code input}, which encoding leaves out of the JSON values. */
  private ByteStream requestStream(JsonObject op, Object input) {
    JsonObject request = op.getAsJsonObject("request");
    String member = payloadMember(request);
    if (input == null || member == null || !flag(request, "streaming")) return null;
    Object value = memberValue(input, shape(text(op, "input")).getAsJsonObject("members").getAsJsonObject(member));
    return (ByteStream) value;
  }

  private static Object memberValue(Object model, JsonObject member) {
    try {
      Object value = model.getClass().getField(text(member, "attribute")).get(model);
      return value instanceof OptionalValue<?> optional ? (optional.present() ? optional.value() : null) : value;
    } catch (ReflectiveOperationException error) {
      throw new IllegalStateException(error);
    }
  }

  private static Long lengthBound(JsonObject member, String key) {
    JsonObject length = member.getAsJsonObject("length");
    return length == null || !length.has(key) || length.get(key).isJsonNull() ? null : length.get(key).getAsLong();
  }

  private Prepared prepare(String name, JsonObject originalValues, ByteStream stream) {
    JsonObject values=originalValues.deepCopy();
    JsonObject op = schema.getAsJsonObject("operations").getAsJsonObject(name),
        request = op.getAsJsonObject("request");
    var checksum=Checksums.prepare(name,op,values,config,stream!=null);
    var wireInput=new JsonObject(); for(var entry:values.entrySet()) wireInput.add(wireName(text(op,"input"),entry.getKey()),entry.getValue());
    var queryInput=queryValue(text(op,"input"),wireInput,null);
    JsonObject resolved;
    if(schema.has("endpointRules")) resolved=Endpoints.resolve(schema.getAsJsonObject("endpointRules"),op.has("endpointParameters") ? op.getAsJsonObject("endpointParameters") : new JsonObject(),queryInput,endpointParameters,endpointBuiltins);
    else { resolved=new JsonObject(); resolved.addProperty("url",config.endpoint); resolved.add("headers",new JsonObject()); resolved.add("properties",new JsonObject()); }
    String endpoint=text(resolved,"url");
    if(op.has("endpoint") && !config.disableHostPrefix) endpoint=Endpoints.hostPrefix(endpoint,op.getAsJsonObject("endpoint"),queryInput.getAsJsonObject());
    endpoint=endpoint.replaceAll("/+$", "");
    var resolvedOp=new JsonObject(); op.entrySet().forEach(entry -> resolvedOp.add(entry.getKey(),entry.getValue())); resolvedOp.add("resolvedEndpoint",resolved);
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
    String dynamicQuery = text(request, "queryParams");
    if (dynamicQuery != null && values.has(dynamicQuery) && !values.get(dynamicQuery).isJsonNull()) {
      Set<String> reserved = new HashSet<>();
      for (var binding : request.getAsJsonArray("query")) reserved.add(binding.getAsJsonArray().get(1).getAsString());
      int question = uri.indexOf('?');
      if (question >= 0) for (String pair : uri.substring(question + 1).split("&"))
        reserved.add(java.net.URLDecoder.decode(pair.split("=", 2)[0], StandardCharsets.UTF_8));
      for (var entry : values.getAsJsonObject(dynamicQuery).entrySet()) {
        if (reserved.contains(entry.getKey())) continue;
        var entries = entry.getValue().isJsonArray() ? entry.getValue().getAsJsonArray() : new JsonArray();
        if (!entry.getValue().isJsonArray()) entries.add(entry.getValue());
        for (var item : entries) query.add(escape(entry.getKey()) + "=" + escape(bound(item)));
      }
    }
    if (!query.isEmpty()) uri += (uri.contains("?") ? "&" : "?") + String.join("&", query);
    Map<String, String> headers = new LinkedHashMap<>(config.headers);
    for(var entry:resolved.getAsJsonObject("headers").entrySet()) {
      headers.keySet().removeIf(key -> key.equalsIgnoreCase(entry.getKey()));
      headers.put(entry.getKey(),String.join(", ",entry.getValue().getAsJsonArray().asList().stream().map(JsonElement::getAsString).toList()));
    }
    if (request.has("prefixHeaders")) {
      Set<String> reserved = new HashSet<>();
      for (var binding : request.getAsJsonArray("headers")) reserved.add(binding.getAsJsonArray().get(1).getAsString().toLowerCase(java.util.Locale.ROOT));
      for (var item : request.getAsJsonArray("prefixHeaders")) {
        var pair = item.getAsJsonArray(); var value = values.get(pair.get(0).getAsString());
        if (value == null || value.isJsonNull()) continue;
        // Each value is media-encoded once, as it becomes a header.
        for (var entry : value.getAsJsonObject().entrySet()) {
          String header = pair.get(1).getAsString() + entry.getKey();
          if (!reserved.contains(header.toLowerCase(java.util.Locale.ROOT))) headers.put(header, bound(headerEncoding(request, pair.get(0).getAsString(), entry.getValue(), true)));
        }
      }
    }
    for (JsonElement item : request.getAsJsonArray("headers")) {
      var binding = item.getAsJsonArray();
      JsonElement value = values.get(binding.get(0).getAsString());
      if (value != null && !value.isJsonNull())
        headers.put(binding.get(1).getAsString(), headerText(shape(text(op,"input")).getAsJsonObject("members").getAsJsonObject(binding.get(0).getAsString()),headerEncoding(request, binding.get(0).getAsString(), value, true),text(formats,binding.get(0).getAsString())));
    }
    byte[] body = null;
    String protocol=text(schema,"protocol");
    List<String> bodyMembers = new ArrayList<>();
    for (var member : request.getAsJsonArray("body")) bodyMembers.add(member.getAsString());
    if (xml != null && xml.query()) {
      // awsQuery and ec2Query always send a form naming the action and API version.
      body = xml.form(name, text(op, "input"), values);
      headers.keySet().removeIf(key -> key.equalsIgnoreCase("content-type"));
      headers.put("content-type", "application/x-www-form-urlencoded");
    } else if (xml != null && !bodyMembers.isEmpty()) {
      body = xml.document(text(op, "input"), bodyMembers, values);
      headers.put("content-type", "application/xml");
    } else if (!request.getAsJsonArray("body").isEmpty()) {
      JsonObject object = new JsonObject();
      for (var member : request.getAsJsonArray("body")) {
        String key = member.getAsString();
        if (values.has(key)) object.add(wireName(text(op, "input"), key), values.get(key));
      }
      body = JSON.toJson(object).getBytes(StandardCharsets.UTF_8);
      headers.put("content-type", "application/json");
    }
    JsonElement payload = request.get("payload");
    if (payload != null && payload.isJsonArray()) {
      // The bytes go as they are: a body digest covers exactly these.
      JsonElement value = values.get(payload.getAsJsonArray().get(0).getAsString());
      if (value != null && (!value.isJsonNull() || "document".equals(text(request, "payloadKind")))) {
        String kind = text(request, "payloadKind");
        byte[] bytes = "string".equals(kind) ? value.getAsString().getBytes(StandardCharsets.UTF_8)
            : "document".equals(kind) ? (xml != null ? xml.payload(text(op, "input"), payload.getAsJsonArray().get(0).getAsString(), value) : JSON.toJson(value).getBytes(StandardCharsets.UTF_8))
            : Base64.getDecoder().decode(value.getAsString());
        if (bytes.length > 0 || "string".equals(kind)) {
          body = bytes;
          headers.put("content-type", payload.getAsJsonArray().get(1).getAsString());
        }
      }
    }
    if (stream != null) {
      if (flag(request, "requiresLength") && stream.length().isEmpty())
        throw new IllegalArgumentException(name + ": streaming payload requires a known length");
      headers.put("content-type", payload.getAsJsonArray().get(1).getAsString());
      stream.sha256().ifPresent(digest -> headers.put("x-amz-content-sha256", digest));
    }
    if("awsJson1_0".equals(protocol) || "awsJson1_1".equals(protocol)) {
      if(body==null) body="{}".getBytes(StandardCharsets.UTF_8);
      headers.keySet().removeIf(key->key.equalsIgnoreCase("content-type") || key.equalsIgnoreCase("x-amz-target"));
      headers.put("content-type","awsJson1_0".equals(protocol) ? "application/x-amz-json-1.0" : "application/x-amz-json-1.1");
      String service=text(schema,"service"); headers.put("x-amz-target",service.substring(service.indexOf('#')+1)+"."+name);
    } else if("awsRestJson1".equals(protocol) && payloadMember(request)==null && headers.keySet().stream().noneMatch(key->key.equalsIgnoreCase("content-type"))) headers.put("content-type","application/json");
    return new Prepared(name, resolvedOp, URI.create(endpoint + uri), headers, body,checksum,stream);
  }

  <T> CompletableFuture<T> call(String name, Object input, Class<T> output) {
    ByteStream stream = null;
    try {
      JsonObject op = schema.getAsJsonObject("operations").getAsJsonObject(name);
      stream = requestStream(op, input);
      return callValues(name, inputValues(op, input), stream, output);
    } catch (RuntimeException e) {
      closeOneShot(stream);
      return CompletableFuture.failedFuture(e);
    }
  }

  private JsonElement queryValue(String target, JsonElement value, String format) {
    if (target==null || value.isJsonNull()) return value;
    JsonObject shape=shape(target);
    switch (text(shape,"kind")) {
      case "timestamp": return encode(target,decode(target,value,format),"epoch-seconds");
      case "structure": case "union": {
        var result=new JsonObject();
        for (var entry:value.getAsJsonObject().entrySet()) {
          String name=null;
          for (String member:shape.getAsJsonObject("members").keySet()) if (wireName(target,member).equals(entry.getKey())) { name=member; break; }
          if (name==null) result.add(entry.getKey(),entry.getValue());
          else { var member=shape.getAsJsonObject("members").getAsJsonObject(name); result.add(name,queryValue(text(member,"target"),entry.getValue(),text(member,"format"))); }
        }
        return result;
      }
      case "list": {
        var result=new JsonArray(); var member=shape.getAsJsonObject("member");
        for (var item:value.getAsJsonArray()) result.add(queryValue(text(member,"target"),item,text(member,"format")));
        return result;
      }
      case "map": {
        var result=new JsonObject(); var member=shape.getAsJsonObject("value");
        for (var entry:value.getAsJsonObject().entrySet()) result.add(entry.getKey(),queryValue(text(member,"target"),entry.getValue(),text(member,"format")));
        return result;
      }
      default: return value;
    }
  }

  <T> CompletableFuture<T> waitUntil(String operation, String name, Object input, Class<T> output, WaiterOptions options) {
    try {
      var op=schema.getAsJsonObject("operations").getAsJsonObject(operation);
      var waiter=op.getAsJsonObject("waiters").getAsJsonObject(name);
      Duration minimum=options.minDelay()==null ? Duration.ofSeconds(waiter.get("minDelay").getAsLong()) : options.minDelay();
      Duration maximum=options.maxDelay()==null ? Duration.ofSeconds(waiter.get("maxDelay").getAsLong()) : options.maxDelay();
      if (options.timeout()==null || options.timeout().isNegative() || options.timeout().isZero() || minimum.isNegative() || maximum.compareTo(minimum)<0) throw new IllegalArgumentException("Invalid waiter timeout or delay bounds");
      long timeout=options.timeout().toNanos(), low=minimum.toNanos(), high=maximum.toNanos();
      var values=inputValues(op,input);
      var stream=requestStream(op,input);
      var queryInput=queryValue(text(op,"input"),encode(text(op,"input"),input,null),null);
      var result=new Call<T>();
      class Poll implements Runnable {
        volatile int attempts;
        volatile T response;
        volatile Throwable error;
        final long started=System.nanoTime();
        public void run() {
          if (result.isDone()) return;
          attempts++;
          try {
            var request=callValues(operation,values.deepCopy(),stream,output); result.track(request);
            request.whenCompleteAsync((value,failure)->{
              if (result.isDone()) {
                // Cancelling a completed operation cannot release its unread response.
                closeStreams(text(op,"output"),value);
                return;
              }
              response=value; error=failure instanceof CompletionException && failure.getCause()!=null ? failure.getCause() : failure;
              try {
                var queryOutput=error==null ? queryValue(text(op,"output"),encode(text(op,"output"),value,null),null) : JsonNull.INSTANCE;
                String state=error==null ? "retry" : "failure"; JsonElement message=null;
                for (var item:waiter.getAsJsonArray("acceptors")) {
                  var acceptor=item.getAsJsonObject(); var matcher=acceptor.getAsJsonObject("matcher");
                  String kind=text(matcher,"kind"); boolean matches;
                  if ("success".equals(kind)) matches=(error==null)==flag(matcher,"expected");
                  else if ("errorType".equals(kind)) { String expected=text(matcher,"name"); matches=error instanceof ServiceError service && (expected.equals(service.waiterErrorName()) || expected.substring(expected.lastIndexOf('#')+1).equals(service.waiterErrorName())); }
                  else if (error!=null) matches=false;
                  else {
                    JsonElement context=queryOutput;
                    if ("inputOutput".equals(kind)) { var pair=new JsonObject(); pair.add("input",queryInput); pair.add("output",queryOutput); context=pair; }
                    var found=Query.evaluate(matcher.getAsJsonObject("path").getAsJsonObject("expression"),context); String expected=text(matcher,"expected");
                    java.util.function.Predicate<JsonElement> stringMatch=v -> v.isJsonPrimitive() && v.getAsJsonPrimitive().isString() && v.getAsString().equals(expected);
                    matches=switch(text(matcher,"comparator")) {
                      case "stringEquals" -> stringMatch.test(found);
                      case "booleanEquals" -> found.isJsonPrimitive() && found.getAsJsonPrimitive().isBoolean() && found.getAsBoolean()==Boolean.parseBoolean(expected);
                      case "allStringEquals" -> found.isJsonArray() && !found.getAsJsonArray().isEmpty() && found.getAsJsonArray().asList().stream().allMatch(stringMatch);
                      case "anyStringEquals" -> found.isJsonArray() && found.getAsJsonArray().asList().stream().anyMatch(stringMatch);
                      default -> throw new IllegalArgumentException("Unknown waiter comparator");
                    };
                  }
                  if (matches) {
                    state=text(acceptor,"state");
                    if (acceptor.has("message") && acceptor.get("message").isJsonObject()) {
                      var candidate=Query.evaluate(acceptor.getAsJsonObject("message").getAsJsonObject("expression"),queryOutput);
                      java.util.function.Predicate<JsonElement> string=v -> v.isJsonPrimitive() && v.getAsJsonPrimitive().isString();
                      if (string.test(candidate) || candidate.isJsonArray() && candidate.getAsJsonArray().asList().stream().allMatch(string)) message=candidate;
                    }
                    break;
                  }
                }
                if ("success".equals(state)) { if (!result.complete(value)) closeStreams(text(op,"output"),value); return; }
                // Only the final response belongs to the caller.
                closeStreams(text(op,"output"),value);
                if ("failure".equals(state)) { result.completeExceptionally(new WaiterError(name,attempts,value,error,message)); return; }
                double ceiling=Math.min((double)high,Math.scalb((double)low,Math.min(attempts-1,62)));
                long delay=low+(long)(ThreadLocalRandom.current().nextDouble()*(ceiling-low));
                long remaining=timeout-(System.nanoTime()-started);
                // Leave time for the final feasible poll instead of sleeping through its deadline.
                long pause=remaining<=low ? Math.max(0,remaining) : remaining-delay<=low ? remaining-low : delay;
                result.schedule(this,pause);
              } catch (RuntimeException invalid) { result.completeExceptionally(invalid); }
            },WORKERS);
          } catch (RuntimeException failure) { result.completeExceptionally(failure); }
        }
      }
      var poll=new Poll();
      Future<?> deadline=TIMER.schedule(()->WORKERS.execute(()->result.completeExceptionally(new WaiterTimeout(name,poll.attempts,poll.response,poll.error))),timeout,TimeUnit.NANOSECONDS);
      result.whenComplete((value,error)->{ deadline.cancel(false); result.stop(); });
      WORKERS.execute(poll);
      return result;
    } catch (RuntimeException failure) { return CompletableFuture.failedFuture(failure); }
  }

  private static final class Call<T> extends CompletableFuture<T> {
    final AtomicReference<Future<?>> active = new AtomicReference<>();
    Future<?> retry;
    // A response stream outlives the call's future and keeps its deadline until it ends.
    volatile Future<?> deadline;
    volatile StreamBody stream;

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

  private void closeStreams(String target, Object value) {
    if (target == null || value == null) return;
    for (var member : shape(target).getAsJsonObject("members").entrySet())
      if (flag(shape(text(member.getValue().getAsJsonObject(), "target")), "streaming")
          && memberValue(value, member.getValue().getAsJsonObject()) instanceof ByteStream stream) stream.close();
  }

  private static void closeOneShot(ByteStream stream) {
    if (stream != null && !stream.replayable()) stream.close();
  }

  private <T> CompletableFuture<T> callValues(String name, JsonObject values, ByteStream stream, Class<T> output) {
    Prepared prepared;
    try { prepared = prepare(name, values, stream); }
    catch (RuntimeException failure) { closeOneShot(stream); throw failure; }
    var result = new Call<T>();
    long start = System.nanoTime();
    result.deadline =
        TIMER.schedule(
            () ->
                WORKERS.execute(
                    () -> {
                      var timeout = new HttpTimeoutException("request deadline exceeded");
                      if (result.completeExceptionally(timeout)) return;
                      StreamBody body = result.stream;
                      if (body != null) body.abort(timeout);
                    }),
            config.timeout.toNanos(),
            TimeUnit.NANOSECONDS);
    result.whenComplete(
        (value, error) -> {
          // A supplied one-shot source is owned for the whole call, including
          // failures and cancellation before its publisher is ever opened.
          closeOneShot(stream);
          if (result.stream == null) result.deadline.cancel(false);
          result.stop();
        });
    WORKERS.execute(() -> attempt(prepared, output, result, 1, start));
    return result;
  }

  private int attempts(Prepared prepared) {
    // A one-shot stream cannot be sent twice.
    return prepared.stream() != null && !prepared.stream().replayable() ? 1 : config.maxAttempts;
  }

  private <T> void attempt(
      Prepared prepared, Class<T> output, Call<T> result, int attempt, long start) {
    if (result.isDone()) return;
    ByteStream stream = prepared.stream();
    StreamBody upload = null;
    try {
      long remaining = config.timeout.toNanos() - (System.nanoTime() - start);
      if (remaining <= 0) throw new HttpTimeoutException("request deadline exceeded");
      Map<String, String> headers = new LinkedHashMap<>(prepared.headers());
      var request =
          new ClientConfig.Request(
              prepared.uri(),
              text(prepared.operation(), "method"),
              headers,
              prepared.body() == null ? null : prepared.body().clone(),
              stream);
      for (var middleware : config.middleware) middleware.apply(request);
      if (digest && stream != null) {
        // A stream is never buffered to hash it; the caller declares its digest.
        if (headers.keySet().stream().noneMatch(key -> key.equalsIgnoreCase("x-amz-content-sha256")))
          throw new IllegalArgumentException("streaming contentSha256 requires a precomputed ByteStream sha256");
      } else if (digest && ((request.body != null && request.body.length > 0)
          || Set.of("POST", "PUT", "PATCH").contains(request.method)))
        headers.put("x-amz-content-sha256", HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(request.body == null ? new byte[0] : request.body)));
      Checksums.request(prepared.checksum(),headers,request.body,stream != null);
      boolean credentials = Auth.apply(config, schema, prepared.operation(), request)
          || headers.keySet().stream().anyMatch(k -> k.equalsIgnoreCase("authorization"));
      String host = request.uri.getHost();
      if (credentials
          && !"https".equals(request.uri.getScheme())
          && !Set.of("localhost", "127.0.0.1", "[::1]", "::1").contains(host))
        throw new IllegalArgumentException(
            "refusing credentials over plaintext HTTP outside loopback");
      var builder = HttpRequest.newBuilder(request.uri).timeout(Duration.ofNanos(remaining));
      headers.forEach(builder::setHeader);
      HttpRequest.BodyPublisher body;
      if (stream != null) {
        JsonObject member = shape(text(prepared.operation(), "input")).getAsJsonObject("members")
            .getAsJsonObject(payloadMember(prepared.operation().getAsJsonObject("request")));
        long length = stream.length().orElse(-1);
        Long minimum = lengthBound(member, "min");
        if (length == 0 && minimum != null && minimum > 0)
          throw new IllegalArgumentException("stream ended before its declared length");
        if (result.isDone()) return;
        // Opened per attempt, after every check that could fail, so a one-shot body is not lost.
        upload = StreamBody.request(stream.open(), length < 0 ? null : length, minimum, lengthBound(member, "max"));
        if (length == 0) {
          body = HttpRequest.BodyPublishers.noBody();
        } else body = length > 0 ? HttpRequest.BodyPublishers.fromPublisher(upload, length) : HttpRequest.BodyPublishers.fromPublisher(upload);
      } else body = request.body == null
              ? HttpRequest.BodyPublishers.noBody()
              : HttpRequest.BodyPublishers.ofByteArray(request.body);
      builder.method(request.method, body);
      JsonObject bindings = prepared.operation().getAsJsonObject("response");
      boolean hasBody =
          !bindings.getAsJsonArray("body").isEmpty() || payloadMember(bindings) != null || prepared.checksum().validate();
      boolean streamingOutput = flag(bindings, "streaming") && payloadMember(bindings) != null;
      if (result.isDone()) {
        if (upload != null) upload.abort(new CancellationException("call cancelled"));
        return;
      }
      StreamBody sending = upload;
      CompletableFuture<Void> verified = sending != null && stream.length().orElse(-1) == 0
          ? verifyEmpty(sending, result) : CompletableFuture.completedFuture(null);
      verified.whenComplete((ignored, validationError) -> {
        if (validationError != null) {
          result.completeExceptionally(unwrap(validationError));
          return;
        }
        if (result.isDone()) return;
        long left = config.timeout.toNanos() - (System.nanoTime() - start);
        if (left <= 0) {
          result.completeExceptionally(new HttpTimeoutException("request deadline exceeded"));
          return;
        }
        try {
          builder.timeout(Duration.ofNanos(left));
          CompletableFuture<HttpResponse<Object>> sent =
              http.sendAsync(
                  builder.build(),
                  info -> {
                    boolean ok = info.statusCode() >= 200 && info.statusCode() < 300;
                    // A successful streaming response is handed over unread; errors are read whole.
                    if (ok && streamingOutput)
                      return HttpResponse.BodySubscribers.mapping(HttpResponse.BodySubscribers.ofPublisher(), value -> (Object) value);
                    return HttpResponse.BodySubscribers.mapping(
                        ok && !hasBody ? emptyBody() : HttpResponse.BodySubscribers.ofByteArray(), value -> (Object) value);
                  });
          result.track(sent);
          sent.whenComplete(
              (response, error) -> {
                if (result.isDone()) {
                  if (response != null && response.body() instanceof Flow.Publisher<?> unread) discard(unread);
                  return;
                }
                Throwable failure = unwrap(error);
                if (failure == null) {
                  try {
                    if (response.body() instanceof Flow.Publisher<?> received) {
                      streamed(prepared, output, result, response, received);
                      return;
                    }
                    byte[] bytes = (byte[]) response.body();
                    if(response.statusCode()>=200 && response.statusCode()<300) Checksums.validate(prepared.checksum(),response.headers(),bytes,config);
                    JsonObject decoded = response(prepared.name(), prepared.operation(), response, bytes);
                    result.complete(
                        output.cast(decode(text(prepared.operation(), "output"), decoded, null)));
                    return;
                  } catch (RuntimeException e) {
                    failure = e;
                  }
                }
                boolean safe = flag(prepared.operation(), "safe");
                boolean retry;
                if (sending != null && sending.failure() != null) {
                  // The caller's stream failed or broke its declared length: not a transport fault.
                  Throwable source = sending.failure();
                  failure = source instanceof IOException io ? new java.io.UncheckedIOException("streaming request body failed", io) : source;
                  retry = false;
                } else if (failure instanceof ServiceError modeled) retry = canRetry(safe, modeled);
                else if (failure instanceof IOException) {
                  retry = safe;
                  failure = new NetworkError(failure);
                } else retry = false;
                if (!retry || attempt >= attempts(prepared)) {
                  result.completeExceptionally(failure);
                  return;
                }
                long delay = delay(attempt, response);
                result.schedule(() -> attempt(prepared, output, result, attempt + 1, start), delay);
              });
        } catch (RuntimeException failure) {
          if (sending != null) sending.abort(failure);
          result.completeExceptionally(failure);
        }
      });
    } catch (Exception e) {
      if (upload != null) upload.abort(e);
      result.completeExceptionally(e);
    }
  }

  /** A zero-length BodyPublisher cannot be sent by HttpClient: validate its EOF first. */
  private static CompletableFuture<Void> verifyEmpty(StreamBody body, Call<?> result) {
    var verified = new CompletableFuture<Void>();
    result.whenComplete((value, error) -> {
      if (!verified.isDone()) body.abort(error == null ? new CancellationException("call cancelled") : unwrap(error));
    });
    body.subscribe(new Flow.Subscriber<java.nio.ByteBuffer>() {
      public void onSubscribe(Flow.Subscription subscription) { subscription.request(1); }
      public void onNext(java.nio.ByteBuffer bytes) {
        body.abort(new IllegalArgumentException("stream exceeds its declared length"));
      }
      public void onError(Throwable failure) {
        Throwable source = body.failure();
        verified.completeExceptionally(source instanceof IOException io
            ? new java.io.UncheckedIOException("streaming request body failed", io) : failure);
      }
      public void onComplete() { verified.complete(null); }
    });
    return verified;
  }

  @SuppressWarnings("unchecked")
  private static void discard(Flow.Publisher<?> body) {
    StreamBody.response((Flow.Publisher<List<java.nio.ByteBuffer>>) body, null, null, null, () -> {})
        .abort(new CancellationException("call cancelled"));
  }

  /** Completes a call whose successful response body is a stream the caller reads. */
  @SuppressWarnings("unchecked")
  private <T> void streamed(Prepared prepared, Class<T> output, Call<T> result, HttpResponse<?> response, Flow.Publisher<?> received) {
    JsonObject op = prepared.operation(), bindings = op.getAsJsonObject("response");
    String target = text(op, "output");
    JsonObject member = shape(target).getAsJsonObject("members").getAsJsonObject(payloadMember(bindings));
    StreamBody body;
    try {
      body = StreamBody.response((Flow.Publisher<List<java.nio.ByteBuffer>>) received,
          lengthBound(member, "min"), lengthBound(member, "max"),
          Checksums.running(prepared.checksum(), response.headers(), config),
          () -> result.deadline.cancel(false));
    } catch (RuntimeException invalid) {
      discard(received);
      throw invalid;
    }
    try {
      JsonObject values = new JsonObject();
      applyHeaders(values, target, bindings, response);
      Object decoded = decode(target, values, null);
      Field field = decoded.getClass().getField(text(member, "attribute"));
      var stream = ByteStream.response(body);
      field.set(decoded, field.getType() == OptionalValue.class ? OptionalValue.of(stream) : stream);
      result.stream = body;
      if (!result.complete(output.cast(decoded))) body.abort(new CancellationException("call cancelled"));
    } catch (RuntimeException | ReflectiveOperationException invalid) {
      body.abort(invalid);
      if (invalid instanceof RuntimeException runtime) throw runtime;
      throw new IllegalStateException(invalid);
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
      if (xml != null && (payload == null || "document".equals(text(op.getAsJsonObject("response"), "payloadKind")))) {
        List<String> members = new ArrayList<>();
        for (var member : op.getAsJsonObject("response").getAsJsonArray("body")) members.add(member.getAsString());
        values = xml.output(operation, text(op, "output"), members, payload, bytes);
      }
      // A payload is the body as received, whatever it holds.
      else if (payload != null) {
        JsonObject member = shape(text(op, "output")).getAsJsonObject("members").getAsJsonObject(payload);
        boolean absentBlob = xml != null && bytes.length == 0 && flag(member, "optional")
            && "blob".equals(text(shape(text(member, "target")), "kind"));
        if (!absentBlob)
          values.add(wireName(text(op, "output"), payload), payloadValue(op.getAsJsonObject("response"), bytes));
      }
      else if (!body.isEmpty()) values = JSON.fromJson(body, JsonObject.class);
      if (values == null) throw new IllegalArgumentException("response requires object");
      applyHeaders(
          values,
          text(op, "output"),
          op.getAsJsonObject("response"),
          response);
      return values;
    }
    XmlProtocol.ErrorEnvelope envelope = xml == null ? null : XmlProtocol.error(bytes);
    if (xml == null) try {
      JsonElement parsed = JSON.fromJson(body, JsonElement.class);
      if (parsed != null && parsed.isJsonObject()) values = parsed.getAsJsonObject();
    } catch (RuntimeException ignored) {
    }
    JsonElement named = values.get("__type");
    if (envelope != null) {
      named = envelope.code() == null ? null : new JsonPrimitive(envelope.code());
      if (envelope.message() != null) values.addProperty("Message", envelope.message());
      if (envelope.requestId() != null) values.addProperty("requestId", envelope.requestId());
    } else if(awsJson()) {
      String errorHeader=response.headers().firstValue("x-amzn-errortype").orElse(null);
      if(errorHeader!=null) named=new JsonPrimitive(errorHeader);
      if(named==null) named=values.get("code");
      if(named!=null && named.isJsonPrimitive() && named.getAsJsonPrimitive().isString()) { String name=named.getAsString().split(":",2)[0]; named=new JsonPrimitive(name.substring(name.indexOf('#')+1)); }
    } else if (named==null) named=response.headers().firstValue("x-crucible-error").map(JsonPrimitive::new).orElse(null);
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
    ServiceError error = new ServiceError();
    if (target != null) {
      try {
        JsonObject typedValues = envelope != null
            ? (envelope.error() == null ? new JsonObject() : xml.members(envelope.error(), target, shape(target).getAsJsonObject("members").keySet()))
            : values.deepCopy();
        String payload = payloadMember(traits);
        if (payload != null) typedValues.add(wireName(target, payload), payloadValue(traits, bytes));
        applyHeaders(typedValues, target, traits, response);
        error = (ServiceError) decode(target, typedValues, null);
        // Waiters match the model's shape name even when the protocol uses a custom wire code.
        error.setModeledErrorName(target.substring(target.indexOf('#') + 1));
      } catch (RuntimeException ignored) {
        // Malformed modeled members must not hide HTTP metadata or the retry policy.
      }
    }
    String message =
        values.has("message") && values.get("message").isJsonPrimitive()
            ? values.get("message").getAsString()
            : values.has("Message") && values.get("Message").isJsonPrimitive() ? values.get("Message").getAsString() : "HTTP " + status;
    JsonElement idValue = values.get("requestId");
    String fallback =
        idValue != null && idValue.isJsonPrimitive() && idValue.getAsJsonPrimitive().isString()
            ? idValue.getAsString()
            : null;
    String requestId = response.headers().firstValue("x-amzn-requestid").orElse(response.headers().firstValue("x-amz-request-id").orElse(response.headers().firstValue("x-request-id").orElse(fallback)));
    error.initialize(
        name == null ? "UnknownError" : name,
        message,
        status,
        requestId,
        response.headers(),
        bytes,
        flag(traits, "retryable"),
        flag(traits, "throttling"));
    throw error;
  }

  private void applyHeaders(
      JsonObject values, String target, JsonObject bindings, HttpResponse<?> response) {
    if (target == null) return;
    if (bindings.has("prefixHeaders")) for (var item : bindings.getAsJsonArray("prefixHeaders")) {
      var pair = item.getAsJsonArray(); String name = pair.get(0).getAsString(), prefix = pair.get(1).getAsString();
      var map = new JsonObject();
      JsonObject member = shape(target).getAsJsonObject("members").getAsJsonObject(name);
      JsonObject mapValue = shape(text(member, "target")).getAsJsonObject("value");
      String format=bindings.has("timestampFormats") ? text(bindings.getAsJsonObject("timestampFormats"),name) : null;
      for (var entry : response.headers().map().entrySet()) if (entry.getKey().toLowerCase(java.util.Locale.ROOT).startsWith(prefix.toLowerCase(java.util.Locale.ROOT)))
        map.add(entry.getKey().substring(prefix.length()), header(mapValue, String.join(", ", entry.getValue()),format));
      values.add(wireName(target, name), boundaryTime(member,headerEncoding(bindings, name, map, false),format,false));
    }
    String status = text(bindings, "responseCode");
    if (status != null) values.addProperty(wireName(target, status), response.statusCode());
    for (JsonElement item : bindings.getAsJsonArray("headers")) {
      var binding = item.getAsJsonArray();
      String member = binding.get(0).getAsString();
      List<String> headers = response.headers().allValues(binding.get(1).getAsString());
      if (!headers.isEmpty()) {
        var description=shape(target).getAsJsonObject("members").getAsJsonObject(member);
        String format=bindings.has("timestampFormats") ? text(bindings.getAsJsonObject("timestampFormats"),member) : null;
        values.add(wireName(target,member),boundaryTime(description,headerEncoding(bindings,member,header(description,String.join(", ",headers),format),false),format,false));
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
    ByteStream stream = requestStream(op, input);
    return subscriber -> {
      java.util.Objects.requireNonNull(subscriber, "subscriber");
      new PageSubscription<T>(subscriber, name, output, op, snapshot.deepCopy(), stream).start();
    };
  }

  /** Every subscriber owns its cursor and demand. Queue callbacks to avoid recursive request(1). */
  private final class PageSubscription<T> implements Flow.Subscription {
    private final Flow.Subscriber<? super T> subscriber;
    private final String name;
    private final Class<T> output;
    private final JsonObject op, pagination, values;
    private final ByteStream stream;
    private final Set<String> seen = new HashSet<>();
    private final ConcurrentLinkedQueue<Runnable> events = new ConcurrentLinkedQueue<>();
    private final AtomicInteger scheduled = new AtomicInteger();
    private final AtomicBoolean cancelled = new AtomicBoolean();
    private final AtomicReference<CompletableFuture<T>> active = new AtomicReference<>();
    private long demand;
    private boolean pending, terminal;

    PageSubscription(Flow.Subscriber<? super T> subscriber, String name, Class<T> output,
        JsonObject op, JsonObject values, ByteStream stream) {
      this.subscriber = subscriber; this.name = name; this.output = output; this.stream = stream;
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
      try { call = callValues(name, values, stream, output); }
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
    JsonObject values =
        inputValues(op, input);
    ByteStream stream = requestStream(op, input);
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
        T page = await(callValues(name, values, stream, output));
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
