#include "weather_client/models.h"
#include <yyjson.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const char *case_id;
static void check(bool value, const char *message) {
    if (!value) { fprintf(stderr, "%s: %s\n", case_id, message); exit(1); }
}
static void good(weather_status status, const weather_error *error) { check(status == weather_ok, error->message); }
static const weather_json *get(const weather_json *value, const char *key) { return weather_json_get(value, weather_string_view(key)); }
static weather_string text(const weather_json *value) {
    weather_string result; weather_error error = {0};
    good(weather_json_text(value, &result, &error), &error); return result;
}
/* Compare decimal coefficients and powers exactly, without a floating-point hop. */
static weather_string number(weather_string input, char digits[1024], long *power) {
    size_t i = 0, length = 0, start = 0; bool fractional = false, negative = false;
    *power = 0; check(input.length < 1000, "Fixture number is too long");
    if (input.data[i] == '-') { negative = true; ++i; }
    for (; i < input.length && input.data[i] != 'e' && input.data[i] != 'E'; ++i) {
        if (input.data[i] == '.') fractional = true;
        else { digits[length++] = input.data[i]; if (fractional) --*power; }
    }
    if (i < input.length) { char exponent[1024]; size_t count = input.length - i - 1;
        memcpy(exponent, input.data + i + 1, count); exponent[count] = 0; *power += strtol(exponent, NULL, 10); }
    while (start < length && digits[start] == '0') ++start;
    if (start == length) { digits[0] = '0'; *power = 0; return (weather_string){digits, 1}; }
    while (length > start && digits[length - 1] == '0') { --length; ++*power; }
    length -= start; memmove(digits + (negative ? 1 : 0), digits + start, length);
    if (negative) { digits[0] = '-'; ++length; }
    return (weather_string){digits, length};
}
static bool equal(const weather_json *a, const weather_json *b) {
    size_t i, count; weather_json_kind kind;
    if (!a || !b || weather_json_type(a) != weather_json_type(b)) return false;
    kind = weather_json_type(a);
    if (kind == weather_json_null) return true;
    if (kind == weather_json_string) return weather_string_equal(text(a), text(b));
    if (kind == weather_json_number) { char x[1024], y[1024]; long xp, yp;
        weather_string xs = number(text(a), x, &xp), ys = number(text(b), y, &yp);
        return xp == yp && weather_string_equal(xs, ys); }
    if (kind == weather_json_boolean) return yyjson_get_bool(a) == yyjson_get_bool(b);
    count = weather_json_size(a); if (count != weather_json_size(b)) return false;
    if (kind == weather_json_array) {
        for (i = 0; i < count; ++i) if (!equal(weather_json_at(a, i), weather_json_at(b, i))) return false;
    } else {
        yyjson_val *key, *value; size_t maximum;
        yyjson_obj_foreach(a, i, maximum, key, value) { if (!equal(value, weather_json_get(b, text(key)))) return false; }
    }
    return true;
}
static const weather_member *member(const weather_shape *shape, weather_string name) {
    size_t i; for (i = 0; i < shape->member_count; ++i) if (weather_string_equal(shape->members[i].name, name)) return &shape->members[i];
    check(false, "Unknown observed member"); return NULL;
}
int main(void) {
    const char *path = getenv("SERIALIZATION_CASES"); FILE *file; long length; char *bytes;
    weather_arena *fixtures = weather_arena_new(); weather_error error = {0}; const weather_json *rows; size_t i;
    case_id = "serialization corpus"; check(path && fixtures, "Missing fixture configuration");
    file = fopen(path, "rb"); check(file != NULL, "Cannot open corpus"); check(fseek(file, 0, SEEK_END) == 0, "Seek failed");
    length = ftell(file); check(length >= 0, "Length failed"); rewind(file); bytes = malloc((size_t)length + 1); check(bytes != NULL, "Allocation failed");
    check(fread(bytes, 1, (size_t)length, file) == (size_t)length, "Read failed"); fclose(file);
    good(weather_json_parse(fixtures, (weather_string){bytes, (size_t)length}, &rows, &error), &error); free(bytes);
    check(weather_json_size(rows) >= 15, "Incomplete corpus");
    for (i = 0; i < weather_json_size(rows); ++i) {
        const weather_json *row = weather_json_at(rows, i), *wire, *encoded, *expected, *expect = get(row, "expect");
        const weather_shape *shape = weather_shape_lookup(text(get(row, "target"))); const void *native = NULL;
        weather_arena *arena = weather_arena_new(); weather_status status; weather_string id = text(get(row, "id"));
        char label[256]; check(id.length < sizeof(label), "Case id too long"); memcpy(label, id.data, id.length); label[id.length] = 0; case_id = label;
        check(shape && arena, "Unknown shape or allocation failure");
        good(weather_json_parse(arena, text(get(row, "wire")), &wire, &error), &error);
        status = weather_model_decode(arena, shape, wire, &native, &error);
        if (get(row, "reject")) check(status == weather_invalid, "Invalid wire value was accepted");
        else {
            const weather_json *states = get(expect, "states"), *instant = get(expect, "instant"), *hex = get(expect, "blobHex");
            weather_string encoded_text;
            good(status, &error); good(weather_model_encode(arena, shape, native, &encoded, &error), &error);
            good(weather_json_write(arena, encoded, &encoded_text, &error), &error);
            good(weather_json_parse(arena, encoded_text, &encoded, &error), &error);
            good(weather_json_parse(arena, text(get(expect, "json")), &expected, &error), &error);
            check(equal(encoded, expected), "Exact structural JSON mismatch");
            if (states) { yyjson_val *key, *value; size_t j, maximum;
                yyjson_obj_foreach(states, j, maximum, key, value) {
                    const weather_member *field = member(shape, text(key)); weather_presence state;
                    memcpy(&state, (const char *)native + field->state_offset, sizeof(state));
                    check(weather_string_equal(text(value), weather_string_view(state == weather_absent ? "absent" : state == weather_null ? "null" : "value")), "Native presence mismatch");
                }
            }
            if (instant) {
                const weather_member *field = member(shape, text(get(instant, "member"))); weather_timestamp value; weather_string seconds;
                char buffer[40]; const weather_json *seconds_json; int64_t wanted_seconds, wanted_nanos;
                memcpy(&value, (const char *)native + field->value_offset, sizeof(value));
                seconds = text(get(instant, "seconds")); check(seconds.length < sizeof(buffer), "Seconds too long"); memcpy(buffer, seconds.data, seconds.length); buffer[seconds.length] = 0;
                good(weather_json_parse(arena, weather_string_view(buffer), &seconds_json, &error), &error);
                good(weather_json_integer(seconds_json, INT64_MIN, INT64_MAX, &wanted_seconds, &error), &error);
                good(weather_json_integer(get(instant, "nanoseconds"), 0, 999999999, &wanted_nanos, &error), &error);
                check(value.seconds == wanted_seconds && value.nanoseconds == (uint32_t)wanted_nanos, "Native timestamp mismatch");
            }
            if (hex) {
                const weather_blob *blob = native; weather_string wanted = text(hex); size_t j; const char *digits = "0123456789abcdef";
                check(wanted.length == blob->length * 2, "Native blob length mismatch");
                for (j = 0; j < blob->length; ++j) check(wanted.data[2*j] == digits[blob->data[j] >> 4] && wanted.data[2*j+1] == digits[blob->data[j] & 15], "Native blob mismatch");
            }
        }
        weather_arena_free(arena); case_id = "serialization corpus";
    }
    weather_arena_free(fixtures); puts("C shared serialization corpus passed"); return 0;
}
