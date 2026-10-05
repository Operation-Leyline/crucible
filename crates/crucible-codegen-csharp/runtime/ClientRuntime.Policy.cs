using System.Text.Json.Nodes;

// Retry modes, endpoint discovery and telemetry; docs/sdk-runtime-policy.md is the cross-language contract.
internal sealed partial class ClientRuntime
{
    private const string Redacted = "***";
    // Always redacted, whatever the model says.
    private static readonly string[] CredentialHeaders = ["authorization", "proxy-authorization", "cookie", "set-cookie", "x-amz-security-token"];
    private static readonly string[] CredentialQuery = ["x-amz-signature", "x-amz-credential", "x-amz-security-token"];
    private RetryStrategy retry = null!;
    private EndpointCache? discovery;
    private HashSet<string> credentialHeaders = null!;
    private HashSet<string> credentialQuery = null!;
    private string service = "";

    private void InitializePolicy()
    {
        if (!Enum.IsDefined(config.RetryMode)) throw new ArgumentException("Invalid retry mode");
        // One quota and rate limiter for every call this client makes.
        retry = new RetryStrategy(config.RetryMode, config.TimeProvider, config.BaseDelay, config.MaxDelay);
        discovery = schema["endpointDiscovery"] is null ? null : new EndpointCache(retry.Now);
        service = Text(schema["service"]) ?? "";
        credentialHeaders = new(CredentialHeaders, StringComparer.OrdinalIgnoreCase);
        credentialQuery = new(CredentialQuery, StringComparer.OrdinalIgnoreCase);
        foreach (var (_, scheme) in schema["authSchemes"]?.AsObject() ?? new JsonObject())
            if (Text(scheme?["kind"]) == "apiKey")
                (Text(scheme!["location"]) == "query" ? credentialQuery : credentialHeaders).Add(Text(scheme["name"])!);
    }

    /// <summary>What a call sends to the discovery operation; the same values key the cache.</summary>
    private sealed record DiscoveryRequest(string Key, string Operation, JsonObject Input, bool Required);

    /// <summary>The discovery this call needs, or null when it uses the configured endpoint.</summary>
    private DiscoveryRequest? DiscoveryFor(string name, JsonObject op, Func<string, JsonNode?> value)
    {
        if (op["discoveredEndpoint"] is not JsonObject discovered || schema["endpointDiscovery"] is not JsonObject settings) return null;
        var required = Flag(discovered["required"]);
        if (config.EndpointDiscovery == false)
        {
            if (required) throw new EndpointDiscoveryException(name + " requires endpoint discovery, which is disabled");
            return null;
        }
        if (!required && config.EndpointDiscovery != true) return null;
        var operation = Text(settings["operation"])!.Split('#')[^1];
        var target = Text(schema["operations"]![operation]!["input"]);
        var operationMember = Text(settings["operationMember"]);
        var identifiersMember = Text(settings["identifiersMember"]);
        var identifiers = new SortedDictionary<string, string>(StringComparer.Ordinal);
        if (identifiersMember is not null)
            foreach (var id in discovered["identifiers"]!.AsArray())
                if (value(Text(id)!) is JsonNode item) identifiers[Text(id)!] = Scalar(item);
        var input = new JsonObject();
        if (operationMember is not null) input[WireName(target, operationMember)] = name;
        if (identifiers.Count != 0)
            input[WireName(target, identifiersMember!)] = new JsonObject(identifiers.Select(pair => KeyValuePair.Create(pair.Key, (JsonNode?)JsonValue.Create(pair.Value))));
        var key = new JsonArray(operationMember is null ? null : JsonValue.Create(name),
            new JsonArray(identifiers.Select(pair => (JsonNode?)new JsonArray(pair.Key, pair.Value)).ToArray())).ToJsonString();
        return new(key, operation, input, required);
    }

    /// <summary>This attempt's discovered endpoint; null means the configured endpoint.</summary>
    private async Task<DiscoveredAddress?> DiscoverAsync(Prepared prepared, TimeSpan remaining, CancellationToken ct)
    {
        if (prepared.Discovery is not DiscoveryRequest request) return null;
        if (!request.Required && discovery!.FailedRecently(request.Key)) return null;
        try
        {
            return await discovery!.ResolveAsync(request.Key, () => FetchEndpointAsync(request, remaining)).WaitAsync(ct).ConfigureAwait(false);
        }
        // An optional operation falls back to the configured endpoint for a while; cancellation is never swallowed.
        catch (Exception) when (!request.Required && !ct.IsCancellationRequested)
        {
            discovery!.RecordFailure(request.Key);
            return null;
        }
    }

    /// <summary>An ordinary call of the discovery operation, bounded by the starting call's remaining time.</summary>
    private async Task<DiscoveredAddress> FetchEndpointAsync(DiscoveryRequest request, TimeSpan remaining)
    {
        if (remaining <= TimeSpan.Zero) throw new TimeoutException("Request deadline exceeded before endpoint discovery");
        var timeout = remaining < config.Timeout ? remaining : config.Timeout;
        var output = await TracedAsync<object>(request.Operation, trace =>
            SendAsync<object>(Prepare(request.Operation, (JsonObject)request.Input.DeepClone()), trace, timeout, CancellationToken.None)).ConfigureAwait(false);
        return Discovered(request.Operation, output);
    }

    /// <summary>The first usable endpoint of a discovery response, expiring after its cache period.</summary>
    private DiscoveredAddress Discovered(string operation, object output)
    {
        var settings = schema["endpointDiscovery"]!;
        var target = Text(schema["operations"]![operation]!["output"])!;
        var endpointsMember = Text(settings["endpointsMember"])!;
        var entryTarget = Text(Shape(Text(Shape(target)["members"]![endpointsMember]!["target"])!)["member"]!["target"])!;
        var values = Encode(target, output) as JsonObject;
        var scheme = Uri.TryCreate(config.Endpoint, UriKind.Absolute, out var configured) ? configured.Scheme : "https";
        foreach (var entry in values?[WireName(target, endpointsMember)] as JsonArray ?? [])
        {
            var address = entry?[WireName(entryTarget, Text(settings["addressMember"])!)];
            var minutes = entry?[WireName(entryTarget, Text(settings["cachePeriodMember"])!)];
            if (address is not JsonValue a || !a.TryGetValue<string>(out var text) || text.Length == 0) continue;
            if (minutes is not JsonValue m || !m.TryGetValue<long>(out var period)) continue;
            // Discovered addresses are usually bare hosts; they keep the configured scheme.
            var url = text.Contains("://", StringComparison.Ordinal) ? text : scheme + "://" + text;
            if (Endpoints.ParseUrl(url) is null) continue;
            return new DiscoveredAddress(url.TrimEnd('/'), retry.Now() + Math.Max(period, 0) * 60.0);
        }
        throw new EndpointDiscoveryException("Endpoint discovery returned no usable endpoint");
    }

    /// <summary>Whether a failure says the discovered endpoint is no longer valid.</summary>
    private bool Invalidates(Exception failure)
    {
        if (failure is not ServiceError error) return false;
        if (error.StatusCode == 421) return true;
        return Text(schema["endpointDiscovery"]?["error"]) is string target && error.ErrorName == ErrorCode(target);
    }

    /// <summary>The retry reason for a failed attempt, or null when the failure is not retryable.</summary>
    private static string? Classify(bool safe, Exception failure, bool timedOut, bool invalidated)
    {
        if (failure is NetworkError) return safe ? timedOut ? "timeout" : "network" : null;
        if (failure is not ServiceError error) return null;
        // A misdirected request was not processed, so even a write may be resent.
        if (invalidated) return "endpoint";
        if (error.Throttling) return "throttling";
        if (safe && (error.Retryable || error.StatusCode is 500 or 502 or 503 or 504)) return "transient";
        return null;
    }

    /// <summary>Records a failed attempt with the retry policy and decides whether, and when, to retry.</summary>
    private (string Reason, TimeSpan Delay, int? Cost)? AfterFailure(Prepared prepared, Exception failure, bool timedOut, int attempt,
        int maxAttempts, DiscoveredAddress? discovered, Dictionary<string, string>? headers)
    {
        retry.AfterAttempt(failure is ServiceError { Throttling: true });
        var invalidated = discovered is not null && Invalidates(failure);
        if (invalidated) discovery!.Invalidate(prepared.Discovery!.Key, discovered!);
        var reason = Classify(Flag(prepared.Operation["safe"]), failure, timedOut, invalidated);
        if (reason is null || attempt >= maxAttempts) return null;
        var (granted, cost) = retry.AcquireRetry(reason == "timeout");
        if (!granted) return null;
        return (reason, Delay(attempt, headers), cost);
    }

    /// <summary>A transport timeout (connect, read, write, or a caller-supplied HttpClient.Timeout), not the call's own deadline.</summary>
    private static bool TimedOut(Exception error, CancellationToken ct)
    {
        if (ct.IsCancellationRequested) return false;
        for (Exception? e = error; e is not null; e = e.InnerException)
            if (e is TimeoutException) return true;
        return false;
    }

    private static string ShowEndpoint(string url) =>
        Uri.TryCreate(url, UriKind.Absolute, out var uri) ? uri.Scheme + "://" + uri.Authority + uri.AbsolutePath.TrimEnd('/') : "";

    private static string? RequestIdOf(Exception? error, Dictionary<string, string>? headers) =>
        error is ServiceError service ? service.RequestId
        : headers?.GetValueOrDefault("x-request-id") ?? headers?.GetValueOrDefault("x-amzn-requestid");

    /// <summary>Runs one call under its own telemetry: validation and discovery failures still finish the operation.</summary>
    private async Task<T> TracedAsync<T>(string name, Func<CallTrace, Task<T>> call)
    {
        var trace = new CallTrace(this, name);
        T result;
        try { result = await call(trace).ConfigureAwait(false); }
        catch (Exception error) { trace.Finished(error); throw; }
        trace.Finished(null);
        return result;
    }

    /// <summary>One call's telemetry. Without a hook it only counts attempts.</summary>
    private sealed class CallTrace
    {
        private readonly ClientRuntime client;
        private readonly TelemetryHook? hook;
        private readonly TimeProvider clock;
        private readonly string operation;
        private readonly string id = "";
        private readonly long start;
        private long attemptStart;
        private int attempts;
        private int? status;

        internal CallTrace(ClientRuntime client, string operation)
        {
            this.client = client; this.operation = operation;
            hook = client.config.Telemetry; clock = client.config.TimeProvider;
            if (hook is null) return;
            id = Guid.NewGuid().ToString("N");
            start = clock.GetTimestamp();
            Emit(hook => hook.OnOperationStarted(new(id, client.service, operation, clock.GetUtcNow())));
        }

        internal void AttemptStarted(Prepared prepared, HttpRequestMessage request, string endpoint, string source)
        {
            attempts++;
            if (hook is null) return;
            attemptStart = clock.GetTimestamp();
            // Reading the length first lists Content-Length among the headers sent.
            var length = request.Content?.Headers.ContentLength;
            var headers = new Dictionary<string, string>(StringComparer.Ordinal);
            foreach (var (name, values) in request.Headers.Concat(request.Content?.Headers ?? Enumerable.Empty<KeyValuePair<string, IEnumerable<string>>>()))
            {
                var lower = name.ToLowerInvariant();
                headers[lower] = client.credentialHeaders.Contains(lower) || prepared.SensitiveHeaders.Contains(lower) ? Redacted : string.Join(", ", values);
            }
            var query = request.RequestUri!.GetComponents(UriComponents.Query, UriFormat.UriEscaped);
            var target = prepared.DisplayPath;
            if (query.Length != 0)
                target += "?" + string.Join("&", query.Split('&').Select(pair => {
                    var name = pair.Split('=', 2)[0];
                    var decoded = Uri.UnescapeDataString(name.Replace('+', ' '));
                    // Names compare case-insensitively: redacting too much is the safe failure.
                    return client.credentialQuery.Contains(decoded) || prepared.SensitiveQuery.Contains(decoded) ? name + "=" + Redacted : pair;
                }));
            var started = new AttemptStarted(id, client.service, operation, attempts, request.Method.Method, ShowEndpoint(endpoint), source, target,
                headers, length, clock.GetUtcNow());
            Emit(hook => hook.OnAttemptStarted(started));
        }

        internal void AttemptFinished(int? status, Exception? error, long? size, string? requestId, (string Reason, TimeSpan Delay, int? Cost)? retry = null)
        {
            this.status = status;
            if (hook is null) return;
            var finished = new AttemptFinished(id, client.service, operation, attempts, status, error is null ? null : ErrorName(error), requestId, size,
                clock.GetElapsedTime(attemptStart), retry?.Reason, retry?.Delay, clock.GetUtcNow());
            Emit(hook => hook.OnAttemptFinished(finished));
        }

        internal void Finished(Exception? error)
        {
            if (hook is null) return;
            var finished = new OperationFinished(id, client.service, operation, attempts, status, error is null ? null : ErrorName(error),
                clock.GetElapsedTime(start), clock.GetUtcNow());
            Emit(hook => hook.OnOperationFinished(finished));
        }

        private static string ErrorName(Exception error) => error is ServiceError service ? service.ErrorName : error.GetType().Name;

        private void Emit(Action<TelemetryHook> send)
        {
            try { send(hook!); }
            catch (Exception error) { System.Diagnostics.Trace.TraceError("Crucible telemetry hook failed: " + error); }
        }
    }
}
