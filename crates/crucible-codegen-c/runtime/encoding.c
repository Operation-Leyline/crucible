#include "features.h"
#include <string.h>

typedef crucible_generated_string string;
static bool alpha(unsigned char c) { return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z'); }
static bool digit(unsigned char c) { return c >= '0' && c <= '9'; }

/* scheme://authority/path?query, without fragments or user information. */
bool crucible_generated_url_split(string text, crucible_generated_url *url) {
    size_t at = 0;
    *url = (crucible_generated_url){0};
    if (!text.data) return false;
    while (at < text.length && text.data[at] != ':') ++at;
    if (at == 0 || at + 2 >= text.length || text.data[at + 1] != '/' || text.data[at + 2] != '/') return false;
    url->scheme = (string){text.data, at};
    size_t start = at + 3, end = start;
    while (end < text.length && text.data[end] != '/' && text.data[end] != '?' && text.data[end] != '#') ++end;
    url->authority = (string){text.data + start, end - start};
    if (!url->authority.length || memchr(url->authority.data, '@', url->authority.length)) return false;
    if (url->authority.data[0] == '[') {
        size_t close = 1;
        while (close < url->authority.length && url->authority.data[close] != ']') ++close;
        if (close == url->authority.length || close == 1) return false;
        url->ipv6 = true;
        url->host = (string){url->authority.data, close + 1};
        if (close + 1 < url->authority.length) {
            if (url->authority.data[close + 1] != ':') return false;
            url->port = (string){url->authority.data + close + 2, url->authority.length - close - 2};
        }
    } else {
        const char *colon = memchr(url->authority.data, ':', url->authority.length);
        size_t host = colon ? (size_t)(colon - url->authority.data) : url->authority.length;
        url->host = (string){url->authority.data, host};
        if (colon) url->port = (string){colon + 1, url->authority.length - host - 1};
    }
    if (!url->host.length) return false;
    for (size_t i = 0; i < url->port.length; ++i) if (!digit((unsigned char)url->port.data[i])) return false;
    size_t path = end;
    while (end < text.length && text.data[end] != '?' && text.data[end] != '#') ++end;
    url->path = (string){text.data + path, end - path};
    if (end < text.length && text.data[end] == '?') {
        size_t query = ++end;
        while (end < text.length && text.data[end] != '#') ++end;
        url->query = (string){text.data + query, end - query};
    }
    return end == text.length;
}
bool crucible_generated_loopback(string host) {
    char lower[16];
    if (host.length >= sizeof(lower)) return false;
    for (size_t i = 0; i < host.length; ++i) lower[i] = (char)(host.data[i] >= 'A' && host.data[i] <= 'Z' ? host.data[i] + 32 : host.data[i]);
    lower[host.length] = 0;
    if (!strcmp(lower, "localhost") || !strcmp(lower, "[::1]")) return true;
    /* Any 127.0.0.0/8 address is loopback. */
    if (strncmp(lower, "127.", 4) != 0) return false;
    unsigned parts = 0, value = 0, digits = 0;
    for (size_t i = 4; i <= host.length; ++i) {
        if (i == host.length || lower[i] == '.') {
            if (!digits || value > 255) return false;
            ++parts; value = 0; digits = 0;
        } else if (digit((unsigned char)lower[i])) { value = value * 10 + (unsigned)(lower[i] - '0'); if (++digits > 3) return false; }
        else return false;
    }
    return parts == 3;
}
static bool label(const char *text, size_t length) {
    if (length < 1 || length > 63) return false;
    for (size_t i = 0; i < length; ++i) {
        unsigned char c = (unsigned char)text[i];
        bool edge = i == 0 || i + 1 == length;
        if (!(alpha(c) || digit(c) || (!edge && c == '-'))) return false;
    }
    return true;
}
bool crucible_generated_host_label(string text, bool subdomains) {
    if (!text.data && text.length) return false;
    if (!subdomains) return label(text.data, text.length);
    size_t start = 0;
    for (size_t i = 0; i <= text.length; ++i) {
        if (i == text.length || text.data[i] == '.') {
            if (!label(text.data + start, i - start)) return false;
            start = i + 1;
        }
    }
    return true;
}
crucible_generated_status crucible_generated_uri_escape(crucible_generated_arena *arena, string text, string *out, crucible_generated_error *error) {
    static const char hex[] = "0123456789ABCDEF";
    if ((!text.data && text.length) || text.length > (SIZE_MAX - 1) / 3) return crucible_generated_fail(error, crucible_generated_invalid, "Invalid escape input");
    char *buffer = crucible_generated_arena_alloc(arena, text.length * 3 + 1, error);
    if (!buffer) return crucible_generated_out_of_memory;
    size_t written = 0;
    for (size_t i = 0; i < text.length; ++i) {
        unsigned char c = (unsigned char)text.data[i];
        if (alpha(c) || digit(c) || c == '-' || c == '_' || c == '.' || c == '~') buffer[written++] = (char)c;
        else { buffer[written++] = '%'; buffer[written++] = hex[c >> 4]; buffer[written++] = hex[c & 15]; }
    }
    buffer[written] = 0;
    *out = (string){buffer, written};
    return crucible_generated_ok;
}
void crucible_generated_base64(const unsigned char *data, size_t length, char *out) {
    static const char alphabet[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    size_t written = 0;
    for (size_t at = 0; at < length; at += 3) {
        unsigned a = data[at], b = at + 1 < length ? data[at + 1] : 0, c = at + 2 < length ? data[at + 2] : 0;
        out[written++] = alphabet[a >> 2];
        out[written++] = alphabet[((a & 3) << 4) | (b >> 4)];
        out[written++] = at + 1 < length ? alphabet[((b & 15) << 2) | (c >> 6)] : '=';
        out[written++] = at + 2 < length ? alphabet[c & 63] : '=';
    }
    out[written] = 0;
}
crucible_generated_status crucible_generated_concat(crucible_generated_arena *arena, const string *parts, size_t count, string *out, crucible_generated_error *error) {
    size_t total = 0;
    for (size_t i = 0; i < count; ++i) {
        if ((!parts[i].data && parts[i].length) || parts[i].length > SIZE_MAX - 1 - total) return crucible_generated_fail(error, crucible_generated_invalid, "Invalid string concatenation");
        total += parts[i].length;
    }
    char *buffer = crucible_generated_arena_alloc(arena, total + 1, error);
    if (!buffer) return crucible_generated_out_of_memory;
    size_t written = 0;
    for (size_t i = 0; i < count; ++i) { if (parts[i].length) memcpy(buffer + written, parts[i].data, parts[i].length); written += parts[i].length; }
    buffer[written] = 0;
    *out = (string){buffer, total};
    return crucible_generated_ok;
}
