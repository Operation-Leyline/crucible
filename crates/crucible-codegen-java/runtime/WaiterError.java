import com.google.gson.JsonElement;

/** A terminal waiter failure, retaining the last operation result for diagnosis. */
public class WaiterError extends RuntimeException {
  private static final long serialVersionUID = 1L;
  private final String waiterName;
  private final int attempts;
  private final Object response;
  private final JsonElement waiterMessage;
  WaiterError(String name, int attempts, Object response, Throwable error, JsonElement message) {
    super("Waiter " + name + " failed after " + attempts + " attempts", error);
    this.waiterName=name; this.attempts=attempts; this.response=response; this.waiterMessage=message == null ? null : message.deepCopy();
  }
  public String waiterName() { return waiterName; }
  public int attempts() { return attempts; }
  public Object response() { return response; }
  public Throwable lastError() { return getCause(); }
  public JsonElement waiterMessage() { return waiterMessage == null ? null : waiterMessage.deepCopy(); }
}
