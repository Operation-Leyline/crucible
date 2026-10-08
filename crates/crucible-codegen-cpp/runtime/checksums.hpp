#pragma once
#include <cstdint>
#include <memory>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

namespace crucible_generated::runtime {
/// AWS SDK checksum policies: WHEN_SUPPORTED adds optional checksums; WHEN_REQUIRED only modeled requirements.
enum class ChecksumPolicy { when_supported, when_required };
class ChecksumError : public std::runtime_error {
    std::string algorithm_;
public:
    explicit ChecksumError(std::string algorithm) : std::runtime_error("Response " + algorithm + " checksum mismatch"), algorithm_(std::move(algorithm)) {}
    [[nodiscard]] const std::string& algorithm() const noexcept { return algorithm_; }
};
[[nodiscard]] bool checksum_supported(std::string_view algorithm) noexcept;
/// Raw big-endian digest bytes for CRC32, CRC32C, CRC64NVME, MD5, SHA1, SHA256, and SHA512.
[[nodiscard]] std::string digest_bytes(std::string_view algorithm, std::string_view data);
/// The base64 header value AWS uses for a checksum.
[[nodiscard]] std::string checksum(std::string_view algorithm, std::string_view data);
[[nodiscard]] std::string hex(std::string_view bytes);
/// A checksum computed as bytes arrive, for streaming bodies.
class IncrementalChecksum {
    struct Digest;
    std::string algorithm_;
    std::uint64_t crc_ = 0;
    std::shared_ptr<Digest> digest_;
public:
    explicit IncrementalChecksum(std::string algorithm);
    [[nodiscard]] const std::string& algorithm() const noexcept { return algorithm_; }
    void update(std::string_view data);
    /// Raw big-endian digest bytes of everything passed to update.
    [[nodiscard]] std::string finish();
};
struct ChecksumSettings {
    std::string algorithm;
    std::string header;
    std::vector<std::string> response;
};
}
