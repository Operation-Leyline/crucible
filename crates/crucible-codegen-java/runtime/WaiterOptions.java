import java.time.Duration;

/**
 * Bounds one waiter call.
 *
 * @param timeout overall deadline for polling, including operation and retry time
 * @param minDelay overrides the modeled minimum delay between polls; {@code null} keeps the model's
 * @param maxDelay overrides the modeled maximum delay between polls; {@code null} keeps the model's
 */
public record WaiterOptions(Duration timeout, Duration minDelay, Duration maxDelay) {
  /** Five minutes with the modeled delays. */
  public WaiterOptions() { this(Duration.ofMinutes(5), null, null); }

  /**
   * Uses the modeled delays.
   *
   * @param timeout overall polling deadline
   */
  public WaiterOptions(Duration timeout) { this(timeout, null, null); }
}
