#ifndef CRUCIBLE_GENERATED_CLIENT_RUNTIME_H
#define CRUCIBLE_GENERATED_CLIENT_RUNTIME_H
#include "http.h"
#include "schema.h"

#ifdef __cplusplus
extern "C" {
#endif

/** HTTP service client; free after calls and page iterators finish. Shared use requires thread-safe callbacks. */
typedef struct crucible_generated_client crucible_generated_client;
/** Lazy mutable page iterator; consume from one thread and free after use. */
typedef struct crucible_generated_pages crucible_generated_pages;
typedef crucible_generated_status (*crucible_generated_token_fn)(void *user, const crucible_generated_scope *scope,
    crucible_generated_arena *arena, bool *present, crucible_generated_string *token, crucible_generated_error *error);
typedef crucible_generated_status (*crucible_generated_middleware_fn)(void *user, const crucible_generated_scope *scope,
    crucible_generated_arena *arena, crucible_generated_request *request, crucible_generated_error *error);
typedef struct { crucible_generated_middleware_fn apply; void *user; } crucible_generated_middleware;
/** Client configuration. Strings and headers are copied. The bearer token provider runs
 * per attempt unless the caller already supplied an Authorization header. */
typedef struct {
    /** HTTP endpoint override; absent uses the generated endpoint. */
    crucible_generated_string endpoint;
    /** Total call deadline in seconds. */
    double timeout;
    /** Initial exponential backoff bound in seconds. */
    double base_delay;
    /** Maximum exponential backoff bound in seconds; Retry-After can override it. */
    double max_delay;
    /** Maximum attempts including the first request (default 3). */
    unsigned max_attempts;
    /** Default request headers applied to every attempt. */
    crucible_generated_headers headers;
    /** Bearer token provider invoked for each authenticated attempt. */
    crucible_generated_token_fn token;
    /** Borrowed context passed to the bearer provider. */
    void *token_user;
    /** Request transformations applied on every attempt after authentication. */
    const crucible_generated_middleware *middleware;
    /** Number of request middleware callbacks. */
    size_t middleware_count;
    /** Custom transport callback; NULL selects the built-in libcurl transport. */
    crucible_generated_send_fn send;
    /** Borrowed context passed to the custom transport callback. */
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

/** Default settings: 30-second call budget, three attempts, 0.1-second base and 10-second maximum backoff. */
crucible_generated_client_config crucible_generated_client_defaults(void);
/** Generated client_new supplies the immutable schema chunks and default endpoint. */
crucible_generated_status crucible_generated_runtime_new(const crucible_generated_client_config *config,
    const crucible_generated_string *schema_chunks, size_t chunk_count,
    crucible_generated_string endpoint,
    crucible_generated_client **client, crucible_generated_error *error);
/** Free clients only after calls and page iterators finish. Callback user data is borrowed. */
void crucible_generated_client_free(crucible_generated_client *client);
/** Frees the result arena, invalidating all borrowed response and model pointers. */
void crucible_generated_result_free(crucible_generated_result *result);
/** Initialize result to zero; free it before reuse. Every call assigns a fresh result,
 * including failures, so response metadata remains available until result_free. */
crucible_generated_status crucible_generated_client_call(crucible_generated_client *client,
    crucible_generated_string operation, const void *input,
    crucible_generated_cancelled_fn cancelled, void *cancel_user,
    crucible_generated_result *result, crucible_generated_error *error);
crucible_generated_status crucible_generated_pages_new(crucible_generated_client *client,
    crucible_generated_string operation, const void *input,
    crucible_generated_cancelled_fn cancelled, void *cancel_user,
    crucible_generated_pages **pages, crucible_generated_error *error);
/** A false has_page is successful exhaustion. Each page owns its own result arena. */
crucible_generated_status crucible_generated_pages_next(crucible_generated_pages *pages,
    bool *has_page, crucible_generated_result *result, crucible_generated_error *error);
/** Frees the iterator; previously returned results retain their own arenas. */
void crucible_generated_pages_free(crucible_generated_pages *pages);
#ifdef __cplusplus
}
#endif
#endif
