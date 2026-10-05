#include "checksum_client/client.h"
#define PREFIX checksum
#include "echo.h"

#define VIEW(text) checksum_string_view(text)
static const char *const algorithms[] = {"CRC32", "CRC32C", "CRC64NVME", "MD5", "SHA1", "SHA256", "SHA512"};
typedef struct { char last[64]; int count; } validations;
static void validated(void *user, checksum_string operation, checksum_string algorithm) {
    validations *seen = user;
    (void)snprintf(seen->last, sizeof(seen->last), "%.*s:%.*s", (int)operation.length, operation.data, (int)algorithm.length, algorithm.data);
    ++seen->count;
}
static checksum_status replace_body(void *user, const checksum_scope *scope, checksum_arena *arena, checksum_request *request, checksum_error *error) {
    (void)scope; (void)arena; (void)error;
    const char *body = user;
    request->has_body = true; request->body = (checksum_blob){(const unsigned char *)body, strlen(body)};
    return checksum_ok;
}
static const checksum_json *vectors(checksum_arena *arena) {
    FILE *stream = fopen(environment("CHECKSUM_CASES"), "rb");
    check(stream != NULL, "open checksum vectors");
    char buffer[4096]; size_t length = fread(buffer, 1, sizeof(buffer), stream); fclose(stream);
    const checksum_json *value = NULL; checksum_error error = {0};
    good(checksum_json_parse(arena, (checksum_string){buffer, length}, &value, &error), &error, "vectors");
    return value;
}
static checksum_client *client_new(checksum_client_config config) {
    checksum_client *client = NULL; checksum_error error = {0};
    good(checksum_client_new(&config, &client, &error), &error, "client_new");
    return client;
}
static checksum_client_config with_mode(checksum_headers *headers, checksum_header *storage, const char *mode) {
    checksum_client_config config = checksum_client_defaults();
    config.endpoint = VIEW(environment("FEATURES_URL"));
    storage[0] = (checksum_header){VIEW("x-mode"), VIEW(mode)};
    *headers = (checksum_headers){storage, 1};
    config.headers = *headers;
    return config;
}
static checksum_string expected(const checksum_json *cases, const char *algorithm) {
    checksum_string value = {0}; checksum_error error = {0};
    good(checksum_json_text(checksum_json_get(checksum_json_get(cases, VIEW("base64")), VIEW(algorithm)), &value, &error), &error, algorithm);
    return value;
}

int main(void) {
    checksum_arena *arena = checksum_arena_new(); checksum_error error = {0};
    const checksum_json *cases = vectors(arena);
    checksum_string input_text = echo_text(cases, "inputUtf8");
    checksum_blob payload = {(const unsigned char *)input_text.data, input_text.length};
    char digest[89];
    for (size_t i = 0; i < sizeof(algorithms) / sizeof(algorithms[0]); ++i) {
        good(checksum_checksum(VIEW(algorithms[i]), payload, digest, &error), &error, algorithms[i]);
        check(equal(expected(cases, algorithms[i]), digest), algorithms[i]);
    }
    good(checksum_checksum(VIEW("MD5"), (checksum_blob){NULL, 0}, digest, &error), &error, "empty MD5");
    check(equal(echo_text(cases, "emptyMd5"), digest), "empty MD5 vector");
    check(checksum_checksum(VIEW("XXHASH64"), payload, digest, &error) == checksum_invalid, "unsupported algorithm");

    validations seen = {{0}, 0};
    checksum_headers headers; checksum_header storage[1];
    checksum_client_config config = with_mode(&headers, storage, "normal");
    config.checksum_validated = validated; config.checksum_user = &seen;
    checksum_client *client = client_new(config);
    checksum_models_input input = {0}, empty = {0};
    checksum_models_input_set_data(&input, payload);
    checksum_operations_legacy_result legacy = {0};
    good(checksum_operations_legacy(client, &input, NULL, NULL, &legacy, &error), &error, "legacy MD5");
    check(legacy.value->data.value.length == payload.length, "legacy payload");
    checksum_operations_legacy_result_free(&legacy);
    checksum_operations_required_result required = {0};
    good(checksum_operations_required(client, &input, NULL, NULL, &required, &error), &error, "required MD5");
    checksum_operations_required_result_free(&required);
    good(checksum_operations_legacy(client, &empty, NULL, NULL, &legacy, &error), &error, "empty MD5 request");
    checksum_operations_legacy_result_free(&legacy);
    checksum_operations_flexible_result flexible = {0};
    good(checksum_operations_flexible(client, &input, NULL, NULL, &flexible, &error), &error, "default flexible");
    check(!checksum_models_input_has_algorithm(&input) && !checksum_models_input_has_validation(&input), "defaults stay private");
    check(equal(echo_header(flexible.base.arena, &flexible.base.response, "x-validation"), "ENABLED"), "validation mode enabled by default");
    check(equal(echo_header(flexible.base.arena, &flexible.base.response, "x-algorithm"), "CRC32"), "CRC32 is the default algorithm");
    check(!strcmp(seen.last, "Flexible:CRC32"), "default response validation");
    checksum_operations_flexible_result_free(&flexible);
    for (size_t i = 0; i < sizeof(algorithms) / sizeof(algorithms[0]); ++i) {
        checksum_models_input selected = {0};
        checksum_models_input_set_data(&selected, payload);
        checksum_models_input_set_algorithm(&selected, VIEW(algorithms[i]));
        good(checksum_operations_flexible(client, &selected, NULL, NULL, &flexible, &error), &error, algorithms[i]);
        char name[32]; (void)snprintf(name, sizeof(name), "Flexible:%s", algorithms[i]);
        check(!strcmp(seen.last, name), "validation callback per algorithm");
        checksum_operations_flexible_result_free(&flexible);
    }
    checksum_models_input unsupported = {0};
    checksum_models_input_set_algorithm(&unsupported, VIEW("XXHASH64"));
    check(checksum_operations_flexible(client, &unsupported, NULL, NULL, &flexible, &error) == checksum_invalid, "unsupported request algorithm");
    checksum_operations_flexible_result_free(&flexible);
    checksum_client_free(client);

    config = with_mode(&headers, storage, "bad"); config.checksum_validated = validated; config.checksum_user = &seen;
    client = client_new(config);
    check(checksum_operations_flexible(client, &input, NULL, NULL, &flexible, &error) == checksum_checksum_mismatch && message_contains(&error, "CRC32"), "corruption detected");
    check(flexible.base.response.status == 200, "mismatch keeps response metadata");
    checksum_operations_flexible_result_free(&flexible); checksum_client_free(client);

    config = with_mode(&headers, storage, "one"); config.checksum_validated = validated; config.checksum_user = &seen;
    client = client_new(config);
    good(checksum_operations_flexible(client, &input, NULL, NULL, &flexible, &error), &error, "first modeled checksum wins");
    checksum_operations_flexible_result_free(&flexible); checksum_client_free(client);

    config = with_mode(&headers, storage, "unmodeled"); config.checksum_validated = validated; config.checksum_user = &seen;
    client = client_new(config);
    int count = seen.count;
    good(checksum_operations_flexible(client, &input, NULL, NULL, &flexible, &error), &error, "unmodeled checksum");
    check(seen.count == count, "unmodeled checksum is not validated");
    checksum_operations_flexible_result_free(&flexible); checksum_client_free(client);

    config = with_mode(&headers, storage, "override");
    client = client_new(config);
    checksum_models_input explicit_checksum = {0};
    checksum_models_input_set_data(&explicit_checksum, payload);
    checksum_models_input_set_checksum(&explicit_checksum, VIEW("supplied"));
    good(checksum_operations_flexible(client, &explicit_checksum, NULL, NULL, &flexible, &error), &error, "caller checksum");
    check(equal(echo_header(flexible.base.arena, &flexible.base.response, "x-amz-checksum-crc32"), "supplied"), "caller checksum is kept");
    checksum_operations_flexible_result_free(&flexible); checksum_client_free(client);

    config = with_mode(&headers, storage, "skip");
    config.request_checksum_calculation = checksum_checksum_when_required; config.response_checksum_validation = checksum_checksum_when_required;
    client = client_new(config);
    good(checksum_operations_flexible(client, &input, NULL, NULL, &flexible, &error), &error, "opt-out");
    check(!echo_header(flexible.base.arena, &flexible.base.response, "x-algorithm").data && !echo_header(flexible.base.arena, &flexible.base.response, "x-validation").data, "opt-out sends no defaults");
    checksum_operations_flexible_result_free(&flexible); checksum_client_free(client);

    checksum_middleware hook = {replace_body, (void *)"123456789"};
    config = with_mode(&headers, storage, "normal"); config.middleware = &hook; config.middleware_count = 1;
    client = client_new(config);
    good(checksum_operations_legacy(client, &empty, NULL, NULL, &legacy, &error), &error, "checksum covers the final body");
    checksum_operations_legacy_result_free(&legacy); checksum_client_free(client);
    checksum_arena_free(arena);
    puts("C HTTP checksums passed");
    return 0;
}
