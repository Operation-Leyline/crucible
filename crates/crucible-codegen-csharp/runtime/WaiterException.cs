/// <summary>A terminal waiter failure, retaining the last operation result for diagnosis.</summary>
public class WaiterException : Exception
{
    /// <summary>The waiter modeled name.</summary>
    public string WaiterName { get; }
    /// <summary>How many polls ran before the waiter gave up.</summary>
    public int Attempts { get; }
    /// <summary>The last operation output, or null when the last attempt raised an error.</summary>
    public object? Response { get; }
    /// <summary>Creates the failure.</summary>
    /// <param name="name">The waiter name.</param>
    /// <param name="attempts">The polls made.</param>
    /// <param name="response">The last output.</param>
    /// <param name="error">The last error.</param>
    /// <param name="message">Detail extracted by the failure acceptor.</param>
    public WaiterException(string name, int attempts, object? response, Exception? error, string? message = null)
        : base($"Waiter {name} failed after {attempts} attempts" + (message is null ? "" : ": " + message), error)
    { WaiterName = name; Attempts = attempts; Response = response; }
}
