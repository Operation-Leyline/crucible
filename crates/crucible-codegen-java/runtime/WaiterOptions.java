import java.time.Duration;

/** Overall polling deadline and optional overrides of modeled delay bounds. */
public record WaiterOptions(Duration timeout, Duration minDelay, Duration maxDelay) {
  public WaiterOptions() { this(Duration.ofMinutes(5), null, null); }
  public WaiterOptions(Duration timeout) { this(timeout, null, null); }
}
