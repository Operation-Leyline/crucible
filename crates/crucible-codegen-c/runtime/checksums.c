#include "features.h"
#include <openssl/evp.h>
#include <string.h>

typedef crucible_generated_status status;
typedef crucible_generated_string string;
typedef crucible_generated_error error;
typedef crucible_generated_arena arena;
#define OK crucible_generated_ok
#define INVALID crucible_generated_invalid
#define TRY(call) do { status code_ = (call); if (code_) return code_; } while (0)
#define VIEW(text) crucible_generated_string_view(text)
static const yyjson_val *get(const yyjson_val *value, const char *key) { return yyjson_obj_get((yyjson_val *)value, key); }
static string text(const yyjson_val *value) { return (string){yyjson_get_str((yyjson_val *)value), yyjson_get_len((yyjson_val *)value)}; }

/* Reflected CRC tables are rebuilt per call: cheap next to network I/O and free of
 * shared mutable state. */
static void crc32_tables(uint32_t *table, uint32_t polynomial) {
    for (uint32_t n = 0; n < 256; ++n) {
        uint32_t value = n;
        for (int k = 0; k < 8; ++k) value = value & 1 ? (value >> 1) ^ polynomial : value >> 1;
        table[n] = value;
    }
}
static const char *const algorithms[] = {"CRC32", "CRC32C", "CRC64NVME", "MD5", "SHA1", "SHA256", "SHA512"};
static const char *supported(string name) {
    for (size_t i = 0; i < sizeof(algorithms) / sizeof(algorithms[0]); ++i) if (crucible_generated_string_equal(name, VIEW(algorithms[i]))) return algorithms[i];
    return NULL;
}
status crucible_generated_checksum(string algorithm, crucible_generated_blob data, char output[89], error *issue) {
    unsigned char raw[EVP_MAX_MD_SIZE]; size_t length = 0;
    if (!output || (!data.data && data.length)) return crucible_generated_fail(issue, INVALID, "Invalid checksum arguments");
    const unsigned char *bytes = data.length ? data.data : (const unsigned char *)"";
    const EVP_MD *md = NULL;
    if (crucible_generated_string_equal(algorithm, VIEW("CRC32")) || crucible_generated_string_equal(algorithm, VIEW("CRC32C"))) {
        uint32_t table[256];
        crc32_tables(table, algorithm.length == 5 ? 0xEDB88320u : 0x82F63B78u);
        uint32_t crc = 0xFFFFFFFFu;
        for (size_t i = 0; i < data.length; ++i) crc = table[(crc ^ bytes[i]) & 255] ^ (crc >> 8);
        crc ^= 0xFFFFFFFFu;
        for (int i = 0; i < 4; ++i) raw[i] = (unsigned char)(crc >> (24 - 8 * i));
        length = 4;
    } else if (crucible_generated_string_equal(algorithm, VIEW("CRC64NVME"))) {
        uint64_t table[256], crc = ~0ull;
        for (uint64_t n = 0; n < 256; ++n) {
            uint64_t value = n;
            for (int k = 0; k < 8; ++k) value = value & 1 ? (value >> 1) ^ 0x9A6C9329AC4BC9B5ull : value >> 1;
            table[n] = value;
        }
        for (size_t i = 0; i < data.length; ++i) crc = table[(crc ^ bytes[i]) & 255] ^ (crc >> 8);
        crc = ~crc;
        for (int i = 0; i < 8; ++i) raw[i] = (unsigned char)(crc >> (56 - 8 * i));
        length = 8;
    } else if (crucible_generated_string_equal(algorithm, VIEW("MD5"))) md = EVP_md5();
    else if (crucible_generated_string_equal(algorithm, VIEW("SHA1"))) md = EVP_sha1();
    else if (crucible_generated_string_equal(algorithm, VIEW("SHA256"))) md = EVP_sha256();
    else if (crucible_generated_string_equal(algorithm, VIEW("SHA512"))) md = EVP_sha512();
    else return crucible_generated_fail(issue, INVALID, "Unsupported checksum algorithm");
    if (md) {
        unsigned size = 0;
        if (EVP_Digest(bytes, data.length, raw, &size, md, NULL) != 1) return crucible_generated_fail(issue, INVALID, "Checksum digest failed");
        length = size;
    }
    crucible_generated_base64(raw, length, output);
    return OK;
}
static status set_member(arena *storage, const yyjson_val **input, string member, const char *value, error *issue) {
    yyjson_mut_doc *doc = yyjson_mut_doc_new(NULL);
    if (!doc) return crucible_generated_fail(issue, crucible_generated_out_of_memory, "JSON allocation failed");
    yyjson_mut_val *copy = yyjson_val_mut_copy(doc, (yyjson_val *)*input);
    yyjson_mut_val *key = yyjson_mut_strncpy(doc, member.data, member.length), *item = yyjson_mut_strcpy(doc, value);
    status code = copy && key && item && yyjson_mut_obj_put(copy, key, item) ? crucible_generated_json_own(storage, copy, input, issue)
        : crucible_generated_fail(issue, crucible_generated_out_of_memory, "JSON allocation failed");
    yyjson_mut_doc_free(doc);
    return code;
}
static const char *algorithm_header(const char *algorithm) {
    /* Static, lowercase header names: "x-amz-checksum-" plus the algorithm. */
    static const char *const names[] = {"x-amz-checksum-crc32", "x-amz-checksum-crc32c", "x-amz-checksum-crc64nvme", "x-amz-checksum-md5",
        "x-amz-checksum-sha1", "x-amz-checksum-sha256", "x-amz-checksum-sha512"};
    for (size_t i = 0; i < sizeof(algorithms) / sizeof(algorithms[0]); ++i) if (algorithms[i] == algorithm) return names[i];
    return NULL;
}
status crucible_generated_checksum_prepare(arena *storage, const yyjson_val *op, const yyjson_val **input,
    crucible_generated_checksum_policy request_policy, crucible_generated_checksum_policy response_policy,
    crucible_generated_checksum_settings *settings, error *issue) {
    *settings = (crucible_generated_checksum_settings){0};
    const yyjson_val *description = get(op, "checksum");
    if (!yyjson_is_obj(description)) return OK;
    if (crucible_generated_string_equal(text(get(description, "kind")), VIEW("md5"))) { settings->algorithm = "MD5"; settings->header = "Content-MD5"; return OK; }
    const bool required = yyjson_get_bool((yyjson_val *)get(description, "requestRequired"));
    const yyjson_val *member = get(description, "requestAlgorithmMember");
    if (yyjson_is_str(member)) {
        string name = text(member);
        const yyjson_val *value = yyjson_obj_getn((yyjson_val *)*input, name.data, name.length);
        string algorithm = yyjson_is_str(value) ? text(value) : (string){"", 0};
        if (!algorithm.length && (required || request_policy != crucible_generated_checksum_when_required)) {
            algorithm = VIEW("CRC32");
            TRY(set_member(storage, input, name, "CRC32", issue));
        }
        if (algorithm.length) {
            settings->algorithm = supported(algorithm);
            if (!settings->algorithm) return crucible_generated_fail(issue, INVALID, "Unsupported checksum algorithm");
            settings->header = algorithm_header(settings->algorithm);
        }
    } else if (required) { settings->algorithm = "MD5"; settings->header = "Content-MD5"; }
    const yyjson_val *mode_member = get(description, "validationModeMember");
    if (yyjson_is_str(mode_member)) {
        string name = text(mode_member);
        const yyjson_val *value = yyjson_obj_getn((yyjson_val *)*input, name.data, name.length);
        string mode = yyjson_is_str(value) ? text(value) : (string){"", 0};
        if (!mode.length && response_policy != crucible_generated_checksum_when_required) {
            mode = VIEW("ENABLED");
            TRY(set_member(storage, input, name, "ENABLED", issue));
        }
        if (crucible_generated_string_equal(mode, VIEW("ENABLED"))) {
            size_t index, count; yyjson_val *item;
            yyjson_arr_foreach((yyjson_val *)get(description, "responseAlgorithms"), index, count, item) {
                const char *algorithm = supported(text(item));
                if (algorithm && settings->response_count < 8) settings->response[settings->response_count++] = algorithm;
            }
        }
    }
    return OK;
}
status crucible_generated_checksum_request(arena *storage, const crucible_generated_checksum_settings *settings, crucible_generated_request *request, error *issue) {
    char value[89];
    if (!settings->header || crucible_generated_header_get(request->headers, VIEW(settings->header), NULL)) return OK;
    TRY(crucible_generated_checksum(VIEW(settings->algorithm), request->has_body ? request->body : (crucible_generated_blob){0}, value, issue));
    return crucible_generated_header_set(storage, &request->headers, VIEW(settings->header), VIEW(value), issue);
}
static bool constant_equal(string a, string b) {
    unsigned char difference = a.length == b.length ? 0 : 1;
    size_t length = a.length < b.length ? a.length : b.length;
    for (size_t i = 0; i < length; ++i) difference |= (unsigned char)(a.data[i] ^ b.data[i]);
    return difference == 0;
}
status crucible_generated_checksum_response(const crucible_generated_checksum_settings *settings, const crucible_generated_response *response,
    const char **validated, error *issue) {
    *validated = NULL;
    for (size_t i = 0; i < settings->response_count; ++i) {
        string expected;
        if (!crucible_generated_header_get(response->headers, VIEW(algorithm_header(settings->response[i])), &expected)) continue;
        char actual[89];
        TRY(crucible_generated_checksum(VIEW(settings->response[i]), response->body, actual, issue));
        if (!constant_equal(expected, VIEW(actual))) {
            char message[64] = "Response checksum mismatch: ";
            size_t used = strlen(message), length = strlen(settings->response[i]);
            memcpy(message + used, settings->response[i], length + 1);
            return crucible_generated_fail(issue, crucible_generated_checksum_mismatch, message);
        }
        *validated = settings->response[i];
        return OK;
    }
    return OK;
}
