using System.Net.Http.Headers;
using System.Text.Json;
using System.Text.Json.Nodes;

internal sealed partial class ClientRuntime
{
    internal async Task<ResponseStream<T>> OpenStreamAsync<T>(string name, object? input, CancellationToken cancellationToken)
    {
        var deadline = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        deadline.CancelAfter(config.Timeout);
        var trace = new CallTrace(this, name);
        try
        {
            var ct = deadline.Token;
            ct.ThrowIfCancellationRequested();
            var op = schema["operations"]![name]!.AsObject();
            var values = Encode(Text(op["input"]), input)?.AsObject() ?? new JsonObject();
            var prepared = Prepare(name, values);
            int? lastCost = null;
            for (int attempt = 1; ; attempt++)
            {
                ct.ThrowIfCancellationRequested();
                var wait = retry.SendDelay();
                if (wait > TimeSpan.Zero)
                {
                    try { await Task.Delay(wait, config.TimeProvider, ct).ConfigureAwait(false); }
                    catch (OperationCanceledException) { retry.CancelSend(); throw; }
                }
                using var request = new HttpRequestMessage(new HttpMethod(Text(op["method"])!), prepared.Uri);
                if (prepared.Body is not null) request.Content = new ByteArrayContent(prepared.Body);
                foreach (var (key, value) in prepared.Headers) SetHeader(request, key, value);
                SetHeader(request, "Accept", "text/event-stream");
                SetHeader(request, "Accept-Encoding", "identity");
                var authenticated = await AuthenticateAsync(op, request, ct).ConfigureAwait(false);
                foreach (var middleware in config.Middleware) await middleware(request, ct).AsTask().WaitAsync(ct).ConfigureAwait(false);
                if (request.RequestUri is null || !request.RequestUri.IsAbsoluteUri || (request.RequestUri.Scheme != "https" && request.RequestUri.Scheme != "http") || request.RequestUri.UserInfo.Length != 0)
                    throw new ArgumentException("Middleware must supply an absolute HTTP(S) URI without credentials");
                if ((authenticated || request.Headers.Contains("Authorization")) && !Secure(request.RequestUri)) throw new ArgumentException("Credentials require HTTPS or loopback HTTP");
                ct.ThrowIfCancellationRequested();
                trace.AttemptStarted(prepared, request, config.Endpoint!);
                HttpResponseMessage? response = null;
                Dictionary<string, string>? headers = null;
                int? status = null; long? size = null;
                Exception? failure = null; bool timedOut = false, successfulHeaders = false;
                ResponseStream<T>? opened = null;
                try
                {
                    response = await streamHttp.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, ct).ConfigureAwait(false);
                    status = (int)response.StatusCode; successfulHeaders = response.IsSuccessStatusCode;
                    headers = response.Headers.Concat(response.Content.Headers).GroupBy(h => h.Key, StringComparer.OrdinalIgnoreCase)
                        .ToDictionary(g => g.Key, g => string.Join(", ", g.SelectMany(h => h.Value)), StringComparer.OrdinalIgnoreCase);
                    if (successfulHeaders)
                    {
                        RequireStreamProfile(headers);
                        var body = await response.Content.ReadAsStreamAsync(ct).ConfigureAwait(false);
                        ct.ThrowIfCancellationRequested();
                        var stream = new ResponseStream<T>(response, body, deadline,
                            new ResponseMetadata(status.Value, headers), config.MaxStreamEventBytes,
                            value => (T)(Decode(Text(op["output"]), value) ?? throw new StreamProtocolError("Null stream item")),
                            error => StreamError(op, headers, error), trace.Finished);
                        response = null;
                        Succeeded(lastCost);
                        // The opening attempt ends here; the operation ends with the stream.
                        trace.AttemptFinished(status, null, null, RequestIdOf(null, headers));
                        opened = stream;
                    }
                    else
                    {
                        (var bytes, size) = await BoundedStreamErrorAsync(response.Content, ct).ConfigureAwait(false);
                        failure = Error(op, status.Value, headers, bytes);
                    }
                }
                catch (Exception error) when (!successfulHeaders && (error is HttpRequestException or IOException || TimedOut(error, ct)))
                { failure = new NetworkError(error.Message, error); timedOut = TimedOut(error, ct); }
                catch (Exception error) when (successfulHeaders && error is HttpRequestException or IOException && !ct.IsCancellationRequested)
                {
                    // After successful headers a transport failure is never retried, but it
                    // is still a NetworkError like every other transport failure.
                    var network = new NetworkError(error.Message, error);
                    Interrupted(status, network, size, headers, trace); throw network;
                }
                catch (Exception error) { Interrupted(status, error, size, headers, trace); throw; }
                finally { response?.Dispose(); }
                if (opened is not null)
                {
                    // Cancellation now ends the operation, after its opening attempt.
                    opened.Activate();
                    if (ct.IsCancellationRequested)
                    {
                        // Canceled during the handoff: never return a released stream.
                        await opened.DisposeAsync().ConfigureAwait(false);
                        throw new OperationCanceledException(ct);
                    }
                    return opened;
                }
                var decision = AfterFailure(prepared, failure!, timedOut, attempt, config.MaxAttempts, headers);
                trace.AttemptFinished(status, failure, size, RequestIdOf(failure, headers), decision);
                if (decision is not (_, var delay, var cost)) throw failure!;
                lastCost = cost;
                await Task.Delay(delay, config.TimeProvider, ct).ConfigureAwait(false);
            }
        }
        catch (Exception error)
        {
            deadline.Dispose(); trace.Finished(error); throw;
        }
    }
    // Returns the body to decode and the bytes read. One byte past the limit tells an
    // oversized body from one of exactly the limit; like Go and Python, an oversized
    // body is never decoded (status and headers still classify it), while telemetry
    // keeps the bytes read.
    private async Task<(byte[] Body, long Read)> BoundedStreamErrorAsync(HttpContent content, CancellationToken ct)
    {
        using var body = await content.ReadAsStreamAsync(ct).ConfigureAwait(false);
        using var bytes = new MemoryStream();
        var buffer = new byte[(int)Math.Min(4096L, (long)config.MaxStreamEventBytes + 1)];
        while (true)
        {
            var count = await body.ReadAsync(buffer.AsMemory(0, (int)Math.Min(buffer.Length, (long)config.MaxStreamEventBytes - bytes.Length + 1)), ct).ConfigureAwait(false);
            if (count == 0) return (bytes.ToArray(), bytes.Length);
            if (bytes.Length + count > config.MaxStreamEventBytes) return ([], bytes.Length + count);
            bytes.Write(buffer, 0, count);
        }
    }
    private static void RequireStreamProfile(Dictionary<string, string> headers)
    {
        if (!headers.TryGetValue("Content-Type", out var value) || !MediaTypeHeaderValue.TryParse(value, out var media)
            || !string.Equals(media.MediaType, "text/event-stream", StringComparison.OrdinalIgnoreCase))
            throw new StreamProtocolError("Expected Crucible SSE profile");
        var profile = media.Parameters.Where(parameter => string.Equals(parameter.Name, "crucible-stream", StringComparison.OrdinalIgnoreCase)).ToArray();
        var charset = media.Parameters.Where(parameter => string.Equals(parameter.Name, "charset", StringComparison.OrdinalIgnoreCase)).ToArray();
        if (profile.Length != 1 || profile[0].Value?.Trim('"') != "1" || charset.Length > 1 || charset.Length == 1 && !string.Equals(charset[0].Value?.Trim('"'), "utf-8", StringComparison.OrdinalIgnoreCase))
            throw new StreamProtocolError("Expected Crucible SSE profile 1 and UTF-8");
        if (headers.TryGetValue("Content-Encoding", out var encoding) && !string.Equals(encoding, "identity", StringComparison.OrdinalIgnoreCase))
            throw new StreamProtocolError("Encoded response streams are unsupported");
    }
    private ServiceError StreamError(JsonObject op, Dictionary<string, string> headers, JsonObject envelope)
    {
        if (envelope["type"] is not JsonValue type || !type.TryGetValue<string>(out var name) || name.Length == 0
            || envelope["status"] is not JsonValue code || !code.TryGetValue<int>(out var status) || status < 400 || status > 599
            || envelope["error"] is not JsonObject fields)
            throw new StreamProtocolError("Invalid error terminal envelope");
        var target = op["errors"]!.AsArray().Select(node => Text(node)!).FirstOrDefault(id => ErrorCode(id) == name);
        var metadata = target is null ? null : Shape(target)["error"];
        // Stream errors are documents. HTTP header/payload bindings never alter their fields.
        var error = target is null ? new ServiceException() : (ServiceError)Decode(target, fields)!;
        static string? StringValue(JsonNode? value) => value is JsonValue json && json.TryGetValue<string>(out var text) ? text : null;
        error.Initialize(name, StringValue(fields["message"]) ?? $"HTTP {status}", status, headers.GetValueOrDefault("x-request-id"), headers,
            Flag(metadata?["retryable"]), Flag(metadata?["throttling"]));
        // BEGIN ERROR DOCUMENTATION
        var documentation = metadata?["documentation"] as JsonObject;
        var url = Text(documentation?["url"]);
        var member = Text(documentation?["reasonMember"]);
        var reason = member is null ? null : StringValue(fields.ContainsKey(member) ? fields[member] : documentation?["reasonDefault"]);
        if (reason is not null && documentation?["reasonUrls"] is JsonObject pages && pages.TryGetPropertyValue(reason, out var page)) url = Text(page);
        error.DocUrl = url;
        // END ERROR DOCUMENTATION
        return error;
    }
}
