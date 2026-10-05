#include "internal.h"
#include <math.h>
#include <string.h>
#include <stdlib.h>

crucible_generated_status crucible_generated_number_scaled(const yyjson_val *value, unsigned places, char output[64], crucible_generated_error *error) {
    char native[32];
    const char *text;
    size_t length, at = 0, mantissa, end, digits = 0, before = 0, first = SIZE_MAX, written = 0;
    bool negative, fraction = false, exponent_negative = false;
    int exponent = 0;
    int64_t point;
    if (!value || !output || places > 20) return crucible_generated_fail(error, crucible_generated_invalid, "Invalid exact number arguments");
    if (yyjson_is_raw(value)) { text = yyjson_get_raw(value); length = yyjson_get_len(value); }
    else if (yyjson_is_num(value)) {
        char *last = yyjson_write_number(value, native);
        if (!last) return crucible_generated_fail(error, crucible_generated_invalid, "Nonfinite number");
        text = native; length = (size_t)(last - native);
    } else return crucible_generated_fail(error, crucible_generated_invalid, "Expected number");
    if (length == 0) return crucible_generated_fail(error, crucible_generated_invalid, "Empty number");
    negative = text[0] == '-';
    if (negative) at++;
    mantissa = at;
    while (at < length && text[at] != 'e' && text[at] != 'E') {
        char c = text[at++];
        if (c == '.') { if (fraction) return crucible_generated_fail(error, crucible_generated_invalid, "Invalid number"); fraction = true; continue; }
        if (c < '0' || c > '9') return crucible_generated_fail(error, crucible_generated_invalid, "Invalid number");
        if (c != '0' && first == SIZE_MAX) first = digits;
        digits++;
        if (!fraction) before++;
        if (digits > 1000000) return crucible_generated_fail(error, crucible_generated_invalid, "Number precision limit");
    }
    end = at;
    if (at < length) {
        at++;
        if (at < length && (text[at] == '+' || text[at] == '-')) { exponent_negative = text[at] == '-'; at++; }
        if (at == length) return crucible_generated_fail(error, crucible_generated_invalid, "Invalid exponent");
        while (at < length) {
            char c = text[at++];
            if (c < '0' || c > '9') return crucible_generated_fail(error, crucible_generated_invalid, "Invalid exponent");
            exponent = exponent * 10 + c - '0';
            if (exponent > 10000) return crucible_generated_fail(error, crucible_generated_invalid, "Number exponent limit");
        }
    }
    if (digits == 0) return crucible_generated_fail(error, crucible_generated_invalid, "Invalid number");
    if (first == SIZE_MAX) { output[0] = '0'; output[1] = '\0'; return crucible_generated_ok; }
    point = (int64_t)before + (exponent_negative ? -exponent : exponent) + places;
    if (point <= (int64_t)first) return crucible_generated_fail(error, crucible_generated_invalid, "Number has excess fractional precision");
    if (point - (int64_t)first > 61) return crucible_generated_fail(error, crucible_generated_invalid, "Number out of range");
    if (negative) output[written++] = '-';
    digits = 0;
    for (at = mantissa; at < end; ++at) {
        char c = text[at];
        if (c == '.') continue;
        if ((int64_t)digits >= point && c != '0') return crucible_generated_fail(error, crucible_generated_invalid, "Number has excess fractional precision");
        if (digits >= first && (int64_t)digits < point) output[written++] = c;
        digits++;
    }
    while ((int64_t)digits < point) { output[written++] = '0'; digits++; }
    output[written] = '\0';
    return crucible_generated_ok;
}
crucible_generated_status crucible_generated_integer_text(const char *text, int64_t minimum, int64_t maximum, int64_t *output, crucible_generated_error *error) {
    bool negative;
    uint64_t magnitude = 0, limit;
    int64_t result;
    size_t at = 0;
    if (!text || !output || minimum > maximum) return crucible_generated_fail(error, crucible_generated_invalid, "Invalid integer arguments");
    negative = text[0] == '-';
    if (negative) at++;
    limit = negative ? (uint64_t)INT64_MAX + 1 : (uint64_t)INT64_MAX;
    if (!text[at]) return crucible_generated_fail(error, crucible_generated_invalid, "Invalid integer");
    for (; text[at]; ++at) {
        unsigned digit = (unsigned)(text[at] - '0');
        if (digit > 9 || magnitude > (limit - digit) / 10) return crucible_generated_fail(error, crucible_generated_invalid, "Integer out of range");
        magnitude = magnitude * 10 + digit;
    }
    result = negative ? magnitude == (uint64_t)INT64_MAX + 1 ? INT64_MIN : -(int64_t)magnitude : (int64_t)magnitude;
    if (result < minimum || result > maximum) return crucible_generated_fail(error, crucible_generated_invalid, "Integer out of range");
    *output = result;
    return crucible_generated_ok;
}
crucible_generated_status crucible_generated_json_integer(const crucible_generated_json *value, int64_t minimum, int64_t maximum, int64_t *output, crucible_generated_error *error) {
    char normalized[64];
    crucible_generated_status status = crucible_generated_number_scaled(value, 0, normalized, error);
    return status == crucible_generated_ok ? crucible_generated_integer_text(normalized, minimum, maximum, output, error) : status;
}
crucible_generated_status crucible_generated_json_double(const crucible_generated_json *value, double *output, crucible_generated_error *error) {
    yyjson_val parsed;
    const yyjson_val *number = value;
    if (!value || !output) return crucible_generated_fail(error, crucible_generated_invalid, "Invalid number arguments");
    if (yyjson_is_raw(value)) {
        const char *text = yyjson_get_raw(value);
        const char *end = yyjson_read_number(text, &parsed, 0, NULL, NULL);
        if (!end || (size_t)(end - text) != yyjson_get_len(value)) return crucible_generated_fail(error, crucible_generated_invalid, "Number out of range");
        number = &parsed;
    }
    if (!yyjson_is_num(number)) return crucible_generated_fail(error, crucible_generated_invalid, "Expected number");
    *output = yyjson_get_num(number);
    return isfinite(*output) ? crucible_generated_ok : crucible_generated_fail(error, crucible_generated_invalid, "Nonfinite number");
}

/* Canonical coefficient/exponent text never expands a large exponent into zeroes. */
crucible_generated_status crucible_generated_number_canonical(const yyjson_val *value, char **output, bool *integral, crucible_generated_error *error) {
    char native[32]; const char *text; size_t length;
    if (yyjson_is_raw(value)) { text = yyjson_get_raw(value); length = yyjson_get_len(value); }
    else if (yyjson_is_num(value)) { char *end = yyjson_write_number(value, native); if (!end) return crucible_generated_fail(error, crucible_generated_invalid, "Nonfinite number"); text = native; length = (size_t)(end - native); }
    else return crucible_generated_fail(error, crucible_generated_invalid, "Expected exact number");
    if (length > (SIZE_MAX - 8) / 3) return crucible_generated_fail(error, crucible_generated_out_of_memory, "Number size overflow");
    char *digits = malloc(length + 2), *exponent = malloc(length + 3), *result = NULL;
    if (!digits || !exponent) { free(digits); free(exponent); return crucible_generated_fail(error, crucible_generated_out_of_memory, "Number allocation failed"); }
    bool negative = length && text[0] == '-', fraction = false, exponent_negative = false;
    size_t count = 0, fractions = 0, at = negative ? 1 : 0;
    while (at < length && text[at] != 'e' && text[at] != 'E') { char ch = text[at++]; if (ch == '.') { fraction = true; continue; } digits[count++] = ch; if (fraction) ++fractions; }
    size_t first = 0; while (first < count && digits[first] == '0') ++first;
    size_t last = count; while (last > first && digits[last - 1] == '0') --last;
    size_t exp_count = 0;
    if (at < length) { ++at; if (at < length && (text[at] == '-' || text[at] == '+')) exponent_negative = text[at++] == '-'; while (at < length && text[at] == '0') ++at; while (at < length) exponent[exp_count++] = text[at++]; }
    if (!exp_count) { exponent[exp_count++] = '0'; exponent_negative = false; }
    size_t trailing = count - last; bool increment = trailing >= fractions; size_t steps = increment ? trailing - fractions : fractions - trailing;
    if (first == count) { first = 0; last = 1; digits[0] = '0'; negative = false; exponent[0] = '0'; exp_count = 1; exponent_negative = false; steps = 0; }
    for (size_t step = 0; step < steps; ++step) {
        bool add = increment != exponent_negative;
        if (!add && exp_count == 1 && exponent[0] == '0') { exponent[0] = '1'; exponent_negative = true; continue; }
        unsigned carry = 1; size_t cursor = exp_count;
        while (cursor && carry) { --cursor; int digit = exponent[cursor] - '0' + (add ? 1 : -1); carry = digit > 9 || digit < 0; exponent[cursor] = (char)('0' + (digit + 10) % 10); }
        if (carry) { memmove(exponent + 1, exponent, exp_count); exponent[0] = '1'; ++exp_count; }
        while (exp_count > 1 && exponent[0] == '0') { memmove(exponent, exponent + 1, --exp_count); }
        if (exp_count == 1 && exponent[0] == '0') exponent_negative = false;
    }
    size_t size = (negative ? 1 : 0) + last - first + 1 + (exponent_negative ? 1 : 0) + exp_count;
    result = malloc(size + 1);
    if (!result) { free(digits); free(exponent); return crucible_generated_fail(error, crucible_generated_out_of_memory, "Number allocation failed"); }
    size_t written = 0; if (negative) result[written++] = '-'; memcpy(result + written, digits + first, last - first); written += last - first; result[written++] = 'e'; if (exponent_negative) result[written++] = '-'; memcpy(result + written, exponent, exp_count); written += exp_count; result[written] = 0;
    *integral = !exponent_negative; *output = result; free(digits); free(exponent); return crucible_generated_ok;
}
static crucible_generated_status equal_json(const yyjson_val *a, const yyjson_val *b, bool *equal, crucible_generated_error *error) {
    *equal = false;
    bool an = yyjson_is_num(a) || yyjson_is_raw(a), bn = yyjson_is_num(b) || yyjson_is_raw(b);
    if (an || bn) {
        if (!an || !bn) return crucible_generated_ok;
        char *left = NULL, *right = NULL; bool integral;
        crucible_generated_status code = crucible_generated_number_canonical(a, &left, &integral, error);
        if (!code) code = crucible_generated_number_canonical(b, &right, &integral, error);
        if (!code) *equal = strcmp(left, right) == 0;
        free(left); free(right); return code;
    }
    if (yyjson_is_arr(a) && yyjson_is_arr(b)) {
        size_t count = yyjson_arr_size(a); if (count != yyjson_arr_size(b)) return crucible_generated_ok;
        for (size_t at = 0; at < count; ++at) { bool item; crucible_generated_status code = equal_json(yyjson_arr_get(a, at), yyjson_arr_get(b, at), &item, error); if (code || !item) return code; }
        *equal = true;
    } else if (yyjson_is_obj(a) && yyjson_is_obj(b)) {
        if (yyjson_obj_size(a) != yyjson_obj_size(b)) return crucible_generated_ok;
        size_t index, count; yyjson_val *key, *value;
        yyjson_obj_foreach(a, index, count, key, value) { const yyjson_val *other = yyjson_obj_getn(b, yyjson_get_str(key), yyjson_get_len(key)); bool item = false; if (!other) return crucible_generated_ok; crucible_generated_status code = equal_json(value, other, &item, error); if (code || !item) return code; }
        *equal = true;
    } else *equal = yyjson_equals(a, b);
    return crucible_generated_ok;
}
crucible_generated_status crucible_generated_json_unique(const yyjson_val *array, crucible_generated_error *error) {
    size_t count = yyjson_arr_size(array);
    for (size_t at = 0; at < count; ++at) for (size_t before = 0; before < at; ++before) {
        bool equal; crucible_generated_status code = equal_json(yyjson_arr_get(array, at), yyjson_arr_get(array, before), &equal, error);
        if (code) return code;
        if (equal) return crucible_generated_fail(error, crucible_generated_invalid, "Set members must be unique");
    }
    return crucible_generated_ok;
}
