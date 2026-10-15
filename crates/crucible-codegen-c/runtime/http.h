#ifndef CRUCIBLE_GENERATED_HTTP_H
#define CRUCIBLE_GENERATED_HTTP_H
#include "types.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef bool (*crucible_generated_cancelled_fn)(void *user);
typedef struct {
    double deadline;
    crucible_generated_cancelled_fn cancelled;
    void *user;
} crucible_generated_scope;
typedef struct { crucible_generated_string name, value; } crucible_generated_header;
typedef struct { const crucible_generated_header *items; size_t count; } crucible_generated_headers;
typedef struct {
    crucible_generated_string url, method;
    crucible_generated_headers headers;
    bool has_body;
    crucible_generated_blob body;
} crucible_generated_request;
typedef struct {
    int status;
    crucible_generated_headers headers;
    crucible_generated_blob body;
} crucible_generated_response;
/** The callback owns its transport. Allocate returned views in the supplied arena. */
typedef crucible_generated_status (*crucible_generated_send_fn)(void *user,
    const crucible_generated_request *request, bool read_body,
    const crucible_generated_scope *scope, crucible_generated_arena *arena,
    crucible_generated_response *response, crucible_generated_error *error);
typedef struct crucible_generated_curl crucible_generated_curl;

crucible_generated_status crucible_generated_scope_begin(double timeout,
    crucible_generated_cancelled_fn cancelled, void *user,
    crucible_generated_scope *scope, crucible_generated_error *error);
crucible_generated_status crucible_generated_scope_check(const crucible_generated_scope *scope, crucible_generated_error *error);
crucible_generated_status crucible_generated_scope_wait(const crucible_generated_scope *scope, double seconds, crucible_generated_error *error);
double crucible_generated_scope_remaining(const crucible_generated_scope *scope);
bool crucible_generated_header_get(crucible_generated_headers headers, crucible_generated_string name, crucible_generated_string *value);
crucible_generated_status crucible_generated_header_set(crucible_generated_arena *arena, crucible_generated_headers *headers,
    crucible_generated_string name, crucible_generated_string value, crucible_generated_error *error);
crucible_generated_status crucible_generated_validate_url(crucible_generated_string url, bool endpoint, bool credentials, crucible_generated_error *error);
crucible_generated_status crucible_generated_validate_request(const crucible_generated_request *request, crucible_generated_error *error);
crucible_generated_status crucible_generated_curl_new(crucible_generated_curl **transport, crucible_generated_error *error);
void crucible_generated_curl_free(crucible_generated_curl *transport);
crucible_generated_status crucible_generated_curl_send(void *transport,
    const crucible_generated_request *request, bool read_body,
    const crucible_generated_scope *scope, crucible_generated_arena *arena,
    crucible_generated_response *response, crucible_generated_error *error);

#ifdef __cplusplus
}
#endif
#endif
