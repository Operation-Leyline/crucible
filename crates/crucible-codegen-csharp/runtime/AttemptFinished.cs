/// <summary>
/// One attempt ended. <see cref="Status"/> is null when no response arrived. <see cref="Error"/> is a modeled or wire
/// error name or the exception type name, never a message. <see cref="RetryReason"/> (<c>throttling</c>, <c>transient</c>,
/// <c>timeout</c>, <c>network</c> or <c>endpoint</c>) and <see cref="RetryDelay"/> are set only when another attempt follows.
/// <see cref="ResponseBytes"/> counts the bytes read, or a streamed response's Content-Length.
/// </summary>
public sealed record AttemptFinished(string InvocationId, string Service, string Operation, int Attempt, int? Status, string? Error,
    string? RequestId, long? ResponseBytes, TimeSpan Latency, string? RetryReason, TimeSpan? RetryDelay, DateTimeOffset Time);
