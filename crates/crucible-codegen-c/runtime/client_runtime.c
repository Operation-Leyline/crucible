#include "client_runtime.h"
#include "bindings.h"
#include <openssl/rand.h>
#include <math.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef crucible_generated_status status;
typedef crucible_generated_string string;
typedef crucible_generated_error error;
typedef crucible_generated_arena arena;
typedef crucible_generated_client client;
#define OK crucible_generated_ok
#define INVALID crucible_generated_invalid
#define OOM crucible_generated_out_of_memory
#define VIEW(text) crucible_generated_string_view(text)
#define TRY(call) do { status code = (call); if (code) return code; } while (0)
static const yyjson_val *get(const yyjson_val *value, const char *key) { return yyjson_obj_get((yyjson_val *)value, key); }
static string text(const yyjson_val *value) { return (string){yyjson_get_str((yyjson_val *)value), yyjson_get_len((yyjson_val *)value)}; }
static status fail(error *issue, const char *message) { return crucible_generated_fail(issue, INVALID, message); }
struct crucible_generated_client {
    arena *arena;
    crucible_generated_client_config config;
    const yyjson_val *schema;
    crucible_generated_curl *owned_transport;
    bool digest;
};
crucible_generated_client_config crucible_generated_client_defaults(void) {
    crucible_generated_client_config config = {0};
    config.timeout = 30; config.max_attempts = 3; config.base_delay = 0.1; config.max_delay = 10;
    return config;
}
void crucible_generated_client_free(client *value) {
    if (!value) return;
    crucible_generated_curl_free(value->owned_transport);
    crucible_generated_arena_free(value->arena); free(value);
}
void crucible_generated_result_free(crucible_generated_result *result) {
    if (!result) return;
    crucible_generated_arena_free(result->arena); *result = (crucible_generated_result){0};
}
status crucible_generated_runtime_new(const crucible_generated_client_config *config, const string *chunks, size_t count,
    string endpoint, bool digest, client **out, error *issue) {
    if (!out || (!chunks && count)) return fail(issue, "Missing client output or schema");
    *out = NULL;
    crucible_generated_client_config settings = config ? *config : crucible_generated_client_defaults();
    crucible_generated_scope scope;
    TRY(crucible_generated_scope_begin(settings.timeout, NULL, NULL, &scope, issue));
    if (!settings.max_attempts || !isfinite(settings.base_delay) || settings.base_delay < 0 || !isfinite(settings.max_delay) || settings.max_delay < 0 || (settings.headers.count && !settings.headers.items) || (settings.middleware_count && !settings.middleware)) return fail(issue, "Invalid client configuration");
    if (settings.endpoint.data) endpoint = settings.endpoint;
    TRY(crucible_generated_validate_url(endpoint, true, false, issue));
    while (endpoint.length && endpoint.data[endpoint.length - 1] == '/') --endpoint.length;
    client *value = calloc(1, sizeof(*value));
    if (!value) return crucible_generated_fail(issue, OOM, "Client allocation failed");
    value->arena = crucible_generated_arena_new(); value->config = settings; value->digest = digest;
    status code = OK;
    if (!value->arena) { code = crucible_generated_fail(issue, OOM, "Client allocation failed"); goto failed; }
    code = crucible_generated_arena_copy(value->arena, endpoint, &value->config.endpoint, issue);
    if (code) goto failed;
    value->config.headers = (crucible_generated_headers){0};
    for (size_t i = 0; i < settings.headers.count; ++i) {
        code = crucible_generated_header_set(value->arena, &value->config.headers, settings.headers.items[i].name, settings.headers.items[i].value, issue);
        if (code) goto failed;
    }
    if (settings.middleware_count > SIZE_MAX / sizeof(crucible_generated_middleware)) { code = fail(issue, "Middleware count overflow"); goto failed; }
    crucible_generated_middleware *middleware = crucible_generated_arena_alloc(value->arena, settings.middleware_count * sizeof(*middleware), issue);
    if (!middleware) { code = OOM; goto failed; }
    for (size_t i = 0; i < settings.middleware_count; ++i) {
        if (!settings.middleware[i].apply) { code = fail(issue, "Missing middleware function"); goto failed; }
        middleware[i] = settings.middleware[i];
    }
    value->config.middleware = middleware;
    size_t size = 0;
    for (size_t i = 0; i < count; ++i) {
        if ((!chunks[i].data && chunks[i].length) || chunks[i].length > SIZE_MAX - size) { code = fail(issue, "Invalid schema chunks"); goto failed; }
        size += chunks[i].length;
    }
    char *joined = crucible_generated_arena_alloc(value->arena, size, issue);
    if (!joined) { code = OOM; goto failed; }
    size_t offset = 0;
    for (size_t i = 0; i < count; ++i) { if (chunks[i].length) memcpy(joined + offset, chunks[i].data, chunks[i].length); offset += chunks[i].length; }
    code = crucible_generated_json_parse(value->arena, (string){joined, size}, &value->schema, issue);
    if (code) goto failed;
    if (!settings.send) {
        code = crucible_generated_curl_new(&value->owned_transport, issue);
        if (code) goto failed;
        value->config.send = crucible_generated_curl_send; value->config.transport_user = value->owned_transport;
    }
    *out = value; return OK;
failed:
    crucible_generated_client_free(value); return code;
}
static const yyjson_val *operation(client *value, string name) { return yyjson_obj_getn((yyjson_val *)get(value->schema, "operations"), name.data, name.length); }
static status encode_input(arena *storage, const yyjson_val *op, const void *input, const yyjson_val **out, error *issue) {
    const yyjson_val *target = get(op, "input");
    if (!yyjson_is_str(target)) return crucible_generated_json_parse(storage, VIEW("{}"), out, issue);
    const crucible_generated_shape *descriptor = crucible_generated_shape_lookup(text(target));
    return crucible_generated_model_encode(storage, descriptor, input, out, issue);
}
static double retry_delay(client *value, arena *storage, unsigned attempt, crucible_generated_headers headers) {
    string header;
    if (crucible_generated_header_get(headers, VIEW("retry-after"), &header)) {
        const yyjson_val *number;
        double seconds;
        if (!crucible_generated_json_parse(storage, header, &number, NULL) && !crucible_generated_json_double(number, &seconds, NULL) && seconds >= 0) return seconds;
        crucible_generated_timestamp date;
        if (!crucible_generated_timestamp_http_parse(header, &date, NULL)) return fmax(0, (double)date.seconds - (double)time(NULL));
    }
    unsigned char random[4];
    double fraction = 0.5;
    if (RAND_bytes(random, 4) == 1) {
        uint32_t bits = ((uint32_t)random[0] << 24) | ((uint32_t)random[1] << 16) | ((uint32_t)random[2] << 8) | random[3];
        fraction = (double)bits / 4294967296.0;
    }
    unsigned exponent = attempt - 1 < 30 ? attempt - 1 : 30;
    return fmin(value->config.max_delay, ldexp(value->config.base_delay, (int)exponent)) * fraction;
}
static status attempt_request(client *value, const yyjson_val *op, arena *storage, const crucible_generated_request *prepared, const crucible_generated_scope *scope, crucible_generated_request *out, error *issue) {
    *out = *prepared;
    if (yyjson_get_bool((yyjson_val *)get(op, "bearer")) && value->config.token) {
        bool present = false; string token = {0};
        TRY(value->config.token(value->config.token_user, scope, storage, &present, &token, issue));
        TRY(crucible_generated_scope_check(scope, issue));
        if (present) {
            if ((!token.data && token.length) || token.length > SIZE_MAX - 7) return fail(issue, "Invalid bearer token view");
            char *header = crucible_generated_arena_alloc(storage, token.length + 7, issue);
            if (!header) return OOM;
            memcpy(header, "Bearer ", 7); if (token.length) memcpy(header + 7, token.data, token.length);
            TRY(crucible_generated_header_set(storage, &out->headers, VIEW("authorization"), (string){header, token.length + 7}, issue));
        }
    }
    for (size_t i = 0; i < value->config.middleware_count; ++i) {
        TRY(crucible_generated_scope_check(scope, issue));
        const crucible_generated_middleware *hook = value->config.middleware + i;
        TRY(hook->apply(hook->user, scope, storage, out, issue));
    }
    TRY(crucible_generated_scope_check(scope, issue));
    TRY(crucible_generated_validate_request(out, issue));
    if (value->digest) { TRY(crucible_generated_binding_digest(storage, out, issue)); }
    return OK;
}
static status execute(client *value, arena *prepared_arena, const yyjson_val *op, const yyjson_val *input,
    const crucible_generated_scope *scope, crucible_generated_result *out, const yyjson_val **wire_output, error *issue) {
    const yyjson_val *tokenized;
    TRY(crucible_generated_binding_token(prepared_arena, op, input, &tokenized, issue));
    crucible_generated_request prepared;
    TRY(crucible_generated_binding_prepare(prepared_arena, op, tokenized, value->config.endpoint, value->config.headers, &prepared, issue));
    const bool safe = yyjson_get_bool((yyjson_val *)get(op, "safe"));
    const yyjson_val *bindings = get(op, "response");
    const bool read_body = yyjson_arr_size((yyjson_val *)get(bindings, "body")) || yyjson_is_arr(get(bindings, "payload"));
    for (unsigned attempt = 1;; ++attempt) {
        TRY(crucible_generated_scope_check(scope, issue));
        crucible_generated_result result = {0};
        result.arena = crucible_generated_arena_new();
        if (!result.arena) return crucible_generated_fail(issue, OOM, "Response arena allocation failed");
        crucible_generated_request request;
        status code = attempt_request(value, op, result.arena, &prepared, scope, &request, issue);
        bool retry = false;
        if (!code) {
            code = value->config.send(value->config.transport_user, &request, read_body, scope, result.arena, &result.response, issue);
            status stopped = crucible_generated_scope_check(scope, issue);
            if (stopped) code = stopped;
            retry = code == crucible_generated_network && safe;
        }
        if (!code) {
            if (result.response.status < 100 || result.response.status > 599 || (result.response.body.length && !result.response.body.data) || (result.response.headers.count && !result.response.headers.items)) code = fail(issue, "Invalid transport response");
            else if (result.response.status >= 200 && result.response.status < 300) {
                const yyjson_val *wire;
                code = crucible_generated_binding_success(result.arena, value->schema, op, &result.response, &wire, issue);
                if (!code && yyjson_is_str(get(op, "output"))) code = crucible_generated_model_decode(result.arena, crucible_generated_shape_lookup(text(get(op, "output"))), wire, &result.output, issue);
                if (!code && wire_output) *wire_output = wire;
            } else {
                code = crucible_generated_binding_failure(result.arena, value->schema, op, &result, issue);
                if (!code) {
                    bool transient = result.response.status == 500 || result.response.status == 502 || result.response.status == 503 || result.response.status == 504;
                    retry = result.throttling || (safe && (result.retryable || transient));
                    code = crucible_generated_fail(issue, crucible_generated_service_error, "Service returned an error; inspect result metadata");
                }
            }
        }
        status stopped = crucible_generated_scope_check(scope, NULL);
        if (stopped) { retry = false; code = crucible_generated_fail(issue, stopped, stopped == crucible_generated_cancelled ? "Call cancelled" : "Call deadline exceeded"); }
        if (!retry || attempt >= value->config.max_attempts) { *out = result; return code; }
        double delay = retry_delay(value, result.arena, attempt, result.response.headers);
        code = crucible_generated_scope_wait(scope, delay, issue);
        if (code) { *out = result; return code; }
        crucible_generated_result_free(&result);
    }
}
status crucible_generated_client_call(client *value, string name, const void *input, crucible_generated_cancelled_fn cancelled,
    void *user, crucible_generated_result *result, error *issue) {
    if (!value || !result || (!name.data && name.length)) return fail(issue, "Client, operation, and result are required");
    *result = (crucible_generated_result){0};
    crucible_generated_scope scope;
    TRY(crucible_generated_scope_begin(value->config.timeout, cancelled, user, &scope, issue));
    const yyjson_val *op = operation(value, name);
    if (!op) return fail(issue, "Unknown operation");
    arena *storage = crucible_generated_arena_new();
    if (!storage) return crucible_generated_fail(issue, OOM, "Request arena allocation failed");
    const yyjson_val *encoded;
    status code = encode_input(storage, op, input, &encoded, issue);
    if (!code) code = execute(value, storage, op, encoded, &scope, result, NULL, issue);
    crucible_generated_arena_free(storage); return code;
}
typedef struct seen_token { struct seen_token *next; string token; } seen_token;
struct crucible_generated_pages {
    client *client;
    arena *arena;
    const yyjson_val *operation, *input;
    string next;
    seen_token *seen;
    bool finished;
    crucible_generated_cancelled_fn cancelled;
    void *cancel_user;
};
void crucible_generated_pages_free(crucible_generated_pages *pages) {
    if (!pages) return;
    crucible_generated_arena_free(pages->arena); free(pages);
}
static status remember(crucible_generated_pages *pages, string token, error *issue) {
    for (seen_token *item = pages->seen; item; item = item->next) if (crucible_generated_string_equal(item->token, token)) return fail(issue, "Pagination token cycle");
    seen_token *item = crucible_generated_arena_alloc(pages->arena, sizeof(*item), issue);
    if (!item) return OOM;
    TRY(crucible_generated_arena_copy(pages->arena, token, &item->token, issue));
    item->next = pages->seen; pages->seen = item; return OK;
}
status crucible_generated_pages_new(client *value, string name, const void *input, crucible_generated_cancelled_fn cancelled,
    void *user, crucible_generated_pages **out, error *issue) {
    if (!value || !out) return fail(issue, "Client and pages output are required");
    *out = NULL;
    const yyjson_val *op = operation(value, name);
    if (!yyjson_is_obj(get(op, "pagination"))) return fail(issue, "Operation is not paginated");
    crucible_generated_pages *pages = calloc(1, sizeof(*pages));
    if (!pages) return crucible_generated_fail(issue, OOM, "Pagination allocation failed");
    pages->client = value; pages->operation = op; pages->cancelled = cancelled; pages->cancel_user = user;
    pages->arena = crucible_generated_arena_new();
    status code = pages->arena ? OK : crucible_generated_fail(issue, OOM, "Pagination allocation failed");
    if (!code) code = encode_input(pages->arena, op, input, &pages->input, issue);
    if (!code) {
        string member = text(get(get(op, "pagination"), "input"));
        const yyjson_val *token = yyjson_obj_getn((yyjson_val *)pages->input, member.data, member.length);
        if (yyjson_is_str(token) && text(token).length) code = remember(pages, text(token), issue);
    }
    if (code) { crucible_generated_pages_free(pages); return code; }
    *out = pages; return OK;
}
status crucible_generated_pages_next(crucible_generated_pages *pages, bool *has_page, crucible_generated_result *result, error *issue) {
    if (!pages || !has_page || !result) return fail(issue, "Pages, result and has_page are required");
    *has_page = false; *result = (crucible_generated_result){0};
    if (pages->finished) return OK;
    crucible_generated_scope scope;
    TRY(crucible_generated_scope_begin(pages->client->config.timeout, pages->cancelled, pages->cancel_user, &scope, issue));
    arena *storage = crucible_generated_arena_new();
    if (!storage) return crucible_generated_fail(issue, OOM, "Page request allocation failed");
    status code = OK;
    const yyjson_val *input = pages->input;
    if (pages->next.length) {
        for (seen_token *item = pages->seen; item; item = item->next) {
            if (crucible_generated_string_equal(item->token, pages->next)) {
                code = fail(issue, "Pagination token cycle"); pages->finished = true; break;
            }
        }
        yyjson_mut_doc *doc = yyjson_mut_doc_new(NULL);
        if (!doc) code = crucible_generated_fail(issue, OOM, "JSON allocation failed");
        if (!code) {
            yyjson_mut_val *copy = yyjson_val_mut_copy(doc, input);
            string member = text(get(get(pages->operation, "pagination"), "input"));
            yyjson_mut_val *key = yyjson_mut_strncpy(doc, member.data, member.length), *token = yyjson_mut_strncpy(doc, pages->next.data, pages->next.length);
            if (!copy || !key || !token || !yyjson_mut_obj_put(copy, key, token)) code = crucible_generated_fail(issue, OOM, "JSON allocation failed");
            if (!code) code = crucible_generated_json_own(storage, copy, &input, issue);
        }
        yyjson_mut_doc_free(doc);
    }
    const yyjson_val *wire = NULL;
    if (!code) code = execute(pages->client, storage, pages->operation, input, &scope, result, &wire, issue);
    if (!code && pages->next.length) code = remember(pages, pages->next, issue);
    if (!code) {
        const yyjson_val *token = wire;
        size_t index, count; yyjson_val *part;
        yyjson_arr_foreach((yyjson_val *)get(get(pages->operation, "pagination"), "output"), index, count, part) {
            string key = text(part);
            token = yyjson_obj_getn((yyjson_val *)token, key.data, key.length);
        }
        if (!token || yyjson_is_null(token) || (yyjson_is_str(token) && !text(token).length)) pages->finished = true;
        else if (!yyjson_is_str(token)) code = fail(issue, "Expected pagination string token");
        else code = crucible_generated_arena_copy(pages->arena, text(token), &pages->next, issue);
        if (!code) *has_page = true;
    }
    crucible_generated_arena_free(storage); return code;
}
