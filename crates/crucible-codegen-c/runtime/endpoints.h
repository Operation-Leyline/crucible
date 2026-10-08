#ifndef CRUCIBLE_GENERATED_ENDPOINTS_H
#define CRUCIBLE_GENERATED_ENDPOINTS_H
#include "http.h"

#ifdef __cplusplus
extern "C" {
#endif

/** Endpoint rule parameters and built-ins supplied by the caller. Strings are copied by
 * client_new. A string array uses items/count. */
typedef enum {
    crucible_generated_endpoint_string = 0,
    crucible_generated_endpoint_boolean,
    crucible_generated_endpoint_string_array
} crucible_generated_endpoint_kind;
typedef struct {
    crucible_generated_string name;
    crucible_generated_endpoint_kind kind;
    crucible_generated_string text;
    bool boolean;
    const crucible_generated_string *items;
    size_t count;
} crucible_generated_endpoint_value;
typedef struct {
    crucible_generated_string url;
    crucible_generated_headers headers;
    /** A JSON object; NULL when the rule declares no properties. */
    const crucible_generated_json *properties;
} crucible_generated_endpoint;

/** Evaluate a lowered endpoint rule set (the schema's endpointRules) with per-parameter
 * sources, the query-model input object, client parameters and built-ins (JSON objects,
 * each may be NULL). Results live in arena. A rule error is crucible_generated_invalid
 * with the rule's message. */
crucible_generated_status crucible_generated_endpoint_resolve(crucible_generated_arena *arena,
    const crucible_generated_json *rules, const crucible_generated_json *sources, const crucible_generated_json *input,
    const crucible_generated_json *client, const crucible_generated_json *builtins,
    crucible_generated_endpoint *endpoint, crucible_generated_error *error);
/** One standard rules function (isSet, not, booleanEquals, stringEquals, getAttr, substring,
 * isValidHostLabel, parseUrl, uriEncode) over a JSON argument array. A NULL output is an
 * absent value. */
crucible_generated_status crucible_generated_endpoint_function(crucible_generated_arena *arena,
    crucible_generated_string name, const crucible_generated_json *arguments,
    const crucible_generated_json **output, crucible_generated_error *error);

#ifdef __cplusplus
}
#endif
#endif
