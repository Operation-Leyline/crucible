public sealed class WaiterTimeoutException(string name, int attempts, object? response, Exception? error)
    : WaiterException(name, attempts, response, error, "Deadline exceeded");
