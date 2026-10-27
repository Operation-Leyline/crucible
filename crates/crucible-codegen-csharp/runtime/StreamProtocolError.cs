/// <summary>Invalid or truncated Crucible response-stream framing or values.</summary>
public sealed class StreamProtocolError : Exception
{
    /// <summary>Creates a protocol failure without disclosing response contents.</summary>
    /// <param name="message">The protocol rule that failed.</param>
    public StreamProtocolError(string message) : base(message) { }
}
