/**
 * An optional model member. Presence is independent of the value: a present {@code null} writes
 * an explicit JSON null, while an unset member is omitted from the request.
 *
 * @param <T> the member's type
 * @param present whether the member was set, possibly to {@code null}
 * @param value the member's value; {@code null} when unset or explicitly null
 */
public record OptionalValue<T>(boolean present, T value) {
  /**
   * Returns the unset marker.
   *
   * @param <T> the member's type
   * @return a value that is not present
   */
  public static <T> OptionalValue<T> unset() {
    return new OptionalValue<>(false, null);
  }

  /**
   * Returns a present value.
   *
   * @param <T> the member's type
   * @param value the value, or {@code null} to send an explicit null
   * @return a present value
   */
  public static <T> OptionalValue<T> of(T value) {
    return new OptionalValue<>(true, value);
  }
}
