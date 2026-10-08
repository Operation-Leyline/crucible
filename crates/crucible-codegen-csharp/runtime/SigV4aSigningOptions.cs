/// <summary>
/// SigV4a signing settings. <see cref="RegionSet"/> is the comma-separated <c>X-Amz-Region-Set</c>, such as <c>us-east-1,us-west-2</c>
/// or <c>*</c>. <see cref="SignContentSha256Header"/> false leaves <c>X-Amz-Content-Sha256</c> unsent and unsigned (the payload
/// hash still ends the canonical request), as the published AWS test suite's <c>sign_body=false</c> cases do.
/// </summary>
/// <param name="RegionSet">The comma-separated region set.</param>
/// <param name="Service">The signing name of the service.</param>
public sealed record SigV4aSigningOptions(string RegionSet, string Service)
{
    /// <summary>The signing time; now when null.</summary>
    public DateTimeOffset? Time { get; init; }
    /// <summary>Signs <c>UNSIGNED-PAYLOAD</c> instead of the body hash.</summary>
    public bool UnsignedPayload { get; init; }
    /// <summary>Whether <c>X-Amz-Content-Sha256</c> is sent and signed.</summary>
    public bool SignContentSha256Header { get; init; } = true;
    /// <summary>Overrides the service default for double-encoding path segments.</summary>
    public bool? DisableDoubleEncoding { get; init; }
    /// <summary>Overrides the service default for path normalization.</summary>
    public bool? DisableNormalizePath { get; init; }
}
