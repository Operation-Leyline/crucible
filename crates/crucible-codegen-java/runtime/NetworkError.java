/** The HTTP transport failed before a response arrived (connection, TLS or timeout); see the cause. */
public final class NetworkError extends RuntimeException {
  private static final long serialVersionUID = 1L;

  /**
   * Wraps a transport failure.
   *
   * @param cause the underlying I/O or timeout exception
   */
  public NetworkError(Throwable cause) {
    super("HTTP transport failed", cause);
  }
}
