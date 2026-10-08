/// <summary>When the client calculates request checksums and validates response checksums.</summary>
public enum ChecksumPolicy
{
    /// <summary>Checksum every request and validate every response whose operation supports it.</summary>
    WhenSupported,
    /// <summary>Only when the model requires it or the caller chose an algorithm.</summary>
    WhenRequired
}
