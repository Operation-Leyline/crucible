#ifndef CRUCIBLE_GENERATED_CLIENT_RUNTIME_H
#define CRUCIBLE_GENERATED_CLIENT_RUNTIME_H
#include "http.h"
#include "schema.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct crucible_generated_client crucible_generated_client;
typedef struct crucible_generated_pages crucible_generated_pages;
typedef crucible_generated_status (*crucible_generated_token_fn)(void *user, const crucible_generated_scope *scope,
    crucible_generated_arena *arena, bool *present, crucible_generated_string *token, crucible_generated_error *error);
typedef crucible_generated_status (*crucible_generated_middleware_fn)(void *user, const crucible_generated_scope *scope,
    crucible_generated_arena *arena, crucible_generated_request *request, crucible_generated_error *error);
typedef struct { crucible_generated_middleware_fn apply; void *user; } crucible_generated_middleware;
typedef struct {
    /* A NULL endpoint uses the generated default. All strings are copied. */
    crucible_generated_string endpoint;
    double timeout, base_delay, max_delay;
    unsigned max_attempts;
    crucible_generated_headers headers;
    crucible_generated_token_fn token;
    void *token_user;
    const crucible_generated_middleware *middleware;
    size_t middleware_count;
    crucible_generated_send_fn send;
    void *transport_user;
} crucible_generated_client_config;
typedef struct {
    crucible_generated_arena *arena;
    const void *output;
    const crucible_generated_shape *error_shape;
    const void *modeled_error;
    crucible_generated_string name, message, request_id;
    crucible_generated_response response;
    bool retryable, throttling;
} crucible_generated_result;

crucible_generated_client_config crucible_generated_client_defaults(void);
/* Generated client_new supplies the immutable schema chunks and default endpoint. */
crucible_generated_status crucible_generated_runtime_new(const crucible_generated_client_config *config,
    const crucible_generated_string *schema_chunks, size_t chunk_count,
    crucible_generated_string endpoint, bool digest,
    crucible_generated_client **client, crucible_generated_error *error);
/* Free clients only after calls and page iterators finish. Callback user data is borrowed. */
void crucible_generated_client_free(crucible_generated_client *client);
void crucible_generated_result_free(crucible_generated_result *result);
/* Initialize result to zero; free it before reuse. Every call assigns a fresh result,
 * including failures, so response metadata remains available until result_free. */
crucible_generated_status crucible_generated_client_call(crucible_generated_client *client,
    crucible_generated_string operation, const void *input,
    crucible_generated_cancelled_fn cancelled, void *cancel_user,
    crucible_generated_result *result, crucible_generated_error *error);
crucible_generated_status crucible_generated_pages_new(crucible_generated_client *client,
    crucible_generated_string operation, const void *input,
    crucible_generated_cancelled_fn cancelled, void *cancel_user,
    crucible_generated_pages **pages, crucible_generated_error *error);
/* A false has_page is successful exhaustion. Each page owns its own result arena. */
crucible_generated_status crucible_generated_pages_next(crucible_generated_pages *pages,
    bool *has_page, crucible_generated_result *result, crucible_generated_error *error);
void crucible_generated_pages_free(crucible_generated_pages *pages);

#ifdef __cplusplus
}
#endif
#endif
