import com.google.gson.JsonElement;

/** A terminal waiter failure, retaining the last operation result for diagnosis. */
public class WaiterError extends RuntimeException {
  private static final long serialVersionUID = 1L;
  /** The waiter name. */
  private final String waiterName;
  /** Polls made. */
  private final int attempts;
  /** The last operation output. */
  private final Object response;
  /** The message the failure acceptor extracted. */
  private final JsonElement waiterMessage;
  WaiterError(String name, int attempts, Object response, Throwable error, JsonElement message) {
    super("Waiter " + name + " failed after " + attempts + " attempts", error);
    this.waiterName=name; this.attempts=attempts; this.response=response; this.waiterMessage=message == null ? null : message.deepCopy();
  }
  /**
   * The waiter modeled name.
   *
   * @return the waiter name
   */
  public String waiterName() { return waiterName; }
  /**
   * How many polls ran before the waiter gave up.
   *
   * @return the attempt count
   */
  public int attempts() { return attempts; }
  /**
   * The last operation output, or {@code null} when the last attempt raised an error.
   *
   * @return the last response
   */
  public Object response() { return response; }
  /**
   * The last error, or {@code null}.
   *
   * @return the last error
   */
  public Throwable lastError() { return getCause(); }
  /**
   * A copy of the message the failure acceptor extracted, or {@code null}.
   *
   * @return the extracted message
   */
  public JsonElement waiterMessage() { return waiterMessage == null ? null : waiterMessage.deepCopy(); }
}
