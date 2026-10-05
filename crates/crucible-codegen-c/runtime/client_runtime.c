#include "client_runtime.h"
#include "features.h"
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
static status endpoint_values(arena *storage, const crucible_generated_endpoint_value *values, size_t count, const yyjson_val **out, error *issue) {
    yyjson_mut_doc *doc = yyjson_mut_doc_new(NULL);
    if (!doc) return crucible_generated_fail(issue, OOM, "JSON allocation failed");
    yyjson_mut_val *object = yyjson_mut_obj(doc);
    status code = object ? OK : crucible_generated_fail(issue, OOM, "JSON allocation failed");
    for (size_t i = 0; !code && i < count; ++i) {
        const crucible_generated_endpoint_value *item = values + i;
        yyjson_mut_val *result = NULL;
        if ((!item->name.data && item->name.length) || (!item->text.data && item->text.length) || (item->count && !item->items)) { code = fail(issue, "Invalid endpoint parameter"); break; }
        if (item->kind == crucible_generated_endpoint_string) result = yyjson_mut_strncpy(doc, item->text.data ? item->text.data : "", item->text.length);
        else if (item->kind == crucible_generated_endpoint_boolean) result = yyjson_mut_bool(doc, item->boolean);
        else if (item->kind == crucible_generated_endpoint_string_array) {
            result = yyjson_mut_arr(doc);
            for (size_t j = 0; result && j < item->count; ++j) {
                if (!item->items[j].data && item->items[j].length) { code = fail(issue, "Invalid endpoint parameter"); break; }
                yyjson_mut_val *element = yyjson_mut_strncpy(doc, item->items[j].data ? item->items[j].data : "", item->items[j].length);
                if (!element || !yyjson_mut_arr_append(result, element)) result = NULL;
            }
        } else { code = fail(issue, "Endpoint parameters require strings, booleans, or string arrays"); break; }
        if (code) break;
        yyjson_mut_val *key = yyjson_mut_strncpy(doc, item->name.data ? item->name.data : "", item->name.length);
        if (!result || !key || !yyjson_mut_obj_put(object, key, result)) code = crucible_generated_fail(issue, OOM, "JSON allocation failed");
    }
    if (!code) code = crucible_generated_json_own(storage, object, out, issue);
    yyjson_mut_doc_free(doc);
    return code;
}
status crucible_generated_runtime_new(const crucible_generated_client_config *config, const string *chunks, size_t count,
    string endpoint, bool digest, client **out, error *issue) {
    if (!out || (!chunks && count)) return fail(issue, "Missing client output or schema");
    *out = NULL;
    crucible_generated_client_config settings = config ? *config : crucible_generated_client_defaults();
    crucible_generated_scope scope;
    TRY(crucible_generated_scope_begin(settings.timeout, NULL, NULL, &scope, issue));
    if (!settings.max_attempts || !isfinite(settings.base_delay) || settings.base_delay < 0 || !isfinite(settings.max_delay) || settings.max_delay < 0 || (settings.headers.count && !settings.headers.items) || (settings.middleware_count && !settings.middleware)
        || (settings.endpoint_parameter_count && !settings.endpoint_parameters) || (settings.endpoint_builtin_count && !settings.endpoint_builtins)
        || settings.request_checksum_calculation > crucible_generated_checksum_when_required || settings.response_checksum_validation > crucible_generated_checksum_when_required
        || (!settings.region.data && settings.region.length)) return fail(issue, "Invalid client configuration");
    if (settings.endpoint.data) endpoint = settings.endpoint;
    while (endpoint.length && endpoint.data[endpoint.length - 1] == '/') --endpoint.length;
    client *value = calloc(1, sizeof(*value));
    if (!value) return crucible_generated_fail(issue, OOM, "Client allocation failed");
    value->arena = crucible_generated_arena_new(); value->config = settings; value->digest = digest;
    status code = OK;
    if (!value->arena) { code = crucible_generated_fail(issue, OOM, "Client allocation failed"); goto failed; }
    code = crucible_generated_arena_copy(value->arena, endpoint, &value->config.endpoint, issue);
    if (!code) code = crucible_generated_arena_copy(value->arena, settings.region, &value->config.region, issue);
    if (!code) code = endpoint_values(value->arena, settings.endpoint_parameters, settings.endpoint_parameter_count, &value->endpoint_parameters, issue);
    if (!code) code = endpoint_values(value->arena, settings.endpoint_builtins, settings.endpoint_builtin_count, &value->endpoint_builtins, issue);
    if (code) goto failed;
    value->config.endpoint_parameters = NULL; value->config.endpoint_parameter_count = 0;
    value->config.endpoint_builtins = NULL; value->config.endpoint_builtin_count = 0;
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
    /* Endpoint rules may supply the whole URL; otherwise an absolute base is required. */
    if (endpoint.length || !yyjson_is_obj(get(value->schema, "endpointRules"))) {
        code = crucible_generated_validate_url(endpoint, true, false, issue);
        if (code) goto failed;
    }
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
status crucible_generated_encode_input(arena *storage, const yyjson_val *op, const void *input, const yyjson_val **out, error *issue) {
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
typedef struct {
    const yyjson_val *properties;
    crucible_generated_checksum_settings checksum;
} call_plan;
static status attempt_request(client *value, const yyjson_val *op, arena *storage, const crucible_generated_request *prepared, const call_plan *plan,
    const crucible_generated_scope *scope, crucible_generated_request *out, error *issue) {
    *out = *prepared;
    bool credentials = false; crucible_generated_signing signing;
    TRY(crucible_generated_authenticate(value, op, plan->properties, scope, storage, out, &credentials, &signing, issue));
    for (size_t i = 0; i < value->config.middleware_count; ++i) {
        TRY(crucible_generated_scope_check(scope, issue));
        const crucible_generated_middleware *hook = value->config.middleware + i;
        TRY(hook->apply(hook->user, scope, storage, out, issue));
    }
    TRY(crucible_generated_scope_check(scope, issue));
    crucible_generated_url url;
    if (!crucible_generated_url_split(out->url, &url)) return fail(issue, "Middleware must supply an absolute HTTP(S) URL");
    if ((credentials || signing.sign || crucible_generated_header_get(out->headers, VIEW("authorization"), NULL)) && !crucible_generated_string_equal(url.scheme, VIEW("https")) && !crucible_generated_loopback(url.host))
        return fail(issue, "Refusing to send credentials over plaintext HTTP outside loopback");
    TRY(crucible_generated_validate_request(out, issue));
    if (value->digest) { TRY(crucible_generated_binding_digest(storage, out, issue)); }
    TRY(crucible_generated_checksum_request(storage, &plan->checksum, out, issue));
    if (signing.sign) { TRY(crucible_generated_sigv4_sign(storage, out, &signing.identity, &signing.options, issue)); }
    return OK;
}
/* Resolves the endpoint (rules, then host prefix) and the request-wide headers. */
static status plan_endpoint(client *value, arena *storage, const yyjson_val *op, const yyjson_val *input, string *endpoint,
    crucible_generated_headers *headers, call_plan *plan, error *issue) {
    *endpoint = value->config.endpoint; *headers = value->config.headers;
    const yyjson_val *rules = get(value->schema, "endpointRules"), *prefix = get(op, "endpoint");
    if (!yyjson_is_obj((yyjson_val *)rules) && !yyjson_is_obj((yyjson_val *)prefix)) return OK;
    crucible_generated_query query;
    TRY(crucible_generated_query_begin(storage, &query, issue));
    yyjson_mut_val *model;
    TRY(crucible_generated_query_model(&query, value->schema, get(op, "input"), input, &model));
    if (yyjson_is_obj((yyjson_val *)rules)) {
        /* Caller built-ins win; the region and configured endpoint fill the standard ones. */
        yyjson_mut_doc *doc = query.doc;
        yyjson_mut_val *builtins = yyjson_val_mut_copy(doc, (yyjson_val *)value->endpoint_builtins);
        if (!builtins) return crucible_generated_fail(issue, OOM, "JSON allocation failed");
        if (value->config.region.length && !yyjson_mut_obj_get(builtins, "AWS::Region")
            && !yyjson_mut_obj_add_strncpy(doc, builtins, "AWS::Region", value->config.region.data, value->config.region.length)) return crucible_generated_fail(issue, OOM, "JSON allocation failed");
        if (value->config.endpoint.length && !yyjson_mut_obj_get(builtins, "SDK::Endpoint")
            && !yyjson_mut_obj_add_strncpy(doc, builtins, "SDK::Endpoint", value->config.endpoint.data, value->config.endpoint.length)) return crucible_generated_fail(issue, OOM, "JSON allocation failed");
        const yyjson_val *owned_builtins;
        TRY(crucible_generated_json_own(storage, builtins, &owned_builtins, issue));
        crucible_generated_headers resolved; yyjson_mut_val *properties;
        TRY(crucible_generated_endpoint_evaluate(&query, rules, get(op, "endpointParameters"), model, value->endpoint_parameters, owned_builtins, endpoint, &resolved, &properties));
        while (endpoint->length && endpoint->data[endpoint->length - 1] == '/') --endpoint->length;
        if (properties) { TRY(crucible_generated_json_own(storage, properties, &plan->properties, issue)); }
        for (size_t i = 0; i < resolved.count; ++i) TRY(crucible_generated_header_set(storage, headers, resolved.items[i].name, resolved.items[i].value, issue));
    }
    if (yyjson_is_obj((yyjson_val *)prefix) && !value->config.disable_host_prefix) TRY(crucible_generated_endpoint_prefix(storage, *endpoint, prefix, model, endpoint, issue));
    return OK;
}
status crucible_generated_execute(client *value, arena *prepared_arena, string name, const yyjson_val *op, const yyjson_val *input,
    const crucible_generated_scope *scope, crucible_generated_result *out, const yyjson_val **wire_output, error *issue) {
    const yyjson_val *tokenized;
    call_plan plan = {0};
    TRY(crucible_generated_binding_token(prepared_arena, op, input, &tokenized, issue));
    TRY(crucible_generated_checksum_prepare(prepared_arena, op, &tokenized, value->config.request_checksum_calculation, value->config.response_checksum_validation, &plan.checksum, issue));
    string endpoint; crucible_generated_headers headers;
    TRY(plan_endpoint(value, prepared_arena, op, tokenized, &endpoint, &headers, &plan, issue));
    crucible_generated_request prepared;
    TRY(crucible_generated_binding_prepare(prepared_arena, value->schema, name, op, tokenized, endpoint, headers, &prepared, issue));
    const bool safe = yyjson_get_bool((yyjson_val *)get(op, "safe"));
    const yyjson_val *bindings = get(op, "response");
    const bool read_body = yyjson_arr_size((yyjson_val *)get(bindings, "body")) || yyjson_is_arr((yyjson_val *)get(bindings, "payload")) || plan.checksum.response_count;
    for (unsigned attempt = 1;; ++attempt) {
        TRY(crucible_generated_scope_check(scope, issue));
        crucible_generated_result result = {0};
        result.arena = crucible_generated_arena_new();
        if (!result.arena) return crucible_generated_fail(issue, OOM, "Response arena allocation failed");
        crucible_generated_request request;
        status code = attempt_request(value, op, result.arena, &prepared, &plan, scope, &request, issue);
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
                const char *validated = NULL;
                code = crucible_generated_checksum_response(&plan.checksum, &result.response, &validated, issue);
                if (!code && validated && value->config.checksum_validated) value->config.checksum_validated(value->config.checksum_user, name, VIEW(validated));
                const yyjson_val *wire = NULL;
                if (!code) code = crucible_generated_binding_success(result.arena, value->schema, op, &result.response, &wire, issue);
                if (!code && yyjson_is_str((yyjson_val *)get(op, "output"))) code = crucible_generated_model_decode(result.arena, crucible_generated_shape_lookup(text(get(op, "output"))), wire, &result.output, issue);
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
    status code = crucible_generated_encode_input(storage, op, input, &encoded, issue);
    if (!code) code = crucible_generated_execute(value, storage, name, op, encoded, &scope, result, NULL, issue);
    crucible_generated_arena_free(storage); return code;
}
typedef struct seen_token { struct seen_token *next; string token; } seen_token;
struct crucible_generated_pages {
    client *client;
    arena *arena;
    const yyjson_val *operation, *input;
    string name, next;
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
    if (!code) code = crucible_generated_arena_copy(pages->arena, name, &pages->name, issue);
    if (!code) code = crucible_generated_encode_input(pages->arena, op, input, &pages->input, issue);
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
    if (!code) code = crucible_generated_execute(pages->client, storage, pages->name, pages->operation, input, &scope, result, &wire, issue);
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
