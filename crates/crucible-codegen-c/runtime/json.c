#include "internal.h"
#include <stdlib.h>

bool crucible_generated_utf8(crucible_generated_string value) {
    size_t at = 0;
    if (!value.data && value.length) return false;
    while (at < value.length) {
        unsigned char first = (unsigned char)value.data[at++];
        unsigned remaining;
        uint32_t point, minimum;
        if (first < 128) continue;
        if (first >= 0xc2 && first <= 0xdf) { remaining = 1; point = first & 31U; minimum = 0x80; }
        else if (first >= 0xe0 && first <= 0xef) { remaining = 2; point = first & 15U; minimum = 0x800; }
        else if (first >= 0xf0 && first <= 0xf4) { remaining = 3; point = first & 7U; minimum = 0x10000; }
        else return false;
        if (remaining > value.length - at) return false;
        while (remaining--) {
            unsigned char byte = (unsigned char)value.data[at++];
            if ((byte & 0xc0) != 0x80) return false;
            point = (point << 6) | (byte & 63U);
        }
        if (point < minimum || point > 0x10ffff || (point >= 0xd800 && point <= 0xdfff)) return false;
    }
    return true;
}
crucible_generated_status crucible_generated_json_depth(const yyjson_val *value, unsigned depth, crucible_generated_error *error) {
    yyjson_val *child;
    size_t index, maximum;
    if (depth > 255) return crucible_generated_fail(error, crucible_generated_invalid, "JSON nesting limit");
    if (yyjson_is_arr(value)) {
        yyjson_arr_foreach(value, index, maximum, child) {
            crucible_generated_status status = crucible_generated_json_depth(child, depth + 1, error);
            if (status != crucible_generated_ok) return status;
        }
    } else if (yyjson_is_obj(value)) {
        yyjson_val *key;
        yyjson_obj_foreach(value, index, maximum, key, child) {
            crucible_generated_status status = crucible_generated_json_depth(child, depth + 1, error);
            if (status != crucible_generated_ok) return status;
        }
    }
    return crucible_generated_ok;
}
static void destroy_document(void *document) { yyjson_doc_free(document); }
crucible_generated_status crucible_generated_json_parse(crucible_generated_arena *arena, crucible_generated_string text, const crucible_generated_json **output, crucible_generated_error *error) {
    yyjson_doc *document;
    yyjson_read_err details;
    crucible_generated_status status;
    if (!arena || !output || (!text.data && text.length)) return crucible_generated_fail(error, crucible_generated_invalid, "Invalid JSON arguments");
    document = yyjson_read_opts((char *)text.data, text.length, YYJSON_READ_NUMBER_AS_RAW, NULL, &details);
    if (!document) return crucible_generated_fail(error, details.code == YYJSON_READ_ERROR_MEMORY_ALLOCATION ? crucible_generated_out_of_memory : crucible_generated_invalid, details.msg ? details.msg : "Invalid JSON");
    status = crucible_generated_json_depth(yyjson_doc_get_root(document), 0, error);
    if (status == crucible_generated_ok) status = crucible_generated_arena_own(arena, document, destroy_document, error);
    if (status != crucible_generated_ok) { yyjson_doc_free(document); return status; }
    *output = yyjson_doc_get_root(document);
    return crucible_generated_ok;
}
crucible_generated_status crucible_generated_json_write(crucible_generated_arena *arena, const crucible_generated_json *value, crucible_generated_string *output, crucible_generated_error *error) {
    yyjson_write_err details;
    size_t length;
    char *text;
    crucible_generated_status status;
    if (!arena || !value || !output) return crucible_generated_fail(error, crucible_generated_invalid, "Invalid JSON arguments");
    status = crucible_generated_json_depth(value, 0, error);
    if (status != crucible_generated_ok) return status;
    text = yyjson_val_write_opts(value, 0, NULL, &length, &details);
    if (!text) return crucible_generated_fail(error, details.code == YYJSON_WRITE_ERROR_MEMORY_ALLOCATION ? crucible_generated_out_of_memory : crucible_generated_invalid, details.msg ? details.msg : "Invalid JSON value");
    status = crucible_generated_arena_own(arena, text, free, error);
    if (status != crucible_generated_ok) { free(text); return status; }
    output->data = text;
    output->length = length;
    return crucible_generated_ok;
}
crucible_generated_json_kind crucible_generated_json_type(const crucible_generated_json *value) {
    if (yyjson_is_bool(value)) return crucible_generated_json_boolean;
    if (yyjson_is_num(value) || yyjson_is_raw(value)) return crucible_generated_json_number;
    if (yyjson_is_str(value)) return crucible_generated_json_string;
    if (yyjson_is_arr(value)) return crucible_generated_json_array;
    if (yyjson_is_obj(value)) return crucible_generated_json_object;
    return crucible_generated_json_null;
}
const crucible_generated_json *crucible_generated_json_get(const crucible_generated_json *value, crucible_generated_string key) {
    if (!key.data && key.length) return NULL;
    return yyjson_obj_getn(value, key.data ? key.data : "", key.length);
}
const crucible_generated_json *crucible_generated_json_at(const crucible_generated_json *value, size_t index) { return yyjson_arr_get(value, index); }
size_t crucible_generated_json_size(const crucible_generated_json *value) { return yyjson_is_arr(value) ? yyjson_arr_size(value) : yyjson_is_obj(value) ? yyjson_obj_size(value) : 0; }
crucible_generated_status crucible_generated_json_text(const crucible_generated_json *value, crucible_generated_string *output, crucible_generated_error *error) {
    if (!output || (!yyjson_is_str(value) && !yyjson_is_raw(value))) return crucible_generated_fail(error, crucible_generated_invalid, "Expected a string or exact number");
    output->data = yyjson_is_raw(value) ? yyjson_get_raw(value) : yyjson_get_str(value);
    output->length = yyjson_get_len(value);
    return crucible_generated_ok;
}
crucible_generated_status crucible_generated_json_boolean_value(const crucible_generated_json *value, bool *output, crucible_generated_error *error) {
    if (!output || !yyjson_is_bool(value)) return crucible_generated_fail(error, crucible_generated_invalid, "Expected a boolean");
    *output = yyjson_get_bool(value);
    return crucible_generated_ok;
}
