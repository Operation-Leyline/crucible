import java.net.http.HttpClient;
import java.time.Duration;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.function.Supplier;

/** Copied by client construction. The caller owns an injected HttpClient. */
public final class ClientConfig {
  public String endpoint;
  public HttpClient httpClient;
  public Supplier<String> token;
  public Supplier<BasicCredentials> basicAuth;
  public Supplier<String> apiKey;
  public Supplier<AwsCredentials> credentials;
  public String region;
  public Map<String,Object> endpointParameters = new LinkedHashMap<>();
  public Map<String,Object> endpointBuiltins = new LinkedHashMap<>();
  public boolean disableHostPrefix;
  public java.time.Clock clock = java.time.Clock.systemUTC();
  public ChecksumPolicy requestChecksumCalculation = ChecksumPolicy.WHEN_SUPPORTED;
  public ChecksumPolicy responseChecksumValidation = ChecksumPolicy.WHEN_SUPPORTED;
  public java.util.function.BiConsumer<String,String> checksumValidated;
  public int maxAttempts = 3;
  public Duration timeout = Duration.ofSeconds(30);
  public Duration baseDelay = Duration.ofMillis(100);
  public Duration maxDelay = Duration.ofSeconds(20);
  public Map<String, String> headers = new LinkedHashMap<>();
  public List<Middleware> middleware = new ArrayList<>();

  @FunctionalInterface
  public interface Middleware {
    void apply(Request request);
  }

  /** Middleware may replace body bytes; the digest is computed afterwards. */
  public static final class Request {
    public java.net.URI uri;
    public final String method;
    public final Map<String, String> headers;
    public byte[] body;

    Request(java.net.URI uri, String method, Map<String, String> headers, byte[] body) {
      this.uri = uri;
      this.method = method;
      this.headers = headers;
      this.body = body;
    }
  }
}
