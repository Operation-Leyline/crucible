#include "features.h"
#include <string.h>

typedef crucible_generated_string string;
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
