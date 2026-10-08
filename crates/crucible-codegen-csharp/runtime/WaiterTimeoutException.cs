/// <summary>The overall polling deadline expired, including operation and retry time.</summary>
/// <param name="name">The waiter name.</param>
/// <param name="attempts">The polls made.</param>
/// <param name="response">The last output.</param>
/// <param name="error">The last error.</param>
public sealed class WaiterTimeoutException(string name, int attempts, object? response, Exception? error)
    : WaiterException(name, attempts, response, error, "Deadline exceeded");
