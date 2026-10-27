// Retry modes and telemetry; docs/sdk-runtime-policy.md is the cross-language contract.
internal sealed partial class ClientRuntime
{
    private const string Redacted = "***";
    // Always redacted, whatever the model says.
    private static readonly string[] CredentialHeaders = ["authorization", "proxy-authorization", "cookie", "set-cookie"];
    private RetryStrategy retry = null!;
    private HashSet<string> credentialHeaders = null!;
    private string service = "";

    private void InitializePolicy()
    {
        if (!Enum.IsDefined(config.RetryMode)) throw new ArgumentException("Invalid retry mode");
        // One quota and rate limiter for every call this client makes.
        retry = new RetryStrategy(config.RetryMode, config.TimeProvider, config.BaseDelay, config.MaxDelay);
        service = Text(schema["service"]) ?? "";
        credentialHeaders = new(CredentialHeaders, StringComparer.OrdinalIgnoreCase);
    }

    /// <summary>The retry reason for a failed attempt, or null when the failure is not retryable.</summary>
    private static string? Classify(bool safe, Exception failure, bool timedOut)
    {
        if (failure is NetworkError) return safe ? timedOut ? "timeout" : "network" : null;
        if (failure is not ServiceError error) return null;
        if (error.Throttling) return "throttling";
        if (safe && (error.Retryable || error.StatusCode is 500 or 502 or 503 or 504)) return "transient";
        return null;
    }

    /// <summary>Records a failed attempt with the retry policy and decides whether, and when, to retry.</summary>
    private (string Reason, TimeSpan Delay, int? Cost)? AfterFailure(Prepared prepared, Exception failure, bool timedOut, int attempt,
        int maxAttempts, Dictionary<string, string>? headers)
    {
        retry.AfterAttempt(failure is ServiceError { Throttling: true });
        var reason = Classify(Flag(prepared.Operation["safe"]), failure, timedOut);
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

    /// <summary>Runs one call under its own telemetry: validation failures still finish the operation.</summary>
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
        private int attempts, operationFinished;
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

        internal void AttemptStarted(Prepared prepared, HttpRequestMessage request, string endpoint)
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
                    return prepared.SensitiveQuery.Contains(decoded) ? name + "=" + Redacted : pair;
                }));
            var started = new AttemptStarted(id, client.service, operation, attempts, request.Method.Method, ShowEndpoint(endpoint), target,
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
            // A stream canceled at its handoff reported its outcome already.
            if (hook is null || Interlocked.Exchange(ref operationFinished, 1) != 0) return;
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
