/** The overall polling deadline expired, including operation and retry time. */
public final class WaiterTimeout extends WaiterError {
  private static final long serialVersionUID = 1L;
  WaiterTimeout(String name, int attempts, Object response, Throwable error) { super(name, attempts, response, error, null); }
}
