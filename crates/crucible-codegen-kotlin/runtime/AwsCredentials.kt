import kotlin.time.Instant

/** AWS access credentials; diagnostic strings exclude credential material. */
class AwsCredentials(val accessKeyId: String, val secretAccessKey: String,
    val sessionToken: String? = null, val expiresAt: Instant? = null) {
    init { require(accessKeyId.isNotEmpty() && secretAccessKey.isNotEmpty()) { "AWS access key and secret must be nonempty" } }
    override fun toString(): String = "AwsCredentials[redacted]"
}
