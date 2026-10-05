package example.auth;

import com.sun.net.httpserver.HttpServer;
import java.net.InetSocketAddress;
import java.net.URI;
import java.time.Clock;
import java.time.Instant;
import java.time.ZoneOffset;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicReference;

public final class AuthTest {
  static void check(boolean ok,String message) { if (!ok) throw new AssertionError(message); }
  public static void main(String[] args) throws Exception {
    var received=new AtomicReference<com.sun.net.httpserver.Headers>();
    var server=HttpServer.create(new InetSocketAddress("127.0.0.1",0),0);
    server.createContext("/", exchange -> { received.set(exchange.getRequestHeaders()); exchange.sendResponseHeaders(204,-1); exchange.close(); });
    server.start();
    try {
      var config=new ClientConfig(); config.endpoint="http://127.0.0.1:"+server.getAddress().getPort();
      config.basicAuth=()->new BasicCredentials("user","secret"); config.apiKey=()-> { throw new AssertionError("Unrelated identity provider invoked"); };
      var client=new AuthClient(config); client.read();
      check(received.get().getFirst("authorization").equals("Basic dXNlcjpzZWNyZXQ="),"basic authentication");
      client.public_(); check(received.get().getFirst("authorization")==null,"anonymous identity invoked");
      config.basicAuth=null; config.apiKey=null; client=new AuthClient(config); client.optional();
      try { client.read(); throw new AssertionError("missing credentials accepted"); } catch (IllegalArgumentException expected) {}
      config.apiKey=()->"secret"; client=new AuthClient(config); client.read();
      check(received.get().getFirst("x-api-key").equals("ApiKey secret"),"API key header");
      config.apiKey=null; config.region="us-east-1"; config.credentials=()->new AwsCredentials("key","secret","session",null);
      config.middleware.add(request -> request.headers.put("x-after-hook","signed"));
      client=new AuthClient(config); client.read();
      check(received.get().getFirst("authorization").contains("x-after-hook"),"signing before middleware");
      check(received.get().getFirst("x-amz-security-token").equals("session"),"session credentials");
      config.endpoint="http://example.invalid";
      try { new AuthClient(config).read(); throw new AssertionError("plaintext credentials accepted"); } catch (IllegalArgumentException expected) {}
    } finally { server.stop(0); }
    var credentials=new AwsCredentials("AKIAIOSFODNN7EXAMPLE","wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY");
    var request=new ClientConfig.Request(URI.create("https://examplebucket.s3.amazonaws.com/test.txt"),"GET",new LinkedHashMap<>(java.util.Map.of("Range","bytes=0-9")),null);
    Auth.sign(request,credentials,"s3","us-east-1",Instant.parse("2013-05-24T00:00:00Z"),false);
    check(request.headers.get("authorization").endsWith("Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"),"published AWS S3 signature vector");
    check(!credentials.toString().contains(credentials.secretAccessKey()) && !new BasicCredentials("user","secret").toString().contains("secret"),"secret in diagnostics");
    var count=new AtomicInteger();
    var cached=new CachedCredentials(()->{ count.incrementAndGet(); try { Thread.sleep(5); } catch (InterruptedException error) { Thread.currentThread().interrupt(); } return credentials; });
    var pool=java.util.concurrent.Executors.newFixedThreadPool(8);
    try {
      var calls=new ArrayList<java.util.concurrent.Future<AwsCredentials>>();
      for (int i=0;i<30;i++) calls.add(pool.submit(cached::get));
      for (var call:calls) check(call.get()==credentials,"cached credentials not shared");
      check(count.get()==1,"parallel refresh not serialized");
    } finally { pool.shutdownNow(); }
    check(CredentialProviders.chain(()->null,cached).get()==credentials,"credential chain");
    var path=java.nio.file.Files.createTempFile("crucible-credentials-",".ini");
    try { java.nio.file.Files.writeString(path,"[test]\naws_access_key_id = profile\naws_secret_access_key = secret%text\n"); check(CredentialProviders.profile(path,"test").get().secretAccessKey().equals("secret%text"),"credential profile"); }
    finally { java.nio.file.Files.delete(path); }
    var now=Instant.parse("2026-01-01T00:00:00Z");
    try { new CachedCredentials(()->new AwsCredentials("key","secret",null,now),java.time.Duration.ZERO,Clock.fixed(now,ZoneOffset.UTC)).get(); throw new AssertionError("expired credentials accepted"); }
    catch (IllegalArgumentException expected) {}
    System.out.println("Java generated authentication, AWS signing, and credential providers passed");
  }
}
