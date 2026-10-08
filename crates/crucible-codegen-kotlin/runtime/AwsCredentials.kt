import kotlin.time.Instant

/**
 * AWS access credentials, optionally expiring. Diagnostic strings exclude credential material.
 *
 * @property accessKeyId the access key id
 * @property secretAccessKey the secret key
 * @property sessionToken the session token, if any
 * @property expiresAt when the credentials expire, or null if they do not
 */
class AwsCredentials(val accessKeyId: String, val secretAccessKey: String,
    val sessionToken: String? = null, val expiresAt: Instant? = null) {
    init { require(accessKeyId.isNotEmpty() && secretAccessKey.isNotEmpty()) { "AWS access key and secret must be nonempty" } }
    override fun toString(): String = "AwsCredentials[redacted]"
}
