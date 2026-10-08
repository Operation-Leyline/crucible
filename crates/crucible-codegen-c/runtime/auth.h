#ifndef CRUCIBLE_GENERATED_AUTH_H
#define CRUCIBLE_GENERATED_AUTH_H
#include "http.h"

#ifdef __cplusplus
extern "C" {
#endif

/** Identities are borrowed views. Providers allocate returned strings in the
 * supplied arena; a false present means "no identity" rather than failure. */
typedef struct { crucible_generated_string username, password; } crucible_generated_basic_credentials;
typedef crucible_generated_status (*crucible_generated_basic_fn)(void *user, const crucible_generated_scope *scope,
    crucible_generated_arena *arena, bool *present, crucible_generated_basic_credentials *credentials, crucible_generated_error *error);
typedef crucible_generated_status (*crucible_generated_api_key_fn)(void *user, const crucible_generated_scope *scope,
    crucible_generated_arena *arena, bool *present, crucible_generated_string *key, crucible_generated_error *error);
typedef struct {
    crucible_generated_string access_key_id, secret_access_key, session_token;
    /** When expires is true, expires_at is in Unix seconds. */
    bool expires;
    double expires_at;
} crucible_generated_aws_credentials;
typedef crucible_generated_status (*crucible_generated_credentials_fn)(void *user, const crucible_generated_scope *scope,
    crucible_generated_arena *arena, bool *present, crucible_generated_aws_credentials *credentials, crucible_generated_error *error);
typedef struct { crucible_generated_credentials_fn resolve; void *user; } crucible_generated_credentials_provider;

/** Reads AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY and AWS_SESSION_TOKEN. user is unused. */
crucible_generated_status crucible_generated_credentials_environment(void *user, const crucible_generated_scope *scope,
    crucible_generated_arena *arena, bool *present, crucible_generated_aws_credentials *credentials, crucible_generated_error *error);
/** Static shared-file credentials; commands are never executed. Empty profile uses AWS_PROFILE
 * or "default"; empty file uses AWS_SHARED_CREDENTIALS_FILE or ~/.aws/credentials.
 * user is a const crucible_generated_profile_source *, or NULL for both defaults. */
typedef struct { crucible_generated_string profile, file; } crucible_generated_profile_source;
crucible_generated_status crucible_generated_credentials_profile(void *user, const crucible_generated_scope *scope,
    crucible_generated_arena *arena, bool *present, crucible_generated_aws_credentials *credentials, crucible_generated_error *error);
/** A chain stops on the first identity or error. user is a const crucible_generated_credentials_chain *
 * whose providers stay alive while it is used. */
typedef struct { const crucible_generated_credentials_provider *providers; size_t count; } crucible_generated_credentials_chain;
crucible_generated_status crucible_generated_credentials_chain_resolve(void *user, const crucible_generated_scope *scope,
    crucible_generated_arena *arena, bool *present, crucible_generated_aws_credentials *credentials, crucible_generated_error *error);

/** A thread-safe cache with single-flight refresh. Identities with an expiry are refreshed
 * min(refresh_before, half the remaining lifetime) before it. Waiting callers observe their
 * own cancellation and deadline. clock returns Unix seconds; NULL uses the system clock.
 * Resolve copies the cached identity into the caller's arena. */
typedef struct crucible_generated_credentials_cache crucible_generated_credentials_cache;
typedef double (*crucible_generated_clock_fn)(void *user);
crucible_generated_status crucible_generated_credentials_cache_new(crucible_generated_credentials_provider provider,
    double refresh_before, crucible_generated_clock_fn clock, void *clock_user,
    crucible_generated_credentials_cache **cache, crucible_generated_error *error);
crucible_generated_status crucible_generated_credentials_cache_resolve(void *user, const crucible_generated_scope *scope,
    crucible_generated_arena *arena, bool *present, crucible_generated_aws_credentials *credentials, crucible_generated_error *error);
/** Free only after every resolve has returned. */
void crucible_generated_credentials_cache_free(crucible_generated_credentials_cache *cache);

typedef enum { crucible_generated_sigv4_default = 0, crucible_generated_sigv4_enabled, crucible_generated_sigv4_disabled } crucible_generated_sigv4_toggle;
typedef struct {
    crucible_generated_string region, service;
    /** Signing time in Unix seconds when has_time is true; otherwise the system clock. */
    bool has_time;
    int64_t time;
    bool unsigned_payload;
    /** Both default to enabled except for the s3 service. */
    crucible_generated_sigv4_toggle double_encoding, normalize_path;
} crucible_generated_sigv4_options;
/** Signs the final request in place: Authorization, X-Amz-Date, X-Amz-Content-Sha256 and,
 * with a session token, X-Amz-Security-Token. Header storage comes from arena. */
crucible_generated_status crucible_generated_sigv4_sign(crucible_generated_arena *arena, crucible_generated_request *request,
    const crucible_generated_aws_credentials *credentials, const crucible_generated_sigv4_options *options,
    crucible_generated_error *error);

#ifdef __cplusplus
}
#endif
#endif
