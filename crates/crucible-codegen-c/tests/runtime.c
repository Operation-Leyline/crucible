#include "weather_client/client.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define VIEW(value) weather_string_view(value)
static void check(bool condition, const char *message) {
    if (!condition) { fprintf(stderr, "C client check failed: %s\n", message); exit(1); }
}
static void good(weather_status status, const weather_error *error) { check(status == weather_ok, error->message); }
static const weather_json *get(const weather_json *object, const char *name) { return weather_json_get(object, VIEW(name)); }
static weather_string text(const weather_json *value) {
    weather_string result = {0}; weather_error error = {0};
    good(weather_json_text(value, &result, &error), &error); return result;
}
static int64_t integer(const weather_json *value) {
    int64_t result = 0; weather_error error = {0};
    good(weather_json_integer(value, INT64_MIN, INT64_MAX, &result, &error), &error); return result;
}
static bool boolean(const weather_json *value) {
    bool result = false; weather_error error = {0};
    good(weather_json_boolean_value(value, &result, &error), &error); return result;
}
static const char *environment(const char *name) { const char *value = getenv(name); check(value != NULL, name); return value; }
static const weather_json *fixture(weather_arena *arena, const char *name) {
    FILE *stream = fopen(environment(name), "rb");
    char *buffer; long size; const weather_json *value = NULL; weather_error error = {0};
    check(stream != NULL, "open fixture"); check(fseek(stream, 0, SEEK_END) == 0, "fixture seek");
    size = ftell(stream); check(size >= 0, "fixture length"); rewind(stream);
    buffer = malloc((size_t)size + 1); check(buffer != NULL, "fixture allocation");
    check(fread(buffer, 1, (size_t)size, stream) == (size_t)size, "fixture read"); fclose(stream);
    buffer[size] = 0;
    good(weather_json_parse(arena, (weather_string){buffer, (size_t)size}, &value, &error), &error);
    free(buffer); return value;
}
static bool equal(weather_string value, const char *expected) { return weather_string_equal(value, VIEW(expected)); }
static bool contains(weather_string value, const char *part) {
    size_t length = strlen(part);
    if (length > value.length) return false;
    for (size_t i = 0; i <= value.length - length; ++i) if (memcmp(value.data + i, part, length) == 0) return true;
    return false;
}
static weather_string header(const weather_request *request, const char *name) {
    weather_string value = {0}; check(weather_header_get(request->headers, VIEW(name), &value), name); return value;
}
static void respond(weather_arena *arena, weather_response *response, int status, weather_string body, weather_error *error) {
    weather_string copied = {0};
    good(weather_arena_copy(arena, body, &copied, error), error);
    response->status = status; response->body = (weather_blob){(const unsigned char *)copied.data, copied.length};
}
enum fake_mode { retry_case, header_case, malformed_case, invalid_success, pagination_cycle, pagination_end, pagination_failure,
                 wait_retry, auth_override, binding_case, network_retry };
typedef struct { enum fake_mode mode; const weather_json *row; unsigned attempts; bool first_only; } fake;

static weather_status send_fake(void *user, const weather_request *request, bool read_body,
    const weather_scope *scope, weather_arena *arena, weather_response *response, weather_error *error) {
    fake *state = user; ++state->attempts;
    good(weather_scope_check(scope, error), error);
    switch (state->mode) {
    case retry_case: {
        char body[256] = "{}"; const weather_json *name = get(state->row, "error");
        if (weather_json_type(name) != weather_json_null) {
            weather_string value = text(name);
            (void)snprintf(body, sizeof(body), "{\"__type\":\"%.*s\"}", (int)value.length, value.data);
        }
        respond(arena, response, state->attempts <= (unsigned)integer(get(state->row, "failures")) ? (int)integer(get(state->row, "status")) : 200, VIEW(body), error);
        good(weather_header_set(arena, &response->headers, VIEW("x-amzn-errortype"), VIEW("Throttled"), error), error);
        break;
    }
    case header_case:
        check(weather_string_equal(header(request, "x-items"), text(get(state->row, "encoded"))), "canonical header encoding");
        check(!read_body, "header-only response does not read body");
        respond(arena, response, 200, VIEW("{bad"), error);
        good(weather_header_set(arena, &response->headers, VIEW("x-items"), text(get(state->row, "wire")), error), error);
        break;
    case malformed_case: {
        const char *names[] = {"x-request-id", "retry-after", "x-extra"};
        if (state->first_only && state->attempts > 1) { respond(arena, response, 200, VIEW("{}"), error); break; }
        respond(arena, response, (int)integer(get(state->row, "status")), text(get(state->row, "body")), error);
        for (size_t i = 0; i < sizeof(names) / sizeof(names[0]); ++i) {
            const weather_json *value = get(get(state->row, "headers"), names[i]);
            if (value) good(weather_header_set(arena, &response->headers, VIEW(names[i]), text(value), error), error);
        }
        break;
    }
    case invalid_success: respond(arena, response, 200, VIEW("{bad"), error); break;
    case pagination_cycle:
    case pagination_end:
    case pagination_failure:
        if (state->attempts == 1) check(!contains(request->url, "nextToken="), "initial paginator input");
        else check(contains(request->url, "nextToken=same"), "next page token");
        if (state->mode == pagination_failure && state->attempts == 2) { error->status = weather_network; return weather_network; }
        respond(arena, response, 200, VIEW(state->mode != pagination_cycle && state->attempts > 1 ? "{\"cities\":[]}" : "{\"cities\":[],\"nextToken\":\"same\"}"), error);
        break;
    case wait_retry:
        respond(arena, response, 503, VIEW("{}"), error);
        good(weather_header_set(arena, &response->headers, VIEW("retry-after"), VIEW("2147484"), error), error);
        break;
    case auth_override:
        check(equal(header(request, "authorization"), "Bearer middleware"), "middleware replaces authorization case-insensitively");
        check(equal(header(request, "content-type"), "custom/type"), "middleware replaces content type");
        respond(arena, response, 201, VIEW("{\"city\":{\"id\":\"x\",\"name\":\"n\"}}"), error);
        break;
    case binding_case:
        check(contains(request->url, "a%2Fb%20%2A%3F%23%C3%A9"), "escaped label");
        check(contains(request->url, "at=2026-10-01T00%3A00%3A00Z"), "timestamp query");
        respond(arena, response, 200, VIEW("{\"city\":{\"id\":\"x\",\"name\":\"n\"}}"), error);
        good(weather_header_set(arena, &response->headers, VIEW("X-Request-Id"), VIEW("bound"), error), error);
        break;
    case network_retry:
        if (state->attempts < 3) { error->status = weather_network; return weather_network; }
        respond(arena, response, 200, VIEW("{}"), error);
        break;
    }
    return weather_ok;
}
static weather_client_config config(fake *state) {
    weather_client_config settings = weather_client_defaults();
    settings.endpoint = VIEW("https://example.invalid"); settings.base_delay = 0;
    if (state) { settings.send = send_fake; settings.transport_user = state; }
    return settings;
}
static weather_client *client_new(weather_client_config settings) {
    weather_client *client = NULL; weather_error error = {0};
    good(weather_client_new(&settings, &client, &error), &error); return client;
}
static void shared_cases(void) {
    weather_arena *arena = weather_arena_new(); weather_error error = {0};
    const weather_json *rows = fixture(arena, "RETRY_CASES");
    for (size_t i = 0; i < weather_json_size(rows); ++i) {
        const weather_json *row = weather_json_at(rows, i); fake state = {retry_case, row, 0, false};
        weather_client_config settings = config(&state); settings.max_attempts = (unsigned)integer(get(row, "maxAttempts"));
        weather_client *client = client_new(settings); weather_result result = {0}; weather_unit unit = {0};
        weather_status status = weather_client_call(client, VIEW(boolean(get(row, "safe")) ? "RetrySafe" : "RetryUnsafe"), &unit, NULL, NULL, &result, &error);
        if (state.attempts != (unsigned)integer(get(row, "attempts")) || (status == weather_ok) != boolean(get(row, "success"))) {
            weather_string name = text(get(row, "name"));
            fprintf(stderr, "retry row %.*s: status=%d attempts=%u message=%s\n", (int)name.length, name.data, (int)status, state.attempts, error.message); exit(1);
        }
        weather_client_free(client); weather_result_free(&result);
    }
    rows = fixture(arena, "HEADER_CASES");
    for (size_t i = 0; i < weather_json_size(rows); ++i) {
        const weather_json *row = weather_json_at(rows, i), *values = get(row, "values");
        fake state = {header_case, row, 0, false}; weather_client *client = client_new(config(&state));
        weather_models_tags tags = {0}; weather_models_header_input input = {0}; weather_operations_header_echo_result result = {0};
        weather_string *items = weather_arena_alloc(arena, weather_json_size(values) * sizeof(*items), &error);
        check(items != NULL, "header values allocation");
        for (size_t j = 0; j < weather_json_size(values); ++j) items[j] = text(weather_json_at(values, j));
        tags.length = weather_json_size(values); tags.items = items;
        input.items.state = weather_present; input.items.value = &tags;
        good(weather_operations_header_echo(client, &input, NULL, NULL, &result, &error), &error);
        check(result.value && result.value->items.state == weather_present, "header-only typed output");
        check(result.value->items.value->length == tags.length, "header list length");
        for (size_t j = 0; j < tags.length; ++j) check(weather_string_equal(result.value->items.value->items[j], items[j]), "header list item");
        weather_operations_header_echo_result_free(&result); weather_client_free(client);
    }
    rows = fixture(arena, "MALFORMED_CASES");
    for (size_t i = 0; i < weather_json_size(rows); ++i) {
        const weather_json *row = weather_json_at(rows, i);
        for (unsigned limit = 1; limit <= 2; ++limit) {
            fake state = {malformed_case, row, 0, true}; weather_client_config settings = config(&state);
            weather_operations_ping_result result = {0}; settings.max_attempts = limit;
            weather_client *client = client_new(settings);
            weather_status status = weather_operations_ping(client, NULL, NULL, &result, &error);
            check(state.attempts == limit, "malformed error attempt count");
            if (limit == 2) good(status, &error);
            else {
                weather_string raw = {(const char *)result.base.response.body.data, result.base.response.body.length}, extra;
                check(status == weather_service_error, "malformed modeled error remains service error");
                check(weather_string_equal(result.base.name, text(get(row, "error"))), "malformed error name");
                check(weather_string_equal(result.base.message, text(get(row, "message"))), "malformed error message");
                check(weather_string_equal(result.base.request_id, text(get(row, "requestId"))), "malformed request ID");
                check(result.base.response.status == integer(get(row, "status")), "actual wire status");
                check(weather_string_equal(raw, text(get(row, "body"))), "raw error body");
                check(result.base.retryable == boolean(get(row, "retryable")) && result.base.throttling == boolean(get(row, "throttling")), "modeled retry traits");
                check(weather_header_get(result.base.response.headers, VIEW("x-extra"), &extra) && equal(extra, "preserved"), "error headers");
            }
            weather_operations_ping_result_free(&result); weather_client_free(client);
        }
    }
    weather_arena_free(arena);
}
static weather_status token(void *user, const weather_scope *scope, weather_arena *arena, bool *present, weather_string *value, weather_error *error) {
    unsigned *count = user; char buffer[32];
    good(weather_scope_check(scope, error), error); ++*count;
    (void)snprintf(buffer, sizeof(buffer), "%u", *count); *present = true;
    return weather_arena_copy(arena, VIEW(buffer), value, error);
}
static weather_status override_headers(void *user, const weather_scope *scope, weather_arena *arena, weather_request *request, weather_error *error) {
    (void)user; (void)scope;
    good(weather_header_set(arena, &request->headers, VIEW("Authorization"), VIEW("Bearer middleware"), error), error);
    return weather_header_set(arena, &request->headers, VIEW("Content-Type"), VIEW("custom/type"), error);
}
static weather_status change_body(void *user, const weather_scope *scope, weather_arena *arena, weather_request *request, weather_error *error) {
    (void)user; (void)scope;
    if (request->has_body) {
        unsigned char *bytes = weather_arena_alloc(arena, request->body.length + 1, error);
        if (!bytes) return weather_out_of_memory;
        memcpy(bytes, request->body.data, request->body.length); bytes[request->body.length] = ' ';
        request->body = (weather_blob){bytes, request->body.length + 1};
    }
    return weather_ok;
}
static bool cancelled(void *user) { return user == NULL || weather_scope_remaining((const weather_scope *)user) <= 0; }
static void runtime_cases(void) {
    weather_error error = {0}; weather_models_get_city_input input = {0}; weather_operations_get_city_result result = {0};
    fake state = {binding_case, NULL, 0, false}; weather_client *client = client_new(config(&state));
    input.city_id.state = weather_present; input.city_id.value = VIEW("a/b *?#\xC3\xA9");
    input.at.state = weather_present; good(weather_timestamp_parse(VIEW("2026-10-01T00:00:00Z"), &input.at.value, &error), &error);
    good(weather_operations_get_city(client, &input, NULL, NULL, &result, &error), &error);
    check(equal(result.value->request_id.value, "bound"), "typed header binding");
    weather_operations_get_city_result_free(&result); weather_client_free(client);
    state = (fake){invalid_success, NULL, 0, false}; client = client_new(config(&state));
    check(weather_operations_get_city(client, &input, NULL, NULL, &result, &error) == weather_invalid, "malformed successful JSON");
    check(state.attempts == 1, "malformed success does not retry");
    weather_operations_get_city_result_free(&result); weather_client_free(client);
    for (unsigned mode = 0; mode < 2; ++mode) {
        weather_models_list_cities_input paging = {0}; weather_pages *pages = NULL;
        weather_operations_list_cities_result page = {0}; bool has_page = false;
        state = (fake){mode ? pagination_end : pagination_cycle, NULL, 0, false}; client = client_new(config(&state));
        good(weather_operations_list_cities_pages_new(client, &paging, NULL, NULL, &pages, &error), &error);
        check(state.attempts == 0, "lazy pagination");
        paging.next_token.state = weather_present; paging.next_token.value = VIEW("caller-mutated");
        for (unsigned i = 0; i < 2; ++i) {
            good(weather_operations_list_cities_pages_next(pages, &has_page, &page, &error), &error);
            check(has_page, "expected page"); weather_operations_list_cities_result_free(&page);
        }
        weather_status status = weather_operations_list_cities_pages_next(pages, &has_page, &page, &error);
        check(status == (mode ? weather_ok : weather_invalid), "pagination exhaustion or cycle");
        check(!has_page && state.attempts == 2, "no request after pagination completion");
        weather_operations_list_cities_result_free(&page); weather_pages_free(pages); weather_client_free(client);
    }
    { weather_models_list_cities_input paging = {0}; weather_pages *pages = NULL; bool has_page = false;
      weather_operations_list_cities_result page = {0}; weather_client_config settings;
      state = (fake){pagination_failure, NULL, 0, false}; settings = config(&state); settings.max_attempts = 1; client = client_new(settings);
      good(weather_operations_list_cities_pages_new(client, &paging, NULL, NULL, &pages, &error), &error);
      good(weather_operations_list_cities_pages_next(pages, &has_page, &page, &error), &error); check(has_page, "first recoverable page");
      weather_operations_list_cities_result_free(&page);
      check(weather_operations_list_cities_pages_next(pages, &has_page, &page, &error) == weather_network, "second page transient failure");
      check(!has_page, "failed page has no output"); weather_operations_list_cities_result_free(&page);
      good(weather_operations_list_cities_pages_next(pages, &has_page, &page, &error), &error);
      check(has_page && state.attempts == 3, "failed page can be retried without false token cycle");
      weather_operations_list_cities_result_free(&page); weather_pages_free(pages); weather_client_free(client); }
    { weather_operations_ping_result ping = {0}; weather_client_config settings;
      state = (fake){wait_retry, NULL, 0, false}; settings = config(&state); settings.timeout = 0.05; client = client_new(settings);
      check(weather_operations_ping(client, NULL, NULL, &ping, &error) == weather_deadline, "Retry-After deadline");
      check(state.attempts == 1, "deadline stops retry"); weather_operations_ping_result_free(&ping); weather_client_free(client);
      state = (fake){network_retry, NULL, 0, false}; client = client_new(config(&state));
      check(weather_operations_ping(client, cancelled, NULL, &ping, &error) == weather_cancelled, "pre-cancelled call");
      check(state.attempts == 0, "pre-cancel prevents transport"); weather_operations_ping_result_free(&ping);
      good(weather_operations_ping(client, NULL, NULL, &ping, &error), &error); check(state.attempts == 3, "safe network failure retries");
      weather_operations_ping_result_free(&ping); weather_client_free(client); }
    { unsigned count = 0; weather_models_create_city_input create = {0}; weather_operations_create_city_result created = {0};
      weather_middleware middleware = {override_headers, NULL}; state = (fake){auth_override, NULL, 0, false};
      weather_client_config settings = config(&state); settings.token = token; settings.token_user = &count; settings.middleware = &middleware; settings.middleware_count = 1;
      client = client_new(settings); create.name.state = weather_present; create.name.value = VIEW("middleware");
      good(weather_operations_create_city(client, &create, NULL, NULL, &created, &error), &error);
      check(create.client_token.state == weather_absent, "generated token does not mutate input");
      weather_client_free(client); check(equal(created.value->city.value->name.value, "n"), "result outlives client");
      weather_operations_create_city_result_free(&created); }
}
static weather_client_config native_config(const char *path, char *endpoint) {
    weather_client_config settings = config(NULL);
    int size = snprintf(endpoint, 2048, "%s/native/%s", environment("FIXTURE_URL"), path);
    check(size > 0 && size < 2048, "fixture endpoint length"); settings.endpoint = VIEW(endpoint); return settings;
}
static void real_http(void) {
    weather_error error = {0}; unsigned count = 0; weather_client_config settings = config(NULL);
    weather_models_create_city_input input = {0}; weather_operations_create_city_result created = {0};
    settings.endpoint = VIEW(environment("WEATHER_URL")); settings.token = token; settings.token_user = &count;
    weather_client *client = client_new(settings); input.name.state = weather_present; input.name.value = VIEW("C city");
    good(weather_operations_create_city(client, &input, NULL, NULL, &created, &error), &error);
    weather_string id = created.value->city.value->id.value;
    { weather_models_get_city_input read = {0}; weather_operations_get_city_result result = {0};
      read.city_id.state = weather_present; read.city_id.value = id;
      good(weather_operations_get_city(client, &read, NULL, NULL, &result, &error), &error);
      check(equal(result.value->city.value->name.value, "C city"), "native create/read"); weather_operations_get_city_result_free(&result); }
    { const unsigned char bytes[] = {0, 128, 255, '{', '}'}; weather_models_put_city_photo_input put = {0};
      weather_operations_put_city_photo_result saved = {0}; weather_models_get_city_photo_input read = {0}; weather_operations_get_city_photo_result loaded = {0};
      put.city_id.state = weather_present; put.city_id.value = id; put.photo.state = weather_present; put.photo.value = (weather_blob){bytes, sizeof(bytes)};
      good(weather_operations_put_city_photo(client, &put, NULL, NULL, &saved, &error), &error);
      check(saved.value->photo.value->size.value == (int32_t)sizeof(bytes), "native payload length");
      read.city_id.state = weather_present; read.city_id.value = id;
      good(weather_operations_get_city_photo(client, &read, NULL, NULL, &loaded, &error), &error);
      check(loaded.value->photo.value.length == sizeof(bytes) && memcmp(loaded.value->photo.value.data, bytes, sizeof(bytes)) == 0, "native payload bytes");
      weather_operations_put_city_photo_result_free(&saved); weather_operations_get_city_photo_result_free(&loaded); }
    { weather_models_list_cities_input paging = {0}; weather_pages *pages = NULL; bool has_page;
      weather_operations_list_cities_result page = {0}; paging.max_results.state = weather_present; paging.max_results.value = 1;
      good(weather_operations_list_cities_pages_new(client, &paging, NULL, NULL, &pages, &error), &error);
      good(weather_operations_list_cities_pages_next(pages, &has_page, &page, &error), &error); check(has_page, "native pagination");
      weather_operations_list_cities_result_free(&page); weather_pages_free(pages); }
    { weather_operations_create_city_result duplicate = {0};
      check(weather_operations_create_city(client, &input, NULL, NULL, &duplicate, &error) == weather_service_error, "native conflict");
      check(weather_error_as_conflict_error(&duplicate.base) != NULL, "typed modeled error accessor"); weather_operations_create_city_result_free(&duplicate); }
    weather_operations_create_city_result_free(&created); weather_client_free(client);
    { const char *paths[] = {"partial", "bad-length"}; char endpoint[2048];
      for (size_t i = 0; i < 2; ++i) {
        settings = native_config(paths[i], endpoint); client = client_new(settings);
        weather_models_get_city_photo_input read = {0}; weather_operations_get_city_photo_result result = {0};
        read.city_id.state = weather_present; read.city_id.value = VIEW("x");
        good(weather_operations_get_city_photo(client, &read, NULL, NULL, &result, &error), &error);
        check(result.value->photo.value.length == 8 && memcmp(result.value->photo.value.data, "complete", 8) == 0, "interrupted response retry");
        weather_operations_get_city_photo_result_free(&result); weather_client_free(client);
      } }
    { char endpoint[2048]; weather_models_get_city_input read = {0}; weather_operations_get_city_result result = {0};
      read.city_id.state = weather_present; read.city_id.value = VIEW("x");
      settings = native_config("not-modified", endpoint); settings.max_attempts = 1; client = client_new(settings);
      check(weather_operations_get_city(client, &read, NULL, NULL, &result, &error) == weather_service_error && result.base.response.status == 304, "bodyless 304");
      weather_operations_get_city_result_free(&result); weather_client_free(client);
      settings = native_config("stall", endpoint); settings.timeout = 0.15; client = client_new(settings);
      weather_scope timer; good(weather_scope_begin(10, NULL, NULL, &timer, &error), &error);
      check(weather_operations_get_city(client, &read, NULL, NULL, &result, &error) == weather_deadline, "stalled response deadline");
      check(weather_scope_remaining(&timer) > 8.5, "deadline interrupts stalled body promptly");
      weather_operations_get_city_result_free(&result); weather_client_free(client);
      settings.timeout = 5; client = client_new(settings); weather_scope stop;
      good(weather_scope_begin(0.10, NULL, NULL, &stop, &error), &error); good(weather_scope_begin(10, NULL, NULL, &timer, &error), &error);
      check(weather_operations_get_city(client, &read, cancelled, &stop, &result, &error) == weather_cancelled, "stalled response cancellation");
      check(weather_scope_remaining(&timer) > 8.5, "cancellation interrupts stalled body promptly");
      weather_operations_get_city_result_free(&result); weather_client_free(client); }
    { char endpoint[2048]; weather_operations_ping_result result = {0};
      settings = native_config("discard", endpoint); settings.timeout = 0.5; client = client_new(settings);
      good(weather_operations_ping(client, NULL, NULL, &result, &error), &error); weather_operations_ping_result_free(&result); weather_client_free(client);
      settings = native_config("redirect", endpoint); settings.max_attempts = 1; client = client_new(settings);
      check(weather_operations_ping(client, NULL, NULL, &result, &error) == weather_service_error && result.base.response.status == 302, "no redirect following");
      weather_operations_ping_result_free(&result); weather_client_free(client);
      weather_header empty = {VIEW("x-empty"), VIEW("")}; settings = native_config("empty-header", endpoint); settings.headers = (weather_headers){&empty, 1}; client = client_new(settings);
      good(weather_operations_ping(client, NULL, NULL, &result, &error), &error); weather_operations_ping_result_free(&result); weather_client_free(client); }
    { char endpoint[2048]; weather_middleware middleware = {change_body, NULL}; weather_operations_create_city_result result = {0};
      count = 0; settings = native_config("echo", endpoint); settings.token = token; settings.token_user = &count; settings.middleware = &middleware; settings.middleware_count = 1; client = client_new(settings);
      good(weather_operations_create_city(client, &input, NULL, NULL, &result, &error), &error); check(count == 2, "native credentials refresh on retry");
      check(input.client_token.state == weather_absent, "native retry does not mutate input");
      weather_operations_create_city_result_free(&result); weather_client_free(client);
      count = 0; settings = native_config("cache", endpoint); settings.token = token; settings.token_user = &count; client = client_new(settings);
      for (unsigned i = 0; i < 2; ++i) {
        weather_models_get_city_input read = {0}; weather_operations_get_city_result got = {0}; read.city_id.state = weather_present; read.city_id.value = VIEW("c");
        good(weather_operations_get_city(client, &read, NULL, NULL, &got, &error), &error);
        check(equal(got.value->city.value->name.value, i ? "Bearer 2" : "Bearer 1"), "fresh authenticated response without cache");
        weather_operations_get_city_result_free(&got);
      }
      weather_client_free(client); }
}
int main(void) {
    shared_cases(); runtime_cases(); real_http();
    puts("C shared fixtures, typed models, cancellation, pagination and native HTTP passed"); return 0;
}
