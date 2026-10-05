#include "internal.h"
#include <stdlib.h>
#include <string.h>

#ifdef _MSC_VER
/* The MSVC C headers omit max_align_t; these cover every emitted native type. */
typedef union { long double floating; int64_t integer; void *pointer; } arena_alignment;
#else
typedef max_align_t arena_alignment;
#endif
typedef struct allocation {
    struct allocation *next;
    arena_alignment alignment;
} allocation;
typedef struct cleanup { struct cleanup *next; void *resource; void (*destroy)(void *); } cleanup;
struct crucible_generated_arena { allocation *allocations; cleanup *cleanups; };

crucible_generated_status crucible_generated_fail(crucible_generated_error *error, crucible_generated_status status, const char *message) {
    if (error) {
        size_t length = strlen(message);
        if (length >= sizeof(error->message)) length = sizeof(error->message) - 1;
        memcpy(error->message, message, length);
        error->message[length] = '\0';
        error->status = status;
    }
    return status;
}
crucible_generated_string crucible_generated_string_view(const char *text) {
    crucible_generated_string result = {text, text ? strlen(text) : 0};
    return result;
}
bool crucible_generated_string_equal(crucible_generated_string left, crucible_generated_string right) {
    return left.length == right.length && (left.length == 0 || (left.data && right.data && memcmp(left.data, right.data, left.length) == 0));
}
crucible_generated_arena *crucible_generated_arena_new(void) { return calloc(1, sizeof(crucible_generated_arena)); }
void crucible_generated_arena_free(crucible_generated_arena *arena) {
    if (!arena) return;
    while (arena->cleanups) {
        cleanup *item = arena->cleanups;
        arena->cleanups = item->next;
        item->destroy(item->resource);
        free(item);
    }
    while (arena->allocations) {
        allocation *item = arena->allocations;
        arena->allocations = item->next;
        free(item);
    }
    free(arena);
}
void *crucible_generated_arena_alloc(crucible_generated_arena *arena, size_t size, crucible_generated_error *error) {
    allocation *item;
    if (!arena) { crucible_generated_fail(error, crucible_generated_invalid, "Arena is required"); return NULL; }
    if (size == 0) size = 1;
    if (size > SIZE_MAX - sizeof(allocation)) { crucible_generated_fail(error, crucible_generated_out_of_memory, "Allocation size overflow"); return NULL; }
    item = calloc(1, sizeof(allocation) + size);
    if (!item) { crucible_generated_fail(error, crucible_generated_out_of_memory, "Allocation failed"); return NULL; }
    item->next = arena->allocations;
    arena->allocations = item;
    return item + 1;
}
crucible_generated_status crucible_generated_arena_own(crucible_generated_arena *arena, void *resource, void (*destroy)(void *), crucible_generated_error *error) {
    cleanup *item;
    if (!arena || !destroy) return crucible_generated_fail(error, crucible_generated_invalid, "Arena and resource destructor are required");
    item = malloc(sizeof(cleanup));
    if (!item) return crucible_generated_fail(error, crucible_generated_out_of_memory, "Allocation failed");
    item->resource = resource;
    item->destroy = destroy;
    item->next = arena->cleanups;
    arena->cleanups = item;
    return crucible_generated_ok;
}
crucible_generated_status crucible_generated_arena_copy(crucible_generated_arena *arena, crucible_generated_string input, crucible_generated_string *output, crucible_generated_error *error) {
    char *data;
    if (!output || (!input.data && input.length)) return crucible_generated_fail(error, crucible_generated_invalid, "Invalid string view");
    if (input.length == SIZE_MAX) return crucible_generated_fail(error, crucible_generated_out_of_memory, "String size overflow");
    data = crucible_generated_arena_alloc(arena, input.length + 1, error);
    if (!data) return error ? error->status : crucible_generated_out_of_memory;
    if (input.length) memcpy(data, input.data, input.length);
    output->data = data;
    output->length = input.length;
    return crucible_generated_ok;
}
