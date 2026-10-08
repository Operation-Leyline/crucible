/**
 * No decoded result is returned when its modeled response checksum fails.
 *
 * @property algorithm the algorithm that failed, such as `CRC32`
 */
class ChecksumMismatch(val algorithm: String) : RuntimeException("Response checksum mismatch: $algorithm")
