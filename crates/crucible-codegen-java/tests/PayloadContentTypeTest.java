package example.payload;

import com.sun.net.httpserver.HttpServer;
import example.payload.model.StreamInput;
import example.payload.model.UploadInput;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.atomic.AtomicReference;

/** A modelled Content-Type member, then the payload's media type, then a client-wide default. */
public final class PayloadContentTypeTest {
  static void check(boolean ok, String message) { if (!ok) throw new AssertionError(message); }

  public static void main(String[] args) throws Exception {
    var seen = new AtomicReference<List<String>>();
    var body = new AtomicReference<String>();
    var server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
    server.createContext("/", exchange -> {
      try {
        body.set(new String(exchange.getRequestBody().readAllBytes(), StandardCharsets.UTF_8));
        var values = exchange.getRequestHeaders().get("Content-Type");
        seen.set(values == null ? List.of() : new ArrayList<>(values));
        exchange.sendResponseHeaders(200, -1);
      } finally { exchange.close(); }
    });
    server.start();
    try {
      for (boolean streaming : new boolean[] {false, true})
        for (String modelled : new String[] {null, "image/png"})
          for (String configured : new String[] {null, "image/jpeg"}) {
            String label = (streaming ? "streaming" : "buffered") + " member=" + modelled + " default=" + configured;
            var config = new ClientConfig();
            config.endpoint = "http://127.0.0.1:" + server.getAddress().getPort();
            config.maxAttempts = 1;
            if (configured != null) config.headers.put("Content-Type", configured);
            var client = new PayloadBindingsClient(config);
            seen.set(null); body.set(null);
            if (streaming) {
              var input = StreamInput.builder().data(ByteStream.fromBytes("PNG".getBytes(StandardCharsets.UTF_8)));
              if (modelled != null) input.contentType(modelled);
              client.uploadStream(input.build());
            } else {
              var input = UploadInput.builder().data("PNG".getBytes(StandardCharsets.UTF_8));
              if (modelled != null) input.contentType(modelled);
              client.upload(input.build());
            }
            String expected = modelled != null ? modelled : "application/octet-stream";
            check("PNG".equals(body.get()), label + ": body " + body.get());
            check(List.of(expected).equals(seen.get()), label + ": Content-Type " + seen.get() + ", want " + expected);
          }
    } finally { server.stop(0); }
    System.out.println("Java payload Content-Type precedence passed");
  }
}
