public sealed record SigningOptions(string Region, string Service)
{
    public DateTimeOffset? Time { get; init; }
    public bool UnsignedPayload { get; init; }
    public bool? DisableDoubleEncoding { get; init; }
    public bool? DisableNormalizePath { get; init; }
}
