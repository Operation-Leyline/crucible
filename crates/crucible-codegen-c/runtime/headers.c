#include "http.h"
#include "internal.h"
#include <string.h>

static unsigned char lower(unsigned char value) { return value >= 'A' && value <= 'Z' ? (unsigned char)(value + 32) : value; }
static bool equal(crucible_generated_string a, crucible_generated_string b) {
    if (a.length != b.length) return false;
    for (size_t i = 0; i < a.length; ++i) if (lower((unsigned char)a.data[i]) != lower((unsigned char)b.data[i])) return false;
    return true;
}
bool crucible_generated_header_get(crucible_generated_headers headers, crucible_generated_string name, crucible_generated_string *value) {
    for (size_t i = headers.count; i > 0; --i) if (equal(headers.items[i - 1].name, name)) {
        if (value) *value = headers.items[i - 1].value;
        return true;
    }
    return false;
}
crucible_generated_status crucible_generated_header_set(crucible_generated_arena *arena, crucible_generated_headers *headers,
    crucible_generated_string name, crucible_generated_string value, crucible_generated_error *error) {
    if (!headers || headers->count >= SIZE_MAX / sizeof(crucible_generated_header)) return crucible_generated_fail(error, crucible_generated_invalid, "Header count overflow");
    crucible_generated_header item;
    crucible_generated_status status = crucible_generated_arena_copy(arena, name, &item.name, error);
    if (status) return status;
    status = crucible_generated_arena_copy(arena, value, &item.value, error);
    if (status) return status;
    crucible_generated_header *items = crucible_generated_arena_alloc(arena, (headers->count + 1) * sizeof(*items), error);
    if (!items) return crucible_generated_out_of_memory;
    size_t count = 0;
    for (size_t i = 0; i < headers->count; ++i) if (!equal(headers->items[i].name, name)) items[count++] = headers->items[i];
    items[count++] = item;
    *headers = (crucible_generated_headers){items, count};
    return crucible_generated_ok;
}
