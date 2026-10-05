/// <summary>
/// One attempt is being sent. <see cref="Endpoint"/> is the scheme, host, port and base path (never user information);
/// <see cref="EndpointSource"/> is <c>configured</c>, <c>rules</c> or <c>discovered</c>. <see cref="Target"/> (path and
/// query) and <see cref="Headers"/> (lowercase names) are as sent, with credential and sensitive values replaced by <c>***</c>.
/// <see cref="RequestBytes"/> is the Content-Length sent, or null for a chunked or bodiless request.
/// </summary>
public sealed record AttemptStarted(string InvocationId, string Service, string Operation, int Attempt, string Method,
    string Endpoint, string EndpointSource, string Target, IReadOnlyDictionary<string, string> Headers, long? RequestBytes, DateTimeOffset Time);
