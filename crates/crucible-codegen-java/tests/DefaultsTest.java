package example.defaults;

import example.defaults.model.DefaultsOutput;
import java.nio.charset.StandardCharsets;
import java.time.Instant;

public final class DefaultsTest {
  static void check(boolean value, String message) {
    if (!value) throw new AssertionError(message);
  }

  public static void main(String[] args) {
    var config = new ClientConfig(); config.endpoint = "https://example.invalid";
    var runtime = new ClientRuntime(config, "", false, ModelRegistry.classes());
    var decoded = (DefaultsOutput) runtime.decode("example.defaults#DefaultsOutput", com.google.gson.JsonParser.parseString("{}"), null);
    check(!decoded.optionalCount.present() && decoded.count() == 17, "decode must ignore clientOptional defaults");
    var builder = DefaultsOutput.builder();
    var value = builder.build();
    check(value.flag() && value.tiny() == -7 && value.small() == 1234, "small scalar defaults");
    check(value.count() == 17 && value.large() == Long.MAX_VALUE, "integer defaults");
    check(value.decimalInteger() == 17, "integral decimal default");
    check(value.ratio() == 1.25f && value.fraction() == -3.125, "floating defaults");
    check(value.label().equals("quote\"\\ newline\n"), "escaped string default");
    check(value.mode().value().equals("known"), "enum default");
    check(value.strings().isEmpty() && value.lookup().isEmpty(), "collection defaults");
    check(value.document().getAsJsonObject().getAsJsonArray("list").get(2).getAsLong() == Long.MAX_VALUE,
        "document default precision");
    check(new String(value.blob(), StandardCharsets.UTF_8).equals("ab"), "blob default");
    check(value.instant().equals(Instant.ofEpochSecond(-1, 750_000_000)), "timestamp default");
    check(!value.optionalCount.present(), "client-optional default must stay unset");
    var explicit = DefaultsOutput.builder().count(null).optionalCount(null).build();
    check(explicit.count() == null && explicit.optionalCount.present() && explicit.optionalCount() == null,
        "explicit null must override defaults");
    var another = builder.build();
    value.strings().add("changed");
    value.lookup().put("changed", 1L);
    value.document().getAsJsonObject().addProperty("changed", true);
    value.blob()[0] = 0;
    check(another.strings().isEmpty() && another.lookup().isEmpty()
        && !another.document().getAsJsonObject().has("changed") && another.blob()[0] == 'a',
        "mutable defaults are shared between instances");
    var suppliedList = new java.util.ArrayList<String>();
    suppliedList.add("supplied");
    builder.strings(suppliedList).lookup(null).document(null).blob(null);
    var assigned = builder.build();
    check(assigned.strings() == suppliedList && assigned.lookup() == null
        && assigned.document() == null && assigned.blob() == null,
        "explicit mutable assignments or nulls were replaced by defaults");
    check(new DefaultsOutput().count() == 17, "direct model default");
    System.out.println("Java native modeled defaults passed");
  }
}
