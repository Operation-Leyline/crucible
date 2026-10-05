public sealed class ClientConfig
{
    public ChecksumPolicy RequestChecksumCalculation { get; init; } = ChecksumPolicy.WhenSupported;
    public ChecksumPolicy ResponseChecksumValidation { get; init; } = ChecksumPolicy.WhenSupported;
    public Action<string,string>? ChecksumValidated { get; init; }
    public string? Endpoint { get; init; }
    /// <summary>Caller-owned when supplied. Configure redirects and handler policy explicitly.</summary>
    public HttpClient? HttpClient { get; init; }
    public Func<CancellationToken, ValueTask<string?>>? Token { get; init; }
    public Func<CancellationToken, ValueTask<BasicCredentials?>>? BasicCredentials { get; init; }
    public Func<CancellationToken, ValueTask<string?>>? ApiKey { get; init; }
    public Func<CancellationToken, ValueTask<AwsCredentials?>>? Credentials { get; init; }
    /// <summary>Credentials for HTTP Digest (RFC 7616) challenges; the first request carries none.</summary>
    public Func<CancellationToken, ValueTask<BasicCredentials?>>? DigestCredentials { get; init; }
    /// <summary>The regions a SigV4a signature covers, such as <c>["*"]</c>; an endpoint rule's set wins, and <see cref="Region"/> is the fallback.</summary>
    public IReadOnlyList<string>? SigV4aSigningRegionSet { get; init; }
    public string? Region { get; init; }
    public Dictionary<string, System.Text.Json.Nodes.JsonNode?> EndpointParameters { get; init; } = new();
    public Dictionary<string, System.Text.Json.Nodes.JsonNode?> EndpointBuiltIns { get; init; } = new();
    public bool DisableHostPrefix { get; init; }
    public int MaxAttempts { get; init; } = 3;
    public TimeSpan Timeout { get; init; } = TimeSpan.FromSeconds(30);
    public TimeSpan BaseDelay { get; init; } = TimeSpan.FromMilliseconds(100);
    public TimeSpan MaxDelay { get; init; } = TimeSpan.FromSeconds(20);
    public Dictionary<string, string> Headers { get; init; } = new(StringComparer.OrdinalIgnoreCase);
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
