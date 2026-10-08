/**
 * How a client limits retries on top of the shared retry eligibility rules.
 *
 * [LEGACY] (the default) retries every eligible failure up to `maxAttempts`. [STANDARD] adds a
 * client-wide [RetryQuota], so a client stops retrying during an outage. [ADAPTIVE] is standard
 * plus a client-side send-rate limiter that slows down after throttling responses.
 */
enum class RetryMode {
    /** Retry every eligible failure up to `maxAttempts`. */
    LEGACY,

    /** Add a client-wide [RetryQuota]. */
    STANDARD,

    /** Standard plus a client-side send-rate limiter. */
    ADAPTIVE,
}
