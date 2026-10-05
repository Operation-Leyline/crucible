#include "dynamic_client/client.h"
#include "types_client/models.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

static dynamic_status send_fake(void *user, const dynamic_request *request, bool read_body,
    const dynamic_scope *scope, dynamic_arena *arena, dynamic_response *response, dynamic_error *error) {
    (void)read_body; (void)scope;
    bool failure = *(bool *)user;
    assert(dynamic_string_equal(request->url, dynamic_string_view("https://example.invalid/files/folder/a%20b/?q=a%20b&q=c")));
    dynamic_string value;
    assert(dynamic_header_get(request->headers, dynamic_string_view("x-meta-trace"), &value));
    assert(dynamic_string_equal(value, dynamic_string_view("abc")));
    assert(request->has_body && request->body.length == strlen("{\"wire-value\":\"request\"}"));
    assert(memcmp(request->body.data, "{\"wire-value\":\"request\"}", request->body.length) == 0);
    response->status = failure ? 409 : 207;
    dynamic_string body = dynamic_string_view(failure ? "{}" : "{\"wire-value\":\"response\"}");
    response->body = (dynamic_blob){(const unsigned char *)body.data, body.length};
    dynamic_status code = dynamic_header_set(arena, &response->headers, dynamic_string_view("X-Meta-Answer"), dynamic_string_view("yes"), error);
    if (!code && failure) code = dynamic_header_set(arena, &response->headers, dynamic_string_view("X-Crucible-Error"), dynamic_string_view("Problem"), error);
    return code;
}
static const types_json *parse(types_arena *arena, const char *text, types_error *error) {
    const types_json *result = NULL;
    assert(types_json_parse(arena, types_string_view(text), &result, error) == types_ok);
    return result;
}
int main(void) {
    bool failure = false;
    dynamic_client_config config = dynamic_client_defaults();
    config.endpoint = dynamic_string_view("https://example.invalid"); config.send = send_fake; config.transport_user = &failure;
    dynamic_client *client = NULL; dynamic_error error = {0};
    assert(dynamic_client_new(&config, &client, &error) == dynamic_ok);
    dynamic_string values[] = {dynamic_string_view("a b"), dynamic_string_view("c")};
    dynamic_models_string_list list = {2, values};
    dynamic_models_string_lists_entry queries[] = {{dynamic_string_view("q"), &list}};
    dynamic_models_string_lists query = {1, queries};
    dynamic_models_strings_entry entries[] = {{dynamic_string_view("trace"), dynamic_string_view("abc")}};
    dynamic_models_strings metadata = {1, entries};
    dynamic_models_input input = {0};
    dynamic_models_input_set_key(&input, dynamic_string_view("folder/a b/"));
    dynamic_models_input_set_query(&input, &query); dynamic_models_input_set_metadata(&input, &metadata);
    dynamic_models_input_set_value(&input, dynamic_string_view("request"));
    dynamic_operations_get_result result = {0};
    assert(dynamic_operations_get(client, &input, NULL, NULL, &result, &error) == dynamic_ok);
    assert(result.value->status.value == 207 && dynamic_string_equal(result.value->value.value, dynamic_string_view("response")));
    assert(result.value->metadata.value->length == 1 && dynamic_string_equal(result.value->metadata.value->items[0].key, dynamic_string_view("answer")));
    dynamic_operations_get_result_free(&result);
    failure = true;
    assert(dynamic_operations_get(client, &input, NULL, NULL, &result, &error) == dynamic_service_error);
    const dynamic_models_problem *problem = dynamic_error_as_problem(&result.base);
    assert(problem && problem->status.value == 409 && problem->details.value->length == 1);
    dynamic_operations_get_result_free(&result);
    dynamic_models_input_set_key(&input, dynamic_string_view("a/../b"));
    assert(dynamic_operations_get(client, &input, NULL, NULL, &result, &error) == dynamic_invalid);
    dynamic_operations_get_result_free(&result); dynamic_client_free(client);

    types_arena *arena = types_arena_new(); types_error issue = {0}; const types_models_output *output = NULL;
    const types_json *wire = parse(arena, "{\"mode\":42,\"sparseNames\":[null,\"x\"],\"sparseMap\":{\"nil\":null,\"mode\":42}}", &issue);
    assert(types_models_output_decode(arena, wire, &output, &issue) == types_ok);
    assert(types_string_equal(output->integer.value, types_string_view("12345678901234567890123456789012345678901234567890")));
    assert(types_string_equal(output->decimal.value, types_string_view("12345678901234567890.12345678901234567890123456789")));
    assert(output->mode.value == 42 && output->sparse_names.value->items[0].state == types_null);
    assert(output->sparse_map.value->items[0].state == types_null && output->sparse_map.value->items[1].value == 42);
    assert(types_models_output_encode(arena, output, &wire, &issue) == types_ok);
    const types_models_names *names = NULL; const types_models_integers *integers = NULL; const types_models_decimals *decimals = NULL;
    assert(types_models_names_decode(arena, parse(arena, "[null]", &issue), &names, &issue) == types_invalid);
    assert(types_models_names_decode(arena, parse(arena, "[\"a\",\"a\"]", &issue), &names, &issue) == types_invalid);
    assert(types_models_decimals_decode(arena, parse(arena, "[1,1.0]", &issue), &decimals, &issue) == types_invalid);
    assert(types_models_integers_decode(arena, parse(arena, "[1,1e0]", &issue), &integers, &issue) == types_invalid);
    assert(types_models_integers_decode(arena, parse(arena, "[18446744073709551617,36893488147419103233]", &issue), &integers, &issue) == types_ok);
    assert(types_models_decimals_decode(arena, parse(arena, "[1.000000000000000000001,1.000000000000000000002]", &issue), &decimals, &issue) == types_ok);
    assert(types_models_decimals_decode(arena, parse(arena, "[10e999999999999999999999,1e1000000000000000000000]", &issue), &decimals, &issue) == types_invalid);
    types_big_decimal repeated[] = {types_string_view("1"), types_string_view("1.0")}; types_models_decimals duplicate = {2, repeated};
    assert(types_models_decimals_encode(arena, &duplicate, &wire, &issue) == types_invalid);
    types_arena_free(arena);
    puts("C dynamic bindings and exact extended types passed");
    return 0;
}
