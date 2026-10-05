/** AWS access credentials, optionally expiring. Secrets are excluded from diagnostic strings. */
public record AwsCredentials(String accessKeyId, String secretAccessKey, String sessionToken, java.time.Instant expiresAt) {
  public AwsCredentials(String accessKeyId, String secretAccessKey) { this(accessKeyId, secretAccessKey, null, null); }
  public AwsCredentials {
    if (accessKeyId == null || accessKeyId.isEmpty() || secretAccessKey == null || secretAccessKey.isEmpty())
      throw new IllegalArgumentException("AWS access key and secret must be nonempty");
  }
  @Override public String toString() { return "AwsCredentials[redacted]"; }
}
