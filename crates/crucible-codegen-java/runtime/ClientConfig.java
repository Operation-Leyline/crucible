import java.net.http.HttpClient;
import java.time.Duration;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.function.Supplier;

/**
 * Settings for a generated client. Set the fields you need and pass the object to the client
 * constructor, which copies it: later changes to this object do not affect an existing client.
 * The caller owns an injected {@link HttpClient} and is responsible for its lifecycle.
 */
public final class ClientConfig {
  /** Creates a configuration with the defaults documented on each field. */
  public ClientConfig() {}

  /**
   * Base URL, absolute HTTP(S) with no credentials, query or fragment. Defaults to the endpoint
   * the SDK was generated with; required when it was generated without one.
   */
  public String endpoint;
  /**
   * Transport used for every call. The client builds a default one that never follows redirects
   * when this is {@code null}. A supplied client is shared and never closed by the SDK.
   */
  public HttpClient httpClient;
  /** Supplies the bearer token for operations that use HTTP bearer auth; read once per attempt. */
  public Supplier<String> token;
  /** Called once per operation with an omitted modeled idempotency token. */
  public Supplier<String> idempotencyToken = () -> java.util.UUID.randomUUID().toString();
  /** Total attempts per call including the first; at least 1. Default 3. */
  public int maxAttempts = 3;
  /** Deadline for a whole call including retries. Default 30 seconds. */
  public Duration timeout = Duration.ofSeconds(30);
  /** Base of the exponential, fully jittered retry backoff. Default 100 ms. */
  public Duration baseDelay = Duration.ofMillis(100);
  /** Upper bound of a single retry delay. Default 20 seconds. */
  public Duration maxDelay = Duration.ofSeconds(20);
  /** Headers added to every request before middleware runs. */
  public Map<String, String> headers = new LinkedHashMap<>();
  /** Hooks run in order on each attempt request after it is built and before it is sent. */
  public List<Middleware> middleware = new ArrayList<>();

  /** A request hook; see {@link ClientConfig#middleware}. */
  @FunctionalInterface
  public interface Middleware {
    /**
     * Inspects or edits the outgoing request.
     *
     * @param request the request about to be sent
     */
    void apply(Request request);
  }

  /** The outgoing request that middleware may inspect or edit. */
  public static final class Request {
    /** Target URI; middleware may replace it. */
    public java.net.URI uri;
    /** HTTP method. */
    public final String method;
    /** Mutable request headers. */
    public final Map<String, String> headers;
    /** Body bytes, or {@code null} for no body. */
    public byte[] body;

    Request(java.net.URI uri, String method, Map<String, String> headers, byte[] body) {
      this.uri = uri;
      this.method = method;
      this.headers = headers;
      this.body = body;
    }
  }
}
