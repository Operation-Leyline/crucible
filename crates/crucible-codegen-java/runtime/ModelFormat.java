/**
 * Renders generated model types as {@code TypeName(member=value, ...)}, the way the AWS SDKs do.
 *
 * <p>Unset optional members are omitted, an explicit {@code null} prints as {@code null}, and
 * collections render their elements recursively. Members that are {@code @sensitive} are never
 * printed: the generated code routes them through {@link #redact(Object)}.
 */
public final class ModelFormat {
  /** Printed in place of the value of a {@code @sensitive} member. */
  public static final String REDACTED = "*** Sensitive Data Redacted ***";

  private ModelFormat() {}

  /**
   * Formats a model.
   *
   * @param type the model's type name
   * @param members alternating member names and values; an unset {@link OptionalValue} is skipped
   * @return {@code type(name=value, ...)}
   */
  public static String format(String type, Object... members) {
    var out = new StringBuilder(type).append('(');
    boolean first = true;
    for (int i = 0; i + 1 < members.length; i += 2) {
      Object value = members[i + 1];
      if (value instanceof OptionalValue<?> optional) {
        if (!optional.present()) continue;
        value = optional.value();
      }
      if (!first) out.append(", ");
      first = false;
      out.append(members[i]).append('=');
      append(out, value);
    }
    return out.append(')').toString();
  }

  /**
   * Hides a sensitive member's value while preserving whether it is set.
   *
   * @param value the member's field value
   * @return an unset {@link OptionalValue} when it is unset, otherwise the redaction marker
   */
  public static Object redact(Object value) {
    if (value instanceof OptionalValue<?> optional && !optional.present()) return value;
    return new Redacted();
  }

  /**
   * Marks a nullable value as present only when it is not null, so an absent one is omitted.
   *
   * @param value a value that may be null
   * @return a present or unset {@link OptionalValue}
   */
  public static Object present(Object value) {
    return value == null ? OptionalValue.unset() : OptionalValue.of(value);
  }

  private static final class Redacted {
    @Override public String toString() { return REDACTED; }
  }

  private static void append(StringBuilder out, Object value) {
    if (value instanceof byte[] bytes) {
      out.append("byte[").append(bytes.length).append(']');
    } else if (value instanceof java.util.Map<?, ?> map) {
      out.append('{');
      boolean first = true;
      for (var entry : map.entrySet()) {
        if (!first) out.append(", ");
        first = false;
        append(out, entry.getKey());
        out.append('=');
        append(out, entry.getValue());
      }
      out.append('}');
    } else if (value instanceof java.util.Collection<?> items) {
      out.append('[');
      boolean first = true;
      for (var item : items) {
        if (!first) out.append(", ");
        first = false;
        append(out, item);
      }
      out.append(']');
    } else {
      out.append(value);
    }
  }
}
