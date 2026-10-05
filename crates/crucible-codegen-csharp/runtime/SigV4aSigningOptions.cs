/// <summary>
/// SigV4a signing settings. <see cref="RegionSet"/> is the comma-separated <c>X-Amz-Region-Set</c>, such as <c>us-east-1,us-west-2</c>
/// or <c>*</c>. <see cref="SignContentSha256Header"/> false leaves <c>X-Amz-Content-Sha256</c> unsent and unsigned (the payload
/// hash still ends the canonical request), as the published AWS test suite's <c>sign_body=false</c> cases do.
/// </summary>
public sealed record SigV4aSigningOptions(string RegionSet, string Service)
{
    public DateTimeOffset? Time { get; init; }
    public bool UnsignedPayload { get; init; }
    public bool SignContentSha256Header { get; init; } = true;
    public bool? DisableDoubleEncoding { get; init; }
    public bool? DisableNormalizePath { get; init; }
}
