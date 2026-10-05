#include "features.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Evaluates the portable JMESPath tree produced by crucible-smithy. Values are mutable
 * JSON owned by one document; a NULL pointer is JSON null. Numbers stay exact decimal
 * text, so comparisons and arithmetic never round through binary floating point. */
typedef crucible_generated_status status;
typedef crucible_generated_string string;
typedef crucible_generated_query query;
typedef yyjson_mut_val value;
#define OK crucible_generated_ok
#define OOM crucible_generated_out_of_memory
#define TRY(call) do { status code_ = (call); if (code_) return code_; } while (0)
#define VIEW(text) crucible_generated_string_view(text)
static status qfail(query *q, const char *message) { return crucible_generated_fail(q->issue, crucible_generated_invalid, message); }
static status nomem(query *q) { return crucible_generated_fail(q->issue, OOM, "JSON allocation failed"); }
static const yyjson_val *get(const yyjson_val *node, const char *key) { return yyjson_obj_get((yyjson_val *)node, key); }
static string itext(const yyjson_val *node) { return (string){yyjson_get_str((yyjson_val *)node), yyjson_get_len((yyjson_val *)node)}; }
static bool named(const yyjson_val *node, const char *name) { return yyjson_is_str((yyjson_val *)node) && crucible_generated_string_equal(itext(node), VIEW(name)); }
static value *norm(value *item) { return item && !yyjson_mut_is_null(item) ? item : NULL; }
static bool is_number(const value *item) { return item && (yyjson_mut_is_num((value *)item) || yyjson_mut_is_raw((value *)item)); }
static bool is_string(const value *item) { return item && yyjson_mut_is_str((value *)item); }
static string mtext(const value *item) { return (string){yyjson_mut_get_str((value *)item), yyjson_mut_get_len((value *)item)}; }

static void free_doc(void *doc) { yyjson_mut_doc_free(doc); }
status crucible_generated_query_begin(crucible_generated_arena *arena, query *q, crucible_generated_error *issue) {
    yyjson_mut_doc *doc = yyjson_mut_doc_new(NULL);
    if (!doc) return crucible_generated_fail(issue, OOM, "JSON allocation failed");
    status code = crucible_generated_arena_own(arena, doc, free_doc, issue);
    if (code) { yyjson_mut_doc_free(doc); return code; }
    *q = (query){doc, arena, issue};
    return OK;
}

/* ---------- exact decimals: (-1)^negative * digits * 10^exponent ---------- */
typedef struct { bool negative; const char *digits; size_t count; int64_t exponent; } decimal;
#define EXPONENT_LIMIT 1000000
static status parse_decimal(query *q, string text, decimal *out) {
    size_t at = 0; bool negative = false;
    if (at < text.length && text.data[at] == '-') { negative = true; ++at; }
    if (at >= text.length) return qfail(q, "Invalid JMESPath number");
    char *digits = crucible_generated_arena_alloc(q->arena, text.length + 1, q->issue);
    if (!digits) return OOM;
    size_t count = 0, fraction = 0; bool point = false, any = false;
    if (text.data[at] == '0' && at + 1 < text.length && text.data[at + 1] >= '0' && text.data[at + 1] <= '9') return qfail(q, "Invalid JMESPath number");
    for (; at < text.length && text.data[at] != 'e' && text.data[at] != 'E'; ++at) {
        char c = text.data[at];
        if (c == '.') { if (point || !any) return qfail(q, "Invalid JMESPath number"); point = true; any = false; continue; }
        if (c < '0' || c > '9') return qfail(q, "Invalid JMESPath number");
        any = true;
        if (count || c != '0') digits[count++] = c;
        if (point) ++fraction;
    }
    if (!any) return qfail(q, "Invalid JMESPath number");
    int64_t exponent = 0;
    if (at < text.length) {
        bool exponent_negative = false; ++at;
        if (at < text.length && (text.data[at] == '+' || text.data[at] == '-')) exponent_negative = text.data[at++] == '-';
        if (at >= text.length) return qfail(q, "Invalid JMESPath number");
        for (; at < text.length; ++at) {
            char c = text.data[at];
            if (c < '0' || c > '9') return qfail(q, "Invalid JMESPath number");
            exponent = exponent * 10 + (c - '0');
            if (exponent > EXPONENT_LIMIT) return qfail(q, "JMESPath arithmetic exponent exceeds supported precision");
        }
        if (exponent_negative) exponent = -exponent;
    }
    exponent -= (int64_t)fraction;
    while (count && digits[count - 1] == '0') { --count; ++exponent; }
    *out = (decimal){count ? negative : false, digits, count, count ? exponent : 0};
    return OK;
}
static bool number_grammar(string text) {
    decimal ignored; crucible_generated_error issue = {0};
    crucible_generated_arena *arena = crucible_generated_arena_new();
    if (!arena) return false;
    query q = {NULL, arena, &issue};
    bool valid = parse_decimal(&q, text, &ignored) == OK;
    crucible_generated_arena_free(arena);
    return valid;
}
static status number_text(query *q, const value *item, string *out) {
    if (yyjson_mut_is_raw((value *)item)) { *out = (string){yyjson_mut_get_raw((value *)item), yyjson_mut_get_len((value *)item)}; return OK; }
    char *buffer = crucible_generated_arena_alloc(q->arena, 40, q->issue);
    if (!buffer) return OOM;
    if (yyjson_mut_is_sint((value *)item)) (void)snprintf(buffer, 40, "%lld", (long long)yyjson_mut_get_sint((value *)item));
    else if (yyjson_mut_is_uint((value *)item)) (void)snprintf(buffer, 40, "%llu", (unsigned long long)yyjson_mut_get_uint((value *)item));
    else {
        /* yyjson_mut_val starts with the same tag and payload as yyjson_val. */
        char *end = yyjson_write_number((const yyjson_val *)(const void *)item, buffer);
        if (!end) return qfail(q, "Nonfinite JMESPath number");
        *end = 0;
    }
    *out = VIEW(buffer); return OK;
}
static status to_decimal(query *q, const value *item, decimal *out) {
    if (!is_number(item)) return qfail(q, "JMESPath expected number");
    string text; TRY(number_text(q, item, &text));
    return parse_decimal(q, text, out);
}
static int compare_magnitude(const decimal *a, const decimal *b) {
    if (!a->count || !b->count) return a->count ? 1 : b->count ? -1 : 0;
    int64_t ea = a->exponent + (int64_t)a->count, eb = b->exponent + (int64_t)b->count;
    if (ea != eb) return ea < eb ? -1 : 1;
    size_t length = a->count > b->count ? a->count : b->count;
    for (size_t i = 0; i < length; ++i) {
        char x = i < a->count ? a->digits[i] : '0', y = i < b->count ? b->digits[i] : '0';
        if (x != y) return x < y ? -1 : 1;
    }
    return 0;
}
static int compare_decimal(const decimal *a, const decimal *b) {
    int sa = !a->count ? 0 : a->negative ? -1 : 1, sb = !b->count ? 0 : b->negative ? -1 : 1;
    if (sa != sb) return sa < sb ? -1 : 1;
    return sa * compare_magnitude(a, b);
}
static status normalize(query *q, const char *digits, size_t count, int64_t exponent, bool negative, decimal *out) {
    size_t first = 0; while (first < count && digits[first] == '0') ++first;
    size_t last = count; while (last > first && digits[last - 1] == '0') { --last; ++exponent; }
    if (exponent > EXPONENT_LIMIT * 2LL || exponent < -EXPONENT_LIMIT * 2LL) return qfail(q, "JMESPath arithmetic exponent exceeds supported precision");
    *out = (decimal){last > first ? negative : false, digits + first, last - first, last > first ? exponent : 0};
    return OK;
}
/* Digits of |a| aligned to exponent base, most significant first, into width columns. */
static void align(const decimal *a, int64_t base, size_t width, char *out) {
    memset(out, '0', width);
    size_t shift = (size_t)(a->exponent - base);
    for (size_t i = 0; i < a->count; ++i) out[width - shift - a->count + i] = a->digits[i];
}
static status add_decimal(query *q, const decimal *a, const decimal *b, decimal *out) {
    if (!a->count) { *out = *b; return OK; }
    if (!b->count) { *out = *a; return OK; }
    int64_t base = a->exponent < b->exponent ? a->exponent : b->exponent;
    int64_t top_a = a->exponent + (int64_t)a->count, top_b = b->exponent + (int64_t)b->count, top = top_a > top_b ? top_a : top_b;
    if (top - base > 4 * EXPONENT_LIMIT) return qfail(q, "JMESPath arithmetic exponent exceeds supported precision");
    size_t width = (size_t)(top - base) + 1;
    char *x = crucible_generated_arena_alloc(q->arena, width, q->issue), *y = crucible_generated_arena_alloc(q->arena, width, q->issue), *r = crucible_generated_arena_alloc(q->arena, width, q->issue);
    if (!x || !y || !r) return OOM;
    align(a, base, width, x); align(b, base, width, y);
    bool negative = a->negative;
    if (a->negative == b->negative) {
        int carry = 0;
        for (size_t i = width; i-- > 0;) { int digit = (x[i] - '0') + (y[i] - '0') + carry; r[i] = (char)('0' + digit % 10); carry = digit / 10; }
    } else {
        if (compare_magnitude(a, b) < 0) { char *swap = x; x = y; y = swap; negative = b->negative; }
        int borrow = 0;
        for (size_t i = width; i-- > 0;) { int digit = (x[i] - '0') - (y[i] - '0') - borrow; borrow = digit < 0; r[i] = (char)('0' + (digit + 10) % 10); }
    }
    return normalize(q, r, width, base, negative, out);
}
static status from_decimal(query *q, const decimal *a, value **out) {
    size_t size = a->count + 64;
    if (a->exponent > 0 && a->exponent <= 40) size += (size_t)a->exponent;
    if (a->exponent < 0 && a->exponent >= -40) size += (size_t)-a->exponent;
    char *buffer = crucible_generated_arena_alloc(q->arena, size, q->issue);
    if (!buffer) return OOM;
    size_t written = 0;
    if (!a->count) buffer[written++] = '0';
    else {
        if (a->negative) buffer[written++] = '-';
        if (a->exponent >= 0 && a->exponent <= 40) {
            memcpy(buffer + written, a->digits, a->count); written += a->count;
            for (int64_t i = 0; i < a->exponent; ++i) buffer[written++] = '0';
        } else if (a->exponent < 0 && a->exponent >= -40) {
            int64_t integer = (int64_t)a->count + a->exponent;
            if (integer <= 0) {
                buffer[written++] = '0'; buffer[written++] = '.';
                for (int64_t i = 0; i < -integer; ++i) buffer[written++] = '0';
                memcpy(buffer + written, a->digits, a->count); written += a->count;
            } else {
                memcpy(buffer + written, a->digits, (size_t)integer); written += (size_t)integer;
                buffer[written++] = '.';
                memcpy(buffer + written, a->digits + integer, a->count - (size_t)integer); written += a->count - (size_t)integer;
            }
        } else {
            memcpy(buffer + written, a->digits, a->count); written += a->count;
            written += (size_t)snprintf(buffer + written, size - written, "e%lld", (long long)a->exponent);
        }
    }
    *out = yyjson_mut_rawncpy(q->doc, buffer, written);
    return *out ? OK : nomem(q);
}
static status integer_value(query *q, int64_t number, value **out) {
    char buffer[32]; int length = snprintf(buffer, sizeof(buffer), "%lld", (long long)number);
    *out = yyjson_mut_rawncpy(q->doc, buffer, (size_t)length);
    return *out ? OK : nomem(q);
}
static status to_integer(query *q, const value *item, int64_t *out) {
    decimal number; TRY(to_decimal(q, item, &number));
    if (number.exponent < 0) return qfail(q, "JMESPath expected integer");
    int64_t result = 0;
    for (size_t i = 0; i < number.count + (size_t)number.exponent; ++i) {
        int digit = i < number.count ? number.digits[i] - '0' : 0;
        if (result > (INT64_MAX - digit) / 10) return qfail(q, "JMESPath integer out of range");
        result = result * 10 + digit;
    }
    *out = number.negative ? -result : result; return OK;
}

/* ---------- values ---------- */
static value *clone(query *q, const value *item) { return item ? yyjson_mut_val_mut_copy(q->doc, (value *)item) : yyjson_mut_null(q->doc); }
static status push(query *q, value *array, const value *item) {
    value *copy = clone(q, item);
    return copy && yyjson_mut_arr_append(array, copy) ? OK : nomem(q);
}
static status put(query *q, value *object, string key, const value *item) {
    value *name = yyjson_mut_strncpy(q->doc, key.data ? key.data : "", key.length), *copy = clone(q, item);
    return name && copy && yyjson_mut_obj_put(object, name, copy) ? OK : nomem(q);
}
static status new_array(query *q, value **out) { *out = yyjson_mut_arr(q->doc); return *out ? OK : nomem(q); }
static status boolean(query *q, bool flag, value **out) { *out = yyjson_mut_bool(q->doc, flag); return *out ? OK : nomem(q); }
static status new_string(query *q, string text, value **out) { *out = yyjson_mut_strncpy(q->doc, text.data ? text.data : "", text.length); return *out ? OK : nomem(q); }
status crucible_generated_query_copy(query *q, const yyjson_val *item, value **out) {
    if (!item || yyjson_is_null((yyjson_val *)item)) { *out = NULL; return OK; }
    *out = yyjson_val_mut_copy(q->doc, (yyjson_val *)item);
    return *out ? OK : nomem(q);
}
bool crucible_generated_query_truth(const value *item) {
    if (!item || yyjson_mut_is_null((value *)item)) return false;
    if (yyjson_mut_is_bool((value *)item)) return yyjson_mut_get_bool((value *)item);
    if (yyjson_mut_is_str((value *)item)) return yyjson_mut_get_len((value *)item) != 0;
    if (yyjson_mut_is_arr((value *)item)) return yyjson_mut_arr_size((value *)item) != 0;
    if (yyjson_mut_is_obj((value *)item)) return yyjson_mut_obj_size((value *)item) != 0;
    return true;
}
status crucible_generated_query_equal(query *q, const value *left, const value *right, bool *equal) {
    left = norm((value *)left); right = norm((value *)right);
    *equal = false;
    if (!left || !right) { *equal = !left && !right; return OK; }
    if (is_number(left) || is_number(right)) {
        if (!is_number(left) || !is_number(right)) return OK;
        decimal a, b; TRY(to_decimal(q, left, &a)); TRY(to_decimal(q, right, &b));
        *equal = compare_decimal(&a, &b) == 0; return OK;
    }
    if (yyjson_mut_is_arr((value *)left) && yyjson_mut_is_arr((value *)right)) {
        size_t count = yyjson_mut_arr_size((value *)left);
        if (count != yyjson_mut_arr_size((value *)right)) return OK;
        for (size_t i = 0; i < count; ++i) {
            bool item; TRY(crucible_generated_query_equal(q, yyjson_mut_arr_get((value *)left, i), yyjson_mut_arr_get((value *)right, i), &item));
            if (!item) return OK;
        }
        *equal = true; return OK;
    }
    if (yyjson_mut_is_obj((value *)left) && yyjson_mut_is_obj((value *)right)) {
        if (yyjson_mut_obj_size((value *)left) != yyjson_mut_obj_size((value *)right)) return OK;
        size_t index, count; value *key, *item;
        yyjson_mut_obj_foreach((value *)left, index, count, key, item) {
            value *other = yyjson_mut_obj_getn((value *)right, yyjson_mut_get_str(key), yyjson_mut_get_len(key));
            bool same; if (!other) return OK;
            TRY(crucible_generated_query_equal(q, item, other, &same));
            if (!same) return OK;
        }
        *equal = true; return OK;
    }
    if (yyjson_mut_is_str((value *)left) && yyjson_mut_is_str((value *)right)) { *equal = crucible_generated_string_equal(mtext(left), mtext(right)); return OK; }
    if (yyjson_mut_is_bool((value *)left) && yyjson_mut_is_bool((value *)right)) { *equal = yyjson_mut_get_bool((value *)left) == yyjson_mut_get_bool((value *)right); return OK; }
    return OK;
}
static int compare_text(string a, string b) {
    size_t shared = a.length < b.length ? a.length : b.length;
    int order = shared ? memcmp(a.data, b.data, shared) : 0;
    return order ? (order < 0 ? -1 : 1) : (a.length < b.length ? -1 : a.length > b.length ? 1 : 0);
}
static status order(query *q, const value *left, const value *right, int *out) {
    if (is_number(left)) {
        decimal a, b; TRY(to_decimal(q, left, &a)); TRY(to_decimal(q, right, &b));
        *out = compare_decimal(&a, &b); return OK;
    }
    if (!is_string(left) || !is_string(right)) return qfail(q, "JMESPath expected string");
    *out = compare_text(mtext(left), mtext(right)); return OK;
}
static status items_of(query *q, const value *array, value ***out, size_t *count) {
    if (!array || !yyjson_mut_is_arr((value *)array)) return qfail(q, "JMESPath expected array");
    *count = yyjson_mut_arr_size((value *)array);
    *out = crucible_generated_arena_alloc(q->arena, (*count ? *count : 1) * sizeof(value *), q->issue);
    if (!*out) return OOM;
    size_t index, maximum; value *item;
    yyjson_mut_arr_foreach((value *)array, index, maximum, item) (*out)[index] = norm(item);
    return OK;
}
/* Stable insertion sort of items by keys; keys must all be numbers or all strings. */
static status sort_items(query *q, value **items, value **keys, size_t count) {
    bool numeric = count && is_number(keys[0]);
    for (size_t i = 0; i < count; ++i) {
        if (numeric ? !is_number(keys[i]) : !is_string(keys[i])) return qfail(q, numeric ? "JMESPath expected number" : "JMESPath expected string");
    }
    for (size_t i = 1; i < count; ++i) {
        value *item = items[i], *key = keys[i]; size_t j = i;
        while (j) {
            int result; TRY(order(q, keys[j - 1], key, &result));
            if (result <= 0) break;
            items[j] = items[j - 1]; keys[j] = keys[j - 1]; --j;
        }
        items[j] = item; keys[j] = key;
    }
    return OK;
}
static status sorted_keys(query *q, const value *object, string **out, size_t *count) {
    if (!object || !yyjson_mut_is_obj((value *)object)) return qfail(q, "JMESPath expected object");
    *count = yyjson_mut_obj_size((value *)object);
    *out = crucible_generated_arena_alloc(q->arena, (*count ? *count : 1) * sizeof(string), q->issue);
    if (!*out) return OOM;
    size_t index, maximum; value *key, *item;
    yyjson_mut_obj_foreach((value *)object, index, maximum, key, item) { (void)item; (*out)[index] = mtext(key); }
    for (size_t i = 1; i < *count; ++i) {
        string key_text = (*out)[i]; size_t j = i;
        while (j && compare_text((*out)[j - 1], key_text) > 0) { (*out)[j] = (*out)[j - 1]; --j; }
        (*out)[j] = key_text;
    }
    return OK;
}
static size_t code_points(string text) {
    size_t count = 0;
    for (size_t i = 0; i < text.length; ++i) if (((unsigned char)text.data[i] & 0xC0) != 0x80) ++count;
    return count;
}

/* ---------- evaluation ---------- */
static status eval(query *q, const yyjson_val *node, value *input, value **out, unsigned depth);
static status child(query *q, const yyjson_val *node, const char *name, value *input, value **out, unsigned depth) {
    return eval(q, get(node, name), input, out, depth + 1);
}
static status reference(query *q, const yyjson_val *node, value *input, value **out, unsigned depth) {
    if (!named(get(node, "kind"), "expressionReference")) return qfail(q, "JMESPath expected expression reference");
    return child(q, node, "expression", input, out, depth);
}
static status function(query *q, const yyjson_val *node, value *input, value **out, unsigned depth) {
    string name = itext(get(node, "function"));
    const yyjson_val *nodes = get(node, "arguments");
    size_t given = yyjson_arr_size((yyjson_val *)nodes), arity = 1;
    static const char *const pairs[] = {"contains", "starts_with", "ends_with", "join", "map", "max_by", "min_by", "sort_by"};
    for (size_t i = 0; i < sizeof(pairs) / sizeof(pairs[0]); ++i) if (crucible_generated_string_equal(name, VIEW(pairs[i]))) arity = 2;
    bool variadic = crucible_generated_string_equal(name, VIEW("merge")) || crucible_generated_string_equal(name, VIEW("not_null"));
    if ((variadic && !given) || (!variadic && given != arity)) return qfail(q, "JMESPath invalid function arity");
    value *args[2] = {NULL, NULL};
#define IS(text) crucible_generated_string_equal(name, VIEW(text))
#define ARG(i) TRY(eval(q, yyjson_arr_get((yyjson_val *)nodes, (i)), input, &args[(i)], depth + 1))
    *out = NULL;
    if (IS("abs") || IS("ceil") || IS("floor")) {
        ARG(0); decimal number; TRY(to_decimal(q, args[0], &number));
        if (IS("abs")) { number.negative = false; return from_decimal(q, &number, out); }
        if (number.exponent >= 0) return from_decimal(q, &number, out);
        int64_t integer = (int64_t)number.count + number.exponent;
        decimal whole = number; whole.count = integer > 0 ? (size_t)integer : 0; whole.exponent = 0;
        TRY(normalize(q, whole.digits, whole.count, 0, number.negative, &whole));
        bool fraction = true; /* normalized digits never end in zero, so a negative exponent is a fraction */
        if (fraction && ((IS("ceil") && !number.negative) || (IS("floor") && number.negative))) {
            decimal one = {number.negative, "1", 1, 0};
            TRY(add_decimal(q, &whole, &one, &whole));
        }
        return from_decimal(q, &whole, out);
    }
    if (IS("sum") || IS("avg")) {
        ARG(0); value **items; size_t count; TRY(items_of(q, args[0], &items, &count));
        decimal total = {false, "", 0, 0};
        for (size_t i = 0; i < count; ++i) { decimal item; TRY(to_decimal(q, items[i], &item)); TRY(add_decimal(q, &total, &item, &total)); }
        if (IS("sum")) return from_decimal(q, &total, out);
        if (!count) return OK;
        if ((uint64_t)count > ((uint64_t)1 << 59)) return qfail(q, "JMESPath average is too large");
        /* Quotient with 35 fractional digits, rounded half away from zero to 34. */
        int64_t shift = total.exponent + 35;
        size_t length = total.count + (shift > 0 ? (size_t)shift : 0);
        if (shift < -(int64_t)total.count) length = 0;
        else if (shift < 0) length = total.count - (size_t)-shift;
        if (length > 4 * EXPONENT_LIMIT) return qfail(q, "JMESPath arithmetic exponent exceeds supported precision");
        char *numerator = crucible_generated_arena_alloc(q->arena, length + 2, q->issue), *quotient = crucible_generated_arena_alloc(q->arena, length + 2, q->issue);
        if (!numerator || !quotient) return OOM;
        for (size_t i = 0; i < length; ++i) numerator[i] = i < total.count ? total.digits[i] : '0';
        uint64_t remainder = 0, divisor = (uint64_t)count;
        for (size_t i = 0; i < length; ++i) {
            remainder = remainder * 10 + (uint64_t)(numerator[i] - '0');
            quotient[i + 1] = (char)('0' + remainder / divisor); remainder %= divisor;
        }
        quotient[0] = '0';
        size_t size = length + 1;
        bool round_up = size > 1 && quotient[size - 1] >= '5';
        --size;
        for (size_t i = size; round_up && i-- > 0;) { if (quotient[i] == '9') quotient[i] = '0'; else { ++quotient[i]; round_up = false; } }
        decimal result; TRY(normalize(q, quotient, size, -34, total.negative, &result));
        return from_decimal(q, &result, out);
    }
    if (IS("contains")) {
        ARG(0); ARG(1);
        if (is_string(args[0])) {
            if (!is_string(args[1])) return qfail(q, "JMESPath expected string");
            string haystack = mtext(args[0]), needle = mtext(args[1]); bool found = needle.length == 0;
            for (size_t i = 0; !found && needle.length <= haystack.length && i <= haystack.length - needle.length; ++i) found = memcmp(haystack.data + i, needle.data, needle.length) == 0;
            return boolean(q, found, out);
        }
        value **items; size_t count; TRY(items_of(q, args[0], &items, &count));
        for (size_t i = 0; i < count; ++i) { bool same; TRY(crucible_generated_query_equal(q, items[i], args[1], &same)); if (same) return boolean(q, true, out); }
        return boolean(q, false, out);
    }
    if (IS("starts_with") || IS("ends_with")) {
        ARG(0); ARG(1);
        if (!is_string(args[0]) || !is_string(args[1])) return qfail(q, "JMESPath expected string");
        string subject = mtext(args[0]), part = mtext(args[1]);
        bool match = part.length <= subject.length && (!part.length || memcmp(IS("starts_with") ? subject.data : subject.data + subject.length - part.length, part.data, part.length) == 0);
        return boolean(q, match, out);
    }
    if (IS("join")) {
        ARG(0); ARG(1);
        if (!is_string(args[0])) return qfail(q, "JMESPath expected string");
        value **items; size_t count; TRY(items_of(q, args[1], &items, &count));
        string *parts = crucible_generated_arena_alloc(q->arena, (count * 2 + 1) * sizeof(string), q->issue);
        if (!parts) return OOM;
        size_t used = 0;
        for (size_t i = 0; i < count; ++i) {
            if (!is_string(items[i])) return qfail(q, "JMESPath expected string");
            if (i) parts[used++] = mtext(args[0]);
            parts[used++] = mtext(items[i]);
        }
        string joined; TRY(crucible_generated_concat(q->arena, parts, used, &joined, q->issue));
        return new_string(q, joined, out);
    }
    if (IS("keys") || IS("values")) {
        ARG(0); string *keys; size_t count; TRY(sorted_keys(q, args[0], &keys, &count));
        TRY(new_array(q, out));
        for (size_t i = 0; i < count; ++i) {
            if (IS("keys")) { value *key; TRY(new_string(q, keys[i], &key)); TRY(push(q, *out, key)); }
            else TRY(push(q, *out, norm(yyjson_mut_obj_getn(args[0], keys[i].data, keys[i].length))));
        }
        return OK;
    }
    if (IS("length")) {
        ARG(0); size_t length;
        if (args[0] && yyjson_mut_is_arr(args[0])) length = yyjson_mut_arr_size(args[0]);
        else if (args[0] && yyjson_mut_is_obj(args[0])) length = yyjson_mut_obj_size(args[0]);
        else if (is_string(args[0])) length = code_points(mtext(args[0]));
        else return qfail(q, "JMESPath expected string");
        return integer_value(q, (int64_t)length, out);
    }
    if (IS("map")) {
        ARG(1); value **items; size_t count; TRY(items_of(q, args[1], &items, &count));
        TRY(new_array(q, out));
        for (size_t i = 0; i < count; ++i) { value *mapped; TRY(reference(q, yyjson_arr_get((yyjson_val *)nodes, 0), items[i], &mapped, depth + 1)); TRY(push(q, *out, norm(mapped))); }
        return OK;
    }
    if (IS("max") || IS("min") || IS("sort")) {
        ARG(0); value **items; size_t count; TRY(items_of(q, args[0], &items, &count));
        value **keys = crucible_generated_arena_alloc(q->arena, (count ? count : 1) * sizeof(value *), q->issue);
        if (!keys) return OOM;
        memcpy(keys, items, count * sizeof(value *));
        TRY(sort_items(q, items, keys, count));
        if (IS("sort")) { TRY(new_array(q, out)); for (size_t i = 0; i < count; ++i) TRY(push(q, *out, items[i])); return OK; }
        if (!count) return OK;
        *out = clone(q, IS("max") ? items[count - 1] : items[0]);
        return *out ? OK : nomem(q);
    }
    if (IS("max_by") || IS("min_by") || IS("sort_by")) {
        ARG(0); value **items; size_t count; TRY(items_of(q, args[0], &items, &count));
        value **keys = crucible_generated_arena_alloc(q->arena, (count ? count : 1) * sizeof(value *), q->issue);
        if (!keys) return OOM;
        for (size_t i = 0; i < count; ++i) { TRY(reference(q, yyjson_arr_get((yyjson_val *)nodes, 1), items[i], &keys[i], depth + 1)); keys[i] = norm(keys[i]); }
        if (!count) { if (IS("sort_by")) return new_array(q, out); return OK; }
        if (IS("sort_by")) { TRY(sort_items(q, items, keys, count)); TRY(new_array(q, out)); for (size_t i = 0; i < count; ++i) TRY(push(q, *out, items[i])); return OK; }
        value **ordered = crucible_generated_arena_alloc(q->arena, count * sizeof(value *), q->issue), **sorted = crucible_generated_arena_alloc(q->arena, count * sizeof(value *), q->issue);
        if (!ordered || !sorted) return OOM;
        memcpy(ordered, keys, count * sizeof(value *)); memcpy(sorted, keys, count * sizeof(value *));
        TRY(sort_items(q, ordered, sorted, count));
        size_t selected = 0;
        for (size_t i = 1; i < count; ++i) {
            int result; TRY(order(q, keys[i], keys[selected], &result));
            if ((IS("max_by") && result > 0) || (IS("min_by") && result < 0)) selected = i;
        }
        *out = clone(q, items[selected]);
        return *out ? OK : nomem(q);
    }
    if (IS("merge")) {
        *out = yyjson_mut_obj(q->doc); if (!*out) return nomem(q);
        for (size_t i = 0; i < given; ++i) {
            value *object; TRY(eval(q, yyjson_arr_get((yyjson_val *)nodes, i), input, &object, depth + 1));
            if (!object || !yyjson_mut_is_obj(object)) return qfail(q, "JMESPath expected object");
            size_t index, maximum; value *key, *item;
            yyjson_mut_obj_foreach(object, index, maximum, key, item) TRY(put(q, *out, mtext(key), norm(item)));
        }
        return OK;
    }
    if (IS("not_null")) {
        for (size_t i = 0; i < given; ++i) { value *item; TRY(eval(q, yyjson_arr_get((yyjson_val *)nodes, i), input, &item, depth + 1)); if (norm(item)) { *out = item; return OK; } }
        return OK;
    }
    if (IS("reverse")) {
        ARG(0);
        if (args[0] && yyjson_mut_is_arr(args[0])) {
            value **items; size_t count; TRY(items_of(q, args[0], &items, &count));
            TRY(new_array(q, out));
            for (size_t i = count; i-- > 0;) TRY(push(q, *out, items[i]));
            return OK;
        }
        if (!is_string(args[0])) return qfail(q, "JMESPath expected string");
        string subject = mtext(args[0]);
        char *reversed = crucible_generated_arena_alloc(q->arena, subject.length + 1, q->issue);
        if (!reversed) return OOM;
        size_t end = subject.length, written = 0;
        while (end) {
            size_t start = end - 1;
            while (start && ((unsigned char)subject.data[start] & 0xC0) == 0x80) --start;
            memcpy(reversed + written, subject.data + start, end - start); written += end - start; end = start;
        }
        return new_string(q, (string){reversed, written}, out);
    }
    if (IS("to_array")) {
        ARG(0);
        if (args[0] && yyjson_mut_is_arr(args[0])) { *out = args[0]; return OK; }
        TRY(new_array(q, out)); return push(q, *out, args[0]);
    }
    if (IS("to_number")) {
        ARG(0);
        if (is_number(args[0])) { *out = args[0]; return OK; }
        if (is_string(args[0]) && number_grammar(mtext(args[0]))) { *out = yyjson_mut_rawncpy(q->doc, mtext(args[0]).data, mtext(args[0]).length); return *out ? OK : nomem(q); }
        return OK;
    }
    if (IS("to_string")) {
        ARG(0);
        if (is_string(args[0])) { *out = args[0]; return OK; }
        if (!args[0]) return new_string(q, VIEW("null"), out);
        size_t length = 0; char *written = yyjson_mut_val_write(args[0], 0, &length);
        if (!written) return nomem(q);
        status code = new_string(q, (string){written, length}, out);
        free(written); return code;
    }
    if (IS("type")) {
        ARG(0);
        const char *kind = !args[0] ? "null" : yyjson_mut_is_bool(args[0]) ? "boolean" : is_string(args[0]) ? "string" : is_number(args[0]) ? "number" : yyjson_mut_is_arr(args[0]) ? "array" : "object";
        return new_string(q, VIEW(kind), out);
    }
#undef IS
#undef ARG
    return qfail(q, "Unknown compiled JMESPath function");
}
static status slice(query *q, const yyjson_val *node, value *input, value **out) {
    *out = NULL;
    if (!input || !yyjson_mut_is_arr(input)) return OK;
    value **items; size_t count; TRY(items_of(q, input, &items, &count));
    int64_t step = 1, length = (int64_t)count, bounds[2];
    if (get(node, "step") && !yyjson_is_null((yyjson_val *)get(node, "step"))) { value *v; TRY(crucible_generated_query_copy(q, get(node, "step"), &v)); TRY(to_integer(q, v, &step)); }
    if (step == 0) return qfail(q, "Zero slice step");
    const char *names[] = {"start", "stop"};
    int64_t fallback[] = {step < 0 ? length - 1 : 0, step < 0 ? -1 : length};
    for (int i = 0; i < 2; ++i) {
        const yyjson_val *bound = get(node, names[i]);
        if (!bound || yyjson_is_null((yyjson_val *)bound)) { bounds[i] = fallback[i]; continue; }
        value *v; int64_t number; TRY(crucible_generated_query_copy(q, bound, &v)); TRY(to_integer(q, v, &number));
        if (number < 0) number += length;
        if (step > 0) bounds[i] = number < 0 ? 0 : number > length ? length : number;
        else bounds[i] = number < -1 ? -1 : number > length - 1 ? length - 1 : number;
    }
    TRY(new_array(q, out));
    for (int64_t i = bounds[0]; (step > 0 && i < bounds[1]) || (step < 0 && i > bounds[1]); i += step) TRY(push(q, *out, items[i]));
    return OK;
}
static status eval(query *q, const yyjson_val *node, value *input, value **out, unsigned depth) {
    *out = NULL;
    input = norm(input);
    if (depth > 255) return qfail(q, "JMESPath nesting limit");
    const yyjson_val *kind = get(node, "kind");
    if (!yyjson_is_str((yyjson_val *)kind)) return qfail(q, "JMESPath expected object");
#define K(text) named(kind, text)
    if (K("identity")) { *out = input; return OK; }
    if (K("literal")) return crucible_generated_query_copy(q, get(node, "value"), out);
    if (K("field")) {
        if (input && yyjson_mut_is_obj(input)) { string name = itext(get(node, "name")); *out = norm(yyjson_mut_obj_getn(input, name.data, name.length)); }
        return OK;
    }
    if (K("index")) {
        if (!input || !yyjson_mut_is_arr(input)) return OK;
        value *v; int64_t index; TRY(crucible_generated_query_copy(q, get(node, "index"), &v)); TRY(to_integer(q, v, &index));
        int64_t length = (int64_t)yyjson_mut_arr_size(input);
        if (index < 0) index += length;
        if (index >= 0 && index < length) *out = norm(yyjson_mut_arr_get(input, (size_t)index));
        return OK;
    }
    if (K("subexpression")) { value *left; TRY(child(q, node, "left", input, &left, depth)); return child(q, node, "right", left, out, depth); }
    if (K("projection")) {
        value *left; TRY(child(q, node, "left", input, &left, depth));
        if (!left || !yyjson_mut_is_arr(left)) return OK;
        value **items; size_t count; TRY(items_of(q, left, &items, &count));
        TRY(new_array(q, out));
        for (size_t i = 0; i < count; ++i) { value *item; TRY(child(q, node, "right", items[i], &item, depth)); if (norm(item)) TRY(push(q, *out, item)); }
        return OK;
    }
    if (K("flatten")) {
        value *list; TRY(child(q, node, "expression", input, &list, depth));
        if (!list || !yyjson_mut_is_arr(list)) return OK;
        value **items; size_t count; TRY(items_of(q, list, &items, &count));
        TRY(new_array(q, out));
        for (size_t i = 0; i < count; ++i) {
            if (items[i] && yyjson_mut_is_arr(items[i])) {
                size_t index, maximum; value *nested;
                yyjson_mut_arr_foreach(items[i], index, maximum, nested) TRY(push(q, *out, norm(nested)));
            } else TRY(push(q, *out, items[i]));
        }
        return OK;
    }
    if (K("objectValues")) {
        value *object; TRY(child(q, node, "expression", input, &object, depth));
        if (!object || !yyjson_mut_is_obj(object)) return OK;
        string *keys; size_t count; TRY(sorted_keys(q, object, &keys, &count));
        TRY(new_array(q, out));
        for (size_t i = 0; i < count; ++i) TRY(push(q, *out, norm(yyjson_mut_obj_getn(object, keys[i].data, keys[i].length))));
        return OK;
    }
    if (K("multiList")) {
        if (!input) return OK;
        TRY(new_array(q, out));
        size_t index, count; yyjson_val *item;
        yyjson_arr_foreach((yyjson_val *)get(node, "expressions"), index, count, item) { value *result; TRY(eval(q, item, input, &result, depth + 1)); TRY(push(q, *out, result)); }
        return OK;
    }
    if (K("multiHash")) {
        if (!input) return OK;
        *out = yyjson_mut_obj(q->doc); if (!*out) return nomem(q);
        size_t index, count; yyjson_val *entry;
        yyjson_arr_foreach((yyjson_val *)get(node, "entries"), index, count, entry) { value *result; TRY(eval(q, get(entry, "value"), input, &result, depth + 1)); TRY(put(q, *out, itext(get(entry, "key")), result)); }
        return OK;
    }
    if (K("and") || K("or")) {
        value *left; TRY(child(q, node, "left", input, &left, depth));
        if (crucible_generated_query_truth(left) == K("and")) return child(q, node, "right", input, out, depth);
        *out = left; return OK;
    }
    if (K("not")) { value *inner; TRY(child(q, node, "expression", input, &inner, depth)); return boolean(q, !crucible_generated_query_truth(inner), out); }
    if (K("condition")) {
        value *predicate; TRY(child(q, node, "predicate", input, &predicate, depth));
        if (crucible_generated_query_truth(predicate)) return child(q, node, "then", input, out, depth);
        return OK;
    }
    if (K("compare")) {
        value *left, *right; TRY(child(q, node, "left", input, &left, depth)); TRY(child(q, node, "right", input, &right, depth));
        const yyjson_val *comparator = get(node, "comparator");
        if (named(comparator, "equal") || named(comparator, "notEqual")) {
            bool same; TRY(crucible_generated_query_equal(q, left, right, &same));
            return boolean(q, named(comparator, "equal") ? same : !same, out);
        }
        if (!is_number(left) || !is_number(right)) return OK;
        int result; TRY(order(q, left, right, &result));
        if (named(comparator, "lessThan")) return boolean(q, result < 0, out);
        if (named(comparator, "lessThanEqual")) return boolean(q, result <= 0, out);
        if (named(comparator, "greaterThan")) return boolean(q, result > 0, out);
        if (named(comparator, "greaterThanEqual")) return boolean(q, result >= 0, out);
        return qfail(q, "Unknown JMESPath comparator");
    }
    if (K("slice")) return slice(q, node, input, out);
    if (K("function")) return function(q, node, input, out, depth);
#undef K
    return qfail(q, "Unknown compiled JMESPath node");
}
status crucible_generated_query_eval(query *q, const yyjson_val *node, value *input, value **out) {
    value *result = NULL;
    TRY(eval(q, node, input, &result, 0));
    *out = norm(result); return OK;
}

/* ---------- wire JSON to the query model ---------- */
static const yyjson_val *shape(const yyjson_val *schema, const yyjson_val *id) {
    string name = itext(id);
    return yyjson_obj_getn((yyjson_val *)get(schema, "shapes"), name.data, name.length);
}
static status convert(query *q, const yyjson_val *schema, const yyjson_val *target, const yyjson_val *item, value **out, unsigned depth) {
    *out = NULL;
    if (!item || yyjson_is_null((yyjson_val *)item)) return OK;
    if (depth > 255) return qfail(q, "Query model nesting limit");
    const yyjson_val *type = shape(schema, target), *kind = get(type, "kind");
    if (named(kind, "timestamp") && yyjson_is_str((yyjson_val *)item)) {
        crucible_generated_timestamp instant; char epoch[40];
        string wire = itext(item);
        if (crucible_generated_timestamp_parse(wire, &instant, NULL) && crucible_generated_timestamp_http_parse(wire, &instant, NULL)) return qfail(q, "Invalid timestamp in query model");
        TRY(crucible_generated_timestamp_epoch_text(instant, epoch, q->issue));
        *out = yyjson_mut_rawcpy(q->doc, epoch);
        return *out ? OK : nomem(q);
    }
    if ((named(kind, "structure") || named(kind, "union")) && yyjson_is_obj((yyjson_val *)item)) {
        *out = yyjson_mut_obj(q->doc); if (!*out) return nomem(q);
        size_t index, count; yyjson_val *key, *member;
        yyjson_obj_foreach((yyjson_val *)get(type, "members"), index, count, key, member) {
            const yyjson_val *field = yyjson_obj_getn((yyjson_val *)item, yyjson_get_str(key), yyjson_get_len(key));
            if (!field) continue;
            value *converted; TRY(convert(q, schema, get(member, "target"), field, &converted, depth + 1));
            TRY(put(q, *out, itext(key), converted));
        }
        return OK;
    }
    if (named(kind, "list") && yyjson_is_arr((yyjson_val *)item)) {
        TRY(new_array(q, out));
        size_t index, count; yyjson_val *element;
        yyjson_arr_foreach((yyjson_val *)item, index, count, element) { value *converted; TRY(convert(q, schema, get(get(type, "member"), "target"), element, &converted, depth + 1)); TRY(push(q, *out, converted)); }
        return OK;
    }
    if (named(kind, "map") && yyjson_is_obj((yyjson_val *)item)) {
        *out = yyjson_mut_obj(q->doc); if (!*out) return nomem(q);
        size_t index, count; yyjson_val *key, *element;
        yyjson_obj_foreach((yyjson_val *)item, index, count, key, element) { value *converted; TRY(convert(q, schema, get(get(type, "value"), "target"), element, &converted, depth + 1)); TRY(put(q, *out, itext(key), converted)); }
        return OK;
    }
    return crucible_generated_query_copy(q, item, out);
}
status crucible_generated_query_model(query *q, const yyjson_val *schema, const yyjson_val *target, const yyjson_val *item, value **out) {
    if (!yyjson_is_str((yyjson_val *)target)) { *out = yyjson_mut_obj(q->doc); return *out ? OK : nomem(q); }
    return convert(q, schema, target, item, out, 0);
}
