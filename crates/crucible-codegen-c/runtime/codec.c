#include "internal.h"
#include "schema.h"
#include <float.h>
#include <math.h>
#include <stdlib.h>
#include <string.h>

typedef crucible_generated_status status;
typedef crucible_generated_shape shape;
typedef crucible_generated_member member;
typedef crucible_generated_arena arena;
typedef crucible_generated_error error;
typedef crucible_generated_string string;
typedef crucible_generated_presence presence;
typedef crucible_generated_timestamp_format format;
#define OK crucible_generated_ok
#define INVALID crucible_generated_invalid
#define OOM crucible_generated_out_of_memory
#define TRY(expr) do { status code_ = (expr); if (code_ != OK) return code_; } while (0)
#define FAIL(message) crucible_generated_fail(issue, INVALID, message)

static bool indirect(const shape *type) {
    return type->kind == crucible_generated_kind_structure || type->kind == crucible_generated_kind_union || type->kind == crucible_generated_kind_list || type->kind == crucible_generated_kind_map;
}
static void destroy_document(void *document) { yyjson_doc_free(document); }
static status own_value(arena *storage, const yyjson_mut_val *value, const yyjson_val **output, error *issue) {
    yyjson_doc *document = yyjson_mut_val_imut_copy(value, NULL);
    status code;
    if (!document) return crucible_generated_fail(issue, OOM, "JSON allocation failed");
    code = crucible_generated_arena_own(storage, document, destroy_document, issue);
    if (code != OK) { yyjson_doc_free(document); return code; }
    *output = yyjson_doc_get_root(document);
    return OK;
}
static status copy_value(arena *storage, const yyjson_val *value, const yyjson_val **output, error *issue) {
    yyjson_mut_doc *document = yyjson_mut_doc_new(NULL);
    yyjson_mut_val *copy;
    status code;
    if (!document) return crucible_generated_fail(issue, OOM, "JSON allocation failed");
    copy = yyjson_val_mut_copy(document, value);
    code = copy ? own_value(storage, copy, output, issue) : crucible_generated_fail(issue, OOM, "JSON allocation failed");
    yyjson_mut_doc_free(document);
    return code;
}
static status allocate(arena *storage, size_t count, size_t size, void **output, error *issue) {
    if (size && count > SIZE_MAX / size) return crucible_generated_fail(issue, OOM, "Model allocation overflow");
    *output = crucible_generated_arena_alloc(storage, count * size, issue);
    return *output ? OK : (issue ? issue->status : OOM);
}
static const member *find_member(const shape *type, string name, size_t *index) {
    size_t at;
    for (at = 0; at < type->member_count; ++at) {
        if (crucible_generated_string_equal(type->members[at].name, name)) { if (index) *index = at; return &type->members[at]; }
    }
    return NULL;
}
static int base64_digit(unsigned char value) {
    if (value >= 'A' && value <= 'Z') return value - 'A';
    if (value >= 'a' && value <= 'z') return value - 'a' + 26;
    if (value >= '0' && value <= '9') return value - '0' + 52;
    return value == '+' ? 62 : value == '/' ? 63 : -1;
}
static status decode_blob(arena *storage, const yyjson_val *value, crucible_generated_blob *output, error *issue) {
    const char *text;
    unsigned char *bytes;
    size_t length, at, written = 0;
    void *allocated;
    if (!yyjson_is_str(value)) return FAIL("Expected base64 string");
    text = yyjson_get_str(value); length = yyjson_get_len(value);
    if (length % 4) return FAIL("Expected padded base64");
    TRY(allocate(storage, length / 4, 3, &allocated, issue));
    bytes = allocated;
    for (at = 0; at < length; at += 4) {
        int a = base64_digit((unsigned char)text[at]), b = base64_digit((unsigned char)text[at + 1]);
        int c = text[at + 2] == '=' ? 0 : base64_digit((unsigned char)text[at + 2]);
        int d = text[at + 3] == '=' ? 0 : base64_digit((unsigned char)text[at + 3]);
        bool pad2 = text[at + 2] == '=', pad1 = text[at + 3] == '=';
        if (a < 0 || b < 0 || c < 0 || d < 0 || (pad2 && !pad1) || ((pad1 || pad2) && at + 4 != length) || (pad2 && (b & 15)) || (pad1 && !pad2 && (c & 3))) return FAIL("Invalid base64");
        bytes[written++] = (unsigned char)((a << 2) | (b >> 4));
        if (!pad2) bytes[written++] = (unsigned char)((b << 4) | (c >> 2));
        if (!pad1) bytes[written++] = (unsigned char)((c << 6) | d);
    }
    output->data = bytes; output->length = written;
    return OK;
}
static yyjson_mut_val *encode_blob(yyjson_mut_doc *document, crucible_generated_blob value) {
    static const char alphabet[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    char *text;
    size_t length, at, written = 0;
    yyjson_mut_val *result;
    if (value.length > (SIZE_MAX - 2) / 4 * 3) return NULL;
    length = ((value.length + 2) / 3) * 4;
    text = malloc(length ? length : 1);
    if (!text) return NULL;
    for (at = 0; at < value.length; at += 3) {
        unsigned a = value.data[at], b = at + 1 < value.length ? value.data[at + 1] : 0, c = at + 2 < value.length ? value.data[at + 2] : 0;
        text[written++] = alphabet[a >> 2]; text[written++] = alphabet[((a & 3) << 4) | (b >> 4)];
        text[written++] = at + 1 < value.length ? alphabet[((b & 15) << 2) | (c >> 6)] : '=';
        text[written++] = at + 2 < value.length ? alphabet[c & 63] : '=';
    }
    result = yyjson_mut_strncpy(document, text, length);
    free(text);
    return result;
}
static status decode_into(arena *, const shape *, format, const yyjson_val *, void *, unsigned, error *);
static status decode_slot(arena *storage, const shape *type, format timestamp_format, const yyjson_val *value, void *output, unsigned depth, error *issue) {
    void *native = output;
    if (indirect(type)) { TRY(allocate(storage, 1, type->size, &native, issue)); }
    TRY(decode_into(storage, type, timestamp_format, value, native, depth, issue));
    if (indirect(type)) memcpy(output, &native, sizeof(native));
    return OK;
}
static status decode_into(arena *storage, const shape *type, format timestamp_format, const yyjson_val *value, void *output, unsigned depth, error *issue) {
    size_t at;
    int64_t integer;
    double number;
    unsigned char *native = output;
    if (depth > 255) return FAIL("Model nesting limit");
    if (!value) return FAIL("Missing JSON value");
    switch (type->kind) {
    case crucible_generated_kind_unit:
        if (!yyjson_is_obj(value) || yyjson_obj_size(value)) return FAIL("Expected unit object");
        break;
    case crucible_generated_kind_string: case crucible_generated_kind_enum:
        if (!yyjson_is_str(value)) return FAIL("Expected string");
        ((string *)output)->data = yyjson_get_str(value); ((string *)output)->length = yyjson_get_len(value);
        break;
    case crucible_generated_kind_big_integer: case crucible_generated_kind_big_decimal: {
        char *canonical = NULL; bool integral;
        TRY(crucible_generated_number_canonical(value, &canonical, &integral, issue)); free(canonical);
        if (type->kind == crucible_generated_kind_big_integer && !integral) return FAIL("Expected integral arbitrary precision number");
        string number_text;
        if (yyjson_is_raw(value)) { number_text.data = yyjson_get_raw(value); number_text.length = yyjson_get_len(value); }
        else { TRY(crucible_generated_json_write(storage, value, &number_text, issue)); }
        memcpy(output, &number_text, sizeof(number_text)); break;
    }
    case crucible_generated_kind_boolean: return crucible_generated_json_boolean_value(value, output, issue);
    case crucible_generated_kind_byte:
        TRY(crucible_generated_json_integer(value, INT8_MIN, INT8_MAX, &integer, issue)); *(int8_t *)output = (int8_t)integer; break;
    case crucible_generated_kind_short:
        TRY(crucible_generated_json_integer(value, INT16_MIN, INT16_MAX, &integer, issue)); *(int16_t *)output = (int16_t)integer; break;
    case crucible_generated_kind_int_enum:
    case crucible_generated_kind_integer:
        TRY(crucible_generated_json_integer(value, INT32_MIN, INT32_MAX, &integer, issue)); *(int32_t *)output = (int32_t)integer; break;
    case crucible_generated_kind_long: return crucible_generated_json_integer(value, INT64_MIN, INT64_MAX, output, issue);
    case crucible_generated_kind_float:
        TRY(crucible_generated_json_double(value, &number, issue));
        if (number > FLT_MAX || number < -FLT_MAX) return FAIL("Float out of range");
        *(float *)output = (float)number; break;
    case crucible_generated_kind_double: return crucible_generated_json_double(value, output, issue);
    case crucible_generated_kind_blob: return decode_blob(storage, value, output, issue);
    case crucible_generated_kind_timestamp:
        if (timestamp_format == crucible_generated_epoch_seconds) return crucible_generated_timestamp_epoch(value, output, issue);
        if (!yyjson_is_str(value)) return FAIL("Expected timestamp string");
        { string text = { yyjson_get_str(value), yyjson_get_len(value) };
          return timestamp_format == crucible_generated_http_date ? crucible_generated_timestamp_http_parse(text, output, issue) : crucible_generated_timestamp_parse(text, output, issue); }
    case crucible_generated_kind_document: memcpy(output, &value, sizeof(value)); break;
    case crucible_generated_kind_structure:
        if (!yyjson_is_obj(value)) return FAIL("Expected structure object");
        for (at = 0; at < type->member_count; ++at) {
            const member *field = &type->members[at];
            const yyjson_val *item = yyjson_obj_getn(value, field->name.data, field->name.length);
            presence state;
            if (!item && field->default_json.data) TRY(crucible_generated_json_parse(storage, field->default_json, &item, issue));
            if (!item) { if (!field->optional) return FAIL("Missing required member"); state = crucible_generated_absent; }
            else if (yyjson_is_null(item) && (field->optional || field->target->kind == crucible_generated_kind_document)) state = crucible_generated_null;
            else { TRY(decode_slot(storage, field->target, field->format, item, native + field->value_offset, depth + 1, issue)); state = crucible_generated_present; }
            memcpy(native + field->state_offset, &state, sizeof(state));
        }
        break;
    case crucible_generated_kind_list: case crucible_generated_kind_map: {
        bool map = type->kind == crucible_generated_kind_map;
        size_t length, maximum, index;
        void *items;
        yyjson_val *item, *key;
        if (map ? !yyjson_is_obj(value) : !yyjson_is_arr(value)) return FAIL("Expected collection");
        length = map ? yyjson_obj_size(value) : yyjson_arr_size(value);
        if (type->unique) { TRY(crucible_generated_json_unique(value, issue)); }
        TRY(allocate(storage, length, type->item_size, &items, issue));
        memcpy(native + type->length_offset, &length, sizeof(length));
        memcpy(native + type->items_offset, &items, sizeof(items));
        if (map) {
            yyjson_obj_foreach(value, index, maximum, key, item) {
                unsigned char *entry = (unsigned char *)items + index * type->item_size;
                TRY(decode_slot(storage, type->members[0].target, type->members[0].format, key, entry + type->members[0].value_offset, depth + 1, issue));
                const member *field = &type->members[1];
                if (yyjson_is_null(item)) { if (!type->sparse) return FAIL("Dense map cannot contain null"); presence state = crucible_generated_null; memcpy(entry + field->state_offset, &state, sizeof(state)); }
                else { TRY(decode_slot(storage, field->target, field->format, item, entry + field->value_offset, depth + 1, issue)); if (type->sparse) { presence state = crucible_generated_present; memcpy(entry + field->state_offset, &state, sizeof(state)); } }
            }
        } else {
            yyjson_arr_foreach(value, index, maximum, item) {
                unsigned char *entry = (unsigned char *)items + index * type->item_size; const member *field = &type->members[0];
                if (yyjson_is_null(item)) { if (!type->sparse) return FAIL("Dense list cannot contain null"); presence state = crucible_generated_null; memcpy(entry + field->state_offset, &state, sizeof(state)); }
                else { TRY(decode_slot(storage, field->target, field->format, item, entry + field->value_offset, depth + 1, issue)); if (type->sparse) { presence state = crucible_generated_present; memcpy(entry + field->state_offset, &state, sizeof(state)); } }
            }
        }
        break;
    }
    case crucible_generated_kind_union: {
        yyjson_val *key, *item;
        string name;
        const member *variant;
        uint32_t tag = 0;
        presence state = crucible_generated_present;
        if (!yyjson_is_obj(value) || yyjson_obj_size(value) != 1) return FAIL("Union requires one member");
        /* Iterators are explicit because yyjson retains embedded NUL key bytes. */
        { yyjson_obj_iter iter = yyjson_obj_iter_with(value); key = yyjson_obj_iter_next(&iter); }
        item = yyjson_obj_iter_get_val(key);
        name.data = yyjson_get_str(key); name.length = yyjson_get_len(key);
        variant = find_member(type, name, &at);
        if (variant) {
            tag = (uint32_t)(at + 1);
            if (yyjson_is_null(item)) state = crucible_generated_null;
            else TRY(decode_slot(storage, variant->target, variant->format, item, native + variant->value_offset, depth + 1, issue));
        } else {
            memcpy(native + type->unknown_name_offset, &name, sizeof(name));
            memcpy(native + type->unknown_value_offset, &item, sizeof(item));
        }
        memcpy(native + type->tag_offset, &tag, sizeof(tag));
        memcpy(native + type->state_offset, &state, sizeof(state));
        break;
    }
    }
    return OK;
}

static status encode_value(yyjson_mut_doc *, const shape *, format, const void *, bool, yyjson_mut_val **, unsigned, error *);
static status add_member(yyjson_mut_doc *document, yyjson_mut_val *object, string name, yyjson_mut_val *value, error *issue) {
    yyjson_mut_val *key;
    if (!crucible_generated_utf8(name)) return FAIL("Invalid member name UTF-8");
    key = yyjson_mut_strncpy(document, name.data ? name.data : "", name.length);
    if (!key || !value || !yyjson_mut_obj_add(object, key, value)) return crucible_generated_fail(issue, OOM, "JSON allocation failed");
    return OK;
}
static status encode_value(yyjson_mut_doc *document, const shape *type, format timestamp_format, const void *input, bool slot, yyjson_mut_val **output, unsigned depth, error *issue) {
    const unsigned char *native = input;
    yyjson_mut_val *result = NULL;
    size_t at;
    if (depth > 255) return FAIL("Model nesting limit");
    if (slot && indirect(type)) memcpy(&native, input, sizeof(native));
    if (!native) return FAIL("Missing native model value");
    switch (type->kind) {
    case crucible_generated_kind_unit: result = yyjson_mut_obj(document); break;
    case crucible_generated_kind_string: case crucible_generated_kind_enum: {
        const string *text = (const string *)native;
        if (!crucible_generated_utf8(*text)) return FAIL("Invalid string UTF-8");
        result = yyjson_mut_strncpy(document, text->data ? text->data : "", text->length); break;
    }
    case crucible_generated_kind_big_integer: case crucible_generated_kind_big_decimal: {
        const string *text = (const string *)native;
        if (!text->data || !text->length) return FAIL("Expected arbitrary precision number token");
        yyjson_doc *parsed = yyjson_read_opts((char *)text->data, text->length, YYJSON_READ_NUMBER_AS_RAW, NULL, NULL);
        if (!parsed) return FAIL("Invalid arbitrary precision number token");
        char *canonical = NULL; bool integral = false;
        status code = crucible_generated_number_canonical(yyjson_doc_get_root(parsed), &canonical, &integral, issue);
        free(canonical); yyjson_doc_free(parsed);
        if (code) return code;
        if (type->kind == crucible_generated_kind_big_integer && !integral) return FAIL("Expected integral arbitrary precision number");
        result = yyjson_mut_rawncpy(document, text->data, text->length); break;
    }
    case crucible_generated_kind_boolean: result = yyjson_mut_bool(document, *(const bool *)native); break;
    case crucible_generated_kind_byte: result = yyjson_mut_sint(document, *(const int8_t *)native); break;
    case crucible_generated_kind_short: result = yyjson_mut_sint(document, *(const int16_t *)native); break;
    case crucible_generated_kind_int_enum:
    case crucible_generated_kind_integer: result = yyjson_mut_sint(document, *(const int32_t *)native); break;
    case crucible_generated_kind_long: result = yyjson_mut_sint(document, *(const int64_t *)native); break;
    case crucible_generated_kind_float: case crucible_generated_kind_double: {
        double value = type->kind == crucible_generated_kind_float ? *(const float *)native : *(const double *)native;
        if (!isfinite(value)) return FAIL("Nonfinite float");
        result = yyjson_mut_real(document, value); break;
    }
    case crucible_generated_kind_blob: {
        const crucible_generated_blob *value = (const crucible_generated_blob *)native;
        if (!value->data && value->length) return FAIL("Invalid blob view");
        result = encode_blob(document, *value); break;
    }
    case crucible_generated_kind_timestamp: {
        char text[40];
        crucible_generated_timestamp value = *(const crucible_generated_timestamp *)native;
        if (timestamp_format == crucible_generated_epoch_seconds) { TRY(crucible_generated_timestamp_epoch_text(value, text, issue)); result = yyjson_mut_rawncpy(document, text, strlen(text)); }
        else { TRY(timestamp_format == crucible_generated_http_date ? crucible_generated_timestamp_http_text(value, text, issue) : crucible_generated_timestamp_text(value, text, issue)); result = yyjson_mut_strcpy(document, text); }
        break;
    }
    case crucible_generated_kind_document: {
        const yyjson_val *value;
        memcpy(&value, native, sizeof(value));
        if (!value) return FAIL("Missing document value");
        TRY(crucible_generated_json_depth(value, depth, issue));
        result = yyjson_val_mut_copy(document, value); break;
    }
    case crucible_generated_kind_structure:
        result = yyjson_mut_obj(document);
        if (!result) break;
        for (at = 0; at < type->member_count; ++at) {
            const member *field = &type->members[at];
            presence state;
            yyjson_mut_val *value;
            memcpy(&state, native + field->state_offset, sizeof(state));
            if (state == crucible_generated_absent) { if (!field->optional) return FAIL("Missing required member"); continue; }
            if (state == crucible_generated_null) { if (!field->optional && field->target->kind != crucible_generated_kind_document) return FAIL("Null required member"); value = yyjson_mut_null(document); }
            else if (state == crucible_generated_present) { TRY(encode_value(document, field->target, field->format, native + field->value_offset, true, &value, depth + 1, issue)); }
            else return FAIL("Invalid member presence");
            TRY(add_member(document, result, field->name, value, issue));
        }
        break;
    case crucible_generated_kind_list: case crucible_generated_kind_map: {
        bool map = type->kind == crucible_generated_kind_map;
        size_t length;
        const unsigned char *items;
        memcpy(&length, native + type->length_offset, sizeof(length)); memcpy(&items, native + type->items_offset, sizeof(items));
        if ((!items && length) || (type->item_size && length > SIZE_MAX / type->item_size)) return FAIL("Invalid collection view");
        result = map ? yyjson_mut_obj(document) : yyjson_mut_arr(document);
        if (!result) break;
        for (at = 0; at < length; ++at) {
            const unsigned char *entry = items + at * type->item_size;
            yyjson_mut_val *value;
            const member *field = &type->members[map ? 1 : 0];
            presence state = crucible_generated_present;
            if (type->sparse) memcpy(&state, entry + field->state_offset, sizeof(state));
            if (state == crucible_generated_null) value = yyjson_mut_null(document);
            else if (state == crucible_generated_present) { TRY(encode_value(document, field->target, field->format, entry + field->value_offset, true, &value, depth + 1, issue)); }
            else return FAIL("Sparse collection values must be present or null");
            if (!type->sparse && yyjson_mut_is_null(value)) return FAIL("Dense collection cannot contain null");
            if (map) {
                yyjson_mut_val *key;
                string name;
                field = &type->members[0];
                TRY(encode_value(document, field->target, field->format, entry + field->value_offset, true, &key, depth + 1, issue));
                if (!yyjson_mut_is_str(key)) return FAIL("Expected string map key");
                name.data = yyjson_mut_get_str(key); name.length = yyjson_mut_get_len(key);
                if (yyjson_mut_obj_getn(result, name.data, name.length)) return FAIL("Duplicate map key");
                TRY(add_member(document, result, name, value, issue));
            } else if (!yyjson_mut_arr_add_val(result, value)) return crucible_generated_fail(issue, OOM, "JSON allocation failed");
        }
        break;
    }
    case crucible_generated_kind_union: {
        uint32_t tag;
        presence state;
        yyjson_mut_val *value;
        string name;
        memcpy(&tag, native + type->tag_offset, sizeof(tag)); memcpy(&state, native + type->state_offset, sizeof(state));
        result = yyjson_mut_obj(document);
        if (!result) break;
        if (tag == 0) {
            const yyjson_val *unknown;
            memcpy(&name, native + type->unknown_name_offset, sizeof(name)); memcpy(&unknown, native + type->unknown_value_offset, sizeof(unknown));
            if (state != crucible_generated_present || !unknown || find_member(type, name, NULL)) return FAIL("Invalid unknown union variant");
            TRY(crucible_generated_json_depth(unknown, depth + 1, issue));
            value = yyjson_val_mut_copy(document, unknown);
        } else {
            const member *field;
            if (tag > type->member_count) return FAIL("Invalid union tag");
            field = &type->members[tag - 1]; name = field->name;
            if (state == crucible_generated_null) value = yyjson_mut_null(document);
            else if (state == crucible_generated_present) { TRY(encode_value(document, field->target, field->format, native + field->value_offset, true, &value, depth + 1, issue)); }
            else return FAIL("Union value must be present or null");
        }
        TRY(add_member(document, result, name, value, issue));
        break;
    }
    }
    if (!result) return crucible_generated_fail(issue, OOM, "JSON allocation failed");
    if (type->unique) {
        yyjson_doc *copy = yyjson_mut_val_imut_copy(result, NULL);
        if (!copy) return crucible_generated_fail(issue, OOM, "JSON allocation failed");
        status code = crucible_generated_json_unique(yyjson_doc_get_root(copy), issue); yyjson_doc_free(copy); if (code) return code;
    }
    *output = result;
    return OK;
}
crucible_generated_status crucible_generated_model_encode(arena *storage, const shape *type, const void *native, const crucible_generated_json **output, error *issue) {
    yyjson_mut_doc *document;
    yyjson_mut_val *value;
    status code;
    if (!storage || !type || !native || !output) return FAIL("Invalid model encode arguments");
    document = yyjson_mut_doc_new(NULL);
    if (!document) return crucible_generated_fail(issue, OOM, "JSON allocation failed");
    code = encode_value(document, type, type->format, native, false, &value, 0, issue);
    if (code == OK) code = own_value(storage, value, output, issue);
    yyjson_mut_doc_free(document);
    return code;
}
crucible_generated_status crucible_generated_model_decode(arena *storage, const shape *type, const crucible_generated_json *value, const void **output, error *issue) {
    const yyjson_val *owned;
    void *native;
    if (!storage || !type || !value || !output) return FAIL("Invalid model decode arguments");
    TRY(crucible_generated_json_depth(value, 0, issue));
    TRY(copy_value(storage, value, &owned, issue));
    TRY(allocate(storage, 1, type->size, &native, issue));
    TRY(decode_into(storage, type, type->format, owned, native, 0, issue));
    *output = native;
    return OK;
}
