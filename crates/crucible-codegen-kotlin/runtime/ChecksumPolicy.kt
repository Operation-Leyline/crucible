/** When the client calculates request checksums and validates response checksums. */
enum class ChecksumPolicy {
    /** Checksum every request and validate every response whose operation supports it. */
    WHEN_SUPPORTED,

    /** Only when the model requires it or the caller chose an algorithm. */
    WHEN_REQUIRED,
}
