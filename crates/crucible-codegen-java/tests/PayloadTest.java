package example.wire;

import com.sun.net.httpserver.HttpServer;
import example.wire.model.TextError;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.util.Base64;
import java.util.List;
import java.util.concurrent.atomic.AtomicReference;

public final class PayloadTest {
  static void check(boolean ok, String message) { if (!ok) throw new AssertionError(message); }
  public static void main(String[] args) throws Exception {
    var metadata = List.of("{\"x\":1}", "é, two");
    String encoded = String.join(", ", metadata.stream().map(s -> Base64.getEncoder().encodeToString(s.getBytes(StandardCharsets.UTF_8))).toList());
    var failure = new AtomicReference<Throwable>();
    var server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
    server.createContext("/", exchange -> {
      try {
        byte[] body = exchange.getRequestBody().readAllBytes();
        String text = new String(body, StandardCharsets.UTF_8);
        int status = 200;
        if (exchange.getRequestURI().getPath().equals("/text")) {
          check(exchange.getRequestHeaders().getFirst("Content-Type").startsWith("text/plain"), "text content type");
          if (text.equals("fail")) { body = "invalid text".getBytes(StandardCharsets.UTF_8); status=422; }
          else {
            check(encoded.equals(exchange.getRequestHeaders().getFirst("x-meta")), "media header encoding");
            exchange.getResponseHeaders().add("x-meta", encoded);
            check(Base64.getEncoder().encodeToString("prefix é".getBytes(StandardCharsets.UTF_8)).equals(exchange.getRequestHeaders().getFirst("x-extra-value")), "media prefix header encoding");
            exchange.getResponseHeaders().add("x-extra-value", exchange.getRequestHeaders().getFirst("x-extra-value"));
          }
        } else {
          check(text.equals("{\"name\":\"record\"}"), "structure payload");
          check(exchange.getRequestHeaders().getFirst("Content-Type").startsWith("application/json"), "JSON content type");
        }
        exchange.sendResponseHeaders(status, body.length == 0 ? -1 : body.length);
        exchange.getResponseBody().write(body);
      } catch (Throwable error) { failure.set(error); }
      finally { exchange.close(); }
    });
    server.start();
    try {
      var config = new ClientConfig(); config.endpoint = "http://127.0.0.1:"+server.getAddress().getPort(); config.maxAttempts=1;
      var client = new WireClient(config);
      String text = "quoted \"é\"\n";
      var result = client.text(b -> b.text(text).meta(metadata).extras(java.util.Map.of("value","prefix é")));
      check(result.extras().get("value").equals("prefix é"), "media prefix header round trip");
      check(result.text().equals(text) && result.meta().equals(metadata), "text payload and media header round trip");
      check(client.text(b -> b.text("").meta(metadata).extras(java.util.Map.of("value","prefix é"))).text().isEmpty(), "empty text payload");
      var document = client.document(b -> b.document(example.wire.model.Record.builder().name("record").build()));
      check(document.document().name().equals("record"), "structured response payload");
      try { client.text(b -> b.text("fail")); throw new AssertionError("error payload not thrown"); }
      catch (TextError error) { check(error.details().equals("invalid text") && error.status()==422, "modeled error payload"); }
      if (failure.get()!=null) throw new AssertionError(failure.get());
    } finally { server.stop(0); }
    System.out.println("Java generated payloads and media headers passed");
  }
}
