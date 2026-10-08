/**
 * When the client calculates request checksums and validates response checksums.
 * {@code WHEN_REQUIRED} preserves explicit operation input choices and required request checksums.
 */
public enum ChecksumPolicy {
  /** Checksum every request and validate every response whose operation supports it. */
  WHEN_SUPPORTED,
  /** Only when the model requires it or the caller chose an algorithm. */
  WHEN_REQUIRED
}
