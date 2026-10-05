#ifndef CRUCIBLE_GENERATED_INTERNAL_H
#define CRUCIBLE_GENERATED_INTERNAL_H
#include "types.h"
#include <yyjson.h>
#if YYJSON_VERSION_HEX < 0x000D00
#error "yyjson 0.13.0 or newer is required"
#endif

crucible_generated_status crucible_generated_fail(crucible_generated_error *error, crucible_generated_status status, const char *message);
crucible_generated_status crucible_generated_arena_own(crucible_generated_arena *arena, void *resource, void (*destroy)(void *), crucible_generated_error *error);
bool crucible_generated_utf8(crucible_generated_string value);
crucible_generated_status crucible_generated_json_depth(const yyjson_val *value, unsigned depth, crucible_generated_error *error);
crucible_generated_status crucible_generated_number_scaled(const yyjson_val *value, unsigned places, char output[64], crucible_generated_error *error);
crucible_generated_status crucible_generated_integer_text(const char *text, int64_t minimum, int64_t maximum, int64_t *output, crucible_generated_error *error);
#endif
