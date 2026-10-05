package example.dynamic;

import com.sun.net.httpserver.HttpServer;
import example.dynamic.model.Input;
import example.dynamic.model.Problem;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.Map;
import java.util.concurrent.atomic.AtomicInteger;

public final class DynamicTest {
  public static void main(String[] args) throws Exception {
    var calls = new AtomicInteger();
    var failure = new java.util.concurrent.atomic.AtomicReference<Throwable>();
    var server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
    server.createContext("/", exchange -> {
      try {
        int call = calls.incrementAndGet();
        if (!exchange.getRequestURI().getRawPath().equals("/files/folder/a%20b")) throw new AssertionError("greedy path");
        var query = exchange.getRequestURI().getRawQuery();
        if (!query.contains("tag=hello%20world") || !query.contains("tag=%2F") || !query.contains("empty=")) throw new AssertionError("query map");
        if (!"abc".equals(exchange.getRequestHeaders().getFirst("X-Meta-Trace"))) throw new AssertionError("prefix request");
        if (!new String(exchange.getRequestBody().readAllBytes(), StandardCharsets.UTF_8).equals("{\"wire-value\":\"hello\"}")) throw new AssertionError("jsonName");
        var body = (call == 1 ? "{\"wire-value\":\"result\"}" : "{\"__type\":\"Problem\"}").getBytes(StandardCharsets.UTF_8);
        exchange.getResponseHeaders().add(call == 1 ? "X-Meta-Trace" : "X-Meta-Reason", call == 1 ? "abc" : "invalid");
        exchange.sendResponseHeaders(call == 1 ? 206 : 422, body.length);
        exchange.getResponseBody().write(body);
      } catch (Throwable error) { failure.set(error); }
      finally { exchange.close(); }
    });
    server.start();
    try {
      var config = new ClientConfig(); config.endpoint = "http://127.0.0.1:" + server.getAddress().getPort(); config.maxAttempts=1;
      var client = new DynamicClient(config);
      var input = Input.builder().key("folder/a b").query(Map.of("tag", List.of("hello world", "/"), "empty", List.of(""))).metadata(Map.of("trace", "abc")).value("hello").build();
      var result = client.get(input);
      if (result.status() != 206 || !"result".equals(result.value()) || !"abc".equals(result.metadata().get("trace"))) throw new AssertionError("response bindings");
      try { client.get(input); throw new AssertionError("error not thrown"); }
      catch (Problem error) {
        if (error.statusValue() != 422 || error.status() != 422 || !"invalid".equals(error.details().get("reason"))) throw new AssertionError("error bindings");
      }
      for (String path : List.of("a/../b", "./a")) {
        try { client.get(Input.builder().key(path).build()); throw new AssertionError("dot path accepted"); }
        catch (IllegalArgumentException expected) {}
      }
      if (calls.get() != 2) throw new AssertionError("unexpected request");
      if (failure.get() != null) throw new AssertionError(failure.get());
    } finally { server.stop(0); }
    System.out.println("Java generated dynamic bindings passed");
  }
}
