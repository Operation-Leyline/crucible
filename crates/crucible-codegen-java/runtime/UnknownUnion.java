/**
 * A union variant this client does not know, preserved so a newer service response round-trips.
 *
 * <p>The payload is never printed by {@link #toString()}, since it may be sensitive.
 *
 * @param name the wire name of the unrecognized variant
 * @param value the variant's raw JSON value
 */
public record UnknownUnion(String name, com.google.gson.JsonElement value) {
  /** Requires both components. */
  public UnknownUnion {
    java.util.Objects.requireNonNull(name);
    java.util.Objects.requireNonNull(value);
  }

  @Override public String toString() {
    return ModelFormat.format("UnknownUnion", "name", name, "value", ModelFormat.redact(value));
  }
}
