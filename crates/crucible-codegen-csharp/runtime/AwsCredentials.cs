public sealed record AwsCredentials(string AccessKeyId, string SecretAccessKey, string? SessionToken = null, DateTimeOffset? ExpiresAt = null)
{
    public override string ToString() => "AwsCredentials { redacted }";
}
