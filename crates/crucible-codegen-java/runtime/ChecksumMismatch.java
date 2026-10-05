/** The received body failed its modeled checksum; no decoded response was returned. */
public final class ChecksumMismatch extends RuntimeException {
  private static final long serialVersionUID = 1L;
  public final String algorithm;
  public ChecksumMismatch(String algorithm) { super("Response checksum mismatch: "+algorithm); this.algorithm=algorithm; }
}
