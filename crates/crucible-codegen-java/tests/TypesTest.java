package example.types;

import com.sun.net.httpserver.HttpServer;
import example.types.model.Mode;
import example.types.model.Output;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;
import java.util.Set;
import java.util.concurrent.atomic.AtomicReference;

public final class TypesTest {
  static void check(boolean ok, String message) { if (!ok) throw new AssertionError(message); }
  public static void main(String[] args) throws Exception {
    String integer = "12345678901234567890123456789012345678901234567890";
    String decimal = "12345678901234567890.12345678901234567890123456789";
    var defaults = Output.builder().build();
    check(defaults.integer().toString().equals(integer), "integer default precision");
    check(defaults.decimal().toPlainString().equals(decimal), "decimal default precision");
    var body = new AtomicReference<>("{\"integer\":"+integer+",\"decimal\":"+decimal+",\"mode\":17,\"names\":[\"a\",\"b\"],\"sparseNames\":[null,\"x\"],\"sparseMap\":{\"null\":null,\"future\":17}}");
    var server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
    server.createContext("/types", exchange -> {
      byte[] bytes = body.get().getBytes(StandardCharsets.UTF_8);
      exchange.sendResponseHeaders(200, bytes.length);
      exchange.getResponseBody().write(bytes); exchange.close();
    });
    server.start();
    try {
      var config = new ClientConfig(); config.endpoint = "http://127.0.0.1:"+server.getAddress().getPort(); config.maxAttempts=1;
      var client = new TypesClient(config);
      var result = client.read();
      check(result.integer().toString().equals(integer) && result.decimal().toPlainString().equals(decimal), "wire precision");
      check(result.mode().equals(new Mode(17)), "open integer enum");
      check(Mode.ON.value() == Integer.MAX_VALUE && Mode.OFF.value() == -1, "known integer enum values");
      check(result.names().equals(Set.of("a", "b")), "native set");
      check(result.sparseNames().equals(Arrays.asList(null,"x")), "sparse list");
      check(result.sparseMap().containsKey("null") && result.sparseMap().get("null")==null && result.sparseMap().get("future").equals(new Mode(17)), "sparse map");
      body.set("{\"integers\":[18446744073709551616,36893488147419103232],\"decimals\":[12345678901234567890.1234567891,12345678901234567890.1234567892]}");
      var precise=client.read();
      check(precise.integers().size()==2 && precise.decimals().size()==2,"collection uniqueness lost numeric precision");
      var runtime = new ClientRuntime(config, "", false, ModelRegistry.classes());
      var encoded = runtime.encode("example.types#Output", result, null).getAsJsonObject();
      check(encoded.get("integer").getAsString().equals(integer) && encoded.get("decimal").getAsString().equals(decimal), "encode precision");
      for (String invalid : new String[]{"{\"integer\":1.5}", "{\"mode\":2147483648}", "{\"names\":[\"a\",\"a\"]}", "{\"decimals\":[1,1.0]}", "{\"integers\":[18446744073709551616,18446744073709551616]}"}) {
        body.set(invalid);
        try { client.read(); throw new AssertionError("invalid value accepted: "+invalid); }
        catch (IllegalArgumentException | ArithmeticException expected) {}
      }
    } finally { server.stop(0); }
    System.out.println("Java generated extended types passed");
  }
}
