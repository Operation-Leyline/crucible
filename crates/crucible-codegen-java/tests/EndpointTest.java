package example.endpoint;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.concurrent.CompletableFuture;

public final class EndpointTest {
  static void check(boolean condition,String message) { if(!condition) throw new AssertionError(message); }
  static final class Transport extends HttpClient {
    HttpRequest request; int count;
    public Optional<java.net.CookieHandler> cookieHandler(){ return Optional.empty(); }
    public Optional<java.time.Duration> connectTimeout(){ return Optional.empty(); }
    public Redirect followRedirects(){ return Redirect.NEVER; }
    public Optional<java.net.ProxySelector> proxy(){ return Optional.empty(); }
    public javax.net.ssl.SSLContext sslContext(){ try { return javax.net.ssl.SSLContext.getDefault(); } catch(Exception e){ throw new RuntimeException(e); } }
    public javax.net.ssl.SSLParameters sslParameters(){ return new javax.net.ssl.SSLParameters(); }
    public Optional<java.net.Authenticator> authenticator(){ return Optional.empty(); }
    public Version version(){ return Version.HTTP_1_1; }
    public Optional<java.util.concurrent.Executor> executor(){ return Optional.empty(); }
    public <T> HttpResponse<T> send(HttpRequest req,HttpResponse.BodyHandler<T> handler){ throw new UnsupportedOperationException(); }
    public <T> CompletableFuture<HttpResponse<T>> sendAsync(HttpRequest req,HttpResponse.BodyHandler<T> handler,HttpResponse.PushPromiseHandler<T> push){ return sendAsync(req,handler); }
    public <T> CompletableFuture<HttpResponse<T>> sendAsync(HttpRequest req,HttpResponse.BodyHandler<T> handler){
      request=req;count++;
      var response=new HttpResponse<T>() {
        public int statusCode(){ return 200; }
        public HttpRequest request(){ return req; }
        public Optional<HttpResponse<T>> previousResponse(){ return Optional.empty(); }
        public java.net.http.HttpHeaders headers(){ return java.net.http.HttpHeaders.of(Map.of(),(a,b)->true); }
        @SuppressWarnings("unchecked") public T body(){ return (T)"{\"wire-status\":\"READY\"}".getBytes(java.nio.charset.StandardCharsets.UTF_8); }
        public Optional<javax.net.ssl.SSLSession> sslSession(){ return Optional.empty(); }
        public URI uri(){ return req.uri(); }
        public Version version(){ return Version.HTTP_1_1; }
      };
      return CompletableFuture.completedFuture(response);
    }
  }
  public static void main(String[] args) {
    var transport=new Transport(); var config=new ClientConfig(); config.httpClient=transport; config.region="ignored";
    config.endpointBuiltins.put("AWS::Region","local"); config.credentials=()->new AwsCredentials("key","secret");
    config.token=()->{ throw new AssertionError("Endpoint-excluded bearer provider invoked"); };
    var client=new ServiceClient(config);
    check(client.read(b->b.account("customer")).status().equals("READY"),"decoded response");
    check(transport.request.uri().getHost().equals("customer.local.example.test"),"host prefix");
    check(transport.request.headers().firstValue("x-region").orElseThrow().equals("local"),"endpoint header");
    check(transport.request.headers().firstValue("authorization").orElseThrow().contains("/local/example/aws4_request"),"endpoint signing overrides");
    check(transport.request.headers().firstValue("x-amz-content-sha256").orElseThrow().equals("UNSIGNED-PAYLOAD"),"unsigned payload");
    for(String invalid:List.of("a.b","-bad","x/y","x@evil","","x".repeat(64))) {
      try { client.read(b->b.account(invalid)); throw new AssertionError("invalid host label accepted"); } catch(IllegalArgumentException expected) {}
    }
    check(transport.count==1,"invalid host labels reached transport");
    config.disableHostPrefix=true; new ServiceClient(config).read(b->b.account("ignored")); check(transport.request.uri().getHost().equals("local.example.test"),"disabled prefix");
    config.endpointBuiltins.put("AWS::Region","missing"); try { new ServiceClient(config).read(b->b.account("customer")); throw new AssertionError("error endpoint accepted"); } catch(IllegalArgumentException expected) { check(expected.getMessage().contains("Unknown region missing"),"endpoint error message"); }
    check(Endpoints.parseUrl("https://example.test:443/a").get("authority").getAsString().equals("example.test"),"default port");
    check(Endpoints.parseUrl("https://[::1]:8443").get("isIp").getAsBoolean(),"IPv6");
    for(String invalid:List.of("file:///path","https://user:password@example.test","https://example.test?q=x","https://example.test#x","https://example.test:99999","https://evil\n.example")) check(Endpoints.parseUrl(invalid)==null,"invalid endpoint "+invalid);
    var rules=JsonParser.parseString("""
      {"parameters":{"Name":{"kind":"string","required":true},"Flag":{"kind":"boolean","required":true}},"rules":[{"conditions":[{"function":"isSet","arguments":[{"kind":"reference","name":"Flag"}],"assign":null}],"result":{"kind":"endpoint","url":{"kind":"string","parts":[{"kind":"literal","value":"https://example.test/base"}]},"headers":{"name":[{"kind":"reference","name":"Name"}]},"properties":{"flag":{"kind":"reference","name":"Flag"}}}}]}
      """).getAsJsonObject();
    var sources=JsonParser.parseString("""
      {"Name":[{"kind":"inputMember","member":"name"},{"kind":"client","name":"name"},{"kind":"default","value":"default"}],"Flag":[{"kind":"builtIn","name":"Flag"}]}
      """).getAsJsonObject();
    var input=new JsonObject();input.addProperty("name","input");var params=new JsonObject();params.addProperty("name","client");var builtins=new JsonObject();builtins.addProperty("Flag",false);
    for(String expected:List.of("input","client","default")) {
      var result=Endpoints.resolve(rules,sources,input,params,builtins); check(result.getAsJsonObject("headers").getAsJsonArray("name").get(0).getAsString().equals(expected),"parameter precedence"); check(!result.getAsJsonObject("properties").get("flag").getAsBoolean(),"false is present"); if(expected.equals("input")) input.remove("name"); else params.remove("name");
    }
    try { Endpoints.resolve(rules,sources,input,params,new JsonObject()); throw new AssertionError("missing required parameter"); } catch(IllegalArgumentException expected) {}
    builtins.addProperty("Flag",0); try { Endpoints.resolve(rules,sources,input,params,builtins); throw new AssertionError("wrong parameter type"); } catch(IllegalArgumentException expected) {}
    System.out.println("Java endpoints passed: functions, host labels, source precedence, signing overrides");
  }
}
