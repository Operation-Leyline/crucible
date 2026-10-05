#ifndef CRUCIBLE_GENERATED_CLIENT_RUNTIME_H
#define CRUCIBLE_GENERATED_CLIENT_RUNTIME_H
#include "auth.h"
#include "endpoints.h"
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
typedef enum { crucible_generated_checksum_when_supported = 0, crucible_generated_checksum_when_required } crucible_generated_checksum_policy;
typedef void (*crucible_generated_checksum_validated_fn)(void *user, crucible_generated_string operation, crucible_generated_string algorithm);
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
    /* Modeled authentication. A provider runs on every attempt for the first modeled
     * scheme it serves; a matching caller-supplied header or query skips providers. */
    crucible_generated_basic_fn basic_credentials;
    void *basic_user;
    crucible_generated_api_key_fn api_key;
    void *api_key_user;
    crucible_generated_credentials_fn credentials;
    void *credentials_user;
    /* SigV4 region, also the AWS::Region endpoint built-in. Copied. */
    crucible_generated_string region;
    /* Endpoint rule client parameters and built-ins. Copied. */
    const crucible_generated_endpoint_value *endpoint_parameters;
    size_t endpoint_parameter_count;
    const crucible_generated_endpoint_value *endpoint_builtins;
    size_t endpoint_builtin_count;
    bool disable_host_prefix;
    crucible_generated_checksum_policy request_checksum_calculation, response_checksum_validation;
    /* Called after a response checksum validates; must be thread-safe if the client is shared. */
    crucible_generated_checksum_validated_fn checksum_validated;
    void *checksum_user;
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
/* Waiter delays in seconds; unset values use the modeled minDelay and maxDelay. */
typedef struct { bool has_min_delay, has_max_delay; double min_delay, max_delay; } crucible_generated_waiter_options;
typedef struct {
    unsigned attempts;
    /* For waiter_failed: the status of the last call (ok when an output matched). */
    crucible_generated_status cause;
    /* JSON text of the failure acceptor message, owned by the result arena; empty if none. */
    crucible_generated_string message;
} crucible_generated_waiter_report;
/* Polls until a success acceptor matches (ok, result holds that response), a failure
 * acceptor matches (waiter_failed, result holds the last response), the total timeout
 * passes (deadline) or the caller cancels. Each attempt is bounded by both the client
 * timeout and the waiter deadline. Free result after every outcome. */
crucible_generated_status crucible_generated_client_wait(crucible_generated_client *client,
    crucible_generated_string operation, crucible_generated_string waiter, const void *input, double timeout,
    const crucible_generated_waiter_options *options, crucible_generated_cancelled_fn cancelled, void *cancel_user,
    crucible_generated_result *result, crucible_generated_waiter_report *report, crucible_generated_error *error);

#ifdef __cplusplus
}
#endif
#endif
