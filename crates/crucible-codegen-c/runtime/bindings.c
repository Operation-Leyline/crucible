#include "bindings.h"
#include <openssl/evp.h>
#include <openssl/rand.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

typedef crucible_generated_status status;
typedef crucible_generated_string string;
typedef crucible_generated_error error;
typedef crucible_generated_arena arena;
typedef struct { char *data; size_t length, capacity; } buffer;
#define OK crucible_generated_ok
#define INVALID crucible_generated_invalid
#define OOM crucible_generated_out_of_memory
#define TRY(call) do { status code = (call); if (code) return code; } while (0)
#define VIEW(text) crucible_generated_string_view(text)
static const yyjson_val *get(const yyjson_val *value, const char *name) { return yyjson_obj_get((yyjson_val *)value, name); }
static string text(const yyjson_val *value) { return (string){yyjson_get_str((yyjson_val *)value), yyjson_get_len((yyjson_val *)value)}; }
static bool named(const yyjson_val *value, const char *name) { return crucible_generated_string_equal(text(value), VIEW(name)); }
static status fail(error *issue, const char *message) { return crucible_generated_fail(issue, INVALID, message); }
static void free_doc(void *document) { yyjson_doc_free(document); }
static void free_mut_doc(void *document) { yyjson_mut_doc_free(document); }
static status append(buffer *out, string value, error *issue) {
    if (value.length > SIZE_MAX - out->length - 1) return crucible_generated_fail(issue, OOM, "Binding size overflow");
    size_t needed = out->length + value.length + 1;
    if (needed > out->capacity) {
        size_t capacity = out->capacity > SIZE_MAX / 2 ? needed : out->capacity * 2;
        if (capacity < needed) capacity = needed;
        char *next = realloc(out->data, capacity);
        if (!next) return crucible_generated_fail(issue, OOM, "Binding allocation failed");
        out->data = next; out->capacity = capacity;
    }
    if (value.length) memcpy(out->data + out->length, value.data, value.length);
    out->length += value.length; out->data[out->length] = 0; return OK;
}
static status take(arena *storage, buffer *value, string *out, error *issue) {
    status code = crucible_generated_arena_copy(storage, (string){value->data, value->length}, out, issue);
    free(value->data); *value = (buffer){0}; return code;
}
static status escape(buffer *out, string value, error *issue) {
    if (!crucible_generated_utf8(value)) return fail(issue, "Invalid UTF-8 binding");
    static const char digits[] = "0123456789ABCDEF";
    for (size_t i = 0; i < value.length; ++i) {
        unsigned char c = (unsigned char)value.data[i];
        if ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '-' || c == '.' || c == '_' || c == '~') { TRY(append(out, (string){value.data + i, 1}, issue)); }
        else { char encoded[] = {'%', digits[c >> 4], digits[c & 15]}; TRY(append(out, (string){encoded, 3}, issue)); }
    }
    return OK;
}
status crucible_generated_json_own(arena *storage, const yyjson_mut_val *value, const yyjson_val **out, error *issue) {
    yyjson_doc *document = yyjson_mut_val_imut_copy(value, NULL);
    if (!document) return crucible_generated_fail(issue, OOM, "JSON allocation failed");
    status code = crucible_generated_arena_own(storage, document, free_doc, issue);
    if (code) { yyjson_doc_free(document); return code; }
    *out = yyjson_doc_get_root(document); return OK;
}
static status mutable(arena *storage, yyjson_mut_doc **out, error *issue) {
    yyjson_mut_doc *document = yyjson_mut_doc_new(NULL);
    if (!document) return crucible_generated_fail(issue, OOM, "JSON allocation failed");
    status code = crucible_generated_arena_own(storage, document, free_mut_doc, issue);
    if (code) { yyjson_mut_doc_free(document); return code; }
    *out = document; return OK;
}
static status set(yyjson_mut_doc *doc, yyjson_mut_val *object, string key, yyjson_mut_val *value, error *issue) {
    yyjson_mut_val *name = yyjson_mut_strncpy(doc, key.data, key.length);
    if (!name || !value || !yyjson_mut_obj_put(object, name, value)) return crucible_generated_fail(issue, OOM, "JSON allocation failed");
    return OK;
}
static status scalar(arena *storage, const yyjson_val *value, string *out, error *issue) {
    if (yyjson_is_str(value) || yyjson_is_raw(value)) { *out = text(value); if (yyjson_is_raw(value)) out->data = yyjson_get_raw((yyjson_val *)value); return OK; }
    if (yyjson_is_bool(value)) { *out = VIEW(yyjson_get_bool((yyjson_val *)value) ? "true" : "false"); return OK; }
    if (yyjson_is_num(value)) return crucible_generated_json_write(storage, value, out, issue);
    return fail(issue, "Expected bound scalar");
}
static string trim(string value) {
    while (value.length && (*value.data == ' ' || *value.data == '\t')) { ++value.data; --value.length; }
    while (value.length && (value.data[value.length - 1] == ' ' || value.data[value.length - 1] == '\t')) --value.length;
    return value;
}
static status bound(arena *storage, const yyjson_val *value, string *out, error *issue) {
    if (!yyjson_is_arr(value)) return scalar(storage, value, out, issue);
    buffer joined = {0}; status code = OK;
    size_t index, count; yyjson_val *item;
    yyjson_arr_foreach((yyjson_val *)value, index, count, item) {
        string part;
        code = scalar(storage, item, &part, issue); if (code) break;
        if (index) { code = append(&joined, VIEW(", "), issue); if (code) break; }
        bool quote = part.length == 0 || trim(part).length != part.length;
        for (size_t i = 0; i < part.length; ++i) if (part.data[i] == ',' || part.data[i] == '"' || part.data[i] == '\\') quote = true;
        if (quote) {
            yyjson_mut_doc *doc;
            code = mutable(storage, &doc, issue); if (code) break;
            yyjson_mut_val *str = yyjson_mut_strncpy(doc, part.data, part.length);
            const yyjson_val *owned;
            code = crucible_generated_json_own(storage, str, &owned, issue); if (code) break;
            code = crucible_generated_json_write(storage, owned, &part, issue); if (code) break;
        }
        code = append(&joined, part, issue); if (code) break;
    }
    if (code) { free(joined.data); return code; }
    return take(storage, &joined, out, issue);
}
static void hex(const unsigned char *bytes, size_t length, char *out) {
    const char *digits = "0123456789abcdef";
    for (size_t i = 0; i < length; ++i) { out[i * 2] = digits[bytes[i] >> 4]; out[i * 2 + 1] = digits[bytes[i] & 15]; }
    out[length * 2] = 0;
}
status crucible_generated_binding_token(arena *storage, const yyjson_val *operation, const yyjson_val *input, const yyjson_val **out, error *issue) {
    const yyjson_val *token = get(operation, "token");
    *out = input;
    if (yyjson_is_str(token)) {
        string name = text(token);
        const yyjson_val *current = yyjson_obj_getn((yyjson_val *)input, name.data, name.length);
        if (!current || yyjson_is_null(current)) {
            unsigned char bytes[16]; char plain[33], uuid[37];
            if (RAND_bytes(bytes, 16) != 1) return fail(issue, "Secure random generation failed");
            bytes[6] = (unsigned char)((bytes[6] & 15) | 64); bytes[8] = (unsigned char)((bytes[8] & 63) | 128);
            hex(bytes, 16, plain);
            (void)snprintf(uuid, sizeof(uuid), "%.8s-%.4s-%.4s-%.4s-%.12s", plain, plain + 8, plain + 12, plain + 16, plain + 20);
            yyjson_mut_doc *doc; TRY(mutable(storage, &doc, issue));
            yyjson_mut_val *copy = yyjson_val_mut_copy(doc, input);
            TRY(set(doc, copy, name, yyjson_mut_strcpy(doc, uuid), issue));
            TRY(crucible_generated_json_own(storage, copy, out, issue));
        }
    }
    return OK;
}
status crucible_generated_binding_digest(arena *storage, crucible_generated_request *request, error *issue) {
    /* A caller-provided digest cannot survive a middleware body replacement. */
    crucible_generated_header *items = crucible_generated_arena_alloc(storage, request->headers.count * sizeof(*items), issue);
    if (!items) return OOM;
    size_t count = 0;
    for (size_t i = 0; i < request->headers.count; ++i) {
        crucible_generated_headers one = {request->headers.items + i, 1};
        if (!crucible_generated_header_get(one, VIEW("x-amz-content-sha256"), NULL)) items[count++] = request->headers.items[i];
    }
    request->headers = (crucible_generated_headers){items, count};
    if (request->has_body || (request->method.length == 4 && memcmp(request->method.data, "POST", 4) == 0) || (request->method.length == 3 && memcmp(request->method.data, "PUT", 3) == 0) || (request->method.length == 5 && memcmp(request->method.data, "PATCH", 5) == 0)) {
        unsigned char bytes[EVP_MAX_MD_SIZE]; unsigned length; char digest[EVP_MAX_MD_SIZE * 2 + 1];
        if (EVP_Digest(request->has_body ? request->body.data : NULL, request->has_body ? request->body.length : 0, bytes, &length, EVP_sha256(), NULL) != 1) return fail(issue, "SHA256 failed");
        hex(bytes, length, digest);
        TRY(crucible_generated_header_set(storage, &request->headers, VIEW("x-amz-content-sha256"), VIEW(digest), issue));
    }
    return OK;
}
static const yyjson_val *shape(const yyjson_val *schema, const yyjson_val *id);
static status document(yyjson_mut_doc *doc, const yyjson_val *schema, const yyjson_val *target, const yyjson_val *value, bool encode, yyjson_mut_val **out, unsigned depth, error *issue) {
    if (depth > 255) return fail(issue, "Document nesting limit");
    const yyjson_val *type = shape(schema, target), *kind = get(type, "kind");
    yyjson_mut_val *result = NULL;
    if (!yyjson_is_null(value) && (named(kind, "structure") || named(kind, "union"))) {
        if (!yyjson_is_obj(value)) return fail(issue, "Expected document object");
        result = yyjson_mut_obj(doc);
        size_t index, count; yyjson_val *key, *item;
        const yyjson_val *members = get(type, "members");
        yyjson_obj_foreach((yyjson_val *)value, index, count, key, item) {
            string original = text(key), destination = original;
            const yyjson_val *member = NULL;
            size_t mi, mc; yyjson_val *mk, *mv;
            yyjson_obj_foreach((yyjson_val *)members, mi, mc, mk, mv) {
                string model = text(mk), wire = yyjson_is_str(get(mv, "jsonName")) ? text(get(mv, "jsonName")) : model;
                if (crucible_generated_string_equal(original, encode ? model : wire)) { member = mv; destination = encode ? wire : model; break; }
            }
            yyjson_mut_val *converted;
            /* JSON protocols may omit null structure members; documents keep explicit null. */
            if (member && yyjson_is_null(item) && named(kind, "structure") && (crucible_generated_protocol_flags & CRUCIBLE_GENERATED_OMIT_NULL_MEMBERS)
                && !named(get(shape(schema, get(member, "target")), "kind"), "document")) continue;
            if (!member && !encode && named(kind, "union") && (crucible_generated_protocol_flags & CRUCIBLE_GENERATED_IGNORE_UNION_TYPE) && crucible_generated_string_equal(original, VIEW("__type"))) continue;
            if (member) { TRY(document(doc, schema, get(member, "target"), item, encode, &converted, depth + 1, issue)); }
            else {
                if (!named(kind, "union")) continue;
                if (encode) {
                    yyjson_obj_foreach((yyjson_val *)members, mi, mc, mk, mv) {
                        string wire = yyjson_is_str(get(mv, "jsonName")) ? text(get(mv, "jsonName")) : text(mk);
                        if (crucible_generated_string_equal(original, wire)) return fail(issue, "Unknown variant name is declared");
                    }
                }
                converted = yyjson_val_mut_copy(doc, item);
            }
            TRY(set(doc, result, destination, converted, issue));
        }
    } else if (!yyjson_is_null(value) && (named(kind, "list") || named(kind, "map"))) {
        bool map = named(kind, "map");
        if (map ? !yyjson_is_obj(value) : !yyjson_is_arr(value)) return fail(issue, "Expected collection document");
        result = map ? yyjson_mut_obj(doc) : yyjson_mut_arr(doc);
        const yyjson_val *child = get(get(type, map ? "value" : "member"), "target");
        size_t index, count; yyjson_val *key, *item;
        if (map) {
            yyjson_obj_foreach((yyjson_val *)value, index, count, key, item) { yyjson_mut_val *converted; TRY(document(doc, schema, child, item, encode, &converted, depth + 1, issue)); TRY(set(doc, result, text(key), converted, issue)); }
        } else {
            yyjson_arr_foreach((yyjson_val *)value, index, count, item) { yyjson_mut_val *converted; TRY(document(doc, schema, child, item, encode, &converted, depth + 1, issue)); if (!yyjson_mut_arr_add_val(result, converted)) return crucible_generated_fail(issue, OOM, "JSON allocation failed"); }
        }
    } else result = yyjson_val_mut_copy(doc, value);
    if (!result) return crucible_generated_fail(issue, OOM, "JSON allocation failed");
    *out = result; return OK;
}
/* HTTP bindings may use another timestamp format than the document encoding; timestampFormats lists only the members that differ. */
static string binding_format(const yyjson_val *bindings, string member) {
    return text(yyjson_obj_getn((yyjson_val *)get(bindings, "timestampFormats"), member.data, member.length));
}
static status timestamp_render(crucible_generated_timestamp time, string format, yyjson_mut_doc *doc, yyjson_mut_val **out, error *issue) {
    char rendered[40];
    if (crucible_generated_string_equal(format, VIEW("http-date"))) { TRY(crucible_generated_timestamp_http_text(time, rendered, issue)); }
    else if (crucible_generated_string_equal(format, VIEW("epoch-seconds"))) { TRY(crucible_generated_timestamp_epoch_text(time, rendered, issue)); }
    else { TRY(crucible_generated_timestamp_text(time, rendered, issue)); }
    *out = yyjson_mut_strcpy(doc, rendered);
    return *out ? OK : crucible_generated_fail(issue, OOM, "JSON allocation failed");
}
/* The document form is epoch seconds or text; either is accepted so an already converted scalar stays valid. */
static status retime(yyjson_mut_doc *doc, const yyjson_val *value, string format, yyjson_mut_val **out, error *issue) {
    if (yyjson_is_arr(value)) {
        yyjson_mut_val *result = yyjson_mut_arr(doc); size_t index, count; yyjson_val *item;
        if (!result) return crucible_generated_fail(issue, OOM, "JSON allocation failed");
        yyjson_arr_foreach((yyjson_val *)value, index, count, item) {
            yyjson_mut_val *converted; TRY(retime(doc, item, format, &converted, issue));
            if (!yyjson_mut_arr_add_val(result, converted)) return crucible_generated_fail(issue, OOM, "JSON allocation failed");
        }
        *out = result; return OK;
    }
    crucible_generated_timestamp time;
    if (yyjson_is_num(value) || yyjson_is_raw(value)) { TRY(crucible_generated_timestamp_epoch(value, &time, issue)); }
    else if (yyjson_is_str(value)) {
        error ignored = {0};
        if (crucible_generated_timestamp_parse(text(value), &time, &ignored)) { TRY(crucible_generated_timestamp_http_parse(text(value), &time, issue)); }
    } else { *out = yyjson_val_mut_copy(doc, value); return *out ? OK : crucible_generated_fail(issue, OOM, "JSON allocation failed"); }
    return timestamp_render(time, format, doc, out, issue);
}
static status binding_value(arena *storage, const yyjson_val *bindings, string member, const yyjson_val *value, const yyjson_val **out, error *issue) {
    string format = binding_format(bindings, member);
    *out = value;
    if (!format.length || yyjson_is_null(value)) return OK;
    yyjson_mut_doc *doc; TRY(mutable(storage, &doc, issue));
    yyjson_mut_val *converted; TRY(retime(doc, value, format, &converted, issue));
    return crucible_generated_json_own(storage, converted, out, issue);
}
static bool explicit_header(const yyjson_val *bindings, string name) {
    size_t index, count; yyjson_val *pair;
    yyjson_arr_foreach((yyjson_val *)get(bindings, "headers"), index, count, pair) {
        crucible_generated_header item = {text(yyjson_arr_get(pair, 1)), VIEW("")};
        if (crucible_generated_header_get((crucible_generated_headers){&item, 1}, name, NULL)) return true;
    }
    return false;
}
static const crucible_generated_shape blob_shape = {.kind = crucible_generated_kind_blob, .size = sizeof(crucible_generated_blob)};
static status media(arena *storage, yyjson_mut_doc *doc, const yyjson_val *bindings, string member, const yyjson_val *value, bool encode, yyjson_mut_val **out, error *issue) {
    const yyjson_val *encodings = get(bindings, "headerEncodings");
    if (!named(yyjson_obj_getn((yyjson_val *)encodings, member.data, member.length), "base64")) { *out = yyjson_val_mut_copy(doc, value); return *out ? OK : crucible_generated_fail(issue, OOM, "JSON allocation failed"); }
    if (yyjson_is_arr(value)) {
        yyjson_mut_val *result = yyjson_mut_arr(doc); size_t index, count; yyjson_val *item;
        yyjson_arr_foreach(value, index, count, item) { yyjson_mut_val *converted; TRY(media(storage, doc, bindings, member, item, encode, &converted, issue)); if (!yyjson_mut_arr_add_val(result, converted)) return crucible_generated_fail(issue, OOM, "JSON allocation failed"); }
        *out = result; return OK;
    }
    if (!yyjson_is_str(value)) return fail(issue, "Expected media header string");
    if (encode) {
        string raw = text(value); crucible_generated_blob bytes = {(const unsigned char *)raw.data, raw.length}; const yyjson_val *encoded;
        TRY(crucible_generated_model_encode(storage, &blob_shape, &bytes, &encoded, issue)); *out = yyjson_val_mut_copy(doc, encoded);
    } else {
        const void *decoded; TRY(crucible_generated_model_decode(storage, &blob_shape, value, &decoded, issue));
        const crucible_generated_blob *bytes = decoded; string raw = {(const char *)bytes->data, bytes->length};
        if (!crucible_generated_utf8(raw)) return fail(issue, "Invalid media header UTF-8");
        *out = yyjson_mut_strncpy(doc, raw.data, raw.length);
    }
    return *out ? OK : crucible_generated_fail(issue, OOM, "JSON allocation failed");
}
static status media_bound(arena *storage, const yyjson_val *bindings, string member, const yyjson_val *value, string *out, error *issue) {
    TRY(binding_value(storage, bindings, member, value, &value, issue));
    yyjson_mut_doc *doc; TRY(mutable(storage, &doc, issue)); yyjson_mut_val *converted;
    TRY(media(storage, doc, bindings, member, value, true, &converted, issue)); const yyjson_val *owned;
    TRY(crucible_generated_json_own(storage, converted, &owned, issue)); return bound(storage, owned, out, issue);
}
static const yyjson_val *payload_target(const yyjson_val *schema, const yyjson_val *target, const yyjson_val *bindings) {
    string member = text(yyjson_arr_get((yyjson_val *)get(bindings, "payload"), 0));
    return get(yyjson_obj_getn((yyjson_val *)get(shape(schema, target), "members"), member.data, member.length), "target");
}
static status read_payload(arena *storage, yyjson_mut_doc *doc, const yyjson_val *schema, const yyjson_val *target, const yyjson_val *bindings, crucible_generated_blob bytes, yyjson_mut_val **out, error *issue) {
    const yyjson_val *kind = get(bindings, "payloadKind");
    if (named(kind, "string")) {
        string raw = {(const char *)bytes.data, bytes.length}; if (!crucible_generated_utf8(raw)) return fail(issue, "Invalid payload UTF-8");
        *out = yyjson_mut_strncpy(doc, raw.data ? raw.data : "", raw.length);
    } else if (named(kind, "document")) {
        if (!bytes.length) { *out = yyjson_mut_null(doc); }
        else { const yyjson_val *parsed; TRY(crucible_generated_json_parse(storage, (string){(const char *)bytes.data, bytes.length}, &parsed, issue)); return document(doc, schema, payload_target(schema, target, bindings), parsed, false, out, 0, issue); }
    } else { const yyjson_val *encoded; TRY(crucible_generated_model_encode(storage, &blob_shape, &bytes, &encoded, issue)); *out = yyjson_val_mut_copy(doc, encoded); }
    return *out ? OK : crucible_generated_fail(issue, OOM, "JSON allocation failed");
}
static int hex_digit(unsigned char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    return -1;
}
static bool query_reserved(const yyjson_val *op, string name) {
    size_t index, count; yyjson_val *pair;
    yyjson_arr_foreach((yyjson_val *)get(get(op, "request"), "query"), index, count, pair) if (crucible_generated_string_equal(text(yyjson_arr_get(pair, 1)), name)) return true;
    string uri = text(get(op, "uri")); size_t start = 0;
    while (start < uri.length && uri.data[start] != '?') ++start;
    if (start == uri.length) return false;
    ++start;
    while (start < uri.length) {
        size_t stop = start; while (stop < uri.length && uri.data[stop] != '=' && uri.data[stop] != '&') ++stop;
        size_t cursor = 0; bool match = true;
        for (size_t at = start; at < stop; ++at) {
            unsigned char ch = (unsigned char)uri.data[at];
            if (ch == '%' && at + 2 < stop) { int a = hex_digit((unsigned char)uri.data[at + 1]), b = hex_digit((unsigned char)uri.data[at + 2]); if (a >= 0 && b >= 0) { ch = (unsigned char)(a * 16 + b); at += 2; } }
            if (cursor >= name.length || ch != (unsigned char)name.data[cursor]) match = false;
            ++cursor;
        }
        if (match && cursor == name.length) return true;
        while (stop < uri.length && uri.data[stop] != '&') ++stop;
        start = stop + 1;
    }
    return false;
}
static status prepare_path(arena *storage, const yyjson_val *op, const yyjson_val *input, string endpoint, string *out, error *issue) {
    buffer path = {0}; status code = append(&path, endpoint, issue);
    string uri = text(get(op, "uri"));
    for (size_t i = 0; !code && i < uri.length;) {
        if (uri.data[i] != '{') { code = append(&path, (string){uri.data + i, 1}, issue); ++i; continue; }
        size_t end = i + 1; while (end < uri.length && uri.data[end] != '}') ++end;
        if (end == uri.length) { code = fail(issue, "Invalid operation URI"); break; }
        bool greedy = end > i + 1 && uri.data[end - 1] == '+';
        const yyjson_val *value = yyjson_obj_getn((yyjson_val *)input, uri.data + i + 1, end - i - 1 - (greedy ? 1 : 0));
        string encoded = {0};
        string label = {uri.data + i + 1, end - i - 1 - (greedy ? 1 : 0)};
        code = binding_value(storage, get(op, "request"), label, value, &value, issue);
        if (!code) code = scalar(storage, value, &encoded, issue);
        if (!code && !encoded.length) code = fail(issue, "Empty HTTP label");
        if (!code && ((encoded.length == 1 && encoded.data[0] == '.') || (encoded.length == 2 && memcmp(encoded.data, "..", 2) == 0))) code = fail(issue, "Dot-segment HTTP label");
        if (!code && greedy) {
            for (size_t start = 0; !code && start <= encoded.length;) {
                size_t stop = start; while (stop < encoded.length && encoded.data[stop] != '/') ++stop;
                string segment = {encoded.data + start, stop - start};
                if ((segment.length == 1 && segment.data[0] == '.') || (segment.length == 2 && memcmp(segment.data, "..", 2) == 0)) { code = fail(issue, "Dot-segment HTTP label"); break; }
                code = escape(&path, segment, issue);
                if (stop == encoded.length) break;
                if (!code) code = append(&path, VIEW("/"), issue);
                start = stop + 1;
            }
        } else if (!code) code = escape(&path, encoded, issue);
        i = end + 1;
    }
    const yyjson_val *bindings = get(get(op, "request"), "query");
    size_t index, count; yyjson_val *pair;
    yyjson_arr_foreach((yyjson_val *)bindings, index, count, pair) {
        if (code) break;
        string member = text(yyjson_arr_get(pair, 0)), name = text(yyjson_arr_get(pair, 1));
        const yyjson_val *value = yyjson_obj_getn((yyjson_val *)input, member.data, member.length);
        if (!value || yyjson_is_null(value)) continue;
        code = binding_value(storage, get(op, "request"), member, value, &value, issue);
        if (code) break;
        size_t length = yyjson_is_arr(value) ? yyjson_arr_size((yyjson_val *)value) : 1;
        for (size_t n = 0; !code && n < length; ++n) {
            string part;
            code = scalar(storage, yyjson_is_arr(value) ? yyjson_arr_get((yyjson_val *)value, n) : value, &part, issue);
            if (!code) code = append(&path, VIEW(memchr(path.data, '?', path.length) ? "&" : "?"), issue);
            if (!code) code = escape(&path, name, issue);
            if (!code) code = append(&path, VIEW("="), issue);
            if (!code) code = escape(&path, part, issue);
        }
    }
    const yyjson_val *query_member = get(get(op, "request"), "queryParams");
    if (!code && yyjson_is_str(query_member)) {
        string member = text(query_member); const yyjson_val *map = yyjson_obj_getn((yyjson_val *)input, member.data, member.length);
        size_t mi, mc; yyjson_val *key, *value;
        yyjson_obj_foreach((yyjson_val *)map, mi, mc, key, value) {
            if (code) break;
            string name = text(key); if (query_reserved(op, name) || yyjson_is_null(value)) continue;
            size_t length = yyjson_is_arr(value) ? yyjson_arr_size(value) : 1;
            for (size_t n = 0; !code && n < length; ++n) {
                const yyjson_val *item = yyjson_is_arr(value) ? yyjson_arr_get(value, n) : value;
                if (yyjson_is_null(item)) continue;
                string part; code = scalar(storage, item, &part, issue);
                if (!code) code = append(&path, VIEW(memchr(path.data, '?', path.length) ? "&" : "?"), issue);
                if (!code) code = escape(&path, name, issue);
                if (!code) code = append(&path, VIEW("="), issue);
                if (!code) code = escape(&path, part, issue);
            }
        }
    }
    if (code) { free(path.data); return code; }
    return take(storage, &path, out, issue);
}
static bool aws_json(const yyjson_val *schema) {
    const yyjson_val *protocol = get(schema, "protocol");
    return named(protocol, "awsJson1_0") || named(protocol, "awsJson1_1");
}
static bool aws_protocol(const yyjson_val *schema) { return aws_json(schema) || named(get(schema, "protocol"), "awsRestJson1"); }
status crucible_generated_binding_prepare(arena *storage, const yyjson_val *schema, string operation_name, const yyjson_val *op, const yyjson_val *input, string endpoint,
    crucible_generated_headers headers, crucible_generated_request *out, error *issue) {
    if (!yyjson_is_obj(input)) return fail(issue, "Operation input must be an object");
    crucible_generated_request request = {0};
    request.method = text(get(op, "method")); request.headers = headers;
    TRY(prepare_path(storage, op, input, endpoint, &request.url, issue));
    const yyjson_val *bindings = get(op, "request");
    size_t index, count; yyjson_val *pair;
    yyjson_arr_foreach((yyjson_val *)get(bindings, "headers"), index, count, pair) {
        string member = text(yyjson_arr_get(pair, 0));
        const yyjson_val *value = yyjson_obj_getn((yyjson_val *)input, member.data, member.length);
        if (!value || yyjson_is_null(value)) continue;
        string header; TRY(media_bound(storage, bindings, member, value, &header, issue));
        TRY(crucible_generated_header_set(storage, &request.headers, text(yyjson_arr_get(pair, 1)), header, issue));
    }
    yyjson_arr_foreach((yyjson_val *)get(bindings, "prefixHeaders"), index, count, pair) {
        string member = text(yyjson_arr_get(pair, 0)), prefix = text(yyjson_arr_get(pair, 1));
        const yyjson_val *map = yyjson_obj_getn((yyjson_val *)input, member.data, member.length);
        size_t mi, mc; yyjson_val *key, *value;
        yyjson_obj_foreach((yyjson_val *)map, mi, mc, key, value) {
            buffer name = {0}; status appended = append(&name, prefix, issue); if (!appended) appended = append(&name, text(key), issue);
            if (appended) { free(name.data); return appended; }
            string header_name; TRY(take(storage, &name, &header_name, issue));
            if (!explicit_header(bindings, header_name)) { string header; TRY(media_bound(storage, bindings, member, value, &header, issue)); TRY(crucible_generated_header_set(storage, &request.headers, header_name, header, issue)); }
        }
    }
    if (yyjson_arr_size((yyjson_val *)get(bindings, "body"))) {
        yyjson_mut_doc *doc; TRY(mutable(storage, &doc, issue));
        yyjson_mut_val *body = yyjson_mut_obj(doc);
        yyjson_val *name;
        yyjson_arr_foreach((yyjson_val *)get(bindings, "body"), index, count, name) {
            string member = text(name);
            const yyjson_val *value = yyjson_obj_getn((yyjson_val *)input, member.data, member.length);
            if (value) { TRY(set(doc, body, member, yyjson_val_mut_copy(doc, value), issue)); }
        }
        const yyjson_val *owned; string encoded;
        TRY(crucible_generated_json_own(storage, body, &owned, issue));
        TRY(document(doc, schema, get(op, "input"), owned, true, &body, 0, issue));
        TRY(crucible_generated_json_own(storage, body, &owned, issue));
        TRY(crucible_generated_json_write(storage, owned, &encoded, issue));
        request.has_body = true; request.body = (crucible_generated_blob){(const unsigned char *)encoded.data, encoded.length};
        TRY(crucible_generated_header_set(storage, &request.headers, VIEW("content-type"), VIEW("application/json"), issue));
    }
    const yyjson_val *payload = get(bindings, "payload");
    if (yyjson_is_arr(payload)) {
        string member = text(yyjson_arr_get((yyjson_val *)payload, 0));
        const yyjson_val *value = yyjson_obj_getn((yyjson_val *)input, member.data, member.length);
        if (value && !yyjson_is_null(value)) {
            const yyjson_val *kind = get(bindings, "payloadKind");
            if (named(kind, "string")) { string raw = text(value); request.body = (crucible_generated_blob){(const unsigned char *)raw.data, raw.length}; }
            else if (named(kind, "document")) {
                yyjson_mut_doc *doc; TRY(mutable(storage, &doc, issue)); yyjson_mut_val *converted; const yyjson_val *owned; string raw;
                TRY(document(doc, schema, payload_target(schema, get(op, "input"), bindings), value, true, &converted, 0, issue));
                TRY(crucible_generated_json_own(storage, converted, &owned, issue)); TRY(crucible_generated_json_write(storage, owned, &raw, issue));
                request.body = (crucible_generated_blob){(const unsigned char *)raw.data, raw.length};
            } else { const void *decoded; TRY(crucible_generated_model_decode(storage, &blob_shape, value, &decoded, issue)); request.body = *(const crucible_generated_blob *)decoded; }
            request.has_body = true;
            TRY(crucible_generated_header_set(storage, &request.headers, VIEW("content-type"), text(yyjson_arr_get((yyjson_val *)payload, 1)), issue));
        }
    }
    if (aws_json(schema)) {
        /* AWS JSON RPC always posts a JSON object and names the target operation. */
        if (!request.has_body) { request.has_body = true; request.body = (crucible_generated_blob){(const unsigned char *)"{}", 2}; }
        TRY(crucible_generated_header_set(storage, &request.headers, VIEW("content-type"), VIEW(named(get(schema, "protocol"), "awsJson1_0") ? "application/x-amz-json-1.0" : "application/x-amz-json-1.1"), issue));
        string service = text(get(schema, "service"));
        for (size_t i = service.length; i > 0; --i) if (service.data[i - 1] == '#') { service = (string){service.data + i, service.length - i}; break; }
        buffer target = {0}; status joined = append(&target, service, issue);
        if (!joined) joined = append(&target, VIEW("."), issue);
        if (!joined) joined = append(&target, operation_name, issue);
        if (joined) { free(target.data); return joined; }
        string value; TRY(take(storage, &target, &value, issue));
        TRY(crucible_generated_header_set(storage, &request.headers, VIEW("X-Amz-Target"), value, issue));
    }
    /* The release this client was generated for (docs/api-versions-clients.md). */
    const yyjson_val *api_version = get(schema, "apiVersion");
    if (api_version && yyjson_is_str((yyjson_val *)api_version))
        TRY(crucible_generated_header_set(storage, &request.headers, VIEW("crucible-version"), text(api_version), issue));
    TRY(crucible_generated_validate_request(&request, issue));
    *out = request; return OK;
}

static const yyjson_val *shape(const yyjson_val *schema, const yyjson_val *id) {
    string name = text(id);
    return yyjson_obj_getn((yyjson_val *)get(schema, "shapes"), name.data, name.length);
}
static status read_header(arena *storage, yyjson_mut_doc *doc, const yyjson_val *schema, const yyjson_val *member, string value, string format, yyjson_mut_val **out, unsigned depth, error *issue) {
    if (depth > 255) return fail(issue, "Header nesting limit exceeded");
    const yyjson_val *type = shape(schema, get(member, "target"));
    const yyjson_val *kind = get(type, "kind");
    if (named(kind, "list")) {
        yyjson_mut_val *array = yyjson_mut_arr(doc);
        if (!array) return crucible_generated_fail(issue, OOM, "JSON allocation failed");
        if (!trim(value).length) { *out = array; return OK; }
        bool quoted = false, escaped = false;
        size_t start = 0;
        /* An unquoted HTTP date holds one comma after its weekday, so only every second comma separates items. */
        bool bare_dates = crucible_generated_string_equal(format, VIEW("http-date")) && named(get(shape(schema, get(get(type, "member"), "target")), "kind"), "timestamp");
        for (size_t i = 0; bare_dates && i < value.length; ++i) if (value.data[i] == '"') bare_dates = false;
        bool inside_date = false;
        for (size_t i = 0; i <= value.length; ++i) {
            const char c = i == value.length ? ',' : value.data[i];
            if (c == ',' && !quoted && bare_dates && i < value.length && (inside_date = !inside_date)) continue;
            if (c == ',' && !quoted) {
                string part = trim((string){value.data + start, i - start});
                if (part.length && *part.data == '"') {
                    const yyjson_val *parsed;
                    TRY(crucible_generated_json_parse(storage, part, &parsed, issue));
                    if (!yyjson_is_str(parsed)) return fail(issue, "Expected quoted header string");
                    part = text(parsed);
                }
                yyjson_mut_val *item;
                TRY(read_header(storage, doc, schema, get(type, "member"), part, format, &item, depth + 1, issue));
                if (!yyjson_mut_arr_append(array, item)) return crucible_generated_fail(issue, OOM, "JSON allocation failed");
                start = i + 1;
            } else if (escaped) escaped = false;
            else if (c == '\\' && quoted) escaped = true;
            else if (c == '"') quoted = !quoted;
        }
        if (quoted) return fail(issue, "Unterminated quoted header");
        *out = array; return OK;
    }
    if (named(kind, "timestamp") && format.length) {
        crucible_generated_timestamp time;
        if (crucible_generated_string_equal(format, VIEW("http-date"))) { TRY(crucible_generated_timestamp_http_parse(value, &time, issue)); }
        else if (crucible_generated_string_equal(format, VIEW("epoch-seconds"))) { const yyjson_val *number; TRY(crucible_generated_json_parse(storage, value, &number, issue)); TRY(crucible_generated_timestamp_epoch(number, &time, issue)); }
        else { TRY(crucible_generated_timestamp_parse(value, &time, issue)); }
        /* The codec reads a bound scalar in its binding format but a list item in the list member's format. */
        const bool epoch = depth ? named(get(member, "format"), "epoch-seconds") : crucible_generated_string_equal(format, VIEW("epoch-seconds"));
        if (epoch) {
            char rendered[40]; const yyjson_val *parsed;
            TRY(crucible_generated_timestamp_epoch_text(time, rendered, issue));
            TRY(crucible_generated_json_parse(storage, VIEW(rendered), &parsed, issue));
            *out = yyjson_val_mut_copy(doc, parsed); return *out ? OK : crucible_generated_fail(issue, OOM, "JSON allocation failed");
        }
        return timestamp_render(time, depth ? (named(get(member, "format"), "http-date") ? VIEW("http-date") : VIEW("date-time")) : format, doc, out, issue);
    }
    if ((named(kind, "float") || named(kind, "double")) && (crucible_generated_string_equal(value, VIEW("NaN")) || crucible_generated_string_equal(value, VIEW("Infinity")) || crucible_generated_string_equal(value, VIEW("-Infinity")))) {
        /* JSON has no non-finite numbers; the codec reads these strings back as floats. */
        *out = yyjson_mut_strncpy(doc, value.data, value.length);
    } else if (named(kind, "byte") || named(kind, "short") || named(kind, "integer") || named(kind, "long") || named(kind, "float") || named(kind, "double") || named(kind, "bigInteger") || named(kind, "bigDecimal") || named(kind, "intEnum") || named(kind, "boolean") || (named(kind, "timestamp") && named(get(member, "format"), "epoch-seconds"))) {
        const yyjson_val *parsed;
        TRY(crucible_generated_json_parse(storage, value, &parsed, issue));
        *out = yyjson_val_mut_copy(doc, parsed);
    } else *out = yyjson_mut_strncpy(doc, value.data, value.length);
    return *out ? OK : crucible_generated_fail(issue, OOM, "JSON allocation failed");
}
static status apply_headers(arena *storage, yyjson_mut_doc *doc, yyjson_mut_val *body, const yyjson_val *schema,
    const yyjson_val *target, const yyjson_val *bindings, crucible_generated_headers headers, int response_status, error *issue) {
    size_t index, count; yyjson_val *pair;
    const yyjson_val *members = get(shape(schema, target), "members");
    yyjson_arr_foreach((yyjson_val *)get(bindings, "headers"), index, count, pair) {
        string name = text(yyjson_arr_get(pair, 0)), value;
        if (crucible_generated_header_get(headers, text(yyjson_arr_get(pair, 1)), &value)) {
            yyjson_mut_val *converted;
            TRY(read_header(storage, doc, schema, yyjson_obj_getn((yyjson_val *)members, name.data, name.length), value, binding_format(bindings, name), &converted, 0, issue));
            const yyjson_val *owned; TRY(crucible_generated_json_own(storage, converted, &owned, issue));
            TRY(media(storage, doc, bindings, name, owned, false, &converted, issue));
            TRY(set(doc, body, name, converted, issue));
        }
    }
    yyjson_arr_foreach((yyjson_val *)get(bindings, "prefixHeaders"), index, count, pair) {
        string prefix = text(yyjson_arr_get(pair, 1)); yyjson_mut_val *map = yyjson_mut_obj(doc);
        for (size_t at = 0; at < headers.count; ++at) {
            string name = headers.items[at].name;
            if (name.length < prefix.length || explicit_header(bindings, name)) continue;
            bool matches = true;
            for (size_t n = 0; n < prefix.length; ++n) {
                unsigned char a = (unsigned char)name.data[n], b = (unsigned char)prefix.data[n];
                if (a >= 'A' && a <= 'Z') { a += 'a' - 'A'; }
                if (b >= 'A' && b <= 'Z') { b += 'a' - 'A'; }
                if (a != b) matches = false;
            }
            if (!matches) continue;
            size_t length = name.length - prefix.length; char *key = crucible_generated_arena_alloc(storage, length + 1, issue); if (!key) return OOM;
            for (size_t n = 0; n < length; ++n) { char ch = name.data[n + prefix.length]; key[n] = ch >= 'A' && ch <= 'Z' ? (char)(ch + 'a' - 'A') : ch; }
            yyjson_mut_val *converted = yyjson_mut_strncpy(doc, headers.items[at].value.data, headers.items[at].value.length); const yyjson_val *owned;
            TRY(crucible_generated_json_own(storage, converted, &owned, issue));
            TRY(media(storage, doc, bindings, text(yyjson_arr_get(pair, 0)), owned, false, &converted, issue));
            TRY(set(doc, map, (string){key, length}, converted, issue));
        }
        TRY(set(doc, body, text(yyjson_arr_get(pair, 0)), map, issue));
    }
    if (yyjson_is_str(get(bindings, "responseCode"))) { TRY(set(doc, body, text(get(bindings, "responseCode")), yyjson_mut_sint(doc, response_status), issue)); }
    return OK;
}
status crucible_generated_binding_success(arena *storage, const yyjson_val *schema, const yyjson_val *op,
    const crucible_generated_response *response, const yyjson_val **out, error *issue) {
    const yyjson_val *bindings = get(op, "response"), *payload = get(bindings, "payload");
    yyjson_mut_doc *doc; TRY(mutable(storage, &doc, issue));
    yyjson_mut_val *body = yyjson_mut_obj(doc);
    if (!body) return crucible_generated_fail(issue, OOM, "JSON allocation failed");
    if (yyjson_is_arr(payload)) {
        yyjson_mut_val *value; TRY(read_payload(storage, doc, schema, get(op, "output"), bindings, response->body, &value, issue));
        TRY(set(doc, body, text(yyjson_arr_get((yyjson_val *)payload, 0)), value, issue));
    } else if (yyjson_arr_size((yyjson_val *)get(bindings, "body")) && response->body.length) {
        const yyjson_val *parsed;
        TRY(crucible_generated_json_parse(storage, (string){(const char *)response->body.data, response->body.length}, &parsed, issue));
        if (!yyjson_is_obj(parsed)) return fail(issue, "Expected response object");
        TRY(document(doc, schema, get(op, "output"), parsed, false, &body, 0, issue));
        if (!body) return crucible_generated_fail(issue, OOM, "JSON allocation failed");
    }
    if (yyjson_is_str(get(op, "output"))) { TRY(apply_headers(storage, doc, body, schema, get(op, "output"), bindings, response->headers, response->status, issue)); }
    return crucible_generated_json_own(storage, body, out, issue);
}
status crucible_generated_binding_failure(arena *storage, const yyjson_val *schema, const yyjson_val *op,
    crucible_generated_result *result, error *issue) {
    const yyjson_val *body = NULL;
    error ignored = {0};
    status parsed = crucible_generated_json_parse(storage, (string){(const char *)result->response.body.data, result->response.body.length}, &body, &ignored);
    if (parsed == OOM) { if (issue) *issue = ignored; return parsed; }
    if (parsed || !yyjson_is_obj(body)) { TRY(crucible_generated_json_parse(storage, VIEW("{}"), &body, issue)); }
    const yyjson_val *matched = NULL, *type = NULL;
    const bool aws = aws_protocol(schema);
    bool has_name = yyjson_is_str(get(body, "__type"));
    string wire_name = has_name ? text(get(body, "__type")) : VIEW(""), error_header;
    if (crucible_generated_header_get(result->response.headers, VIEW("x-crucible-error"), &error_header)) { wire_name = error_header; has_name = true; }
    if (aws) {
        if (yyjson_is_str(get(body, "code"))) { wire_name = text(get(body, "code")); has_name = true; }
        if (crucible_generated_header_get(result->response.headers, VIEW("x-amzn-errortype"), &error_header)) { wire_name = error_header; has_name = true; }
        /* "namespace#Name:metadata" names the shape Name. */
        const char *colon = wire_name.length ? memchr(wire_name.data, ':', wire_name.length) : NULL;
        if (colon) wire_name.length = (size_t)(colon - wire_name.data);
        for (size_t i = wire_name.length; i > 0; --i) if (wire_name.data[i - 1] == '#') { wire_name = (string){wire_name.data + i, wire_name.length - i}; break; }
        has_name = has_name && wire_name.length;
    }
    size_t index, count, matches = 0; yyjson_val *candidate;
    yyjson_arr_foreach((yyjson_val *)get(op, "errors"), index, count, candidate) {
        const yyjson_val *target = shape(schema, candidate);
        int64_t expected = 0;
        bool match;
        if (has_name) { const yyjson_val *wire_code = get(get(target, "error"), "wireCode"); match = crucible_generated_string_equal(wire_name, text(wire_code ? wire_code : get(target, "name"))); }
        else { TRY(crucible_generated_json_integer(get(get(target, "error"), "status"), 100, 599, &expected, issue)); match = expected == result->response.status; }
        if (match) { ++matches; matched = candidate; type = target; }
    }
    if (matches != 1) { matched = NULL; type = NULL; }
    result->name = has_name ? wire_name : type ? text(get(type, "name")) : VIEW("UnknownError");
    TRY(crucible_generated_arena_copy(storage, result->name, &result->name, issue));
    const yyjson_val *message = get(body, "message");
    if (!yyjson_is_str(message) && aws) message = get(body, "Message");
    if (yyjson_is_str(message)) result->message = text(message);
    else {
        char fallback[32]; (void)snprintf(fallback, sizeof(fallback), "HTTP %d", result->response.status);
        TRY(crucible_generated_arena_copy(storage, VIEW(fallback), &result->message, issue));
    }
    bool request_id = crucible_generated_header_get(result->response.headers, VIEW("x-request-id"), &result->request_id);
    if (aws) {
        string aws_id;
        if (crucible_generated_header_get(result->response.headers, VIEW("x-amzn-requestid"), &aws_id) || crucible_generated_header_get(result->response.headers, VIEW("x-amz-request-id"), &aws_id)) { result->request_id = aws_id; request_id = true; }
    }
    if (!request_id && yyjson_is_str(get(body, "requestId"))) result->request_id = text(get(body, "requestId"));
    const yyjson_val *traits = get(type, "error");
    result->retryable = yyjson_get_bool((yyjson_val *)get(traits, "retryable"));
    result->throttling = yyjson_get_bool((yyjson_val *)get(traits, "throttling"));
    if (matched) {
        const crucible_generated_shape *descriptor = crucible_generated_shape_lookup(text(matched));
        yyjson_mut_doc *doc; TRY(mutable(storage, &doc, issue));
        yyjson_mut_val *typed = NULL;
        const yyjson_val *payload = get(traits, "payload"); status code;
        if (yyjson_is_arr(payload)) {
            yyjson_mut_val *value; typed = yyjson_mut_obj(doc);
            code = read_payload(storage, doc, schema, matched, traits, result->response.body, &value, &ignored);
            if (!code) code = set(doc, typed, text(yyjson_arr_get((yyjson_val *)payload, 0)), value, &ignored);
        } else code = document(doc, schema, matched, body, false, &typed, 0, &ignored);
        if (!code) code = apply_headers(storage, doc, typed, schema, matched, traits, result->response.headers, result->response.status, &ignored);
        const yyjson_val *value = NULL;
        if (!code) code = crucible_generated_json_own(storage, typed, &value, &ignored);
        if (!code) code = crucible_generated_model_decode(storage, descriptor, value, &result->modeled_error, &ignored);
        if (!code) result->error_shape = descriptor;
        else if (code == OOM) { if (issue) *issue = ignored; return code; }
    }
    return OK;
}
