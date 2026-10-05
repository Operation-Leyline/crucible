public sealed class ChecksumException(string algorithm) : Exception("Response " + algorithm + " checksum mismatch")
{
    public string Algorithm { get; } = algorithm;
}
