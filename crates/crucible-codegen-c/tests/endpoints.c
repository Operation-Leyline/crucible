#include "behavior_client/client.h"
#define PREFIX behavior
#include "echo.h"

#define VIEW(text) behavior_string_view(text)
typedef struct { int calls; char host[128]; bool authorized; } fake;
static behavior_string header(const behavior_request *request, const char *name) {
    behavior_string value = {0}; (void)behavior_header_get(request->headers, VIEW(name), &value); return value;
}
static behavior_status send_fake(void *user, const behavior_request *request, bool read_body,
    const behavior_scope *scope, behavior_arena *arena, behavior_response *response, behavior_error *error) {
    fake *state = user; (void)read_body; (void)scope; (void)arena; (void)error;
    ++state->calls;
    const char *start = (const char *)memchr(request->url.data, ':', request->url.length) + 3, *end = start;
    while (end < request->url.data + request->url.length && *end != '/' && *end != ':') ++end;
    check((size_t)(end - start) < sizeof(state->host), "host length");
    memcpy(state->host, start, (size_t)(end - start)); state->host[end - start] = 0;
    check(equal(header(request, "x-region"), "local"), "endpoint rule header");
    behavior_string authorization = header(request, "authorization");
    state->authorized = authorization.data != NULL;
    if (equal(request->url, "https://local.example.test/public") || contains(request->url, "/public")) check(!authorization.data, "public operation is anonymous");
    else {
        check(contains(authorization, "/local/example/aws4_request"), "endpoint signing region and name");
        check(equal(header(request, "x-amz-content-sha256"), "UNSIGNED-PAYLOAD"), "unsigned payload");
    }
    static const char body[] = "{\"wire-status\":\"READY\"}";
    response->status = 200; response->body = (behavior_blob){(const unsigned char *)body, sizeof(body) - 1};
    return behavior_ok;
}
static behavior_status credentials(void *user, const behavior_scope *scope, behavior_arena *arena, bool *present, behavior_aws_credentials *out, behavior_error *error) {
    (void)user; (void)scope; (void)arena; (void)error;
    *present = true; *out = (behavior_aws_credentials){VIEW("key"), VIEW("secret"), {0}, false, 0}; return behavior_ok;
}
static behavior_client *client_new(behavior_client_config config) {
    behavior_client *client = NULL; behavior_error error = {0};
    good(behavior_client_new(&config, &client, &error), &error, "client_new");
    return client;
}
static const behavior_json *parse(behavior_arena *arena, const char *text) {
    const behavior_json *value = NULL; behavior_error error = {0};
    good(behavior_json_parse(arena, VIEW(text), &value, &error), &error, text);
    return value;
}
static const behavior_json *call(behavior_arena *arena, const char *name, const char *arguments) {
    const behavior_json *value = NULL; behavior_error error = {0};
    good(behavior_endpoint_function(arena, VIEW(name), parse(arena, arguments), &value, &error), &error, name);
    return value;
}
static bool text_is(const behavior_json *value, const char *expected) {
    behavior_string text = {0}; behavior_error error = {0};
    return value && behavior_json_type(value) == behavior_json_string && !behavior_json_text(value, &text, &error) && equal(text, expected);
}
static bool boolean_is(const behavior_json *value, bool expected) {
    bool flag = !expected; behavior_error error = {0};
    return value && !behavior_json_boolean_value(value, &flag, &error) && flag == expected;
}

static void generated_resolution(void) {
    fake state = {0, {0}, false}; behavior_error error = {0};
    behavior_client_config config = behavior_client_defaults();
    config.send = send_fake; config.transport_user = &state; config.region = VIEW("local");
    config.credentials = credentials;
    behavior_client *client = client_new(config);
    behavior_models_input input = {0};
    behavior_models_input_set_account(&input, VIEW("customer"));
    behavior_operations_read_result result = {0};
    good(behavior_operations_read(client, &input, NULL, NULL, &result, &error), &error, "resolved read");
    check(equal(behavior_models_output_get_status(result.value), "READY") && !strcmp(state.host, "customer.local.example.test"), "rule endpoint with host prefix");
    behavior_operations_read_result_free(&result);
    char long_label[65]; memset(long_label, 'x', 64); long_label[64] = 0;
    const char *labels[] = {"a.b", "-bad", "x/y", "x@evil", "", long_label};
    for (size_t i = 0; i < sizeof(labels) / sizeof(labels[0]); ++i) {
        behavior_models_input_set_account(&input, VIEW(labels[i]));
        check(behavior_operations_read(client, &input, NULL, NULL, &result, &error) == behavior_invalid && message_contains(&error, "host label"), labels[i]);
        behavior_operations_read_result_free(&result);
    }
    check(state.calls == 1, "invalid hosts are never dispatched");
    behavior_operations_public_result anonymous = {0};
    good(behavior_operations_public(client, NULL, NULL, &anonymous, &error), &error, "public");
    check(!state.authorized && !strcmp(state.host, "local.example.test"), "anonymous rule endpoint");
    behavior_operations_public_result_free(&anonymous);
    behavior_client_free(client);

    config.disable_host_prefix = true; client = client_new(config);
    behavior_models_input_set_account(&input, VIEW("ignored"));
    good(behavior_operations_read(client, &input, NULL, NULL, &result, &error), &error, "prefix disabled");
    check(!strcmp(state.host, "local.example.test"), "host prefix disabled");
    behavior_operations_read_result_free(&result); behavior_client_free(client);

    config.region = VIEW("missing"); client = client_new(config);
    check(behavior_operations_read(client, &input, NULL, NULL, &result, &error) == behavior_invalid && message_contains(&error, "Unknown region missing"), "rule error message");
    behavior_operations_read_result_free(&result); behavior_client_free(client);

    config.region = VIEW(""); client = client_new(config);
    check(behavior_operations_read(client, &input, NULL, NULL, &result, &error) == behavior_invalid && message_contains(&error, "Region"), "required built-in");
    behavior_operations_read_result_free(&result); behavior_client_free(client);

    behavior_endpoint_value builtin = {VIEW("AWS::Region"), behavior_endpoint_string, VIEW("local"), false, NULL, 0};
    config.endpoint_builtins = &builtin; config.endpoint_builtin_count = 1; client = client_new(config);
    good(behavior_operations_read(client, &input, NULL, NULL, &result, &error), &error, "caller built-in");
    behavior_operations_read_result_free(&result); behavior_client_free(client);
}

static void parameter_precedence(void) {
    behavior_arena *arena = behavior_arena_new(); behavior_error error = {0};
    const behavior_json *rules = parse(arena, "{\"parameters\":{\"Name\":{\"kind\":\"string\",\"required\":true},\"Flag\":{\"kind\":\"boolean\",\"required\":true}},"
        "\"rules\":[{\"conditions\":[{\"function\":\"isSet\",\"arguments\":[{\"kind\":\"reference\",\"name\":\"Flag\"}],\"assign\":null}],"
        "\"result\":{\"kind\":\"endpoint\",\"url\":{\"kind\":\"string\",\"parts\":[{\"kind\":\"literal\",\"value\":\"https://example.test/base\"}]},"
        "\"headers\":{\"name\":[{\"kind\":\"reference\",\"name\":\"Name\"}]},\"properties\":{\"flag\":{\"kind\":\"reference\",\"name\":\"Flag\"}}}}]}");
    const behavior_json *sources = parse(arena, "{\"Name\":[{\"kind\":\"inputMember\",\"member\":\"name\"},{\"kind\":\"client\",\"name\":\"name\"},{\"kind\":\"default\",\"value\":\"default\"}],"
        "\"Flag\":[{\"kind\":\"builtIn\",\"name\":\"Flag\"}]}");
    const behavior_json *flag = parse(arena, "{\"Flag\":false}"), *client = parse(arena, "{\"name\":\"client\"}"), *none = parse(arena, "{}");
    behavior_endpoint endpoint = {0}; behavior_string value = {0};
    good(behavior_endpoint_resolve(arena, rules, sources, parse(arena, "{\"name\":\"input\"}"), client, flag, &endpoint, &error), &error, "input wins");
    check(equal(endpoint.url, "https://example.test/base") && behavior_header_get(endpoint.headers, VIEW("name"), &value) && equal(value, "input"), "input member");
    check(boolean_is(behavior_json_get(endpoint.properties, VIEW("flag")), false), "false is a set parameter");
    good(behavior_endpoint_resolve(arena, rules, sources, none, client, flag, &endpoint, &error), &error, "client");
    check(behavior_header_get(endpoint.headers, VIEW("name"), &value) && equal(value, "client"), "client parameter");
    good(behavior_endpoint_resolve(arena, rules, sources, none, none, flag, &endpoint, &error), &error, "default");
    check(behavior_header_get(endpoint.headers, VIEW("name"), &value) && equal(value, "default"), "default parameter");
    check(behavior_endpoint_resolve(arena, rules, sources, none, none, none, &endpoint, &error) == behavior_invalid && message_contains(&error, "missing"), "required parameter");
    check(behavior_endpoint_resolve(arena, rules, sources, none, none, parse(arena, "{\"Flag\":0}"), &endpoint, &error) == behavior_invalid && message_contains(&error, "type"), "parameter type");
    behavior_arena_free(arena);
}

static void standard_functions(void) {
    behavior_arena *arena = behavior_arena_new();
    check(text_is(behavior_json_get(call(arena, "parseUrl", "[\"https://example.test:443/a\"]"), VIEW("authority")), "example.test"), "default port elided");
    check(text_is(behavior_json_get(call(arena, "parseUrl", "[\"https://[::1]:8443\"]"), VIEW("authority")), "[::1]:8443"), "IPv6 authority");
    check(boolean_is(behavior_json_get(call(arena, "parseUrl", "[\"https://[::1]\"]"), VIEW("isIp")), true), "IPv6 is an IP");
    check(text_is(behavior_json_get(call(arena, "parseUrl", "[\"https://Example.test/a\"]"), VIEW("normalizedPath")), "/a/"), "normalized path");
    const char *invalid[] = {"[\"file:///path\"]", "[\"https://user:password@example.test\"]", "[\"https://example.test?q=x\"]", "[\"https://example.test#x\"]", "[\"https://example.test:99999\"]", "[\"https://evil\\n.example\"]"};
    for (size_t i = 0; i < sizeof(invalid) / sizeof(invalid[0]); ++i) check(call(arena, "parseUrl", invalid[i]) == NULL, invalid[i]);
    check(text_is(call(arena, "substring", "[\"abcdef\",1,3,true]"), "de"), "reverse substring");
    check(call(arena, "substring", "[\"\\u00e9\",0,1,false]") == NULL, "non-ASCII substring");
    check(call(arena, "substring", "[\"a\",0,2,false]") == NULL, "substring bounds");
    check(text_is(call(arena, "uriEncode", "[\"a/b +\\u00e9\"]"), "a%2Fb%20%2B%C3%A9"), "URI encoding");
    check(text_is(call(arena, "getAttr", "[{\"items\":[\"a\",\"b\"]},\"items[1]\"]"), "b"), "getAttr path");
    check(boolean_is(call(arena, "isSet", "[false]"), true), "false is set");
    check(boolean_is(call(arena, "isValidHostLabel", "[\"x.y\",false]"), false), "single label");
    check(boolean_is(call(arena, "isValidHostLabel", "[\"x.y\",true]"), true), "subdomain labels");
    check(boolean_is(call(arena, "stringEquals", "[\"a\",\"a\"]"), true) && boolean_is(call(arena, "booleanEquals", "[true,false]"), false), "equality");
    const behavior_json *ignored = NULL; behavior_error error = {0};
    check(behavior_endpoint_function(arena, VIEW("aws.partition"), parse(arena, "[\"x\"]"), &ignored, &error) == behavior_invalid, "unknown function");
    behavior_arena_free(arena);
}

int main(void) {
    generated_resolution();
    parameter_precedence();
    standard_functions();
    puts("C endpoint rules and host prefixes passed");
    return 0;
}
