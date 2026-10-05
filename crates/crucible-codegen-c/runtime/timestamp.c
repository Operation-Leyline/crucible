#include "internal.h"
#include <inttypes.h>
#include <stdio.h>
#include <string.h>

static const char *months[] = {"Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"};
static const char *weekdays[] = {"Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"};
static const unsigned month_start[] = {0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334, 365};
static bool leap(unsigned year) { return year % 4 == 0 && (year % 100 != 0 || year % 400 == 0); }
static int64_t year_days(unsigned year) { int64_t y = (int64_t)year - 1; return y * 365 + y / 4 - y / 100 + y / 400 - 719162; }
static bool natural(const char *text, size_t length, unsigned *output) {
    size_t i; unsigned value = 0;
    for (i = 0; i < length; ++i) { unsigned digit = (unsigned)(text[i] - '0'); if (digit > 9) return false; value = value * 10 + digit; }
    *output = value; return true;
}
static bool utc(unsigned year, unsigned month, unsigned day, unsigned hour, unsigned minute, unsigned second, int64_t *output) {
    unsigned days;
    if (year < 1 || year > 9999 || month < 1 || month > 12 || hour > 23 || minute > 59 || second > 59) return false;
    days = month_start[month] - month_start[month - 1] + (month == 2 && leap(year) ? 1U : 0U);
    if (day < 1 || day > days) return false;
    *output = (year_days(year) + month_start[month - 1] + (month > 2 && leap(year) ? 1 : 0) + day - 1) * 86400 + hour * 3600 + minute * 60 + second;
    return true;
}
typedef struct { unsigned year, month, day, hour, minute, second, weekday; } parts;
static bool split(int64_t seconds, parts *output) {
    int64_t days, remaining, day_of_year;
    unsigned low = 1, high = 10000, month = 1;
    if (seconds < -62135596800LL || seconds > 253402300799LL) return false;
    days = seconds / 86400; remaining = seconds % 86400;
    if (remaining < 0) { days--; remaining += 86400; }
    while (low + 1 < high) { unsigned middle = low + (high - low) / 2; if (year_days(middle) <= days) low = middle; else high = middle; }
    day_of_year = days - year_days(low);
    while (month < 12 && day_of_year >= (int64_t)month_start[month] + (month >= 2 && leap(low) ? 1 : 0)) month++;
    output->year = low; output->month = month;
    output->day = (unsigned)(day_of_year - month_start[month - 1] - (month > 2 && leap(low) ? 1 : 0) + 1);
    output->hour = (unsigned)(remaining / 3600); output->minute = (unsigned)((remaining % 3600) / 60); output->second = (unsigned)(remaining % 60);
    output->weekday = (unsigned)(((days + 4) % 7 + 7) % 7);
    return true;
}
static void fraction(uint32_t nanos, char output[10]) {
    size_t length = 9;
    (void)snprintf(output, 10, "%09" PRIu32, nanos);
    while (length > 0 && output[length - 1] == '0') output[--length] = '\0';
}
crucible_generated_status crucible_generated_timestamp_epoch(const crucible_generated_json *value, crucible_generated_timestamp *output, crucible_generated_error *error) {
    char normalized[64], whole[64];
    const char *digits;
    size_t length, count;
    bool negative;
    unsigned nanos = 0;
    int64_t seconds;
    crucible_generated_status status;
    if (!output) return crucible_generated_fail(error, crucible_generated_invalid, "Output is required");
    status = crucible_generated_number_scaled(value, 9, normalized, error);
    if (status != crucible_generated_ok) return status;
    negative = normalized[0] == '-'; digits = normalized + (negative ? 1 : 0); length = strlen(digits);
    count = length > 9 ? length - 9 : 0;
    if (count) {
        if (negative) whole[0] = '-';
        memcpy(whole + (negative ? 1 : 0), digits, count); whole[count + (negative ? 1 : 0)] = '\0';
    } else { whole[0] = '0'; whole[1] = '\0'; }
    status = crucible_generated_integer_text(whole, INT64_MIN, INT64_MAX, &seconds, error);
    if (status != crucible_generated_ok) return status;
    (void)natural(digits + count, length - count, &nanos);
    if (negative && nanos) { if (seconds == INT64_MIN) return crucible_generated_fail(error, crucible_generated_invalid, "Timestamp out of range"); seconds--; nanos = 1000000000U - nanos; }
    output->seconds = seconds; output->nanoseconds = nanos; return crucible_generated_ok;
}
crucible_generated_status crucible_generated_timestamp_epoch_text(crucible_generated_timestamp value, char output[40], crucible_generated_error *error) {
    char tail[10];
    if (!output || value.nanoseconds >= 1000000000U) return crucible_generated_fail(error, crucible_generated_invalid, "Invalid timestamp");
    if (!value.nanoseconds) (void)snprintf(output, 40, "%" PRId64, value.seconds);
    else if (value.seconds >= 0) { fraction(value.nanoseconds, tail); (void)snprintf(output, 40, "%" PRId64 ".%s", value.seconds, tail); }
    else { fraction(1000000000U - value.nanoseconds, tail); (void)snprintf(output, 40, "-%" PRId64 ".%s", -(value.seconds + 1), tail); }
    return crucible_generated_ok;
}
crucible_generated_status crucible_generated_timestamp_text(crucible_generated_timestamp value, char output[40], crucible_generated_error *error) {
    parts p; char tail[10];
    if (!output || value.nanoseconds >= 1000000000U || !split(value.seconds, &p)) return crucible_generated_fail(error, crucible_generated_invalid, "Invalid text timestamp");
    (void)snprintf(output, 40, "%04u-%02u-%02uT%02u:%02u:%02u", p.year, p.month, p.day, p.hour, p.minute, p.second);
    if (value.nanoseconds) { fraction(value.nanoseconds, tail); strcat(output, "."); strcat(output, tail); }
    strcat(output, "Z"); return crucible_generated_ok;
}
crucible_generated_status crucible_generated_timestamp_http_text(crucible_generated_timestamp value, char output[30], crucible_generated_error *error) {
    parts p;
    if (!output || value.nanoseconds >= 1000000000U || !split(value.seconds, &p)) return crucible_generated_fail(error, crucible_generated_invalid, "Invalid HTTP timestamp");
    (void)snprintf(output, 30, "%s, %02u %s %04u %02u:%02u:%02u GMT", weekdays[p.weekday], p.day, months[p.month - 1], p.year, p.hour, p.minute, p.second);
    return crucible_generated_ok;
}
crucible_generated_status crucible_generated_timestamp_parse(crucible_generated_string text, crucible_generated_timestamp *output, crucible_generated_error *error) {
    unsigned year, month, day, hour, minute, second, nanos = 0;
    size_t at = 19;
    int64_t seconds;
    parts verified;
    if (!output || !text.data || text.length < 20 || text.data[4] != '-' || text.data[7] != '-' || text.data[10] != 'T' || text.data[13] != ':' || text.data[16] != ':') goto invalid;
    if (!natural(text.data, 4, &year) || !natural(text.data + 5, 2, &month) || !natural(text.data + 8, 2, &day) || !natural(text.data + 11, 2, &hour) || !natural(text.data + 14, 2, &minute) || !natural(text.data + 17, 2, &second) || !utc(year, month, day, hour, minute, second, &seconds)) goto invalid;
    if (text.data[at] == '.') {
        size_t start = ++at, length;
        while (at < text.length && text.data[at] >= '0' && text.data[at] <= '9') at++;
        length = at - start;
        if (length == 0 || length > 9 || !natural(text.data + start, length, &nanos)) goto invalid;
        while (length++ < 9) nanos *= 10;
    }
    if (at + 1 == text.length && text.data[at] == 'Z') {}
    else if (at + 6 == text.length && (text.data[at] == '+' || text.data[at] == '-') && text.data[at + 3] == ':') {
        unsigned hours, minutes; int64_t offset;
        if (!natural(text.data + at + 1, 2, &hours) || !natural(text.data + at + 4, 2, &minutes) || hours > 23 || minutes > 59) goto invalid;
        offset = (int64_t)(hours * 3600 + minutes * 60);
        seconds += text.data[at] == '-' ? offset : -offset;
    } else goto invalid;
    if (!split(seconds, &verified)) goto invalid;
    output->seconds = seconds; output->nanoseconds = nanos; return crucible_generated_ok;
invalid:
    return crucible_generated_fail(error, crucible_generated_invalid, "Invalid date-time");
}
crucible_generated_status crucible_generated_timestamp_http_parse(crucible_generated_string text, crucible_generated_timestamp *output, crucible_generated_error *error) {
    unsigned year, month, day, hour, minute, second;
    char canonical[30]; int64_t seconds;
    if (!output || !text.data || text.length != 29 || memcmp(text.data + 3, ", ", 2) || text.data[7] != ' ' || text.data[11] != ' ' || text.data[16] != ' ' || text.data[19] != ':' || text.data[22] != ':' || memcmp(text.data + 25, " GMT", 4)) goto invalid;
    for (month = 1; month <= 12; ++month) if (memcmp(months[month - 1], text.data + 8, 3) == 0) break;
    if (!natural(text.data + 12, 4, &year) || !natural(text.data + 5, 2, &day) || !natural(text.data + 17, 2, &hour) || !natural(text.data + 20, 2, &minute) || !natural(text.data + 23, 2, &second) || !utc(year, month, day, hour, minute, second, &seconds)) goto invalid;
    output->seconds = seconds; output->nanoseconds = 0;
    if (crucible_generated_timestamp_http_text(*output, canonical, error) != crucible_generated_ok || memcmp(canonical, text.data, 29)) goto invalid;
    return crucible_generated_ok;
invalid:
    return crucible_generated_fail(error, crucible_generated_invalid, "Invalid HTTP date");
}
