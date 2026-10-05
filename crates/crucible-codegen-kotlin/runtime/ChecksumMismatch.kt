/** No decoded result is returned when its modeled response checksum fails. */
class ChecksumMismatch(val algorithm: String) : RuntimeException("Response checksum mismatch: $algorithm")
