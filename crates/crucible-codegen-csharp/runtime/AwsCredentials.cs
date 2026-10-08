/// <summary>AWS access credentials, optionally expiring. Diagnostic strings exclude credential material.</summary>
/// <param name="AccessKeyId">The access key id.</param>
/// <param name="SecretAccessKey">The secret key.</param>
/// <param name="SessionToken">The session token, or null.</param>
/// <param name="ExpiresAt">When the credentials expire, or null if they do not.</param>
public sealed record AwsCredentials(string AccessKeyId, string SecretAccessKey, string? SessionToken = null, DateTimeOffset? ExpiresAt = null)
{
    /// <inheritdoc/>
    public override string ToString() => "AwsCredentials { redacted }";
}
