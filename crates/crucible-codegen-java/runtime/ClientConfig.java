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
   * the SDK was generated with; required when the model has neither that nor endpoint rules.
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
  /** Supplies HTTP Basic credentials; read once per attempt. */
  public Supplier<BasicCredentials> basicAuth;
  /** Supplies the API key for operations that use API key auth; read once per attempt. */
  public Supplier<String> apiKey;
  /**
   * Supplies AWS credentials for SigV4 signing; read once per attempt, so a provider such as
   * {@link CachedCredentials} can refresh them. See {@link CredentialProviders}.
   */
  public Supplier<AwsCredentials> credentials;
  /** AWS region used for SigV4 signing and endpoint rules. */
  public String region;
  /** Values for the endpoint rule set parameters, by parameter name. */
  public Map<String,Object> endpointParameters = new LinkedHashMap<>();
  /** Overrides for endpoint rule built-ins such as {@code AWS::UseFIPS}, by built-in name. */
  public Map<String,Object> endpointBuiltins = new LinkedHashMap<>();
  /** Suppresses the modeled {@code @endpoint} host prefix. */
  public boolean disableHostPrefix;
  /** Time source for signing and retry deadlines; replace it in tests. */
  public java.time.Clock clock = java.time.Clock.systemUTC();
  /** When request checksums are computed. Default {@link ChecksumPolicy#WHEN_SUPPORTED}. */
  public ChecksumPolicy requestChecksumCalculation = ChecksumPolicy.WHEN_SUPPORTED;
  /** When response checksums are validated. Default {@link ChecksumPolicy#WHEN_SUPPORTED}. */
  public ChecksumPolicy responseChecksumValidation = ChecksumPolicy.WHEN_SUPPORTED;
  /** Called with the operation name and algorithm after a response checksum validated. */
  public java.util.function.BiConsumer<String,String> checksumValidated;
  /** Total attempts per call including the first; at least 1. Default 3. */
  public int maxAttempts = 3;
  /** Deadline for a whole call including retries and, for streams, the body read. Default 30 seconds. */
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

  /**
   * Middleware may replace body bytes; the digest is computed afterwards. A streaming
   * payload is {@code stream}, with a null {@code body}; it is opened only after middleware.
   */
  public static final class Request {
    /** Target URI; middleware may replace it. */
    public java.net.URI uri;
    /** HTTP method. */
    public final String method;
    /** Mutable request headers. */
    public final Map<String, String> headers;
    /** Body bytes, or {@code null} for no body or a streaming payload. */
    public byte[] body;
    /** The streaming payload, or {@code null}. */
    public final ByteStream stream;

    Request(java.net.URI uri, String method, Map<String, String> headers, byte[] body) {
      this(uri, method, headers, body, null);
    }

    Request(java.net.URI uri, String method, Map<String, String> headers, byte[] body, ByteStream stream) {
      this.uri = uri;
      this.method = method;
      this.headers = headers;
      this.body = body;
      this.stream = stream;
    }
  }
}
