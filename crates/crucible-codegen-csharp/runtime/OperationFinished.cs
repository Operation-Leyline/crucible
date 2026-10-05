/// <summary>A call ended. <see cref="Status"/> is the last attempt's; <see cref="Attempts"/> is zero when the call failed before sending.</summary>
public sealed record OperationFinished(string InvocationId, string Service, string Operation, int Attempts, int? Status, string? Error,
    TimeSpan Latency, DateTimeOffset Time);
