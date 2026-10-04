/** Presence is independent of the value: present null writes explicit JSON null. */
public record OptionalValue<T>(boolean present, T value) {
  public static <T> OptionalValue<T> unset() {
    return new OptionalValue<>(false, null);
  }

  public static <T> OptionalValue<T> of(T value) {
    return new OptionalValue<>(true, value);
  }
}
