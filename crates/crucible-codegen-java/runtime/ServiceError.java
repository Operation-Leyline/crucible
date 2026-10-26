/**
 * An error response from the service, carrying the HTTP status, request id, headers and raw body.
 * Modeled errors are generated subclasses of the service exception, which extends this base.
 */
public class ServiceError extends RuntimeException {
  private static final long serialVersionUID = 1L;

  /** Creates an uninitialized error; the client fills it in before throwing. */
  public ServiceError() {}

  /** The error code from the wire. */
  private String errorName;
  /** The message from the response. */
  private String errorMessage;
  /** The HTTP status. */
  private int status;
  /** The service request id. */
  private String requestId;
  /** The response headers. */
  private java.net.http.HttpHeaders headers;
  /** The raw response body. */
  private byte[] body = new byte[0];
  /** Whether a retry may help. */
  private boolean retryable;
  /** Whether the service asked the client to slow down. */
  private boolean throttling;
  // BEGIN ERROR DOCUMENTATION
  private String documentationUrl;

  /**
   * Static documentation for the modeled error code and its known reason.
   *
   * @return the documentation URL, or {@code null} for an unknown identity
   */
  public String docUrl() {
    return documentationUrl;
  }

  void initializeDocumentation(String url) {
    documentationUrl = url;
  }
  // END ERROR DOCUMENTATION

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

  /**
   * The error message from the response, or {@code null} when it had none.
   *
   * @return the message
   */
  @Override
  public String getMessage() {
    return errorMessage;
  }

  /**
   * The error code as it appeared on the wire.
   *
   * @return the error code
   */
  public String errorName() {
    return errorName;
  }

  /**
   * The HTTP status code of the response.
   *
   * @return the status
   */
  public int status() {
    return status;
  }

  /**
   * The request id the service reported, or {@code null}.
   *
   * @return the request id
   */
  public String requestId() {
    return requestId;
  }

  /**
   * The response headers.
   *
   * @return the headers
   */
  public java.net.http.HttpHeaders headers() {
    return headers;
  }

  /**
   * A copy of the raw response body.
   *
   * @return the body bytes
   */
  public byte[] body() {
    return body.clone();
  }

  /**
   * Whether the client treats the error as retryable.
   *
   * @return whether it is retryable
   */
  public boolean retryable() {
    return retryable;
  }

  /**
   * Whether the client treats the error as throttling.
   *
   * @return whether it is throttling
   */
  public boolean throttling() {
    return throttling;
  }
}
