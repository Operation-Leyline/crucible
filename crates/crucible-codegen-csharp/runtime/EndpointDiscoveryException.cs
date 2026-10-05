/// <summary>An operation requires endpoint discovery, which is disabled or produced no usable endpoint.</summary>
public sealed class EndpointDiscoveryException(string message) : Exception(message);
