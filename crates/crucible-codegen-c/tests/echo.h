/* Shared helpers for the feature tests: PREFIX names the generated package prefix. The
 * loopback feature server echoes each request as base64 JSON in an x-echo header. */
#ifndef ECHO_TEST_H
#define ECHO_TEST_H
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define JOIN_(a, b) a##b
#define JOIN(a, b) JOIN_(a, b)
#define P(name) JOIN(JOIN(PREFIX, _), name)

static void check(bool condition, const char *message) {
    if (!condition) { fprintf(stderr, "C feature check failed: %s\n", message); exit(1); }
}
static void good(P(status) status, const P(error) *error, const char *context) {
    if (status != JOIN(PREFIX, _ok)) { fprintf(stderr, "C feature call failed (%s): %d %s\n", context, (int)status, error->message); exit(1); }
}
static bool equal(P(string) value, const char *expected) { return P(string_equal)(value, P(string_view)(expected)); }
static bool contains(P(string) value, const char *part) {
    size_t length = strlen(part);
    if (length > value.length) return false;
    for (size_t i = 0; i + length <= value.length; ++i) if (memcmp(value.data + i, part, length) == 0) return true;
    return false;
}
static bool message_contains(const P(error) *error, const char *part) { return contains(P(string_view)(error->message), part); }
static const char *environment(const char *name) { const char *value = getenv(name); check(value != NULL, name); return value; }
static int base64_value(char c) {
    if (c >= 'A' && c <= 'Z') return c - 'A';
    if (c >= 'a' && c <= 'z') return c - 'a' + 26;
    if (c >= '0' && c <= '9') return c - '0' + 52;
    return c == '+' ? 62 : c == '/' ? 63 : -1;
}
/* The echoed request as JSON {method, path, body, headers}; lives in arena. */
static const P(json) *echo(P(arena) *arena, const P(response) *response) {
    P(string) encoded = {0};
    check(P(header_get)(response->headers, P(string_view)("x-echo"), &encoded), "x-echo header");
    char *decoded = P(arena_alloc)(arena, encoded.length + 1, NULL);
    check(decoded != NULL, "echo allocation");
    size_t written = 0; unsigned bits = 0; int count = 0;
    for (size_t i = 0; i < encoded.length && encoded.data[i] != '='; ++i) {
        int digit = base64_value(encoded.data[i]); check(digit >= 0, "echo base64");
        bits = (bits << 6) | (unsigned)digit; count += 6;
        if (count >= 8) { count -= 8; decoded[written++] = (char)((bits >> count) & 255); }
    }
    const P(json) *value = NULL; P(error) error = {0};
    good(P(json_parse)(arena, (P(string)){decoded, written}, &value, &error), &error, "echo JSON");
    return value;
}
static P(string) echo_text(const P(json) *value, const char *name) {
    P(string) text = {0}; P(error) error = {0};
    const P(json) *item = P(json_get)(value, P(string_view)(name));
    if (!item) return text;
    good(P(json_text)(item, &text, &error), &error, name);
    return text;
}
/* A request header as received by the server; absent headers have a NULL data pointer. */
static P(string) echo_header(P(arena) *arena, const P(response) *response, const char *name) {
    return echo_text(P(json_get)(echo(arena, response), P(string_view)("headers")), name);
}
#endif
