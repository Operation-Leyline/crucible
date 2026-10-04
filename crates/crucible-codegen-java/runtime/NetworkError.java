public final class NetworkError extends RuntimeException {
  private static final long serialVersionUID = 1L;

  public NetworkError(Throwable cause) {
    super("HTTP transport failed", cause);
  }
}
