public record UnknownUnion(String name, com.google.gson.JsonElement value) {
  public UnknownUnion {
    java.util.Objects.requireNonNull(name);
    java.util.Objects.requireNonNull(value);
  }
}
