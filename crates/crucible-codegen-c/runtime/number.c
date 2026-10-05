#include "internal.h"
#include <math.h>
#include <string.h>

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
