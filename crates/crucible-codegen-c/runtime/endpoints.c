#include "features.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Smithy endpoint rule sets lowered by crucible-smithy, evaluated over mutable JSON. */
typedef crucible_generated_status status;
typedef crucible_generated_string string;
typedef crucible_generated_query query;
typedef yyjson_mut_val value;
#define OK crucible_generated_ok
#define OOM crucible_generated_out_of_memory
#define TRY(call) do { status code_ = (call); if (code_) return code_; } while (0)
#define VIEW(text) crucible_generated_string_view(text)
static status efail(query *q, const char *message) { return crucible_generated_fail(q->issue, crucible_generated_invalid, message); }
static status nomem(query *q) { return crucible_generated_fail(q->issue, OOM, "JSON allocation failed"); }
static const yyjson_val *get(const yyjson_val *node, const char *key) { return yyjson_obj_get((yyjson_val *)node, key); }
static string itext(const yyjson_val *node) { return (string){yyjson_get_str((yyjson_val *)node), yyjson_get_len((yyjson_val *)node)}; }
static bool named(const yyjson_val *node, const char *name) { return yyjson_is_str((yyjson_val *)node) && crucible_generated_string_equal(itext(node), VIEW(name)); }
static value *norm(value *item) { return item && !yyjson_mut_is_null(item) ? item : NULL; }
static bool is_string(const value *item) { return item && yyjson_mut_is_str((value *)item); }
static bool is_bool(const value *item) { return item && yyjson_mut_is_bool((value *)item); }
static bool is_number(const value *item) { return item && (yyjson_mut_is_num((value *)item) || yyjson_mut_is_raw((value *)item)); }
static string mtext(const value *item) { return (string){yyjson_mut_get_str((value *)item), yyjson_mut_get_len((value *)item)}; }
static status new_string(query *q, string text, value **out) { *out = yyjson_mut_strncpy(q->doc, text.data ? text.data : "", text.length); return *out ? OK : nomem(q); }
static status put(query *q, value *object, const char *key, value *item) {
    value *name = yyjson_mut_strcpy(q->doc, key), *copy = item ? yyjson_mut_val_mut_copy(q->doc, item) : yyjson_mut_null(q->doc);
    return name && copy && yyjson_mut_obj_put(object, name, copy) ? OK : nomem(q);
}
static status integer(query *q, const value *item, int64_t *out) {
    if (!is_number(item)) return efail(q, "Endpoint function requires an integer");
    if (yyjson_mut_is_sint((value *)item)) { *out = yyjson_mut_get_sint((value *)item); return OK; }
    if (yyjson_mut_is_uint((value *)item)) { uint64_t number = yyjson_mut_get_uint((value *)item); if (number > INT64_MAX) return efail(q, "Endpoint integer out of range"); *out = (int64_t)number; return OK; }
    if (!yyjson_mut_is_raw((value *)item)) return efail(q, "Endpoint function requires an integer");
    char buffer[32]; string raw = {yyjson_mut_get_raw((value *)item), yyjson_mut_get_len((value *)item)};
    if (raw.length >= sizeof(buffer)) return efail(q, "Endpoint integer out of range");
    memcpy(buffer, raw.data, raw.length); buffer[raw.length] = 0;
    return crucible_generated_integer_text(buffer, INT64_MIN, INT64_MAX, out, q->issue);
}
static bool ipv4(string host) {
    unsigned parts = 0, number = 0, digits = 0;
    for (size_t i = 0; i <= host.length; ++i) {
        if (i == host.length || host.data[i] == '.') {
            if (!digits || number > 255) return false;
            ++parts; number = 0; digits = 0;
        } else if (host.data[i] >= '0' && host.data[i] <= '9') { number = number * 10 + (unsigned)(host.data[i] - '0'); if (++digits > 3) return false; }
        else return false;
    }
    return parts == 4;
}
static status parse_url(query *q, string text, value **out) {
    *out = NULL;
    for (size_t i = 0; i < text.length; ++i) {
        unsigned char c = (unsigned char)text.data[i];
        if (c <= 32 || c == 127 || c == '?' || c == '#') return OK;
    }
    crucible_generated_url url;
    if (!crucible_generated_url_split(text, &url)) return OK;
    bool https = crucible_generated_string_equal(url.scheme, VIEW("https")), http = crucible_generated_string_equal(url.scheme, VIEW("http"));
    if (!https && !http) return OK;
    bool ip = url.ipv6 || ipv4(url.host);
    if (url.ipv6) {
        for (size_t i = 1; i + 1 < url.host.length; ++i) { char c = url.host.data[i]; if (!((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F') || c == ':' || c == '.')) return OK; }
    } else if (!ip) {
        string host = url.host;
        if (host.length && host.data[host.length - 1] == '.') --host.length;
        if (!crucible_generated_host_label(host, true)) return OK;
    }
    char authority[600]; size_t written = 0;
    if (url.host.length + url.port.length + 2 > sizeof(authority)) return OK;
    for (size_t i = 0; i < url.host.length; ++i) { char c = url.host.data[i]; authority[written++] = (char)(c >= 'A' && c <= 'Z' ? c + 32 : c); }
    if (url.port.length) {
        uint32_t port = 0;
        for (size_t i = 0; i < url.port.length; ++i) { port = port * 10 + (uint32_t)(url.port.data[i] - '0'); if (port > 65535) return OK; }
        if (!((https && port == 443) || (http && port == 80))) { authority[written++] = ':'; memcpy(authority + written, url.port.data, url.port.length); written += url.port.length; }
    }
    string path = url.path.length ? url.path : VIEW("/");
    value *result = yyjson_mut_obj(q->doc), *item;
    if (!result) return nomem(q);
    TRY(new_string(q, url.scheme, &item)); TRY(put(q, result, "scheme", item));
    TRY(new_string(q, (string){authority, written}, &item)); TRY(put(q, result, "authority", item));
    TRY(new_string(q, path, &item)); TRY(put(q, result, "path", item));
    string parts[] = {path, VIEW(path.data[path.length - 1] == '/' ? "" : "/")}, normalized;
    TRY(crucible_generated_concat(q->arena, parts, 2, &normalized, q->issue));
    TRY(new_string(q, normalized, &item)); TRY(put(q, result, "normalizedPath", item));
    item = yyjson_mut_bool(q->doc, ip); if (!item) return nomem(q);
    TRY(put(q, result, "isIp", item));
    *out = result; return OK;
}
/* getAttr paths are "a.b[1]" strings or lowered step arrays. */
static status attribute(query *q, value *item, const value *text_path, const yyjson_val *steps, value **out) {
    *out = NULL;
    item = norm(item);
    if (text_path) {
        string path = mtext(text_path);
        size_t start = 0;
        for (size_t i = 0; i <= path.length; ++i) {
            if (i < path.length && path.data[i] != '.') continue;
            string part = {path.data + start, i - start};
            start = i + 1;
            const char *bracket = memchr(part.data, '[', part.length);
            string key = {part.data, bracket ? (size_t)(bracket - part.data) : part.length};
            if (key.length) { if (!item || !yyjson_mut_is_obj(item)) return OK; item = norm(yyjson_mut_obj_getn(item, key.data, key.length)); if (!item) return OK; }
            if (bracket) {
                string index = {bracket + 1, (size_t)(part.data + part.length - bracket - 1)};
                if (!index.length || index.data[index.length - 1] != ']') return efail(q, "Invalid endpoint attribute index");
                char buffer[24]; int64_t number;
                if (index.length - 1 >= sizeof(buffer) || index.length < 2) return efail(q, "Invalid endpoint attribute index");
                memcpy(buffer, index.data, index.length - 1); buffer[index.length - 1] = 0;
                if (crucible_generated_integer_text(buffer, INT64_MIN, INT64_MAX, &number, NULL)) return efail(q, "Invalid endpoint attribute index");
                if (!item || !yyjson_mut_is_arr(item)) return OK;
                int64_t length = (int64_t)yyjson_mut_arr_size(item);
                if (number < 0) number += length;
                if (number < 0 || number >= length) return OK;
                item = norm(yyjson_mut_arr_get(item, (size_t)number));
                if (!item) return OK;
            }
        }
        *out = item; return OK;
    }
    size_t index, count; yyjson_val *step;
    yyjson_arr_foreach((yyjson_val *)steps, index, count, step) {
        if (!item) return OK;
        if (named(get(step, "kind"), "member")) {
            if (!yyjson_mut_is_obj(item)) return OK;
            string key = itext(get(step, "name"));
            item = norm(yyjson_mut_obj_getn(item, key.data, key.length));
        } else {
            if (!yyjson_mut_is_arr(item)) return OK;
            value *position; int64_t number;
            TRY(crucible_generated_query_copy(q, get(step, "index"), &position)); TRY(integer(q, position, &number));
            int64_t length = (int64_t)yyjson_mut_arr_size(item);
            if (number < 0) number += length;
            if (number < 0 || number >= length) return OK;
            item = norm(yyjson_mut_arr_get(item, (size_t)number));
        }
    }
    *out = item; return OK;
}
static status call(query *q, string name, value **args, size_t count, value **out) {
    static const struct { const char *name; size_t arity; } functions[] = {
        {"isSet", 1}, {"not", 1}, {"booleanEquals", 2}, {"stringEquals", 2}, {"getAttr", 2},
        {"substring", 4}, {"isValidHostLabel", 2}, {"parseUrl", 1}, {"uriEncode", 1}};
    size_t known = sizeof(functions) / sizeof(functions[0]), at = 0;
    while (at < known && !crucible_generated_string_equal(name, VIEW(functions[at].name))) ++at;
    if (at == known) return efail(q, "Unknown endpoint function");
    if (count != functions[at].arity) return efail(q, "Invalid endpoint function arity");
    for (size_t i = 0; i < count; ++i) args[i] = norm(args[i]);
    *out = NULL;
#define BOOL(flag) do { *out = yyjson_mut_bool(q->doc, (flag)); return *out ? OK : nomem(q); } while (0)
    switch (at) {
    case 0: BOOL(args[0] != NULL);
    case 1: if (!is_bool(args[0])) return efail(q, "Endpoint function requires boolean"); BOOL(!yyjson_mut_get_bool(args[0]));
    case 2: BOOL(is_bool(args[0]) && is_bool(args[1]) && yyjson_mut_get_bool(args[0]) == yyjson_mut_get_bool(args[1]));
    case 3: BOOL(is_string(args[0]) && is_string(args[1]) && crucible_generated_string_equal(mtext(args[0]), mtext(args[1])));
    case 4:
        if (!is_string(args[1])) return efail(q, "Endpoint attribute path must be a string");
        return attribute(q, args[0], args[1], NULL, out);
    case 5: {
        if (!is_string(args[0])) return efail(q, "Endpoint function requires string");
        if (!is_bool(args[3])) return efail(q, "Endpoint function requires boolean");
        int64_t start, end; TRY(integer(q, args[1], &start)); TRY(integer(q, args[2], &end));
        string text = mtext(args[0]);
        for (size_t i = 0; i < text.length; ++i) if ((unsigned char)text.data[i] > 127) return OK;
        if (start < 0 || end < start || end > (int64_t)text.length) return OK;
        size_t from = (size_t)start, to = (size_t)end;
        if (yyjson_mut_get_bool(args[3])) { from = text.length - (size_t)end; to = text.length - (size_t)start; }
        return new_string(q, (string){text.data + from, to - from}, out);
    }
    case 6:
        if (!is_string(args[0])) return efail(q, "Endpoint function requires string");
        if (!is_bool(args[1])) return efail(q, "Endpoint function requires boolean");
        BOOL(crucible_generated_host_label(mtext(args[0]), yyjson_mut_get_bool(args[1])));
    case 7:
        if (!is_string(args[0])) return efail(q, "Endpoint function requires string");
        return parse_url(q, mtext(args[0]), out);
    default: {
        if (!is_string(args[0])) return efail(q, "Endpoint function requires string");
        string escaped; TRY(crucible_generated_uri_escape(q->arena, mtext(args[0]), &escaped, q->issue));
        return new_string(q, escaped, out);
    }
    }
#undef BOOL
}
typedef struct binding { const struct binding *next; string name; value *item; } binding;
static value *lookup(const binding *scope, string name) {
    for (; scope; scope = scope->next) if (crucible_generated_string_equal(scope->name, name)) return scope->item;
    return NULL;
}
static status scalar_text(query *q, const value *item, string *out) {
    if (is_string(item)) { *out = mtext(item); return OK; }
    if (is_bool(item)) { *out = VIEW(yyjson_mut_get_bool((value *)item) ? "true" : "false"); return OK; }
    if (is_number(item)) {
        size_t length; char *written = yyjson_mut_val_write((value *)item, 0, &length);
        if (!written) return nomem(q);
        status code = crucible_generated_arena_copy(q->arena, (string){written, length}, out, q->issue);
        free(written); return code;
    }
    return efail(q, "Endpoint template references missing or non-scalar value");
}
static status expression(query *q, const yyjson_val *node, const binding *scope, value **out, unsigned depth) {
    *out = NULL;
    if (depth > 255) return efail(q, "Endpoint expression nesting limit");
    const yyjson_val *kind = get(node, "kind");
    if (named(kind, "reference")) { *out = lookup(scope, itext(get(node, "name"))); return OK; }
    if (named(kind, "boolean") || named(kind, "integer")) return crucible_generated_query_copy(q, get(node, "value"), out);
    if (named(kind, "array")) {
        *out = yyjson_mut_arr(q->doc); if (!*out) return nomem(q);
        size_t index, count; yyjson_val *item;
        yyjson_arr_foreach((yyjson_val *)get(node, "values"), index, count, item) {
            value *result; TRY(expression(q, item, scope, &result, depth + 1));
            value *copy = result ? yyjson_mut_val_mut_copy(q->doc, result) : yyjson_mut_null(q->doc);
            if (!copy || !yyjson_mut_arr_append(*out, copy)) return nomem(q);
        }
        return OK;
    }
    if (named(kind, "object")) {
        *out = yyjson_mut_obj(q->doc); if (!*out) return nomem(q);
        size_t index, count; yyjson_val *key, *item;
        yyjson_obj_foreach((yyjson_val *)get(node, "values"), index, count, key, item) {
            value *result; TRY(expression(q, item, scope, &result, depth + 1));
            value *name = yyjson_mut_strncpy(q->doc, yyjson_get_str(key), yyjson_get_len(key)), *copy = result ? yyjson_mut_val_mut_copy(q->doc, result) : yyjson_mut_null(q->doc);
            if (!name || !copy || !yyjson_mut_obj_put(*out, name, copy)) return nomem(q);
        }
        return OK;
    }
    if (named(kind, "call")) {
        const yyjson_val *arguments = get(node, "arguments");
        value *args[4] = {NULL, NULL, NULL, NULL}; size_t count = yyjson_arr_size((yyjson_val *)arguments);
        if (count > 4) return efail(q, "Invalid endpoint function arity");
        for (size_t i = 0; i < count; ++i) TRY(expression(q, yyjson_arr_get((yyjson_val *)arguments, i), scope, &args[i], depth + 1));
        return call(q, itext(get(node, "function")), args, count, out);
    }
    if (named(kind, "string")) {
        const yyjson_val *parts = get(node, "parts");
        size_t count = yyjson_arr_size((yyjson_val *)parts);
        string *pieces = crucible_generated_arena_alloc(q->arena, (count ? count : 1) * sizeof(string), q->issue);
        if (!pieces) return OOM;
        size_t index, maximum; yyjson_val *part;
        yyjson_arr_foreach((yyjson_val *)parts, index, maximum, part) {
            if (named(get(part, "kind"), "literal")) { pieces[index] = itext(get(part, "value")); continue; }
            value *item; TRY(attribute(q, lookup(scope, itext(get(part, "name"))), NULL, get(part, "path"), &item));
            TRY(scalar_text(q, item, &pieces[index]));
        }
        string joined; TRY(crucible_generated_concat(q->arena, pieces, count, &joined, q->issue));
        return new_string(q, joined, out);
    }
    return efail(q, "Unknown endpoint expression");
}
static status rules(query *q, const yyjson_val *nodes, const binding *scope, bool *matched, string *url, crucible_generated_headers *headers, value **properties, unsigned depth) {
    *matched = false;
    if (depth > 255) return efail(q, "Endpoint rule nesting limit");
    size_t index, count; yyjson_val *rule;
    yyjson_arr_foreach((yyjson_val *)nodes, index, count, rule) {
        const binding *local = scope; bool passed = true;
        size_t ci, cc; yyjson_val *condition;
        yyjson_arr_foreach((yyjson_val *)get(rule, "conditions"), ci, cc, condition) {
            const yyjson_val *arguments = get(condition, "arguments");
            value *args[4] = {NULL, NULL, NULL, NULL}; size_t argc = yyjson_arr_size((yyjson_val *)arguments);
            if (argc > 4) return efail(q, "Invalid endpoint function arity");
            for (size_t i = 0; i < argc; ++i) TRY(expression(q, yyjson_arr_get((yyjson_val *)arguments, i), local, &args[i], depth + 1));
            value *result; TRY(call(q, itext(get(condition, "function")), args, argc, &result));
            result = norm(result);
            if (!result || (is_bool(result) && !yyjson_mut_get_bool(result))) { passed = false; break; }
            if (yyjson_is_str(get(condition, "assign"))) {
                binding *assigned = crucible_generated_arena_alloc(q->arena, sizeof(*assigned), q->issue);
                if (!assigned) return OOM;
                *assigned = (binding){local, itext(get(condition, "assign")), result};
                local = assigned;
            }
        }
        if (!passed) continue;
        const yyjson_val *result = get(rule, "result");
        if (named(get(result, "kind"), "tree")) {
            TRY(rules(q, get(result, "rules"), local, matched, url, headers, properties, depth + 1));
            if (*matched) return OK;
            continue;
        }
        if (named(get(result, "kind"), "error")) {
            value *message; TRY(expression(q, get(result, "message"), local, &message, depth + 1));
            if (!is_string(message)) return efail(q, "Endpoint rule error");
            char text[256]; string view = mtext(message);
            size_t length = view.length < sizeof(text) - 1 ? view.length : sizeof(text) - 1;
            memcpy(text, view.data, length); text[length] = 0;
            return efail(q, text);
        }
        value *location; TRY(expression(q, get(result, "url"), local, &location, depth + 1));
        value *parsed = NULL;
        if (is_string(location)) TRY(parse_url(q, mtext(location), &parsed));
        if (!parsed) return efail(q, "Endpoint rule returned invalid URL");
        *url = mtext(location);
        size_t hi, hc; yyjson_val *name, *values;
        yyjson_obj_foreach((yyjson_val *)get(result, "headers"), hi, hc, name, values) {
            size_t total = yyjson_arr_size(values), vi, vc; yyjson_val *item;
            string *pieces = crucible_generated_arena_alloc(q->arena, (total * 2 + 1) * sizeof(string), q->issue);
            if (!pieces) return OOM;
            size_t used = 0;
            yyjson_arr_foreach(values, vi, vc, item) {
                value *header; TRY(expression(q, item, local, &header, depth + 1));
                if (!is_string(header)) return efail(q, "Endpoint header values must be strings");
                if (used) pieces[used++] = VIEW(", ");
                pieces[used++] = mtext(header);
            }
            string joined; TRY(crucible_generated_concat(q->arena, pieces, used, &joined, q->issue));
            TRY(crucible_generated_header_set(q->arena, headers, itext(name), joined, q->issue));
        }
        *properties = yyjson_mut_obj(q->doc); if (!*properties) return nomem(q);
        size_t pi, pc; yyjson_val *key, *item;
        yyjson_obj_foreach((yyjson_val *)get(result, "properties"), pi, pc, key, item) {
            value *property; TRY(expression(q, item, local, &property, depth + 1));
            value *pname = yyjson_mut_strncpy(q->doc, yyjson_get_str(key), yyjson_get_len(key)), *copy = property ? yyjson_mut_val_mut_copy(q->doc, property) : yyjson_mut_null(q->doc);
            if (!pname || !copy || !yyjson_mut_obj_put(*properties, pname, copy)) return nomem(q);
        }
        *matched = true; return OK;
    }
    return OK;
}
static value *source_value(const yyjson_val *object, string name, query *q) {
    value *copy = NULL;
    const yyjson_val *item = yyjson_obj_getn((yyjson_val *)object, name.data, name.length);
    if (item && !yyjson_is_null((yyjson_val *)item) && crucible_generated_query_copy(q, item, &copy) == OK) return copy;
    return NULL;
}
status crucible_generated_endpoint_evaluate(query *q, const yyjson_val *rule_set, const yyjson_val *sources, value *input,
    const yyjson_val *client, const yyjson_val *builtins, string *url, crucible_generated_headers *headers, value **properties) {
    const binding *scope = NULL;
    *headers = (crucible_generated_headers){0}; *properties = NULL; *url = (string){0};
    size_t index, count; yyjson_val *name, *parameter;
    yyjson_obj_foreach((yyjson_val *)get(rule_set, "parameters"), index, count, name, parameter) {
        value *item = NULL;
        size_t si, sc; yyjson_val *source;
        yyjson_arr_foreach((yyjson_val *)yyjson_obj_getn((yyjson_val *)sources, yyjson_get_str(name), yyjson_get_len(name)), si, sc, source) {
            const yyjson_val *kind = get(source, "kind");
            if (named(kind, "static") || named(kind, "default")) TRY(crucible_generated_query_copy(q, get(source, "value"), &item));
            else if (named(kind, "inputMember")) { string member = itext(get(source, "member")); if (input && yyjson_mut_is_obj(input)) item = yyjson_mut_obj_getn(input, member.data, member.length); }
            else if (named(kind, "inputPath")) TRY(crucible_generated_query_eval(q, get(get(source, "path"), "expression"), input, &item));
            else if (named(kind, "client")) item = source_value(client, itext(get(source, "name")), q);
            else if (named(kind, "builtIn")) item = source_value(builtins, itext(get(source, "name")), q);
            item = norm(item);
            if (item) break;
        }
        char message[256];
        if (!item && yyjson_get_bool(get(parameter, "required"))) {
            (void)snprintf(message, sizeof(message), "Required endpoint parameter missing: %.*s", (int)yyjson_get_len(name), yyjson_get_str(name));
            return efail(q, message);
        }
        if (item) {
            const yyjson_val *kind = get(parameter, "kind");
            bool valid = named(kind, "string") ? is_string(item) : named(kind, "boolean") ? is_bool(item) : false;
            if (named(kind, "stringArray") && yyjson_mut_is_arr(item)) {
                valid = true; size_t ai, ac; value *element;
                yyjson_mut_arr_foreach(item, ai, ac, element) if (!is_string(element)) valid = false;
            }
            if (!valid) {
                (void)snprintf(message, sizeof(message), "Wrong endpoint parameter type: %.*s", (int)yyjson_get_len(name), yyjson_get_str(name));
                return efail(q, message);
            }
        }
        binding *bound = crucible_generated_arena_alloc(q->arena, sizeof(*bound), q->issue);
        if (!bound) return OOM;
        *bound = (binding){scope, itext(name), item};
        scope = bound;
    }
    bool matched = false;
    TRY(rules(q, get(rule_set, "rules"), scope, &matched, url, headers, properties, 0));
    return matched ? OK : efail(q, "No endpoint rule matched");
}
static status own(query *q, const value *item, const crucible_generated_json **out) {
    *out = NULL;
    if (!item) return OK;
    return crucible_generated_json_own(q->arena, item, out, q->issue);
}
status crucible_generated_endpoint_resolve(crucible_generated_arena *arena, const crucible_generated_json *rule_set, const crucible_generated_json *sources,
    const crucible_generated_json *input, const crucible_generated_json *client, const crucible_generated_json *builtins,
    crucible_generated_endpoint *endpoint, crucible_generated_error *issue) {
    if (!arena || !rule_set || !endpoint) return crucible_generated_fail(issue, crucible_generated_invalid, "Endpoint rules, arena and output are required");
    query q; TRY(crucible_generated_query_begin(arena, &q, issue));
    value *copy = NULL, *properties = NULL;
    TRY(crucible_generated_query_copy(&q, input, &copy));
    crucible_generated_endpoint result = {0};
    TRY(crucible_generated_endpoint_evaluate(&q, rule_set, sources, copy, client, builtins, &result.url, &result.headers, &properties));
    TRY(own(&q, properties, &result.properties));
    *endpoint = result; return OK;
}
status crucible_generated_endpoint_function(crucible_generated_arena *arena, string name, const crucible_generated_json *arguments,
    const crucible_generated_json **output, crucible_generated_error *issue) {
    if (!arena || !output || !yyjson_is_arr((yyjson_val *)arguments)) return crucible_generated_fail(issue, crucible_generated_invalid, "Endpoint function arguments must be an array");
    query q; TRY(crucible_generated_query_begin(arena, &q, issue));
    value *args[4] = {NULL, NULL, NULL, NULL}, *result = NULL;
    size_t count = yyjson_arr_size((yyjson_val *)arguments);
    if (count > 4) return crucible_generated_fail(issue, crucible_generated_invalid, "Invalid endpoint function arity");
    for (size_t i = 0; i < count; ++i) TRY(crucible_generated_query_copy(&q, yyjson_arr_get((yyjson_val *)arguments, i), &args[i]));
    TRY(call(&q, name, args, count, &result));
    return own(&q, norm(result), output);
}
status crucible_generated_endpoint_prefix(crucible_generated_arena *arena, string endpoint, const yyjson_val *description, const value *input,
    string *out, crucible_generated_error *issue) {
    const yyjson_val *parts = get(description, "parts");
    size_t total = yyjson_arr_size((yyjson_val *)parts);
    string *pieces = crucible_generated_arena_alloc(arena, (total + 1) * sizeof(string), issue);
    if (!pieces) return OOM;
    size_t index, count; yyjson_val *part;
    yyjson_arr_foreach((yyjson_val *)parts, index, count, part) {
        if (named(get(part, "kind"), "literal")) { pieces[index] = itext(get(part, "value")); continue; }
        string member = itext(get(part, "member"));
        const value *label = input && yyjson_mut_is_obj((value *)input) ? yyjson_mut_obj_getn((value *)input, member.data, member.length) : NULL;
        if (!is_string(label) || !crucible_generated_host_label(mtext(label), false)) return crucible_generated_fail(issue, crucible_generated_invalid, "Invalid endpoint host label");
        pieces[index] = mtext(label);
    }
    crucible_generated_url url;
    if (!crucible_generated_url_split(endpoint, &url)) return crucible_generated_fail(issue, crucible_generated_invalid, "Invalid endpoint URL");
    pieces[total] = url.host;
    string host; TRY(crucible_generated_concat(arena, pieces, total + 1, &host, issue));
    if (host.length > 253 || !crucible_generated_host_label(host, true)) return crucible_generated_fail(issue, crucible_generated_invalid, "Invalid expanded endpoint hostname");
    string rebuilt[] = {url.scheme, VIEW("://"), host, VIEW(url.port.data ? ":" : ""), url.port, url.path, VIEW(url.query.data ? "?" : ""), url.query};
    return crucible_generated_concat(arena, rebuilt, sizeof(rebuilt) / sizeof(rebuilt[0]), out, issue);
}
