public sealed class NetworkError(string message, Exception inner) : Exception(message, inner);
