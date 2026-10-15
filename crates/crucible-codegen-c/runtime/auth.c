#include "features.h"
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
static unsigned char lower(unsigned char c) { return c >= 'A' && c <= 'Z' ? (unsigned char)(c + 32) : c; }
static bool iequal(string a, string b) {
    if (a.length != b.length) return false;
    for (size_t i = 0; i < a.length; ++i) if (lower((unsigned char)a.data[i]) != lower((unsigned char)b.data[i])) return false;
    return true;
}
/* A caller-supplied Authorization header bypasses the token provider. */
static bool preset(const crucible_generated_request *request) {
    string value = {0};
    const string prefix = VIEW("Bearer");
    if (!crucible_generated_header_get(request->headers, VIEW("authorization"), &value)) return false;
    return value.length > prefix.length + 1 && iequal((string){value.data, prefix.length}, prefix) && value.data[prefix.length] == ' ';
}
status crucible_generated_authenticate(crucible_generated_client *client, const yyjson_val *op, const crucible_generated_scope *scope,
    arena *storage, crucible_generated_request *request, bool *credentials, error *issue) {
    const crucible_generated_client_config *config = &client->config;
    *credentials = false;
    if (!yyjson_get_bool((yyjson_val *)get(op, "bearer"))) return OK;
    if (preset(request)) { *credentials = true; return OK; }
    if (!config->token) return OK;
    bool present = false; string token = {0};
    TRY(config->token(config->token_user, scope, storage, &present, &token, issue));
    TRY(crucible_generated_scope_check(scope, issue));
    if (!present) return OK;
    if (!token.data && token.length) return crucible_generated_fail(issue, INVALID, "Invalid bearer token view");
    string parts[] = {VIEW("Bearer "), token}, header;
    TRY(crucible_generated_concat(storage, parts, 2, &header, issue));
    TRY(crucible_generated_header_set(storage, &request->headers, VIEW("authorization"), header, issue));
    *credentials = true; return OK;
}
