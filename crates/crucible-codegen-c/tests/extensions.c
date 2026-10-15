#include "types_client/models.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

static const types_json *parse(types_arena *arena, const char *text, types_error *error) {
    const types_json *result = NULL;
    assert(types_json_parse(arena, types_string_view(text), &result, error) == types_ok);
    return result;
}
int main(void) {
    types_arena *arena = types_arena_new(); types_error issue = {0}; const types_models_read_output *output = NULL;
    const types_json *wire = parse(arena, "{\"mode\":42,\"sparseNames\":[null,\"x\"],\"sparseMap\":{\"nil\":null,\"mode\":42}}", &issue);
    assert(types_models_read_output_decode(arena, wire, &output, &issue) == types_ok);
    assert(types_string_equal(output->integer.value, types_string_view("12345678901234567890123456789012345678901234567890")));
    assert(types_string_equal(output->decimal.value, types_string_view("12345678901234567890.12345678901234567890123456789")));
    assert(output->mode.value == 42 && output->sparse_names.value->items[0].state == types_null);
    assert(output->sparse_map.value->items[0].state == types_null && output->sparse_map.value->items[1].value == 42);
    assert(types_models_read_output_encode(arena, output, &wire, &issue) == types_ok);
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
    puts("C exact extended types passed");
    return 0;
}
