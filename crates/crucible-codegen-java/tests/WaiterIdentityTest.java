package example.waiterjson;

import com.sun.net.httpserver.HttpServer;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.List;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicReference;

/** Generated waiter consumers cross real HTTP and keep wire metadata distinct from model identity. */
public final class WaiterIdentityTest {
  record Reply(int status, String contentType, String body) {}

  static final AtomicInteger CALLS = new AtomicInteger();
  static final AtomicReference<List<Reply>> REPLIES = new AtomicReference<>();
  static final AtomicReference<Throwable> FAILURE = new AtomicReference<>();
  static final WaiterOptions OPTIONS = new WaiterOptions(Duration.ofSeconds(5), Duration.ZERO, Duration.ZERO);

  static void check(boolean value, String message) {
    if (!value) throw new AssertionError(message);
  }

  static void plan(Reply... replies) {
    CALLS.set(0);
    REPLIES.set(List.of(replies));
  }

  static void calls(int expected) {
    check(FAILURE.get() == null, "loopback failure: " + FAILURE.get());
    check(CALLS.get() == expected, "expected " + expected + " requests, received " + CALLS.get());
  }

  public static void main(String[] args) throws Exception {
    var server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
    server.createContext("/", exchange -> {
      try {
        exchange.getRequestBody().readAllBytes();
        int index = CALLS.getAndIncrement();
        List<Reply> replies = REPLIES.get();
        check(index < replies.size(), "unexpected extra HTTP request");
        Reply reply = replies.get(index);
        exchange.getResponseHeaders().add("content-type", reply.contentType());
        byte[] body = reply.body().getBytes(StandardCharsets.UTF_8);
        exchange.sendResponseHeaders(reply.status(), body.length == 0 ? -1 : body.length);
        if (body.length > 0) exchange.getResponseBody().write(body);
      } catch (Throwable failure) {
        FAILURE.compareAndSet(null, failure);
        exchange.sendResponseHeaders(599, -1);
      } finally {
        exchange.close();
      }
    });
    server.start();
    try {
      String endpoint = "http://127.0.0.1:" + server.getAddress().getPort();
      var config = new ClientConfig();
      config.endpoint = endpoint;
      config.maxAttempts = 1;
      var json = new JsonIdentityClient(config);
      Reply renamed = new Reply(400, "application/json", "{\"__type\":\"OriginalError\",\"value\":7}");
      plan(renamed);
      try {
        json.jsonRead();
        throw new AssertionError("expected renamed error");
      } catch (ServiceError error) {
        check(error instanceof example.waiterjson.model.RenamedError && "OriginalError".equals(error.errorName()), "renamed public class preserves original wire name");
        calls(1);
      }
      Reply jsonSuccess = new Reply(200, "application/json", "");
      plan(renamed, jsonSuccess);
      json.waitUntilByAbsolute(OPTIONS);
      calls(2);
      plan(renamed, jsonSuccess);
      json.waitUntilByRelative(OPTIONS);
      calls(2);
    } finally {
      server.stop(0);
    }
    System.out.println("Java generated waiter error identity passed over HTTP");
  }
}
