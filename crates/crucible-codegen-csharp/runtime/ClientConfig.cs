/// <summary>Settings for a generated client, read once when the client is constructed. Properties are init-only, so set them in an object initializer.</summary>
public sealed class ClientConfig
{
    /// <summary>Base URL, absolute HTTP(S) with no credentials, query or fragment. Defaults to the endpoint the SDK was generated with; required when the SDK was generated without one.</summary>
    public string? Endpoint { get; init; }
    /// <summary>Supplies the bearer token for HTTP bearer auth, called once per attempt.</summary>
    public Func<CancellationToken, ValueTask<string?>>? Token { get; init; }
    /// <summary>Caller-owned when supplied. Configure redirects and handler policy explicitly.</summary>
    public HttpClient? HttpClient { get; init; }
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
    /// <summary>Receives redacted operation and attempt events. A hook that throws is traced and otherwise ignored.</summary>
    public TelemetryHook? Telemetry { get; init; }
    /// <summary>The clock for retry and rate-limit sleeps, the rate limiter, and telemetry. <see cref="Timeout"/> always uses real time.</summary>
    public TimeProvider TimeProvider { get; init; } = TimeProvider.System;
}
