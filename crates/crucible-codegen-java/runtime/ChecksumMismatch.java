/** The received body failed its modeled checksum; no decoded response was returned. */
public final class ChecksumMismatch extends RuntimeException {
  private static final long serialVersionUID = 1L;
  /** The algorithm that failed, such as {@code CRC32}. */
  public final String algorithm;

  /**
   * Reports a failed validation.
   *
   * @param algorithm the algorithm that failed
   */
  public ChecksumMismatch(String algorithm) { super("Response checksum mismatch: "+algorithm); this.algorithm=algorithm; }
}
