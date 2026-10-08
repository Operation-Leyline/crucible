#ifndef CRUCIBLE_GENERATED_SCHEMA_H
#define CRUCIBLE_GENERATED_SCHEMA_H
#include "types.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    crucible_generated_kind_unit, crucible_generated_kind_string,
    crucible_generated_kind_boolean, crucible_generated_kind_byte,
    crucible_generated_kind_short, crucible_generated_kind_integer,
    crucible_generated_kind_long, crucible_generated_kind_float,
    crucible_generated_kind_double, crucible_generated_kind_blob,
    crucible_generated_kind_timestamp, crucible_generated_kind_document,
    crucible_generated_kind_enum, crucible_generated_kind_structure,
    crucible_generated_kind_union, crucible_generated_kind_list,
    crucible_generated_kind_map, crucible_generated_kind_big_integer,
    crucible_generated_kind_big_decimal, crucible_generated_kind_int_enum
} crucible_generated_shape_kind;
typedef enum {
    crucible_generated_date_time, crucible_generated_epoch_seconds,
    crucible_generated_http_date
} crucible_generated_timestamp_format;
typedef struct crucible_generated_shape crucible_generated_shape;
typedef struct {
    crucible_generated_string name;
    const crucible_generated_shape *target;
    size_t state_offset;
    size_t value_offset;
    bool optional;
    crucible_generated_timestamp_format format;
    /** A NULL data pointer means no default; otherwise this is exact JSON. */
    crucible_generated_string default_json;
    bool sensitive;
} crucible_generated_member;
struct crucible_generated_shape {
    crucible_generated_string id;
    /** Public model name after service renames, used in diagnostics. */
    crucible_generated_string name;
    crucible_generated_shape_kind kind;
    size_t size;
    crucible_generated_timestamp_format format;
    const crucible_generated_member *members;
    size_t member_count;
    /** Collection layout: items are scalar slots or pointers to complex values. */
    size_t length_offset, items_offset, item_size;
    /** Union tag: 0 = unknown; declared members use their index plus one. */
    size_t tag_offset, state_offset, unknown_name_offset, unknown_value_offset;
    bool sparse, unique;
    bool sensitive;
};

/** Root native points at scalar storage or directly at a complex struct.
 * Nested structures/unions/lists/maps are const pointers. Decoded roots and
 * all reachable data belong to arena, independently of the source JSON arena.
 * Outputs are assigned only on success. Descriptor pointers have static life. */
crucible_generated_status crucible_generated_model_encode(crucible_generated_arena *arena, const crucible_generated_shape *shape, const void *native, const crucible_generated_json **output, crucible_generated_error *error);
crucible_generated_status crucible_generated_model_decode(crucible_generated_arena *arena, const crucible_generated_shape *shape, const crucible_generated_json *value, const void **output, crucible_generated_error *error);
/** Formats TypeName(member=value, ...) without exposing sensitive values. The returned string belongs to arena. */
crucible_generated_status crucible_generated_model_debug(crucible_generated_arena *arena, const crucible_generated_shape *shape, const void *native, crucible_generated_string *output, crucible_generated_error *error);
const crucible_generated_shape *crucible_generated_shape_lookup(crucible_generated_string id);

#ifdef __cplusplus
}
#endif
#endif
