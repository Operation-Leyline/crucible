/// <summary>The HTTP transport failed before a response arrived (connection, TLS or timeout); see the inner exception.</summary>
/// <param name="message">A description of the failure.</param>
/// <param name="inner">The underlying exception.</param>
public sealed class NetworkError(string message, Exception inner) : Exception(message, inner);
