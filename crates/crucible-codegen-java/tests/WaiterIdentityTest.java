package example.waiterquery;

import com.sun.net.httpserver.HttpServer;
import example.waiterquery.model.CustomCodeError;
import example.waiterquery.model.OtherError;
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

  static Reply xml(String code, String value) {
    return new Reply(400, "application/xml", "<ErrorResponse><Error><Type>Sender</Type><Code>" + code
        + "</Code><Message>wire detail</Message><value>" + value + "</value></Error><RequestId>identity</RequestId></ErrorResponse>");
  }

  static void plan(Reply... replies) {
    CALLS.set(0);
    REPLIES.set(List.of(replies));
  }

  static void calls(int expected) {
    check(FAILURE.get() == null, "loopback failure: " + FAILURE.get());
    check(CALLS.get() == expected, "expected " + expected + " requests, received " + CALLS.get());
  }

  static ServiceError queryError(QueryIdentityClient client, Reply reply) {
    plan(reply);
    try {
      client.queryRead();
      throw new AssertionError("expected query error");
    } catch (ServiceError error) {
      calls(1);
      check(error.status() == 400 && "identity".equals(error.requestId()), "HTTP error metadata");
      check(new String(error.body(), StandardCharsets.UTF_8).equals(reply.body()), "raw error body");
      return error;
    }
  }

  static WaiterError queryFailure(Runnable call) {
    try {
      call.run();
      throw new AssertionError("expected terminal waiter failure");
    } catch (WaiterError error) {
      calls(1);
      check(error.attempts() == 1, "unmatched errors fail on the first attempt");
      return error;
    }
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
      var query = new QueryIdentityClient(config);
      Reply success = new Reply(200, "application/xml", "");
      ServiceError modeled = queryError(query, xml("Customized", "7"));
      check(modeled instanceof CustomCodeError && "Customized".equals(modeled.errorName()), "typed custom wire code stays visible");
      plan(xml("Customized", "7"), success);
      query.waitUntilByAbsolute(OPTIONS);
      calls(2);
      plan(xml("Customized", "7"), success);
      query.waitUntilByRelative(OPTIONS);
      calls(2);

      ServiceError unknown = queryError(query, xml("UnmodeledWire", "7"));
      check(unknown.getClass() == QueryIdentityException.class && "UnmodeledWire".equals(unknown.errorName()), "unmodeled wire name");
      plan(xml("UnmodeledWire", "7"), success);
      query.waitUntilByUnknown(OPTIONS);
      calls(2);

      plan(xml("Customized", "7"));
      WaiterError byWire = queryFailure(() -> query.waitUntilByWire(OPTIONS));
      check(byWire.lastError() instanceof CustomCodeError && "Customized".equals(((ServiceError) byWire.lastError()).errorName()), "wire code is not a modeled waiter identity");

      ServiceError malformed = queryError(query, xml("Customized", "not-an-integer"));
      check(malformed.getClass() == QueryIdentityException.class && "Customized".equals(malformed.errorName()), "malformed modeled error retains generic wire metadata");
      plan(xml("Customized", "not-an-integer"), success);
      query.waitUntilByWire(OPTIONS);
      calls(2);
      plan(xml("Customized", "not-an-integer"));
      queryFailure(() -> query.waitUntilByAbsolute(OPTIONS));
      plan(xml("OtherError", "7"));
      check(queryFailure(() -> query.waitUntilByAbsolute(OPTIONS)).lastError() instanceof OtherError, "unrelated modeled error fails immediately");

      var jsonConfig = new example.waiterjson.ClientConfig();
      jsonConfig.endpoint = endpoint;
      jsonConfig.maxAttempts = 1;
      var json = new example.waiterjson.JsonIdentityClient(jsonConfig);
      Reply renamed = new Reply(400, "application/json", "{\"__type\":\"OriginalError\",\"value\":7}");
      plan(renamed);
      try {
        json.jsonRead();
        throw new AssertionError("expected renamed error");
      } catch (example.waiterjson.ServiceError error) {
        check(error instanceof example.waiterjson.model.RenamedError && "OriginalError".equals(error.errorName()), "renamed public class preserves original wire name");
        calls(1);
      }
      var jsonOptions = new example.waiterjson.WaiterOptions(Duration.ofSeconds(5), Duration.ZERO, Duration.ZERO);
      Reply jsonSuccess = new Reply(200, "application/json", "");
      plan(renamed, jsonSuccess);
      json.waitUntilByAbsolute(jsonOptions);
      calls(2);
      plan(renamed, jsonSuccess);
      json.waitUntilByRelative(jsonOptions);
      calls(2);
    } finally {
      server.stop(0);
    }
    System.out.println("Java generated waiter error identity passed over HTTP");
  }
}
