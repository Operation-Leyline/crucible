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
}
