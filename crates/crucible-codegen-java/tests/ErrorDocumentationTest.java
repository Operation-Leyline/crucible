package example.error_docs;

import com.sun.net.httpserver.HttpServer;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.concurrent.CompletionException;
import java.util.concurrent.atomic.AtomicReference;

/** A separate consumer of the packaged SDK, over native JDK HTTP. */
public final class ErrorDocumentationTest {
  private record Case(int status, String identity, String body, String header, String page, boolean typed) {}
  private static final String BASE = "https://docs.example.test/errors/";
  private static Case denied(String reason, String suffix) {
    return new Case(403, "Denied", "{\"message\":\"denied\",\"detail\":" + reason + "}", null,
        "Denied" + suffix, true);
  }
  public static void main(String[] args) throws Exception {
    var cases = List.of(
        new Case(419, "DefaultReason", "{\"message\":\"denied\"}", null, "DefaultReason/reason-access%2Fneeded", true),
        new Case(420, "DefaultEmptyReason", "{\"message\":\"denied\"}", null, "DefaultEmptyReason/reason-", true),
        new Case(419, "DefaultReason", "{\"message\":\"denied\",\"detail\":null}", null, "DefaultReason", false),
        new Case(419, "DefaultReason", "{\"message\":\"denied\",\"detail\":\"future\"}", null, "DefaultReason", true),
        new Case(419, "DefaultReason", "{invalid", null, "DefaultReason", false),
        new Case(419, "DefaultReason", "42", null, "DefaultReason", false),
        new Case(419, "DefaultReason", "[]", null, "DefaultReason", false),
        new Case(419, "DefaultReason", "null", null, "DefaultReason", false),
        new Case(419, "DefaultReason", "{}", null, "DefaultReason/reason-access%2Fneeded", true),
        new Case(420, "DefaultEmptyReason", "{invalid", null, "DefaultEmptyReason", false),
        new Case(420, "DefaultEmptyReason", "42", null, "DefaultEmptyReason", false),
        new Case(420, "DefaultEmptyReason", "[]", null, "DefaultEmptyReason", false),
        new Case(420, "DefaultEmptyReason", "null", null, "DefaultEmptyReason", false),
        new Case(420, "DefaultEmptyReason", "{}", null, "DefaultEmptyReason/reason-", true),
        new Case(419, "DefaultReason", "", null, "DefaultReason/reason-access%2Fneeded", true),
        new Case(420, "DefaultEmptyReason", "", null, "DefaultEmptyReason/reason-", true),
        new Case(421, "BlobPayloadError", "not-json /blob", "access/needed", "BlobPayloadError/reason-access%2Fneeded", true),
        new Case(421, "BlobPayloadError", "", "", "BlobPayloadError/reason-", true),
        new Case(419, "DefaultReason", " \t\r\n ", null, "DefaultReason/reason-access%2Fneeded", true),
        new Case(419, "DefaultReason", "\u000b", null, "DefaultReason", false),
        new Case(419, "DefaultReason", "\u00a0", null, "DefaultReason", false),
        new Case(420, "DefaultEmptyReason", " \t\r\n ", null, "DefaultEmptyReason/reason-", true),
        new Case(420, "DefaultEmptyReason", "\u000b", null, "DefaultEmptyReason", false),
        new Case(420, "DefaultEmptyReason", "\u00a0", null, "DefaultEmptyReason", false),
        new Case(403, "Denied", "{\"message\":\"denied\",\"detail\":\"access/needed\",\"extra\":NaN}", null, "Denied", false),
        new Case(403, "Denied", "{\"message\":\"denied\",\"detail\":\"access/needed\",\"extra\":Infinity}", null, "Denied", false),
        new Case(403, "Denied", "{\"message\":\"denied\",\"detail\":\"access/needed\",\"extra\":-Infinity}", null, "Denied", false),
        denied("\"access/needed\"", "/reason-access%2Fneeded"),
        denied("\"\"", "/reason-"), denied("\"..\"", "/reason-%2E%2E"),
        denied("\"caf\u00e9\"", "/reason-caf%C3%A9"), denied("null", ""),
        denied("\"future\"", ""), denied("\"secret\"", ""),
        new Case(403, "Denied", "{\"message\":\"denied\"}", null, "Denied", true),
        new Case(418, "MappedReasonError", "{\"message\":\"denied\",\"failureReason\":\"access/needed\"}", null,
            "MappedReasonError/reason-access%2Fneeded", true),
        new Case(409, "HeaderError", "{\"message\":\"denied\",\"reason\":\"future\"}", "access/needed",
            "HeaderError/reason-access%2Fneeded", true),
        new Case(409, "HeaderError", "{\"message\":\"denied\"}", null, "HeaderError", true),
        new Case(409, "HeaderError", "{\"message\":\"denied\"}", "", "HeaderError/reason-", true),
        new Case(422, "PayloadError", "access/needed", null, "PayloadError/reason-access%2Fneeded", true),
        new Case(422, "PayloadError", "", null, "PayloadError/reason-", true),
        new Case(422, "PayloadError", "future", null, "PayloadError", true),
        new Case(400, "PlainError", "{\"message\":\"denied\",\"reason\":\"access/needed\"}", null, "PlainError", true),
        new Case(402, "RequiredReason", "{\"reason\":\"access/needed\"}", null,
            "RequiredReason/reason-access%2Fneeded", true),
        new Case(402, "RequiredReason", "{}", null, "RequiredReason", false),
        new Case(402, "RequiredReason", "{\"reason\":null}", null, "RequiredReason", false),
        new Case(403, "Denied", "{\"detail\":\"access/needed\"}", null, "Denied", false),
        new Case(403, "Denied", "{\"message\":\"denied\",\"detail\":{}}", null, "Denied", false),
        new Case(403, "UnknownFuture", "{\"message\":\"denied\",\"detail\":\"access/needed\"}", null, null, false),
        new Case(403, null, "{\"message\":\"denied\",\"detail\":\"access/needed\"}", null,
            "Denied/reason-access%2Fneeded", true),
        new Case(599, null, "{}", null, null, false),
        new Case(403, "Denied", "{\"message\":\"denied\",\"docUrl\":\"https://evil.test/\"}", null, "Denied", true));
    var selected = new AtomicReference<Case>();
    var failure = new AtomicReference<Throwable>();
    var server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
    server.createContext("/inspect", exchange -> {
      try {
        if (!exchange.getRequestMethod().equals("GET")) throw new AssertionError("GET request");
        var test = selected.get();
        if (test.identity != null) exchange.getResponseHeaders().add("x-crucible-error", test.identity);
        if (test.header != null) exchange.getResponseHeaders().add("X-Error-Reason", test.header);
        exchange.getResponseHeaders().add("x-request-id", "documentation-consumer");
        var bytes = test.body.getBytes(StandardCharsets.UTF_8);
        exchange.sendResponseHeaders(test.status, bytes.length);
        exchange.getResponseBody().write(bytes);
      } catch (Throwable error) { failure.set(error); }
      finally { exchange.close(); }
    });
    server.start();
    try {
      var config = new ClientConfig();
      config.endpoint = "http://127.0.0.1:" + server.getAddress().getPort();
      config.maxAttempts = 1;
      var client = new TroubleshootingClient(config);
      for (var test : cases) for (boolean async : List.of(false, true)) {
        selected.set(test);
        ServiceError actual;
        try {
          if (async) client.inspectAsync().join(); else client.inspect();
          throw new AssertionError("expected service error");
        } catch (CompletionException error) {
          if (!(error.getCause() instanceof ServiceError)) throw error;
          actual = (ServiceError) error.getCause();
        } catch (ServiceError error) { actual = error; }
        var expected = test.page == null ? null : BASE + test.page;
        if (!java.util.Objects.equals(actual.docUrl(), expected))
          throw new AssertionError(test + " async=" + async + " URL=" + actual.docUrl());
        boolean typed = !actual.getClass().equals(TroubleshootingException.class);
        if (typed != test.typed || actual.status() != test.status
            || !"documentation-consumer".equals(actual.requestId()))
          throw new AssertionError(test + " type/metadata=" + actual.getClass());
      }
      if (failure.get() != null) throw new AssertionError(failure.get());
    } finally { server.stop(0); }
    System.out.println("Java error documentation: " + cases.size() * 2 + " real HTTP cases passed");
  }
}
