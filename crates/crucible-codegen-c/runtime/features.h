#ifndef CRUCIBLE_GENERATED_FEATURES_H
#define CRUCIBLE_GENERATED_FEATURES_H
/* Runtime internals shared by authentication, endpoints, checksums and waiters. */
#include "bindings.h"

struct crucible_generated_client {
    crucible_generated_arena *arena;
    crucible_generated_client_config config;
    const yyjson_val *schema;
    crucible_generated_curl *owned_transport;
    bool digest;
    /* Caller endpoint parameters and built-ins as JSON objects (never NULL). */
    const yyjson_val *endpoint_parameters, *endpoint_builtins;
};
crucible_generated_status crucible_generated_execute(crucible_generated_client *client, crucible_generated_arena *prepared_arena,
    crucible_generated_string name, const yyjson_val *op, const yyjson_val *input, const crucible_generated_scope *scope,
    crucible_generated_result *out, const yyjson_val **wire_output, crucible_generated_error *error);
crucible_generated_status crucible_generated_encode_input(crucible_generated_arena *storage, const yyjson_val *op, const void *input,
    const yyjson_val **out, crucible_generated_error *error);

/* URLs and encoding. */
typedef struct {
    crucible_generated_string scheme, authority, host, port, path, query;
    bool ipv6;
} crucible_generated_url;
bool crucible_generated_url_split(crucible_generated_string text, crucible_generated_url *url);
bool crucible_generated_loopback(crucible_generated_string host);
bool crucible_generated_host_label(crucible_generated_string text, bool subdomains);
crucible_generated_status crucible_generated_uri_escape(crucible_generated_arena *arena, crucible_generated_string text,
    crucible_generated_string *out, crucible_generated_error *error);
/* Writes padded base64 of length 4*ceil(length/3) plus a terminator. */
void crucible_generated_base64(const unsigned char *data, size_t length, char *out);
crucible_generated_status crucible_generated_concat(crucible_generated_arena *arena, const crucible_generated_string *parts, size_t count,
    crucible_generated_string *out, crucible_generated_error *error);

/* JMESPath over mutable JSON. A NULL value is JSON null. */
typedef struct { yyjson_mut_doc *doc; crucible_generated_arena *arena; crucible_generated_error *issue; } crucible_generated_query;
crucible_generated_status crucible_generated_query_begin(crucible_generated_arena *arena, crucible_generated_query *query, crucible_generated_error *error);
crucible_generated_status crucible_generated_query_eval(crucible_generated_query *query, const yyjson_val *node, yyjson_mut_val *input, yyjson_mut_val **out);
bool crucible_generated_query_truth(const yyjson_mut_val *value);
crucible_generated_status crucible_generated_query_equal(crucible_generated_query *query, const yyjson_mut_val *left, const yyjson_mut_val *right, bool *equal);
/* Converts a model-named wire value to the query model: timestamps become epoch seconds. */
crucible_generated_status crucible_generated_query_model(crucible_generated_query *query, const yyjson_val *schema, const yyjson_val *target,
    const yyjson_val *value, yyjson_mut_val **out);
crucible_generated_status crucible_generated_query_copy(crucible_generated_query *query, const yyjson_val *value, yyjson_mut_val **out);

/* Endpoint rules against mutable values. */
crucible_generated_status crucible_generated_endpoint_evaluate(crucible_generated_query *query, const yyjson_val *rules,
    const yyjson_val *sources, yyjson_mut_val *input, const yyjson_val *client, const yyjson_val *builtins,
    crucible_generated_string *url, crucible_generated_headers *headers, yyjson_mut_val **properties);
crucible_generated_status crucible_generated_endpoint_prefix(crucible_generated_arena *arena, crucible_generated_string endpoint,
    const yyjson_val *description, const yyjson_mut_val *input, crucible_generated_string *out, crucible_generated_error *error);

/* Checksums. */
typedef struct {
    const char *algorithm, *header;
    const char *response[8];
    size_t response_count;
} crucible_generated_checksum_settings;
crucible_generated_status crucible_generated_checksum_prepare(crucible_generated_arena *arena, const yyjson_val *op, const yyjson_val **input,
    crucible_generated_checksum_policy request_policy, crucible_generated_checksum_policy response_policy,
    crucible_generated_checksum_settings *settings, crucible_generated_error *error);
crucible_generated_status crucible_generated_checksum_request(crucible_generated_arena *arena, const crucible_generated_checksum_settings *settings,
    crucible_generated_request *request, crucible_generated_error *error);
crucible_generated_status crucible_generated_checksum_response(const crucible_generated_checksum_settings *settings,
    const crucible_generated_response *response, const char **validated, crucible_generated_error *error);

/* Modeled authentication. */
typedef struct {
    bool sign;
    crucible_generated_aws_credentials identity;
    crucible_generated_sigv4_options options;
} crucible_generated_signing;
crucible_generated_status crucible_generated_authenticate(crucible_generated_client *client, const yyjson_val *op,
    const yyjson_val *properties, const crucible_generated_scope *scope, crucible_generated_arena *arena,
    crucible_generated_request *request, bool *credentials, crucible_generated_signing *signing, crucible_generated_error *error);
#endif
