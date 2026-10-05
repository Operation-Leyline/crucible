#include "checksums.hpp"
#include "types.hpp"
#include <openssl/evp.h>
#include <array>
#include <cstdint>

namespace crucible_generated::runtime {
namespace {
template<class T, T polynomial> const std::array<T, 256>& table() {
    static const auto values = [] {
        std::array<T, 256> result{};
        for (unsigned index = 0; index < 256; ++index) {
            T value = static_cast<T>(index);
            for (int bit = 0; bit < 8; ++bit) value = (value & 1) ? static_cast<T>((value >> 1) ^ polynomial) : static_cast<T>(value >> 1);
            result[index] = value;
        }
        return result;
    }();
    return values;
}
// Reflected CRCs with all-ones initial value and final inversion, as the AWS SDKs compute them.
// The state is kept inverted, so it starts at zero and finish() inverts once.
template<class T, T polynomial> std::uint64_t crc_update(std::uint64_t state, std::string_view data) {
    const auto& values = table<T, polynomial>();
    T value = static_cast<T>(~static_cast<T>(state));
    for (const unsigned char byte : data) value = static_cast<T>(values[(value ^ byte) & 0xff] ^ (value >> 8));
    return static_cast<T>(~value);
}
template<class T> std::string big_endian(std::uint64_t value) {
    std::string result(sizeof(T), '\0');
    for (std::size_t index = 0; index < sizeof(T); ++index) result[sizeof(T) - 1 - index] = static_cast<char>((value >> (8 * index)) & 0xff);
    return result;
}
const EVP_MD* evp_type(std::string_view algorithm) {
    if (algorithm == "MD5") return EVP_md5();
    if (algorithm == "SHA1") return EVP_sha1();
    if (algorithm == "SHA256") return EVP_sha256();
    if (algorithm == "SHA512") return EVP_sha512();
    return nullptr;
}
std::string evp(const EVP_MD* type, std::string_view data) {
    std::array<unsigned char, EVP_MAX_MD_SIZE> bytes{};
    unsigned length = 0;
    if (EVP_Digest(data.data(), data.size(), bytes.data(), &length, type, nullptr) != 1) throw std::runtime_error("Digest failed");
    return std::string(reinterpret_cast<const char*>(bytes.data()), length);
}
}
bool checksum_supported(std::string_view algorithm) noexcept {
    return algorithm == "CRC32" || algorithm == "CRC32C" || algorithm == "CRC64NVME" || algorithm == "MD5" || algorithm == "SHA1" || algorithm == "SHA256" || algorithm == "SHA512";
}
std::string digest_bytes(std::string_view algorithm, std::string_view data) {
    if (const auto* type = evp_type(algorithm)) return evp(type, data);
    IncrementalChecksum checksum{std::string(algorithm)};
    checksum.update(data);
    return checksum.finish();
}
struct IncrementalChecksum::Digest {
    EVP_MD_CTX* context = EVP_MD_CTX_new();
    ~Digest() { EVP_MD_CTX_free(context); }
};
IncrementalChecksum::IncrementalChecksum(std::string algorithm) : algorithm_(std::move(algorithm)) {
    if (!checksum_supported(algorithm_)) throw std::invalid_argument("Unsupported checksum algorithm: " + algorithm_);
    if (const auto* type = evp_type(algorithm_)) {
        digest_ = std::make_shared<Digest>();
        if (!digest_->context || EVP_DigestInit_ex(digest_->context, type, nullptr) != 1) throw std::runtime_error("Digest failed");
    }
}
void IncrementalChecksum::update(std::string_view data) {
    if (digest_) { if (EVP_DigestUpdate(digest_->context, data.data(), data.size()) != 1) throw std::runtime_error("Digest failed"); return; }
    if (algorithm_ == "CRC32") crc_ = crc_update<std::uint32_t, 0xEDB88320U>(crc_, data);
    else if (algorithm_ == "CRC32C") crc_ = crc_update<std::uint32_t, 0x82F63B78U>(crc_, data);
    else crc_ = crc_update<std::uint64_t, 0x9A6C9329AC4BC9B5ULL>(crc_, data);
}
std::string IncrementalChecksum::finish() {
    if (digest_) {
        std::array<unsigned char, EVP_MAX_MD_SIZE> bytes{};
        unsigned length = 0;
        if (EVP_DigestFinal_ex(digest_->context, bytes.data(), &length) != 1) throw std::runtime_error("Digest failed");
        return std::string(reinterpret_cast<const char*>(bytes.data()), length);
    }
    return algorithm_ == "CRC64NVME" ? big_endian<std::uint64_t>(crc_) : big_endian<std::uint32_t>(crc_);
}
std::string checksum(std::string_view algorithm, std::string_view data) {
    const auto raw = digest_bytes(algorithm, data);
    Blob bytes;
    bytes.reserve(raw.size());
    for (const unsigned char byte : raw) bytes.push_back(static_cast<std::byte>(byte));
    return base64(bytes);
}
std::string hex(std::string_view bytes) {
    constexpr char alphabet[] = "0123456789abcdef";
    std::string result;
    result.reserve(bytes.size() * 2);
    for (const unsigned char byte : bytes) { result += alphabet[byte >> 4]; result += alphabet[byte & 15]; }
    return result;
}
}
