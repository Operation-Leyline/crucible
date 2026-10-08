/// <summary>SigV4 signing settings.</summary>
/// <param name="Region">The signing region.</param>
/// <param name="Service">The signing name of the service.</param>
public sealed record SigningOptions(string Region, string Service)
{
    /// <summary>The signing time; now when null.</summary>
    public DateTimeOffset? Time { get; init; }
    /// <summary>Signs <c>UNSIGNED-PAYLOAD</c> instead of the body hash.</summary>
    public bool UnsignedPayload { get; init; }
    /// <summary>Overrides the service default for double-encoding path segments.</summary>
    public bool? DisableDoubleEncoding { get; init; }
    /// <summary>Overrides the service default for path normalization.</summary>
    public bool? DisableNormalizePath { get; init; }
}
