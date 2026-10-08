/// <summary>Settings for a generated client, read once when the client is constructed. Properties are init-only, so set them in an object initializer.</summary>
public sealed class ClientConfig
{
    /// <summary>When request checksums are computed. Default <see cref="ChecksumPolicy.WhenSupported"/>.</summary>
    public ChecksumPolicy RequestChecksumCalculation { get; init; } = ChecksumPolicy.WhenSupported;
    /// <summary>When response checksums are validated. Default <see cref="ChecksumPolicy.WhenSupported"/>.</summary>
    public ChecksumPolicy ResponseChecksumValidation { get; init; } = ChecksumPolicy.WhenSupported;
    /// <summary>Called with the operation and algorithm after a response checksum validated.</summary>
    public Action<string,string>? ChecksumValidated { get; init; }
    /// <summary>Base URL, absolute HTTP(S) with no credentials, query or fragment. Defaults to the endpoint the SDK was generated with; required when the model has neither that nor endpoint rules.</summary>
    public string? Endpoint { get; init; }
    /// <summary>Caller-owned when supplied. Configure redirects and handler policy explicitly.</summary>
    public HttpClient? HttpClient { get; init; }
    /// <summary>Supplies the bearer token for HTTP bearer auth, called once per attempt.</summary>
    public Func<CancellationToken, ValueTask<string?>>? Token { get; init; }
    /// <summary>Supplies HTTP Basic credentials, called once per attempt.</summary>
    public Func<CancellationToken, ValueTask<BasicCredentials?>>? BasicCredentials { get; init; }
    /// <summary>Supplies the API key for API key auth, called once per attempt.</summary>
    public Func<CancellationToken, ValueTask<string?>>? ApiKey { get; init; }
    /// <summary>Supplies AWS credentials for SigV4 signing, called once per attempt so a provider such as <see cref="CachedCredentials"/> can refresh them. See <see cref="CredentialProviders"/>.</summary>
    public Func<CancellationToken, ValueTask<AwsCredentials?>>? Credentials { get; init; }
    /// <summary>Credentials for HTTP Digest (RFC 7616) challenges; the first request carries none.</summary>
    public Func<CancellationToken, ValueTask<BasicCredentials?>>? DigestCredentials { get; init; }
    /// <summary>The regions a SigV4a signature covers, such as <c>["*"]</c>; an endpoint rule's set wins, and <see cref="Region"/> is the fallback.</summary>
    public IReadOnlyList<string>? SigV4aSigningRegionSet { get; init; }
    /// <summary>AWS region used for SigV4 signing and endpoint rules.</summary>
    public string? Region { get; init; }
    /// <summary>Values for the endpoint rule set parameters, by name.</summary>
    public Dictionary<string, System.Text.Json.Nodes.JsonNode?> EndpointParameters { get; init; } = new();
    /// <summary>Overrides for endpoint rule built-ins such as <c>AWS::UseFIPS</c>.</summary>
    public Dictionary<string, System.Text.Json.Nodes.JsonNode?> EndpointBuiltIns { get; init; } = new();
    /// <summary>Suppresses the modeled <c>@endpoint</c> host prefix.</summary>
    public bool DisableHostPrefix { get; init; }
    /// <summary>Total attempts per call including the first; at least 1. Default 3.</summary>
    public int MaxAttempts { get; init; } = 3;
    /// <summary>Deadline for a whole call including retries. Default 30 seconds.</summary>
    public TimeSpan Timeout { get; init; } = TimeSpan.FromSeconds(30);
    /// <summary>Base of the exponential, fully jittered retry backoff. Default 100 ms.</summary>
    public TimeSpan BaseDelay { get; init; } = TimeSpan.FromMilliseconds(100);
    /// <summary>Upper bound of a single retry delay. Default 20 seconds.</summary>
    public TimeSpan MaxDelay { get; init; } = TimeSpan.FromSeconds(20);
    /// <summary>Headers added to every request before middleware runs; names are case-insensitive.</summary>
    public Dictionary<string, string> Headers { get; init; } = new(StringComparer.OrdinalIgnoreCase);
    /// <summary>Hooks run in order on every attempt request before it is sent.</summary>
    public List<Func<HttpRequestMessage, CancellationToken, ValueTask>> Middleware { get; init; } = [];
    /// <summary>Legacy (the default) has no retry quota; Standard adds the client's retry quota; Adaptive also limits the send rate after throttling.</summary>
    public RetryMode RetryMode { get; init; } = RetryMode.Legacy;
    /// <summary>Null discovers endpoints only for operations that require it, true also for optional ones, false never (a required operation then fails before sending).</summary>
    public bool? EndpointDiscovery { get; init; }
    /// <summary>Receives redacted operation and attempt events. A hook that throws is traced and otherwise ignored.</summary>
    public TelemetryHook? Telemetry { get; init; }
    /// <summary>The clock for retry and rate-limit sleeps, the rate limiter, the discovery cache, and telemetry. <see cref="Timeout"/> always uses real time.</summary>
    public TimeProvider TimeProvider { get; init; } = TimeProvider.System;
}
