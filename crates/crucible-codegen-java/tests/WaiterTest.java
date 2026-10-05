package example.wait;

import com.sun.net.httpserver.HttpServer;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.List;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicReference;

public final class WaiterTest {
  static void check(boolean value,String label) { if (!value) throw new AssertionError(label); }
  public static void main(String[] args) throws Exception {
    var count=new AtomicInteger(); var mode=new AtomicReference<>("ready");
    var entered=new CountDownLatch(1); var release=new CountDownLatch(1);
    var server=HttpServer.create(new InetSocketAddress("127.0.0.1",0),0);
    server.createContext("/",exchange->{
      try {
        exchange.getRequestBody().readAllBytes(); int attempt=count.incrementAndGet(); int status=200;
        String body=switch(mode.get()) {
          case "ready" -> attempt==1 ? "{\"__type\":\"Missing\"}" : attempt==2 ? "{\"wire-status\":\"PENDING\"}" : "{\"wire-status\":\"READY\"}";
          case "gone" -> "{\"__type\":\"Missing\"}";
          case "query" -> "{\"items\":[{\"wire-status\":\"READY\"},{\"wire-status\":\"READY\"}],\"updatedAt\":\"2026-01-01T00:00:00Z\"}";
          case "failed" -> "{\"wire-status\":\"FAILED\",\"reasons\":[\"job rejected\"]}";
          case "final-poll" -> { Thread.sleep(100); yield attempt==1 ? "{\"wire-status\":\"PENDING\"}" : "{\"wire-status\":\"READY\"}"; }
          case "blocked" -> { entered.countDown(); release.await(5,TimeUnit.SECONDS); yield "{\"wire-status\":\"PENDING\"}"; }
          default -> "{\"wire-status\":\"PENDING\"}";
        };
        if (body.contains("__type")) status=404;
        var bytes=body.getBytes(StandardCharsets.UTF_8); exchange.sendResponseHeaders(status,bytes.length); exchange.getResponseBody().write(bytes);
      } catch (Exception failure) { if (!mode.get().equals("blocked")) throw new RuntimeException(failure); }
      finally { exchange.close(); }
    });
    server.start();
    try {
      var config=new ClientConfig(); config.endpoint="http://127.0.0.1:"+server.getAddress().getPort(); config.maxAttempts=1;
      var client=new JobsClient(config); var options=new WaiterOptions(Duration.ofSeconds(3),Duration.ZERO,Duration.ZERO);
      var input=example.wait.model.Input.builder().names(List.of("a","b")).build();
      check(client.waitUntilReadyAsync(input,options).get(4,TimeUnit.SECONDS).status().equals("READY"),"async ready");
      check(count.get()==3,"retry error then pending"); mode.set("gone"); check(client.waitUntilDeleted(input,options)==null,"error success returns absent result"); mode.set("query");
      client.waitUntilAllReady(input,options); client.waitUntilAnyReady(input,options); client.waitUntilFilteredReady(input,options);
      client.waitUntilCountsMatch(options,b->b.names(List.of("a","b"))); client.waitUntilUpdated(input,options); client.waitUntilFunctions(input,options);
      mode.set("final-poll"); count.set(0);
      var finalPoll=new WaiterOptions(Duration.ofMillis(1500),Duration.ofSeconds(1),Duration.ofSeconds(1));
      check(client.waitUntilReadyAsync(input,finalPoll).get(3,TimeUnit.SECONDS).status().equals("READY") && count.get()==2,
          "final feasible poll reserves call time instead of sleeping until deadline");
      mode.set("failed");
      try { client.waitUntilReady(input,options); throw new AssertionError("expected failure"); }
      catch (WaiterError failure) { check(failure.attempts()==1 && failure.waiterMessage().toString().equals("[\"job rejected\"]"),"failure metadata"); }
      mode.set("pending");
      try { client.waitUntilReady(input,new WaiterOptions(Duration.ofMillis(50))); throw new AssertionError("expected timeout"); }
      catch (WaiterTimeout timeout) { check(timeout.attempts()>=1,"timeout attempts"); }
      mode.set("blocked"); var future=client.waitUntilReadyAsync(input,options);
      check(entered.await(2,TimeUnit.SECONDS),"operation entered"); int before=count.get(); check(future.cancel(true),"cancel waiter");
      release.countDown(); Thread.sleep(100); check(count.get()==before && future.isCancelled(),"no polling after cancellation");
      System.out.println("Java waiters passed: async, queries, modeled errors, deadline, cancellation");
    } finally { release.countDown(); server.stop(0); }
  }
}
