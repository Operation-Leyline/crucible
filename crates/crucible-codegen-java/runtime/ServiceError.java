public class ServiceError extends RuntimeException {
  private static final long serialVersionUID = 1L;
  private String errorName;
  private String modeledErrorName;
  private String errorMessage;
  private int status;
  private String requestId;
  private java.net.http.HttpHeaders headers;
  private byte[] body = new byte[0];
  private boolean retryable;
  private boolean throttling;

  void initialize(
      String name,
      String message,
      int status,
      String requestId,
      java.net.http.HttpHeaders headers,
      byte[] body,
      boolean retryable,
      boolean throttling) {
    this.errorName = name;
    this.errorMessage = message;
    this.status = status;
    this.requestId = requestId;
    this.headers = headers;
    this.body = body.clone();
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

  void setModeledErrorName(String name) {
    this.modeledErrorName = name;
  }

  String waiterErrorName() {
    return modeledErrorName == null ? errorName : modeledErrorName;
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

  public byte[] body() {
    return body.clone();
  }

  public boolean retryable() {
    return retryable;
  }

  public boolean throttling() {
    return throttling;
  }
}
