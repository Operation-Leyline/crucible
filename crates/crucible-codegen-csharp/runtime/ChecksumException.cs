/// <summary>A received response body failed its modeled checksum; no decoded response was returned.</summary>
/// <param name="algorithm">The algorithm that failed, such as <c>CRC32</c>.</param>
public sealed class ChecksumException(string algorithm) : Exception("Response " + algorithm + " checksum mismatch")
{
    /// <summary>The algorithm that failed.</summary>
    public string Algorithm { get; } = algorithm;
}
