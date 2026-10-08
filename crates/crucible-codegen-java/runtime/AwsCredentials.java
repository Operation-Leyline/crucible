/**
 * AWS access credentials, optionally expiring. Secrets are excluded from diagnostic strings.
 *
 * @param accessKeyId the access key id
 * @param secretAccessKey the secret key
 * @param sessionToken the session token, or {@code null}
 * @param expiresAt when the credentials expire, or {@code null} if they do not
 */
public record AwsCredentials(String accessKeyId, String secretAccessKey, String sessionToken, java.time.Instant expiresAt) {
  /**
   * Long-lived credentials.
   *
   * @param accessKeyId the access key id
   * @param secretAccessKey the secret key
   */
  public AwsCredentials(String accessKeyId, String secretAccessKey) { this(accessKeyId, secretAccessKey, null, null); }
  /** Requires a nonempty key and secret. */
  public AwsCredentials {
    if (accessKeyId == null || accessKeyId.isEmpty() || secretAccessKey == null || secretAccessKey.isEmpty())
      throw new IllegalArgumentException("AWS access key and secret must be nonempty");
  }
  @Override public String toString() { return "AwsCredentials[redacted]"; }
}
