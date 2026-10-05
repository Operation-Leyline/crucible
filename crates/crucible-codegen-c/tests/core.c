#include "types.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void check(bool condition, const char *message) { if (!condition) { fprintf(stderr, "%s\n", message); exit(1); } }
static const crucible_generated_json *parse(crucible_generated_arena *arena, const char *text) {
    const crucible_generated_json *value = NULL;
    crucible_generated_error error = {0};
    check(crucible_generated_json_parse(arena, crucible_generated_string_view(text), &value, &error) == crucible_generated_ok, error.message);
    return value;
}
int main(void) {
    crucible_generated_arena *arena = crucible_generated_arena_new();
    crucible_generated_error error = {0};
    const crucible_generated_json *value, *invalid;
    crucible_generated_string text;
    crucible_generated_timestamp time, copy;
    int64_t integer;
    char encoded[40];
    const char *bad_json[] = {"[1,]", "01", "NaN", "\"\\udc00\"", "\"\\ud800\"", "{\"x\":}"};
    const int64_t seconds[] = {INT64_MIN, -1, 0, INT64_MAX};
    size_t i;
    check(arena != NULL, "Arena allocation");
    value = parse(arena, "{\"integer\":9223372036854775807,\"decimal\":1.000000000000000000001,\"list\":[],\"object\":{}}");
    check(crucible_generated_json_integer(crucible_generated_json_get(value, crucible_generated_string_view("integer")), INT64_MIN, INT64_MAX, &integer, &error) == crucible_generated_ok && integer == INT64_MAX, "Exact int64");
    check(crucible_generated_json_write(arena, value, &text, &error) == crucible_generated_ok && strstr(text.data, "1.000000000000000000001") != NULL, "Exact decimal document");
    for (i = 0; i < sizeof(bad_json) / sizeof(*bad_json); ++i) check(crucible_generated_json_parse(arena, crucible_generated_string_view(bad_json[i]), &invalid, &error) == crucible_generated_invalid, "Malformed JSON accepted");
    check(crucible_generated_json_integer(parse(arena, "-9223372036854775808.0"), INT64_MIN, INT64_MAX, &integer, &error) == crucible_generated_ok && integer == INT64_MIN, "Exact negative bound");
    check(crucible_generated_json_integer(parse(arena, "9223372036854775808"), INT64_MIN, INT64_MAX, &integer, &error) == crucible_generated_invalid, "Integer overflow accepted");
    check(crucible_generated_json_integer(parse(arena, "1.01"), INT64_MIN, INT64_MAX, &integer, &error) == crucible_generated_invalid, "Fractional integer accepted");
    check(crucible_generated_json_integer(parse(arena, "1.000e2"), INT64_MIN, INT64_MAX, &integer, &error) == crucible_generated_ok && integer == 100, "Exact exponent integer");
    for (i = 0; i < sizeof(seconds) / sizeof(*seconds); ++i) {
        time.seconds = seconds[i]; time.nanoseconds = 999999999;
        check(crucible_generated_timestamp_epoch_text(time, encoded, &error) == crucible_generated_ok, "Epoch formatting");
        check(crucible_generated_timestamp_epoch(parse(arena, encoded), &copy, &error) == crucible_generated_ok && time.seconds == copy.seconds && time.nanoseconds == copy.nanoseconds, "Epoch round trip");
    }
    check(crucible_generated_timestamp_epoch(parse(arena, "-0.000000001"), &time, &error) == crucible_generated_ok, "Negative epoch");
    check(crucible_generated_timestamp_text(time, encoded, &error) == crucible_generated_ok && strcmp(encoded, "1969-12-31T23:59:59.999999999Z") == 0, "Negative fraction");
    check(crucible_generated_timestamp_parse(crucible_generated_string_view("1582-10-10T00:00:00Z"), &time, &error) == crucible_generated_ok, "Gregorian historical date");
    check(crucible_generated_timestamp_text(time, encoded, &error) == crucible_generated_ok && strcmp(encoded, "1582-10-10T00:00:00Z") == 0, "Gregorian round trip");
    check(crucible_generated_timestamp_parse(crucible_generated_string_view("1500-02-29T00:00:00Z"), &time, &error) == crucible_generated_invalid, "Invalid leap day");
    check(crucible_generated_timestamp_parse(crucible_generated_string_view("2026-10-01T02:30:00+02:30"), &time, &error) == crucible_generated_ok, "Timezone parse");
    check(crucible_generated_timestamp_text(time, encoded, &error) == crucible_generated_ok && strcmp(encoded, "2026-10-01T00:00:00Z") == 0, "Timezone conversion");
    check(crucible_generated_timestamp_http_parse(crucible_generated_string_view("Thu, 01 Jan 1970 00:00:00 GMT"), &time, &error) == crucible_generated_ok && time.seconds == 0, "HTTP date");
    check(crucible_generated_timestamp_http_parse(crucible_generated_string_view("Fri, 01 Jan 1970 00:00:00 GMT"), &time, &error) == crucible_generated_invalid, "HTTP weekday mismatch");
    crucible_generated_arena_free(arena);
    puts("C exact JSON, integer, and timestamp checks passed");
    return 0;
}
