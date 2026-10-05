#ifndef _WIN32
#define _POSIX_C_SOURCE 200809L
#endif
#include "auth_client/client.h"
#define PREFIX auth
#include "echo.h"
#include "threads.h"

#define VIEW(text) auth_string_view(text)

/* ---------- providers ---------- */
static auth_status basic_provider(void *user, const auth_scope *scope, auth_arena *arena, bool *present, auth_basic_credentials *out, auth_error *error) {
    (void)scope; (void)arena; (void)error; ++*(int *)user;
    *present = true; out->username = VIEW("user"); out->password = VIEW("password"); return auth_ok;
}
static auth_status forbidden_basic(void *user, const auth_scope *scope, auth_arena *arena, bool *present, auth_basic_credentials *out, auth_error *error) {
    (void)user; (void)scope; (void)arena; (void)present; (void)out; (void)error;
    check(false, "preconfigured identity invoked the basic provider"); return auth_invalid;
}
static auth_status api_key(void *user, const auth_scope *scope, auth_arena *arena, bool *present, auth_string *key, auth_error *error) {
    (void)user; (void)scope; (void)arena; (void)error; *present = true; *key = VIEW("secret"); return auth_ok;
}
static auth_status forbidden_key(void *user, const auth_scope *scope, auth_arena *arena, bool *present, auth_string *key, auth_error *error) {
    (void)user; (void)scope; (void)arena; (void)present; (void)key; (void)error;
    check(false, "preconfigured key invoked the API key provider"); return auth_invalid;
}
typedef struct { int calls; const char *ids[2]; const char *tokens[2]; } identities;
static auth_status aws_identities(void *user, const auth_scope *scope, auth_arena *arena, bool *present, auth_aws_credentials *out, auth_error *error) {
    identities *state = user; (void)scope; (void)arena; (void)error;
    int index = state->calls < 2 ? state->calls : 1; ++state->calls;
    *present = true;
    *out = (auth_aws_credentials){VIEW(state->ids[index]), VIEW("secret"), VIEW(state->tokens[index] ? state->tokens[index] : ""), false, 0};
    return auth_ok;
}
static auth_status absent(void *user, const auth_scope *scope, auth_arena *arena, bool *present, auth_aws_credentials *out, auth_error *error) {
    (void)user; (void)scope; (void)arena; (void)out; (void)error; *present = false; return auth_ok;
}
static auth_status add_header(void *user, const auth_scope *scope, auth_arena *arena, auth_request *request, auth_error *error) {
    (void)scope; return auth_header_set(arena, &request->headers, VIEW((const char *)user), VIEW("final"), error);
}

static auth_client *client_new(auth_client_config config) {
    auth_client *client = NULL; auth_error error = {0};
    good(auth_client_new(&config, &client, &error), &error, "client_new");
    return client;
}
static auth_client_config base(void) {
    auth_client_config config = auth_client_defaults();
    config.endpoint = VIEW(environment("FEATURES_URL"));
    return config;
}

static void published_vector(void) {
    auth_arena *arena = auth_arena_new(); auth_error error = {0};
    auth_header range = {VIEW("Range"), VIEW("bytes=0-9")};
    auth_request request = {VIEW("https://examplebucket.s3.amazonaws.com/test.txt"), VIEW("GET"), {&range, 1}, false, {0}};
    auth_aws_credentials credentials = {VIEW("AKIAIOSFODNN7EXAMPLE"), VIEW("wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"), {0}, false, 0};
    auth_sigv4_options options = {VIEW("us-east-1"), VIEW("s3"), true, 1369353600, false, auth_sigv4_default, auth_sigv4_default};
    good(auth_sigv4_sign(arena, &request, &credentials, &options, &error), &error, "sign vector");
    auth_string value = {0};
    check(auth_header_get(request.headers, VIEW("authorization"), &value), "authorization header");
    check(equal(value, "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"), "published SigV4 signature");
    check(auth_header_get(request.headers, VIEW("x-amz-date"), &value) && equal(value, "20130524T000000Z"), "signing date");
    /* Re-signing replaces, rather than accumulates, the signature. */
    good(auth_sigv4_sign(arena, &request, &credentials, &options, &error), &error, "re-sign");
    size_t authorizations = 0;
    for (size_t i = 0; i < request.headers.count; ++i) if (auth_header_get((auth_headers){&request.headers.items[i], 1}, VIEW("authorization"), NULL)) ++authorizations;
    check(authorizations == 1, "one authorization header");
    credentials.expires = true; credentials.expires_at = 1369353600;
    check(auth_sigv4_sign(arena, &request, &credentials, &options, &error) == auth_invalid && message_contains(&error, "expired"), "expired identity rejected");
    credentials.expires = false; options.region = VIEW("");
    check(auth_sigv4_sign(arena, &request, &credentials, &options, &error) == auth_invalid, "region is required");
    auth_arena_free(arena);
}

static void modeled_authentication(void) {
    auth_error error = {0}; int calls = 0;
    auth_client_config config = base();
    config.basic_credentials = basic_provider; config.basic_user = &calls;
    auth_client *client = client_new(config);
    auth_operations_public_result result = {0};
    good(auth_operations_public(client, NULL, NULL, &result, &error), &error, "public");
    check(calls == 0 && !echo_header(result.base.arena, &result.base.response, "authorization").data, "anonymous operation sends no identity");
    auth_operations_public_result_free(&result);
    auth_operations_read_result read = {0};
    good(auth_operations_read(client, NULL, NULL, &read, &error), &error, "basic read");
    check(equal(echo_header(read.base.arena, &read.base.response, "authorization"), "Basic dXNlcjpwYXNzd29yZA=="), "basic header");
    auth_operations_read_result_free(&read); auth_client_free(client);

    config = base(); config.api_key = api_key;
    client = client_new(config);
    good(auth_operations_read(client, NULL, NULL, &read, &error), &error, "api key read");
    check(equal(echo_header(read.base.arena, &read.base.response, "x-api-key"), "ApiKey secret"), "API key header");
    auth_operations_read_result_free(&read); auth_client_free(client);

    identities aws = {0, {"id", "id"}, {NULL, NULL}};
    auth_middleware hook = {add_header, (void *)"X-Hook"};
    config = base(); config.credentials = aws_identities; config.credentials_user = &aws; config.region = VIEW("us-east-1");
    config.middleware = &hook; config.middleware_count = 1;
    client = client_new(config);
    good(auth_operations_read(client, NULL, NULL, &read, &error), &error, "SigV4 read");
    auth_string authorization = echo_header(read.base.arena, &read.base.response, "authorization");
    check(contains(authorization, "AWS4-HMAC-SHA256 Credential=id/") && contains(authorization, "/us-east-1/example/aws4_request"), "SigV4 scope");
    check(contains(authorization, ";x-hook"), "middleware headers are signed");
    check(!echo_header(read.base.arena, &read.base.response, "x-amz-security-token").data, "no session token header");
    auth_operations_read_result_free(&read); auth_client_free(client);

    config = base(); config.region = VIEW("us-east-1");
    client = client_new(config);
    check(auth_operations_read(client, NULL, NULL, &read, &error) == auth_invalid && message_contains(&error, "No identity"), "required identity missing");
    auth_operations_read_result_free(&read);
    auth_operations_optional_result optional = {0};
    good(auth_operations_optional(client, NULL, NULL, &optional, &error), &error, "optional auth");
    check(!echo_header(optional.base.arena, &optional.base.response, "authorization").data, "optional auth sends anonymously");
    auth_operations_optional_result_free(&optional); auth_client_free(client);

    config = base(); config.endpoint = VIEW("http://example.test"); config.api_key = api_key;
    client = client_new(config);
    check(auth_operations_read(client, NULL, NULL, &read, &error) == auth_invalid && message_contains(&error, "plaintext"), "plaintext credentials refused");
    auth_operations_read_result_free(&read); auth_client_free(client);
}

static void preconfigured_credentials(void) {
    auth_error error = {0};
    auth_header supplied = {VIEW("Authorization"), VIEW("Basic supplied")};
    auth_client_config config = base(); config.headers = (auth_headers){&supplied, 1}; config.basic_credentials = forbidden_basic;
    auth_client *client = client_new(config);
    auth_operations_read_result read = {0};
    good(auth_operations_read(client, NULL, NULL, &read, &error), &error, "preconfigured basic");
    check(equal(echo_header(read.base.arena, &read.base.response, "authorization"), "Basic supplied"), "caller authorization kept");
    auth_operations_read_result_free(&read); auth_client_free(client);

    auth_header key = {VIEW("X-Api-Key"), VIEW("ApiKey supplied")};
    config = base(); config.headers = (auth_headers){&key, 1}; config.api_key = forbidden_key;
    client = client_new(config);
    good(auth_operations_read(client, NULL, NULL, &read, &error), &error, "preconfigured key");
    auth_operations_read_result_free(&read); auth_client_free(client);

    auth_header unrelated = {VIEW("Authorization"), VIEW("Digest unrelated")};
    config = base(); config.headers = (auth_headers){&unrelated, 1};
    client = client_new(config);
    check(auth_operations_read(client, NULL, NULL, &read, &error) == auth_invalid, "unrelated authorization rejected");
    auth_operations_read_result_free(&read); auth_client_free(client);
}

static void refresh_per_attempt(void) {
    auth_error error = {0};
    identities aws = {0, {"first", "second"}, {"session-one", "session-two"}};
    auth_header mode[] = {{VIEW("x-mode"), VIEW("flaky")}, {VIEW("x-run"), VIEW("refresh")}};
    auth_middleware hook = {add_header, (void *)"x-custom"};
    auth_client_config config = base(); config.headers = (auth_headers){mode, 2}; config.base_delay = 0;
    config.credentials = aws_identities; config.credentials_user = &aws; config.region = VIEW("us-east-1");
    config.middleware = &hook; config.middleware_count = 1;
    auth_client *client = client_new(config);
    auth_operations_read_result read = {0};
    good(auth_operations_read(client, NULL, NULL, &read, &error), &error, "retried SigV4 read");
    check(aws.calls == 2, "identity resolved for every attempt");
    auth_string authorization = echo_header(read.base.arena, &read.base.response, "authorization");
    check(contains(authorization, "Credential=second/") && contains(authorization, "x-custom"), "retry signs with the refreshed identity");
    check(equal(echo_header(read.base.arena, &read.base.response, "x-amz-security-token"), "session-two"), "refreshed session token");
    auth_operations_read_result_free(&read); auth_client_free(client);
}

static void set_variable(const char *name, const char *value) {
#ifdef _WIN32
    check(_putenv_s(name, value) == 0, "set environment");
#else
    check(setenv(name, value, 1) == 0, "set environment");
#endif
}
static void environment_and_profiles(void) {
    auth_arena *arena = auth_arena_new(); auth_error error = {0}; auth_scope scope;
    good(auth_scope_begin(5, NULL, NULL, &scope, &error), &error, "scope");
    bool present = false; auth_aws_credentials value = {0};
    set_variable("AWS_ACCESS_KEY_ID", "env"); set_variable("AWS_SECRET_ACCESS_KEY", "secret"); set_variable("AWS_SESSION_TOKEN", "token");
    good(auth_credentials_environment(NULL, &scope, arena, &present, &value, &error), &error, "environment");
    check(present && equal(value.access_key_id, "env") && equal(value.session_token, "token"), "environment identity");
    set_variable("AWS_SECRET_ACCESS_KEY", "");
    check(auth_credentials_environment(NULL, &scope, arena, &present, &value, &error) == auth_invalid, "incomplete environment identity");
    set_variable("AWS_ACCESS_KEY_ID", "");
    good(auth_credentials_environment(NULL, &scope, arena, &present, &value, &error), &error, "empty environment");
    check(!present, "no environment identity");

    char path[1024];
    (void)snprintf(path, sizeof(path), "%s/credentials", environment("FEATURE_TEMP"));
    FILE *file = fopen(path, "wb"); check(file != NULL, "write profile");
    fputs("# comment\n[default]\naws_access_key_id = other\naws_secret_access_key = other\n\n[test]\naws_access_key_id = id\r\naws_secret_access_key = s%cret\naws_session_token = token\n[dup]\naws_access_key_id = a\naws_access_key_id = b\n", file);
    fclose(file);
    auth_profile_source source = {VIEW("test"), VIEW(path)};
    good(auth_credentials_profile(&source, &scope, arena, &present, &value, &error), &error, "profile");
    check(present && equal(value.access_key_id, "id") && equal(value.secret_access_key, "s%cret") && equal(value.session_token, "token"), "profile identity");
    source.profile = VIEW("missing");
    good(auth_credentials_profile(&source, &scope, arena, &present, &value, &error), &error, "missing profile");
    check(!present, "missing profile is absent");
    source.profile = VIEW("dup");
    check(auth_credentials_profile(&source, &scope, arena, &present, &value, &error) == auth_invalid && message_contains(&error, "Duplicate"), "duplicate profile entry");
    auth_profile_source nowhere = {VIEW("test"), VIEW("missing-credentials-file")};
    good(auth_credentials_profile(&nowhere, &scope, arena, &present, &value, &error), &error, "missing file");
    check(!present, "missing file is absent");

    source.profile = VIEW("test");
    auth_credentials_provider providers[] = {{absent, NULL}, {NULL, NULL}, {auth_credentials_profile, &source}};
    auth_credentials_chain chain = {providers, 3};
    good(auth_credentials_chain_resolve(&chain, &scope, arena, &present, &value, &error), &error, "chain");
    check(present && equal(value.access_key_id, "id"), "chain falls through to the profile");
    auth_arena_free(arena);
}

typedef struct { double now; atomic_flag_value calls, started, release; double lifetime; unsigned delay; } slow_state;
static double test_clock(void *user) { return ((slow_state *)user)->now; }
static auth_status slow_provider(void *user, const auth_scope *scope, auth_arena *arena, bool *present, auth_aws_credentials *out, auth_error *error) {
    slow_state *state = user; (void)scope; (void)arena; (void)error;
    ATOMIC_ADD(state->calls);
    ATOMIC_SET(state->started, 1);
    while (state->delay == 0 && !ATOMIC_GET(state->release)) pause_ms(1);
    if (state->delay) pause_ms(state->delay);
    *present = true;
    *out = (auth_aws_credentials){VIEW("key"), VIEW("secret"), {0}, state->lifetime != 0, state->now + state->lifetime};
    return auth_ok;
}
typedef struct { auth_credentials_cache *cache; auth_status status; bool present; char key[8]; bool cancel; int checks; } resolver;
/* Cancels after the first few checks, so the caller is already waiting on a refresh. */
static bool cancel_later(void *user) { return ++*(int *)user > 3; }
static void resolve(void *value) {
    resolver *task = value; auth_error error = {0}; auth_scope scope;
    auth_arena *arena = auth_arena_new();
    task->status = auth_scope_begin(10, task->cancel ? cancel_later : NULL, &task->checks, &scope, &error);
    auth_aws_credentials identity = {0};
    if (!task->status) task->status = auth_credentials_cache_resolve(task->cache, &scope, arena, &task->present, &identity, &error);
    if (!task->status && task->present && identity.access_key_id.length < sizeof(task->key)) { memcpy(task->key, identity.access_key_id.data, identity.access_key_id.length); task->key[identity.access_key_id.length] = 0; }
    auth_arena_free(arena);
}
static void concurrent(auth_credentials_cache *cache) {
    enum { count = 30 };
    resolver tasks[count]; job jobs[count]; worker threads[count];
    for (int i = 0; i < count; ++i) { tasks[i] = (resolver){cache, auth_invalid, false, {0}, false, 0}; jobs[i] = (job){resolve, &tasks[i]}; check(spawn(&threads[i], &jobs[i]), "spawn"); }
    for (int i = 0; i < count; ++i) join(threads[i]);
    for (int i = 0; i < count; ++i) check(tasks[i].status == auth_ok && tasks[i].present && !strcmp(tasks[i].key, "key"), "concurrent resolve");
}
static void credential_cache(void) {
    auth_error error = {0};
    slow_state state = {1767225600, 0, 0, 0, 300, 5};
    auth_credentials_cache *cache = NULL;
    good(auth_credentials_cache_new((auth_credentials_provider){slow_provider, &state}, 60, test_clock, &state, &cache, &error), &error, "cache");
    concurrent(cache);
    check(ATOMIC_GET(state.calls) == 1, "single-flight refresh");
    auth_arena *arena = auth_arena_new(); auth_scope scope; bool present = false;
    auth_aws_credentials first = {0}, second = {0};
    good(auth_scope_begin(5, NULL, NULL, &scope, &error), &error, "scope");
    good(auth_credentials_cache_resolve(cache, &scope, arena, &present, &first, &error), &error, "cached");
    good(auth_credentials_cache_resolve(cache, &scope, arena, &present, &second, &error), &error, "cached again");
    check(first.access_key_id.data != second.access_key_id.data && equal(second.access_key_id, "key"), "callers receive independent copies");
    check(ATOMIC_GET(state.calls) == 1, "cache hit");
    state.now += 270;
    good(auth_credentials_cache_resolve(cache, &scope, arena, &present, &first, &error), &error, "refresh");
    check(ATOMIC_GET(state.calls) == 2, "refresh before expiry");
    auth_credentials_cache_free(cache);

    slow_state brief = {1767225600, 0, 0, 0, 20, 5};
    good(auth_credentials_cache_new((auth_credentials_provider){slow_provider, &brief}, 60, test_clock, &brief, &cache, &error), &error, "short cache");
    concurrent(cache);
    check(ATOMIC_GET(brief.calls) == 1, "short-lived identity is still shared");
    auth_credentials_cache_free(cache);

    slow_state expired = {1767225600, 0, 0, 0, -1, 1};
    good(auth_credentials_cache_new((auth_credentials_provider){slow_provider, &expired}, 0, test_clock, &expired, &cache, &error), &error, "expired cache");
    check(auth_credentials_cache_resolve(cache, &scope, arena, &present, &first, &error) == auth_invalid && message_contains(&error, "expired"), "expired identity rejected");
    auth_credentials_cache_free(cache);

    slow_state blocked = {1767225600, 0, 0, 0, 0, 0};
    good(auth_credentials_cache_new((auth_credentials_provider){slow_provider, &blocked}, 60, test_clock, &blocked, &cache, &error), &error, "blocked cache");
    resolver owner = {cache, auth_invalid, false, {0}, false, 0}; job task = {resolve, &owner}; worker thread;
    check(spawn(&thread, &task), "spawn refresher");
    while (!ATOMIC_GET(blocked.started)) pause_ms(1);
    resolver waiter = {cache, auth_invalid, false, {0}, true, 0};
    resolve(&waiter);
    check(waiter.status == auth_cancelled, "waiting caller observes its own cancellation");
    ATOMIC_SET(blocked.release, 1);
    join(thread);
    check(owner.status == auth_ok && owner.present, "refreshing caller completes");
    auth_credentials_cache_free(cache);
    check(auth_credentials_cache_new((auth_credentials_provider){NULL, NULL}, 60, NULL, NULL, &cache, &error) == auth_invalid, "provider required");
    auth_arena_free(arena);
}

int main(void) {
    published_vector();
    modeled_authentication();
    preconfigured_credentials();
    refresh_per_attempt();
    environment_and_profiles();
    credential_cache();
    puts("C authentication, SigV4 and credential providers passed");
    return 0;
}
