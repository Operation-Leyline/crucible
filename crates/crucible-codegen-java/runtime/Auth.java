import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.security.GeneralSecurityException;
import java.security.MessageDigest;
import java.time.Instant;
import java.time.ZoneOffset;
import java.time.format.DateTimeFormatter;
import java.util.ArrayList;
import java.util.Base64;
import java.util.HexFormat;
import java.util.Locale;
import java.util.Set;
import java.util.TreeMap;
import javax.crypto.Mac;
import javax.crypto.spec.SecretKeySpec;

final class Auth {
  private Auth() {}
  private static String text(JsonObject object, String key) { return ClientRuntime.text(object,key); }
  static boolean apply(ClientConfig config, JsonObject schema, JsonObject op, ClientConfig.Request request) {
    JsonArray options=op.getAsJsonArray("auth");
    if (options==null) { options=new JsonArray(); if (op.has("bearer") && op.get("bearer").getAsBoolean()) options.add("smithy.api#httpBearerAuth"); }
    for (var option:options) {
      String id=option.getAsString();
      JsonObject scheme=schema.has("authSchemes") ? schema.getAsJsonObject("authSchemes").getAsJsonObject(id) : null;
      String kind=scheme==null && id.equals("smithy.api#httpBearerAuth") ? "bearer" : scheme==null ? null : text(scheme,"kind");
      if (kind==null) throw new IllegalArgumentException("Unknown authentication scheme "+id);
      var endpoint=op.getAsJsonObject("resolvedEndpoint");
      JsonArray endpointAuth=endpoint==null ? null : endpoint.getAsJsonObject("properties").getAsJsonArray("authSchemes");
      if(endpointAuth!=null) {
        JsonObject selected=null;
        for(var item:endpointAuth) if(kind.equalsIgnoreCase(text(item.getAsJsonObject(),"name"))) { selected=item.getAsJsonObject(); break; }
        if(selected==null) continue;
        scheme=scheme==null ? new JsonObject() : scheme.deepCopy();
        for(var entry:selected.entrySet()) if(!entry.getKey().equals("name")) scheme.add(entry.getKey(),entry.getValue());
      }
      Object identity=switch(kind) {
        case "bearer" -> config.token==null ? null : config.token.get();
        case "basic" -> config.basicAuth==null ? null : config.basicAuth.get();
        case "apiKey" -> config.apiKey==null ? null : config.apiKey.get();
        case "sigV4" -> config.credentials==null ? null : config.credentials.get();
        default -> throw new IllegalArgumentException("Unsupported authentication scheme "+kind);
      };
      if (identity==null) continue;
      switch(kind) {
        case "bearer" -> set(request,"authorization","Bearer "+identity);
        case "basic" -> { var basic=(BasicCredentials)identity; set(request,"authorization","Basic "+Base64.getEncoder().encodeToString((basic.username()+":"+basic.password()).getBytes(StandardCharsets.UTF_8))); }
        case "apiKey" -> {
          String key=(String)identity; if (key.isEmpty()) throw new IllegalArgumentException("Empty API key");
          if ("query".equals(text(scheme,"location"))) {
            String name=text(scheme,"name"), query=request.uri.getRawQuery(); var parts=new ArrayList<String>();
            if (query!=null) for (String part:query.split("&")) if (!decode(part.split("=",2)[0]).equals(name)) parts.add(part);
            parts.add(escape(name)+"="+escape(key));
            request.uri=URI.create(request.uri.toString().split("\\?",2)[0]+"?"+String.join("&",parts));
          } else set(request,text(scheme,"name"),(text(scheme,"scheme")==null ? "" : text(scheme,"scheme")+" ")+key);
        }
        case "sigV4" -> sign(request,(AwsCredentials)identity,text(scheme,"signingName"),text(scheme,"signingRegion")==null ? config.region : text(scheme,"signingRegion"),config.clock.instant(),op.has("unsignedPayload") && op.get("unsignedPayload").getAsBoolean(),scheme.has("disableDoubleEncoding") ? scheme.get("disableDoubleEncoding").getAsBoolean() : null,scheme.has("disableNormalizePath") ? scheme.get("disableNormalizePath").getAsBoolean() : null);
        default -> throw new IllegalArgumentException("Unsupported authentication scheme");
      }
      return true;
    }
    if (options.asList().stream().anyMatch(value -> !value.getAsString().equals("smithy.api#httpBearerAuth")) && !(op.has("optionalAuth") && op.get("optionalAuth").getAsBoolean()))
      throw new IllegalArgumentException("No credentials available for operation authentication schemes");
    return false;
  }
  private static void set(ClientConfig.Request request,String name,String value) {
    request.headers.keySet().removeIf(key -> key.equalsIgnoreCase(name)); request.headers.put(name,value);
  }
  private static String escape(String text) { return java.net.URLEncoder.encode(text,StandardCharsets.UTF_8).replace("+","%20").replace("*","%2A").replace("%7E","~"); }
  private static String decode(String text) { return java.net.URLDecoder.decode(text.replace("+","%2B"),StandardCharsets.UTF_8); }
  // A stream is signed with its declared digest; it is never buffered to compute one.
  private static String streamDigest(ClientConfig.Request request) {
    return request.headers.entrySet().stream().filter(entry -> entry.getKey().equalsIgnoreCase("x-amz-content-sha256")).map(java.util.Map.Entry::getValue).findFirst()
        .orElseThrow(() -> new IllegalArgumentException("SigV4 streaming payload requires a precomputed ByteStream sha256"));
  }
  private static String hash(byte[] bytes) throws GeneralSecurityException { return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes)); }
  private static byte[] hmac(byte[] key,String value) throws GeneralSecurityException {
    var mac=Mac.getInstance("HmacSHA256"); mac.init(new SecretKeySpec(key,"HmacSHA256")); return mac.doFinal(value.getBytes(StandardCharsets.UTF_8));
  }
  static void sign(ClientConfig.Request request,AwsCredentials credentials,String service,String region,Instant now,boolean unsigned) {
    sign(request,credentials,service,region,now,unsigned,null,null);
  }
  private static void sign(ClientConfig.Request request,AwsCredentials credentials,String service,String region,Instant now,boolean unsigned,Boolean disableDoubleEncoding,Boolean disableNormalizePath) {
    if (service==null || region==null || service.isEmpty() || region.isEmpty() || (service+region).matches("(?s).*[\\r\\n/].*")) throw new IllegalArgumentException("SigV4 requires valid service and region");
    if (credentials.expiresAt()!=null && !credentials.expiresAt().isAfter(now)) throw new IllegalArgumentException("AWS credentials expired");
    try {
      String date=DateTimeFormatter.ofPattern("yyyyMMdd",Locale.ROOT).withZone(ZoneOffset.UTC).format(now);
      String timestamp=DateTimeFormatter.ofPattern("yyyyMMdd'T'HHmmss'Z'",Locale.ROOT).withZone(ZoneOffset.UTC).format(now);
      set(request,"x-amz-date",timestamp);
      request.headers.keySet().removeIf(key -> key.equalsIgnoreCase("authorization") || key.equalsIgnoreCase("x-amz-security-token"));
      if (credentials.sessionToken()!=null) set(request,"x-amz-security-token",credentials.sessionToken());
      String digest=unsigned ? "UNSIGNED-PAYLOAD" : request.stream!=null ? streamDigest(request) : hash(request.body==null ? new byte[0] : request.body);
      set(request,"x-amz-content-sha256",digest);
      String path=request.uri.getRawPath(); if (path==null || path.isEmpty()) path="/";
      if (!(disableNormalizePath==null ? service.equals("s3") : disableNormalizePath)) {
        var segments=new ArrayList<String>();
        for (String part:path.split("/")) { if (part.equals("..")) { if (!segments.isEmpty()) segments.remove(segments.size()-1); } else if (!part.isEmpty() && !part.equals(".")) segments.add(part); }
        String normalized="/"+String.join("/",segments); if (path.endsWith("/") && !normalized.endsWith("/")) normalized+="/";
        path=normalized;
      }
      if (!(disableDoubleEncoding==null ? service.equals("s3") : disableDoubleEncoding)) path=escape(path).replace("%2F","/");
      var query=new ArrayList<String>();
      if (request.uri.getRawQuery()!=null) for (String part:request.uri.getRawQuery().split("&")) if (!part.isEmpty()) {
        String[] pair=part.split("=",2); query.add(escape(decode(pair[0]))+"="+escape(decode(pair.length==2 ? pair[1] : "")));
      }
      query.sort(java.util.Comparator.comparing((String item) -> item.substring(0,item.indexOf('='))).thenComparing(item -> item.substring(item.indexOf('=')+1)));
      var headers=new TreeMap<String,String>();
      Set<String> excluded=Set.of("authorization","connection","expect","transfer-encoding","user-agent","x-amzn-trace-id","accept-encoding","content-length");
      request.headers.forEach((key,value) -> { String name=key.toLowerCase(Locale.ROOT); if (!excluded.contains(name)) headers.merge(name,value.strip().replaceAll("\\s+"," "),(a,b)->a+","+b); });
      int port=request.uri.getPort();
      headers.put("host",request.uri.getHost().toLowerCase(Locale.ROOT)+(port==-1 || port==443 && request.uri.getScheme().equals("https") || port==80 && request.uri.getScheme().equals("http") ? "" : ":"+port));
      String signed=String.join(";",headers.keySet()); var canonicalHeaders=new StringBuilder(); headers.forEach((key,value)->canonicalHeaders.append(key).append(':').append(value).append('\n'));
      String canonical=String.join("\n",request.method,path,String.join("&",query),canonicalHeaders.toString(),signed,digest);
      String scope=date+"/"+region+"/"+service+"/aws4_request";
      String toSign="AWS4-HMAC-SHA256\n"+timestamp+"\n"+scope+"\n"+hash(canonical.getBytes(StandardCharsets.UTF_8));
      byte[] key=("AWS4"+credentials.secretAccessKey()).getBytes(StandardCharsets.UTF_8);
      for (String component:new String[]{date,region,service,"aws4_request"}) key=hmac(key,component);
      set(request,"authorization","AWS4-HMAC-SHA256 Credential="+credentials.accessKeyId()+"/"+scope+", SignedHeaders="+signed+", Signature="+HexFormat.of().formatHex(hmac(key,toSign)));
    } catch (GeneralSecurityException error) { throw new IllegalStateException(error); }
  }
}
