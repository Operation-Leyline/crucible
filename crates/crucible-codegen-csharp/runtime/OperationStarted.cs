/// <summary>A call started. <see cref="Time"/> is read from the client's <see cref="TimeProvider"/>.</summary>
public sealed record OperationStarted(string InvocationId, string Service, string Operation, DateTimeOffset Time);
