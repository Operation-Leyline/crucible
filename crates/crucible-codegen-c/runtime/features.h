#ifndef CRUCIBLE_GENERATED_FEATURES_H
#define CRUCIBLE_GENERATED_FEATURES_H
/* Runtime internals shared by authentication and the call pipeline. */
#include "bindings.h"

struct crucible_generated_client {
    crucible_generated_arena *arena;
    crucible_generated_client_config config;
    const yyjson_val *schema;
    crucible_generated_curl *owned_transport;
};
crucible_generated_status crucible_generated_execute(crucible_generated_client *client, crucible_generated_arena *prepared_arena,
    crucible_generated_string name, const yyjson_val *op, const yyjson_val *input, const crucible_generated_scope *scope,
    crucible_generated_result *out, const yyjson_val **wire_output, crucible_generated_error *error);
crucible_generated_status crucible_generated_encode_input(crucible_generated_arena *storage, const yyjson_val *op, const void *input,
    const yyjson_val **out, crucible_generated_error *error);

/* URLs and string helpers. */
typedef struct {
    crucible_generated_string scheme, authority, host, port, path, query;
    bool ipv6;
} crucible_generated_url;
bool crucible_generated_url_split(crucible_generated_string text, crucible_generated_url *url);
bool crucible_generated_loopback(crucible_generated_string host);
crucible_generated_status crucible_generated_concat(crucible_generated_arena *arena, const crucible_generated_string *parts, size_t count,
    crucible_generated_string *out, crucible_generated_error *error);

/* Bearer authentication: sets the Authorization header from the token provider. */
crucible_generated_status crucible_generated_authenticate(crucible_generated_client *client, const yyjson_val *op,
    const crucible_generated_scope *scope, crucible_generated_arena *arena, crucible_generated_request *request,
    bool *credentials, crucible_generated_error *error);
#endif
