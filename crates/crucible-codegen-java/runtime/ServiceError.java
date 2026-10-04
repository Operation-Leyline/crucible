public class ServiceError extends RuntimeException {
  private static final long serialVersionUID = 1L;
  private String errorName;
  private String errorMessage;
  private int status;
  private String requestId;
  private java.net.http.HttpHeaders headers;
  private boolean retryable;
  private boolean throttling;

  void initialize(
      String name,
      String message,
      int status,
      String requestId,
      java.net.http.HttpHeaders headers,
      boolean retryable,
      boolean throttling) {
    this.errorName = name;
    this.errorMessage = message;
    this.status = status;
    this.requestId = requestId;
    this.headers = headers;
    this.retryable = retryable;
    this.throttling = throttling;
  }

  @Override
  public String getMessage() {
    return errorMessage;
  }

  public String errorName() {
    return errorName;
  }

  public int status() {
    return status;
  }

  public String requestId() {
    return requestId;
  }

  public java.net.http.HttpHeaders headers() {
    return headers;
  }

  public boolean retryable() {
    return retryable;
  }

  public boolean throttling() {
    return throttling;
  }
}
