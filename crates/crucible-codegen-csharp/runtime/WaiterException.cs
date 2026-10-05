public class WaiterException : Exception
{
    public string WaiterName { get; }
    public int Attempts { get; }
    public object? Response { get; }
    public WaiterException(string name, int attempts, object? response, Exception? error, string? message = null)
        : base($"Waiter {name} failed after {attempts} attempts" + (message is null ? "" : ": " + message), error)
    { WaiterName = name; Attempts = attempts; Response = response; }
}
