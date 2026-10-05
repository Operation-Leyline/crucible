package example.json;

import com.google.gson.JsonNull;
import com.google.gson.JsonParser;
import com.sun.net.httpserver.HttpServer;
import example.json.model.Choice;
import example.json.model.Problem;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.util.List;
import java.util.concurrent.atomic.AtomicReference;

/** Independent generated-package consumer: each protocol uses this same model. */
public final class ProtocolTest {
  static void check(boolean condition,String message) { if(!condition) throw new AssertionError(message); }
  public static void main(String[] args) throws Exception {
    boolean rest=args[0].equals("restJson1");
    String content=rest?"application/json":"application/x-amz-json-"+(args[0].equals("awsJson1_0")?"1.0":"1.1");
    var failure=new AtomicReference<Throwable>(); var next=new AtomicReference<String>("echo");
    var server=HttpServer.create(new InetSocketAddress("127.0.0.1",0),0);
    server.createContext("/", exchange -> {
      try {
        String mode=next.get(); String request=new String(exchange.getRequestBody().readAllBytes(),StandardCharsets.UTF_8);
        check(exchange.getRequestMethod().equals("POST"),"HTTP method");
        check(exchange.getRequestURI().getPath().equals(rest?(mode.equals("empty")?"/empty":"/echo"):"/"),"operation path");
        if(!rest) check(exchange.getRequestHeaders().getFirst("x-amz-target").equals("JsonService."+(mode.equals("empty")?"Empty":"Echo")),"RPC target");
        if(mode.equals("echo")) {
          check(exchange.getRequestHeaders().getFirst("content-type").startsWith(content),"protocol content type");
          var body=JsonParser.parseString(request).getAsJsonObject();
          check(body.get(rest?"wire-name":"renamed").getAsString().equals("value"),"jsonName policy");
          check(body.get("when").getAsBigDecimal().compareTo(new java.math.BigDecimal("-0.25"))==0,"epoch timestamp");
          check(body.get("iso").getAsString().equals("1969-12-31T23:59:59.750Z"),"explicit timestamp");
          check(!body.has("nullable") && body.has("document") && body.get("document").isJsonNull(),"structure null policy");
          check(body.get("floatValue").getAsString().equals("NaN") && body.get("doubleValue").getAsString().equals("Infinity"),"nonfinite numbers");
          if(rest) {
            check(!body.has("date") && !body.has("query"),"HTTP bound members excluded from body");
            check(exchange.getRequestHeaders().getFirst("x-date").equals("Thu, 01 Jan 1970 00:00:00 GMT"),"header timestamp");
            check(exchange.getRequestHeaders().getFirst("x-dates").equals("Thu, 01 Jan 1970 00:00:00 GMT, Thu, 01 Jan 1970 00:00:01 GMT"),"header timestamp list");
            check(exchange.getRequestURI().getRawQuery().contains("time=1970-01-01T00%3A00%3A00Z"),"query timestamp");
          } else check(body.get("date").getAsInt()==0 && body.getAsJsonArray("dates").size()==2,"RPC ignores HTTP binding");
        } else if(mode.equals("empty") && !rest) check(request.equals("{}"),"empty RPC JSON object");
        int status=200; String response="{\"choice\":{\"__type\":\"metadata\",\"time\":0.25},\"floatValue\":\"NaN\",\"doubleValue\":\"-Infinity\",\"nullable\":null,\"document\":null}";
        if(mode.equals("empty")) { response="{}"; status=rest?204:200; }
        if(mode.startsWith("error")) {
          status=409; response="{\"Message\":\"failure\"}";
          if(mode.equals("errorHeader")) exchange.getResponseHeaders().add("x-amzn-errortype","Problem:https://example.test");
          if(mode.equals("errorType")) response="{\"__type\":\"example.json#Problem\",\"Message\":\"failure\"}";
          if(mode.equals("errorCode")) response="{\"code\":\"Problem\",\"Message\":\"failure\"}";
          exchange.getResponseHeaders().add("x-amzn-requestid","request-123");
        }
        if(rest) { exchange.getResponseHeaders().add("x-date","Thu, 01 Jan 1970 00:00:00 GMT"); exchange.getResponseHeaders().add("x-dates","Thu, 01 Jan 1970 00:00:00 GMT, Thu, 01 Jan 1970 00:00:01 GMT"); }
        byte[] bytes=response.getBytes(StandardCharsets.UTF_8);exchange.sendResponseHeaders(status,status==204?-1:bytes.length);if(status!=204)exchange.getResponseBody().write(bytes);
      } catch(Throwable error) { failure.set(error); error.printStackTrace(); exchange.sendResponseHeaders(500,-1); }
      finally { exchange.close(); }
    });
    server.start();
    try {
      var config=new ClientConfig();config.endpoint="http://127.0.0.1:"+server.getAddress().getPort();config.maxAttempts=1;
      var client=new JsonServiceClient(config);var negative=Instant.ofEpochSecond(-1,750000000);var zero=Instant.EPOCH;
      var result=client.echo(b->b.renamed("value").when(negative).iso(negative).date(zero).dates(List.of(zero,zero.plusSeconds(1))).query(zero).queries(List.of(zero)).choice(Choice.builder().time(zero).build()).floatValue(Float.NaN).doubleValue(Double.POSITIVE_INFINITY).nullable(null).document(JsonNull.INSTANCE));
      check(result.when().equals(negative) && result.iso().equals(negative),"decoded timestamp defaults");
      check(result.choice().time().equals(Instant.ofEpochSecond(0,250000000)),"union ignores metadata");
      check(result.floatValue().isNaN() && result.doubleValue()==Double.NEGATIVE_INFINITY,"nonfinite response");
      check(!result.nullable.present() && result.document.present() && result.document()==null,"decoded null policy");
      if(rest) check(result.date().equals(zero) && result.dates().equals(List.of(zero,zero.plusSeconds(1))),"header timestamp decode");
      next.set("empty");client.empty();
      for(String mode:List.of("errorHeader","errorType","errorCode")) { next.set(mode);try {client.echo();throw new AssertionError("modeled error missing");}catch(Problem e){check(e.getMessage().equals("failure") && e.requestId().equals("request-123"),"error metadata");} }
      if(failure.get()!=null)throw new AssertionError(failure.get());
    } finally {server.stop(0);}
    System.out.println("Java generated protocol passed: "+args[0]);
  }
}


