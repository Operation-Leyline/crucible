#ifndef CRUCIBLE_GENERATED_TYPES_H
#define CRUCIBLE_GENERATED_TYPES_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    crucible_generated_ok = 0,
    crucible_generated_invalid,
    crucible_generated_out_of_memory,
    crucible_generated_cancelled,
    crucible_generated_deadline,
    crucible_generated_network,
    crucible_generated_service_error,
    /* A waiter reached a modeled failure state; see the waiter report. */
    crucible_generated_waiter_failed,
    /* A validated response checksum did not match the received body. */
    crucible_generated_checksum_mismatch
} crucible_generated_status;

typedef struct { const char *data; size_t length; } crucible_generated_string;
/* Exact JSON-number views. Construction is allocation-free; codecs validate the token. */
typedef crucible_generated_string crucible_generated_big_integer;
typedef crucible_generated_string crucible_generated_big_decimal;
typedef struct { const unsigned char *data; size_t length; } crucible_generated_blob;
typedef struct { int64_t seconds; uint32_t nanoseconds; } crucible_generated_timestamp;
typedef struct { unsigned char unused; } crucible_generated_unit;
typedef enum { crucible_generated_absent = 0, crucible_generated_null, crucible_generated_present } crucible_generated_presence;
typedef struct { crucible_generated_status status; char message[256]; } crucible_generated_error;
typedef struct crucible_generated_arena crucible_generated_arena;
typedef struct yyjson_val crucible_generated_json;
typedef enum {
    crucible_generated_json_null,
    crucible_generated_json_boolean,
    crucible_generated_json_number,
    crucible_generated_json_string,
    crucible_generated_json_array,
    crucible_generated_json_object
} crucible_generated_json_kind;

/* Strings and blobs are borrowed views; arenas own decoded values. */
crucible_generated_string crucible_generated_string_view(const char *text);
bool crucible_generated_string_equal(crucible_generated_string left, crucible_generated_string right);
crucible_generated_arena *crucible_generated_arena_new(void);
void crucible_generated_arena_free(crucible_generated_arena *arena);
void *crucible_generated_arena_alloc(crucible_generated_arena *arena, size_t size, crucible_generated_error *error);
crucible_generated_status crucible_generated_arena_copy(crucible_generated_arena *arena, crucible_generated_string input, crucible_generated_string *output, crucible_generated_error *error);
crucible_generated_status crucible_generated_json_parse(crucible_generated_arena *arena, crucible_generated_string text, const crucible_generated_json **output, crucible_generated_error *error);
crucible_generated_status crucible_generated_json_write(crucible_generated_arena *arena, const crucible_generated_json *value, crucible_generated_string *output, crucible_generated_error *error);
crucible_generated_json_kind crucible_generated_json_type(const crucible_generated_json *value);
const crucible_generated_json *crucible_generated_json_get(const crucible_generated_json *value, crucible_generated_string key);
const crucible_generated_json *crucible_generated_json_at(const crucible_generated_json *value, size_t index);
size_t crucible_generated_json_size(const crucible_generated_json *value);
crucible_generated_status crucible_generated_json_text(const crucible_generated_json *value, crucible_generated_string *output, crucible_generated_error *error);
crucible_generated_status crucible_generated_json_boolean_value(const crucible_generated_json *value, bool *output, crucible_generated_error *error);
crucible_generated_status crucible_generated_json_integer(const crucible_generated_json *value, int64_t minimum, int64_t maximum, int64_t *output, crucible_generated_error *error);
crucible_generated_status crucible_generated_json_double(const crucible_generated_json *value, double *output, crucible_generated_error *error);
crucible_generated_status crucible_generated_timestamp_epoch(const crucible_generated_json *value, crucible_generated_timestamp *output, crucible_generated_error *error);
crucible_generated_status crucible_generated_timestamp_parse(crucible_generated_string text, crucible_generated_timestamp *output, crucible_generated_error *error);
crucible_generated_status crucible_generated_timestamp_http_parse(crucible_generated_string text, crucible_generated_timestamp *output, crucible_generated_error *error);
/* Buffers include the terminator: epoch 40 bytes, date-time 40, HTTP date 30. */
crucible_generated_status crucible_generated_timestamp_epoch_text(crucible_generated_timestamp value, char output[40], crucible_generated_error *error);
crucible_generated_status crucible_generated_timestamp_text(crucible_generated_timestamp value, char output[40], crucible_generated_error *error);
crucible_generated_status crucible_generated_timestamp_http_text(crucible_generated_timestamp value, char output[30], crucible_generated_error *error);

#ifdef __cplusplus
}
#endif
#endif
