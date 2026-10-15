#ifndef CRUCIBLE_GENERATED_TYPES_H
#define CRUCIBLE_GENERATED_TYPES_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    /** Successful completion. */
    crucible_generated_ok = 0,
    /** Invalid input or malformed wire data. */
    crucible_generated_invalid,
    /** An allocation failed. */
    crucible_generated_out_of_memory,
    /** The caller cancelled the call. */
    crucible_generated_cancelled,
    /** The total call budget expired. */
    crucible_generated_deadline,
    /** An HTTP transport or body read failed. */
    crucible_generated_network,
    /** An HTTP service failure; modeled data is retained in the result. */
    crucible_generated_service_error
} crucible_generated_status;

/** Borrowed UTF-8 string view; data need not be NUL-terminated. */
typedef struct { const char *data; size_t length; } crucible_generated_string;
/** Exact JSON-number views. Construction is allocation-free; codecs validate the token. */
typedef crucible_generated_string crucible_generated_big_integer;
typedef crucible_generated_string crucible_generated_big_decimal;
/** Borrowed byte view. */
typedef struct { const unsigned char *data; size_t length; } crucible_generated_blob;
/** Exact Unix seconds and nanoseconds; nanoseconds are in [0, 1000000000). */
typedef struct { int64_t seconds; uint32_t nanoseconds; } crucible_generated_timestamp;
/** Value for an operation without modeled input or output. */
typedef struct { unsigned char unused; } crucible_generated_unit;
typedef enum { crucible_generated_absent = 0, crucible_generated_null, crucible_generated_present } crucible_generated_presence;
/** Runtime failure status and bounded diagnostic message. */
typedef struct { crucible_generated_status status; char message[256]; } crucible_generated_error;
/** Arena owning decoded models, JSON values, and copied strings. */
typedef struct crucible_generated_arena crucible_generated_arena;
/** Borrowed immutable JSON value; owned by an arena. */
typedef struct yyjson_val crucible_generated_json;
typedef enum {
    crucible_generated_json_null,
    crucible_generated_json_boolean,
    crucible_generated_json_number,
    crucible_generated_json_string,
    crucible_generated_json_array,
    crucible_generated_json_object
} crucible_generated_json_kind;

/** Strings and blobs are borrowed views; arenas own decoded values. */
/** Creates a borrowed string view of a NUL-terminated string; NULL is empty. */
crucible_generated_string crucible_generated_string_view(const char *text);
/** Compares string views by length and bytes. */
bool crucible_generated_string_equal(crucible_generated_string left, crucible_generated_string right);
/** Creates an allocation arena; NULL means allocation failure. */
crucible_generated_arena *crucible_generated_arena_new(void);
/** Frees all arena-owned data; accepts NULL. */
void crucible_generated_arena_free(crucible_generated_arena *arena);
/** Allocates zeroed, aligned arena storage; returns NULL on failure. */
void *crucible_generated_arena_alloc(crucible_generated_arena *arena, size_t size, crucible_generated_error *error);
/** Copies a string view into arena-owned storage. */
crucible_generated_status crucible_generated_arena_copy(crucible_generated_arena *arena, crucible_generated_string input, crucible_generated_string *output, crucible_generated_error *error);
/** Parses UTF-8 JSON into arena-owned data, preserving exact numeric tokens. */
crucible_generated_status crucible_generated_json_parse(crucible_generated_arena *arena, crucible_generated_string text, const crucible_generated_json **output, crucible_generated_error *error);
/** Writes JSON into an arena-owned string. */
crucible_generated_status crucible_generated_json_write(crucible_generated_arena *arena, const crucible_generated_json *value, crucible_generated_string *output, crucible_generated_error *error);
/** Returns the JSON value kind. */
crucible_generated_json_kind crucible_generated_json_type(const crucible_generated_json *value);
/** Borrows an object member, or returns NULL when absent. */
const crucible_generated_json *crucible_generated_json_get(const crucible_generated_json *value, crucible_generated_string key);
/** Borrows an array element, or returns NULL when out of bounds. */
const crucible_generated_json *crucible_generated_json_at(const crucible_generated_json *value, size_t index);
/** Returns an object or array size. */
size_t crucible_generated_json_size(const crucible_generated_json *value);
/** Borrows a JSON string view; fails for other kinds. */
crucible_generated_status crucible_generated_json_text(const crucible_generated_json *value, crucible_generated_string *output, crucible_generated_error *error);
/** Reads a JSON boolean; fails for other kinds. */
crucible_generated_status crucible_generated_json_boolean_value(const crucible_generated_json *value, bool *output, crucible_generated_error *error);
/** Reads an exact signed integer within the supplied bounds. */
crucible_generated_status crucible_generated_json_integer(const crucible_generated_json *value, int64_t minimum, int64_t maximum, int64_t *output, crucible_generated_error *error);
/** Reads a JSON floating-point number. */
crucible_generated_status crucible_generated_json_double(const crucible_generated_json *value, double *output, crucible_generated_error *error);
/** Reads exact epoch seconds and nanoseconds from a JSON number. */
crucible_generated_status crucible_generated_timestamp_epoch(const crucible_generated_json *value, crucible_generated_timestamp *output, crucible_generated_error *error);
/** Parses an RFC 3339 timestamp. */
crucible_generated_status crucible_generated_timestamp_parse(crucible_generated_string text, crucible_generated_timestamp *output, crucible_generated_error *error);
/** Parses an HTTP date. */
crucible_generated_status crucible_generated_timestamp_http_parse(crucible_generated_string text, crucible_generated_timestamp *output, crucible_generated_error *error);
/** Buffers include the terminator: epoch 40 bytes, date-time 40, HTTP date 30. */
/** Writes exact epoch seconds into a caller-owned 40-byte buffer. */
crucible_generated_status crucible_generated_timestamp_epoch_text(crucible_generated_timestamp value, char output[40], crucible_generated_error *error);
/** Writes RFC 3339 text into a caller-owned 40-byte buffer. */
crucible_generated_status crucible_generated_timestamp_text(crucible_generated_timestamp value, char output[40], crucible_generated_error *error);
/** Writes an HTTP date into a caller-owned 30-byte buffer. */
crucible_generated_status crucible_generated_timestamp_http_text(crucible_generated_timestamp value, char output[30], crucible_generated_error *error);

#ifdef __cplusplus
}
#endif
#endif
