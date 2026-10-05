#include "checksums.hpp"
#include "types.hpp"
#include <openssl/evp.h>
#include <array>
#include <cstdint>

namespace crucible_generated::runtime {
namespace {
template<class T, T polynomial> std::array<T, 256> table() {
    std::array<T, 256> result{};
    for (unsigned index = 0; index < 256; ++index) {
        T value = static_cast<T>(index);
        for (int bit = 0; bit < 8; ++bit) value = (value & 1) ? static_cast<T>((value >> 1) ^ polynomial) : static_cast<T>(value >> 1);
        result[index] = value;
    }
    return result;
}
// Reflected CRCs with all-ones initial value and final inversion, as the AWS SDKs compute them.
template<class T, T polynomial> std::string crc(std::string_view data) {
    static const auto values = table<T, polynomial>();
    T value = static_cast<T>(~T{});
    for (const unsigned char byte : data) value = static_cast<T>(values[(value ^ byte) & 0xff] ^ (value >> 8));
    value = static_cast<T>(~value);
    std::string result(sizeof(T), '\0');
    for (std::size_t index = 0; index < sizeof(T); ++index) result[sizeof(T) - 1 - index] = static_cast<char>((value >> (8 * index)) & 0xff);
    return result;
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
    if (algorithm == "CRC32") return crc<std::uint32_t, 0xEDB88320U>(data);
    if (algorithm == "CRC32C") return crc<std::uint32_t, 0x82F63B78U>(data);
    if (algorithm == "CRC64NVME") return crc<std::uint64_t, 0x9A6C9329AC4BC9B5ULL>(data);
    if (algorithm == "MD5") return evp(EVP_md5(), data);
    if (algorithm == "SHA1") return evp(EVP_sha1(), data);
    if (algorithm == "SHA256") return evp(EVP_sha256(), data);
    if (algorithm == "SHA512") return evp(EVP_sha512(), data);
    throw std::invalid_argument("Unsupported checksum algorithm: " + std::string(algorithm));
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
