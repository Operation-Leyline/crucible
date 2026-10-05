import java.time.Clock;
import java.time.Duration;
import java.util.function.Supplier;

/** Serializes refreshes across callers; failed refreshes are never cached. */
public final class CachedCredentials implements Supplier<AwsCredentials> {
  private final Supplier<AwsCredentials> provider;
  private final Clock clock;
  private final Duration refreshBefore;
  private AwsCredentials value;
  public CachedCredentials(Supplier<AwsCredentials> provider) { this(provider, Duration.ofMinutes(1), Clock.systemUTC()); }
  public CachedCredentials(Supplier<AwsCredentials> provider, Duration refreshBefore, Clock clock) {
    this.provider=java.util.Objects.requireNonNull(provider); this.clock=java.util.Objects.requireNonNull(clock);
    if (refreshBefore.isNegative()) throw new IllegalArgumentException("Negative refresh window");
    this.refreshBefore=refreshBefore;
  }
  @Override public synchronized AwsCredentials get() {
    var now=clock.instant();
    if (value!=null && (value.expiresAt()==null || value.expiresAt().isAfter(now.plus(refreshBefore)))) return value;
    var next=provider.get();
    if (next!=null && next.expiresAt()!=null && !next.expiresAt().isAfter(clock.instant()))
      throw new IllegalArgumentException("Credential provider returned expired credentials");
    value=next; return next;
  }
}
