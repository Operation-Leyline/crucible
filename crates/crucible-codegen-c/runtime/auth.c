#ifndef _WIN32
#define _POSIX_C_SOURCE 200809L
#endif
#include "features.h"
#include <openssl/evp.h>
#include <openssl/hmac.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
typedef SRWLOCK mutex;
#else
#include <pthread.h>
typedef pthread_mutex_t mutex;
#endif

typedef crucible_generated_status status;
typedef crucible_generated_string string;
typedef crucible_generated_error error;
typedef crucible_generated_arena arena;
typedef crucible_generated_aws_credentials aws_credentials;
#define OK crucible_generated_ok
#define INVALID crucible_generated_invalid
#define OOM crucible_generated_out_of_memory
#define TRY(call) do { status code_ = (call); if (code_) return code_; } while (0)
#define VIEW(text) crucible_generated_string_view(text)
static status fail(error *issue, const char *message) { return crucible_generated_fail(issue, INVALID, message); }
static const yyjson_val *get(const yyjson_val *value, const char *key) { return yyjson_obj_get((yyjson_val *)value, key); }
static string text(const yyjson_val *value) { return (string){yyjson_get_str((yyjson_val *)value), yyjson_get_len((yyjson_val *)value)}; }
static unsigned char lower(unsigned char c) { return c >= 'A' && c <= 'Z' ? (unsigned char)(c + 32) : c; }
static bool iequal(string a, string b) {
    if (a.length != b.length) return false;
    for (size_t i = 0; i < a.length; ++i) if (lower((unsigned char)a.data[i]) != lower((unsigned char)b.data[i])) return false;
    return true;
}
static bool equal(string a, const char *b) { return crucible_generated_string_equal(a, VIEW(b)); }
static string trim(string value) {
    while (value.length && (*value.data == ' ' || *value.data == '\t' || *value.data == '\r' || *value.data == '\n')) { ++value.data; --value.length; }
    while (value.length && strchr(" \t\r\n", value.data[value.length - 1])) --value.length;
    return value;
}
static double wall_clock(void) {
    struct timespec now;
    if (timespec_get(&now, TIME_UTC) != TIME_UTC) return (double)time(NULL);
    return (double)now.tv_sec + (double)now.tv_nsec / 1e9;
}
static status copy(arena *storage, string value, string *out, error *issue) {
    if (!value.data && value.length) return fail(issue, "Invalid credential view");
    return crucible_generated_arena_copy(storage, value, out, issue);
}
static status identity_copy(arena *storage, const aws_credentials *value, aws_credentials *out, error *issue) {
    aws_credentials result = *value;
    TRY(copy(storage, value->access_key_id, &result.access_key_id, issue));
    TRY(copy(storage, value->secret_access_key, &result.secret_access_key, issue));
    TRY(copy(storage, value->session_token, &result.session_token, issue));
    *out = result; return OK;
}
static status static_credentials(arena *storage, string access, string secret, string token, bool *present, aws_credentials *out, error *issue) {
    *present = false;
    if (!access.length && !secret.length) return OK;
    if (!access.length || !secret.length) return fail(issue, "AWS credentials are incomplete");
    aws_credentials value = {access, secret, token, false, 0};
    TRY(identity_copy(storage, &value, out, issue));
    *present = true; return OK;
}

status crucible_generated_credentials_environment(void *user, const crucible_generated_scope *scope, arena *storage, bool *present, aws_credentials *out, error *issue) {
    (void)user;
    if (!present || !out) return fail(issue, "Credential outputs are required");
    *present = false;
    TRY(crucible_generated_scope_check(scope, issue));
    return static_credentials(storage, VIEW(getenv("AWS_ACCESS_KEY_ID")), VIEW(getenv("AWS_SECRET_ACCESS_KEY")), VIEW(getenv("AWS_SESSION_TOKEN")), present, out, issue);
}

typedef struct entry { struct entry *next; string key, value; } entry;
static status profile_path(arena *storage, string file, string *out, error *issue) {
    if (file.length) return copy(storage, file, out, issue);
    const char *configured = getenv("AWS_SHARED_CREDENTIALS_FILE");
    if (configured && *configured) return copy(storage, VIEW(configured), out, issue);
    const char *home = getenv("HOME");
#ifdef _WIN32
    if (!home || !*home) home = getenv("USERPROFILE");
#endif
    if (!home || !*home) return fail(issue, "Cannot locate the home directory for shared credentials");
    string parts[] = {VIEW(home), VIEW("/.aws/credentials")};
    return crucible_generated_concat(storage, parts, 2, out, issue);
}
status crucible_generated_credentials_profile(void *user, const crucible_generated_scope *scope, arena *storage, bool *present, aws_credentials *out, error *issue) {
    const crucible_generated_profile_source *source = user;
    if (!present || !out || !storage) return fail(issue, "Credential outputs are required");
    *present = false;
    TRY(crucible_generated_scope_check(scope, issue));
    string profile = source ? source->profile : (string){0}, path;
    if (!profile.length) { const char *name = getenv("AWS_PROFILE"); profile = VIEW(name && *name ? name : "default"); }
    TRY(profile_path(storage, source ? source->file : (string){0}, &path, issue));
    FILE *stream = fopen(path.data, "rb");
    if (!stream) return OK;
    /* The whole file is read first so the parser can work on bounded views. */
    char *data = NULL; size_t length = 0, capacity = 0; status code = OK;
    for (;;) {
        if (length == capacity) {
            if (capacity >= (size_t)1 << 24) { code = fail(issue, "Shared credentials file is too large"); break; }
            size_t next = capacity ? capacity * 2 : 4096;
            char *grown = realloc(data, next);
            if (!grown) { code = crucible_generated_fail(issue, OOM, "Credentials allocation failed"); break; }
            data = grown; capacity = next;
        }
        size_t read = fread(data + length, 1, capacity - length, stream);
        length += read;
        if (read == 0) { if (ferror(stream)) code = fail(issue, "Cannot read shared credentials file"); break; }
    }
    fclose(stream);
    string section = {0}; entry *values = NULL;
    for (size_t at = 0; !code && at < length;) {
        size_t end = at; while (end < length && data[end] != '\n') ++end;
        string line = trim((string){data + at, end - at});
        at = end + 1;
        if (!line.length || line.data[0] == '#' || line.data[0] == ';') continue;
        if (line.data[0] == '[' && line.data[line.length - 1] == ']') { section = trim((string){line.data + 1, line.length - 2}); continue; }
        if (!crucible_generated_string_equal(section, profile)) continue;
        const char *separator = memchr(line.data, '=', line.length);
        if (!separator) { code = fail(issue, "Invalid credentials profile entry"); break; }
        string key = trim((string){line.data, (size_t)(separator - line.data)});
        string value = trim((string){separator + 1, (size_t)(line.data + line.length - separator - 1)});
        for (entry *item = values; item; item = item->next) if (crucible_generated_string_equal(item->key, key)) { code = fail(issue, "Duplicate credentials profile entry"); break; }
        if (code) break;
        entry *item = crucible_generated_arena_alloc(storage, sizeof(*item), issue);
        if (!item) { code = OOM; break; }
        item->key = key; item->value = value; item->next = values; values = item;
    }
    string access = {0}, secret = {0}, token = {0};
    for (entry *item = values; item; item = item->next) {
        if (equal(item->key, "aws_access_key_id")) access = item->value;
        else if (equal(item->key, "aws_secret_access_key")) secret = item->value;
        else if (equal(item->key, "aws_session_token")) token = item->value;
    }
    if (!code) code = static_credentials(storage, access, secret, token, present, out, issue);
    free(data);
    return code;
}

status crucible_generated_credentials_chain_resolve(void *user, const crucible_generated_scope *scope, arena *storage, bool *present, aws_credentials *out, error *issue) {
    const crucible_generated_credentials_chain *chain = user;
    if (!present || !out || !chain || (chain->count && !chain->providers)) return fail(issue, "Credential chain and outputs are required");
    *present = false;
    for (size_t i = 0; i < chain->count; ++i) {
        TRY(crucible_generated_scope_check(scope, issue));
        if (!chain->providers[i].resolve) continue;
        TRY(chain->providers[i].resolve(chain->providers[i].user, scope, storage, present, out, issue));
        if (*present) return OK;
    }
    return OK;
}

struct crucible_generated_credentials_cache {
    crucible_generated_credentials_provider provider;
    double skew;
    crucible_generated_clock_fn clock;
    void *clock_user;
    mutex lock;
    bool refreshing, cached;
    aws_credentials value;
    arena *storage;
    double refresh_at;
};
static void lock(mutex *value) {
#ifdef _WIN32
    AcquireSRWLockExclusive(value);
#else
    (void)pthread_mutex_lock(value);
#endif
}
static void unlock(mutex *value) {
#ifdef _WIN32
    ReleaseSRWLockExclusive(value);
#else
    (void)pthread_mutex_unlock(value);
#endif
}
status crucible_generated_credentials_cache_new(crucible_generated_credentials_provider provider, double refresh_before,
    crucible_generated_clock_fn clock, void *clock_user, crucible_generated_credentials_cache **out, error *issue) {
    if (!out || !provider.resolve || !isfinite(refresh_before) || refresh_before < 0) return fail(issue, "A provider and nonnegative refresh interval are required");
    *out = NULL;
    crucible_generated_credentials_cache *cache = calloc(1, sizeof(*cache));
    if (!cache) return crucible_generated_fail(issue, OOM, "Credential cache allocation failed");
    cache->provider = provider; cache->skew = refresh_before; cache->clock = clock; cache->clock_user = clock_user;
#ifdef _WIN32
    InitializeSRWLock(&cache->lock);
#else
    if (pthread_mutex_init(&cache->lock, NULL)) { free(cache); return fail(issue, "Mutex initialization failed"); }
#endif
    *out = cache; return OK;
}
void crucible_generated_credentials_cache_free(crucible_generated_credentials_cache *cache) {
    if (!cache) return;
    crucible_generated_arena_free(cache->storage);
#ifndef _WIN32
    (void)pthread_mutex_destroy(&cache->lock);
#endif
    free(cache);
}
static double cache_now(const crucible_generated_credentials_cache *cache) { return cache->clock ? cache->clock(cache->clock_user) : wall_clock(); }
status crucible_generated_credentials_cache_resolve(void *user, const crucible_generated_scope *scope, arena *storage, bool *present, aws_credentials *out, error *issue) {
    crucible_generated_credentials_cache *cache = user;
    if (!cache || !present || !out || !storage) return fail(issue, "Credential cache and outputs are required");
    *present = false;
    for (;;) {
        TRY(crucible_generated_scope_check(scope, issue));
        lock(&cache->lock);
        if (cache->cached && (cache->refresh_at == 0 || cache_now(cache) < cache->refresh_at)) {
            status code = identity_copy(storage, &cache->value, out, issue);
            unlock(&cache->lock);
            if (!code) *present = true;
            return code;
        }
        if (cache->refreshing) {
            unlock(&cache->lock);
            /* Another caller is refreshing; poll so this caller keeps its own deadline. */
            TRY(crucible_generated_scope_wait(scope, 0.001, issue));
            continue;
        }
        cache->refreshing = true;
        unlock(&cache->lock);
        arena *fresh = crucible_generated_arena_new();
        bool found = false; aws_credentials value = {0};
        status code = fresh ? cache->provider.resolve(cache->provider.user, scope, fresh, &found, &value, issue) : crucible_generated_fail(issue, OOM, "Credential allocation failed");
        double now = cache_now(cache);
        if (!code && found && value.expires && !(value.expires_at > now)) code = fail(issue, "Credential provider returned expired credentials");
        if (!code && found && (!value.access_key_id.length || !value.secret_access_key.length)) code = fail(issue, "AWS credentials are incomplete");
        if (!code && found) code = identity_copy(fresh, &value, &value, issue);
        if (!code && found) code = identity_copy(storage, &value, out, issue);
        lock(&cache->lock);
        if (!code) {
            crucible_generated_arena_free(cache->storage);
            cache->storage = found ? fresh : NULL;
            if (found) fresh = NULL;
            cache->cached = found; cache->value = value; cache->refresh_at = 0;
            if (found && value.expires) cache->refresh_at = value.expires_at - fmin(cache->skew, (value.expires_at - now) / 2);
        }
        cache->refreshing = false;
        unlock(&cache->lock);
        crucible_generated_arena_free(fresh);
        if (!code) *present = found;
        return code;
    }
}

static void hex(const unsigned char *bytes, size_t length, char *out) {
    static const char digits[] = "0123456789abcdef";
    for (size_t i = 0; i < length; ++i) { out[i * 2] = digits[bytes[i] >> 4]; out[i * 2 + 1] = digits[bytes[i] & 15]; }
    out[length * 2] = 0;
}
static status sha256_hex(const void *data, size_t length, char out[65], error *issue) {
    unsigned char digest[EVP_MAX_MD_SIZE]; unsigned size = 0;
    if (EVP_Digest(length ? data : "", length, digest, &size, EVP_sha256(), NULL) != 1 || size != 32) return fail(issue, "SHA256 failed");
    hex(digest, 32, out); return OK;
}
static status hmac(const unsigned char *key, size_t key_length, string value, unsigned char out[32], error *issue) {
    unsigned size = 0; unsigned char result[EVP_MAX_MD_SIZE];
    if (key_length > INT32_MAX || !HMAC(EVP_sha256(), key, (int)key_length, (const unsigned char *)(value.length ? value.data : ""), value.length, result, &size) || size != 32) return fail(issue, "HMAC-SHA256 failed");
    memcpy(out, result, 32); OPENSSL_cleanse(result, sizeof(result));
    return OK;
}
typedef struct { char *data; size_t length, capacity; } buffer;
static status append(buffer *out, string value, error *issue) {
    if (value.length > SIZE_MAX - out->length - 1) return crucible_generated_fail(issue, OOM, "Signing size overflow");
    size_t needed = out->length + value.length + 1;
    if (needed > out->capacity) {
        size_t capacity = out->capacity > SIZE_MAX / 2 ? needed : out->capacity * 2;
        if (capacity < needed) capacity = needed;
        char *next = realloc(out->data, capacity);
        if (!next) return crucible_generated_fail(issue, OOM, "Signing allocation failed");
        out->data = next; out->capacity = capacity;
    }
    if (value.length) memcpy(out->data + out->length, value.data, value.length);
    out->length += value.length; out->data[out->length] = 0; return OK;
}
static int hex_value(unsigned char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}
static status unescape(arena *storage, string value, string *out, error *issue) {
    char *result = crucible_generated_arena_alloc(storage, value.length + 1, issue);
    if (!result) return OOM;
    size_t written = 0;
    for (size_t i = 0; i < value.length; ++i) {
        if (value.data[i] != '%') { result[written++] = value.data[i]; continue; }
        if (i + 2 >= value.length) return fail(issue, "Invalid percent-encoding in query");
        int a = hex_value((unsigned char)value.data[i + 1]), b = hex_value((unsigned char)value.data[i + 2]);
        if (a < 0 || b < 0) return fail(issue, "Invalid percent-encoding in query");
        result[written++] = (char)(a * 16 + b); i += 2;
    }
    *out = (string){result, written}; return OK;
}
static int compare(string a, string b) {
    size_t shared = a.length < b.length ? a.length : b.length;
    int order = shared ? memcmp(a.data, b.data, shared) : 0;
    return order ? order : (a.length < b.length ? -1 : a.length > b.length ? 1 : 0);
}
typedef struct canonical_query_pair { string key, value; } canonical_query_pair;
static int compare_query_pair(canonical_query_pair a, canonical_query_pair b) {
    int order = compare(a.key, b.key);
    return order ? order : compare(a.value, b.value);
}
static void sort_query_pairs(canonical_query_pair *items, size_t count) {
    for (size_t i = 1; i < count; ++i) {
        canonical_query_pair item = items[i]; size_t j = i;
        while (j && compare_query_pair(items[j - 1], item) > 0) { items[j] = items[j - 1]; --j; }
        items[j] = item;
    }
}
static status canonical_path(arena *storage, string path, bool normalize, bool double_encode, string *out, error *issue) {
    if (!path.length) path = VIEW("/");
    string *segments = crucible_generated_arena_alloc(storage, (path.length + 1) * sizeof(*segments), issue);
    if (!segments) return OOM;
    size_t count = 0, start = 0;
    for (size_t i = 0; i <= path.length; ++i) {
        if (i < path.length && path.data[i] != '/') continue;
        string segment = {path.data + start, i - start};
        start = i + 1;
        if (normalize) {
            if (!segment.length || equal(segment, ".")) continue;
            if (equal(segment, "..")) { if (count) --count; continue; }
        }
        segments[count++] = segment;
    }
    buffer result = {0}; status code = OK;
    if (normalize) {
        for (size_t i = 0; !code && i < count; ++i) {
            code = append(&result, VIEW("/"), issue);
            string segment = segments[i];
            if (!code && double_encode) code = crucible_generated_uri_escape(storage, segment, &segment, issue);
            if (!code) code = append(&result, segment, issue);
        }
        if (!code && (!result.length || (path.data[path.length - 1] == '/' && result.data[result.length - 1] != '/'))) code = append(&result, VIEW("/"), issue);
    } else {
        /* Without normalization every original segment, including empty ones, is kept. */
        for (size_t i = 0; !code && i < count; ++i) {
            string segment = segments[i];
            if (i) code = append(&result, VIEW("/"), issue);
            if (!code && double_encode) code = crucible_generated_uri_escape(storage, segment, &segment, issue);
            if (!code) code = append(&result, segment, issue);
        }
    }
    if (!code) code = crucible_generated_arena_copy(storage, (string){result.data, result.length}, out, issue);
    free(result.data); return code;
}
static status canonical_query(arena *storage, string query, string *out, error *issue) {
    size_t pairs = 1;
    for (size_t i = 0; i < query.length; ++i) if (query.data[i] == '&') ++pairs;
    canonical_query_pair *items = crucible_generated_arena_alloc(storage, pairs * sizeof(*items), issue);
    if (!items) return OOM;
    size_t count = 0, start = 0;
    for (size_t i = 0; i <= query.length; ++i) {
        if (i < query.length && query.data[i] != '&') continue;
        string pair = {query.data + start, i - start};
        start = i + 1;
        if (!pair.length) continue;
        const char *separator = memchr(pair.data, '=', pair.length);
        string key = {pair.data, separator ? (size_t)(separator - pair.data) : pair.length};
        string value = separator ? (string){separator + 1, pair.length - key.length - 1} : (string){"", 0};
        TRY(unescape(storage, key, &key, issue)); TRY(unescape(storage, value, &value, issue));
        TRY(crucible_generated_uri_escape(storage, key, &key, issue)); TRY(crucible_generated_uri_escape(storage, value, &value, issue));
        items[count++] = (canonical_query_pair){key, value};
    }
    sort_query_pairs(items, count);
    buffer result = {0}; status code = OK;
    for (size_t i = 0; !code && i < count; ++i) {
        if (i) code = append(&result, VIEW("&"), issue);
        if (!code) code = append(&result, items[i].key, issue);
        if (!code) code = append(&result, VIEW("="), issue);
        if (!code) code = append(&result, items[i].value, issue);
    }
    if (!code) code = crucible_generated_arena_copy(storage, (string){result.data ? result.data : "", result.length}, out, issue);
    free(result.data); return code;
}
static bool excluded(string name) {
    static const char *names[] = {"authorization", "connection", "expect", "transfer-encoding", "user-agent", "x-amzn-trace-id", "accept-encoding", "content-length", "host"};
    for (size_t i = 0; i < sizeof(names) / sizeof(names[0]); ++i) if (equal(name, names[i])) return true;
    return false;
}
static status collapse(arena *storage, string value, string *out, error *issue) {
    char *result = crucible_generated_arena_alloc(storage, value.length + 1, issue);
    if (!result) return OOM;
    size_t written = 0; bool space = false;
    for (size_t i = 0; i < value.length; ++i) {
        char c = value.data[i];
        if (c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\v' || c == '\f') { space = written > 0; continue; }
        if (space) { result[written++] = ' '; space = false; }
        result[written++] = c;
    }
    *out = (string){result, written}; return OK;
}
static void civil(int64_t seconds, char date[9], char stamp[17]) {
    int64_t days = seconds >= 0 ? seconds / 86400 : -((-seconds + 86399) / 86400);
    int64_t rest = seconds - days * 86400;
    days += 719468;
    int64_t era = (days >= 0 ? days : days - 146096) / 146097;
    int64_t doe = days - era * 146097;
    int64_t yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    int64_t year = yoe + era * 400, doy = doe - (365 * yoe + yoe / 4 - yoe / 100), mp = (5 * doy + 2) / 153;
    int64_t day = doy - (153 * mp + 2) / 5 + 1, month = mp < 10 ? mp + 3 : mp - 9;
    if (month <= 2) ++year;
    /* Callers bound the year to 1..9999, so fixed-width digits never truncate. */
    int64_t fields[] = {year, month, day, rest / 3600, rest / 60 % 60, rest % 60};
    int widths[] = {4, 2, 2, 2, 2, 2};
    size_t written = 0;
    for (size_t i = 0; i < 6; ++i) {
        if (i == 3) stamp[written++] = 'T';
        for (int w = widths[i] - 1; w >= 0; --w) {
            int64_t value = fields[i];
            for (int k = 0; k < w; ++k) value /= 10;
            stamp[written++] = (char)('0' + value % 10);
        }
    }
    stamp[written++] = 'Z'; stamp[written] = 0;
    memcpy(date, stamp, 8); date[8] = 0;
}
status crucible_generated_sigv4_sign(arena *storage, crucible_generated_request *request, const aws_credentials *credentials,
    const crucible_generated_sigv4_options *options, error *issue) {
    if (!storage || !request || !credentials || !options) return fail(issue, "SigV4 arguments are required");
    int64_t now = options->has_time ? options->time : (int64_t)floor(wall_clock());
    if (!credentials->access_key_id.length || !credentials->secret_access_key.length || (credentials->expires && !(credentials->expires_at > (double)now))) return fail(issue, "AWS credentials are missing or expired");
    string scope_parts[] = {options->region, options->service};
    for (size_t i = 0; i < 2; ++i) {
        if (!scope_parts[i].length || !scope_parts[i].data) return fail(issue, "SigV4 region and service are required scope components");
        for (size_t j = 0; j < scope_parts[i].length; ++j) if (strchr("/\r\n", scope_parts[i].data[j]) && scope_parts[i].data[j]) return fail(issue, "SigV4 region and service are required scope components");
    }
    if (now < -62135596800LL || now > 253402300799LL) return fail(issue, "SigV4 time is out of range");
    crucible_generated_url url;
    if (!crucible_generated_url_split(request->url, &url)) return fail(issue, "SigV4 requires an absolute request URL");
    char date[9], stamp[17], digest[65];
    civil(now, date, stamp);
    /* Rebuild headers without a stale signature or token. */
    crucible_generated_header *items = crucible_generated_arena_alloc(storage, (request->headers.count + 1) * sizeof(*items), issue);
    if (!items) return OOM;
    size_t count = 0;
    for (size_t i = 0; i < request->headers.count; ++i) {
        string name = request->headers.items[i].name;
        if (iequal(name, VIEW("authorization")) || iequal(name, VIEW("x-amz-security-token"))) continue;
        items[count++] = request->headers.items[i];
    }
    request->headers = (crucible_generated_headers){items, count};
    TRY(crucible_generated_header_set(storage, &request->headers, VIEW("X-Amz-Date"), VIEW(stamp), issue));
    if (credentials->session_token.length) TRY(crucible_generated_header_set(storage, &request->headers, VIEW("X-Amz-Security-Token"), credentials->session_token, issue));
    if (options->unsigned_payload) memcpy(digest, "UNSIGNED-PAYLOAD", 17);
    else TRY(sha256_hex(request->has_body ? request->body.data : NULL, request->has_body ? request->body.length : 0, digest, issue));
    TRY(crucible_generated_header_set(storage, &request->headers, VIEW("X-Amz-Content-Sha256"), VIEW(digest), issue));
    bool s3 = equal(options->service, "s3");
    bool normalize = options->normalize_path == crucible_generated_sigv4_default ? !s3 : options->normalize_path == crucible_generated_sigv4_enabled;
    bool double_encode = options->double_encoding == crucible_generated_sigv4_default ? !s3 : options->double_encoding == crucible_generated_sigv4_enabled;
    string path, query;
    TRY(canonical_path(storage, url.path, normalize, double_encode, &path, issue));
    TRY(canonical_query(storage, url.query, &query, issue));
    /* Canonical headers: lowercase names, sorted, repeated names joined with commas. */
    size_t total = request->headers.count + 1;
    string *names = crucible_generated_arena_alloc(storage, total * sizeof(*names), issue), *values = crucible_generated_arena_alloc(storage, total * sizeof(*values), issue);
    if (!names || !values) return OOM;
    size_t used = 0;
    string host = {0};
    if (!crucible_generated_header_get(request->headers, VIEW("host"), &host)) {
        host = url.authority;
        bool http_default = equal(url.scheme, "http") && equal(url.port, "80"), https_default = equal(url.scheme, "https") && equal(url.port, "443");
        if (http_default || https_default) host.length = url.host.length;
        char *lowered = crucible_generated_arena_alloc(storage, host.length + 1, issue);
        if (!lowered) return OOM;
        for (size_t i = 0; i < host.length; ++i) lowered[i] = (char)lower((unsigned char)host.data[i]);
        host.data = lowered;
    }
    names[used] = VIEW("host"); values[used++] = host;
    for (size_t i = 0; i < request->headers.count; ++i) {
        string name = request->headers.items[i].name;
        char *lowered = crucible_generated_arena_alloc(storage, name.length + 1, issue);
        if (!lowered) return OOM;
        for (size_t j = 0; j < name.length; ++j) lowered[j] = (char)lower((unsigned char)name.data[j]);
        name.data = lowered;
        if (excluded(name)) continue;
        string value; TRY(collapse(storage, request->headers.items[i].value, &value, issue));
        size_t j = used;
        while (j && compare(names[j - 1], name) > 0) { names[j] = names[j - 1]; values[j] = values[j - 1]; --j; }
        names[j] = name; values[j] = value; ++used;
    }
    buffer canonical = {0}, signed_headers = {0}; status code = OK;
#define ADD(target, ...) do { if (!code) code = append(&(target), (__VA_ARGS__), issue); } while (0)
    ADD(canonical, request->method); ADD(canonical, VIEW("\n")); ADD(canonical, path); ADD(canonical, VIEW("\n")); ADD(canonical, query); ADD(canonical, VIEW("\n"));
    for (size_t i = 0; i < used; ++i) {
        bool first = i == 0 || !crucible_generated_string_equal(names[i - 1], names[i]);
        bool last = i + 1 == used || !crucible_generated_string_equal(names[i + 1], names[i]);
        if (first) { ADD(canonical, names[i]); ADD(canonical, VIEW(":")); if (signed_headers.length) ADD(signed_headers, VIEW(";")); ADD(signed_headers, names[i]); }
        else ADD(canonical, VIEW(","));
        ADD(canonical, values[i]);
        if (last) ADD(canonical, VIEW("\n"));
    }
    ADD(canonical, VIEW("\n")); ADD(canonical, (string){signed_headers.data ? signed_headers.data : "", signed_headers.length}); ADD(canonical, VIEW("\n")); ADD(canonical, VIEW(digest));
    char canonical_hash[65];
    if (!code) code = sha256_hex(canonical.data, canonical.length, canonical_hash, issue);
    buffer scope = {0}, to_sign = {0};
    ADD(scope, VIEW(date)); ADD(scope, VIEW("/")); ADD(scope, options->region); ADD(scope, VIEW("/")); ADD(scope, options->service); ADD(scope, VIEW("/aws4_request"));
    ADD(to_sign, VIEW("AWS4-HMAC-SHA256\n")); ADD(to_sign, VIEW(stamp)); ADD(to_sign, VIEW("\n"));
    ADD(to_sign, (string){scope.data ? scope.data : "", scope.length}); ADD(to_sign, VIEW("\n")); ADD(to_sign, VIEW(canonical_hash));
    unsigned char key[32]; char signature[65];
    buffer secret = {0};
    ADD(secret, VIEW("AWS4")); ADD(secret, credentials->secret_access_key);
    if (!code) code = hmac((const unsigned char *)secret.data, secret.length, VIEW(date), key, issue);
    string steps[] = {options->region, options->service, VIEW("aws4_request")};
    for (size_t i = 0; !code && i < 3; ++i) code = hmac(key, sizeof(key), steps[i], key, issue);
    if (!code) code = hmac(key, sizeof(key), (string){to_sign.data, to_sign.length}, key, issue);
    if (!code) hex(key, 32, signature);
    buffer authorization = {0};
    ADD(authorization, VIEW("AWS4-HMAC-SHA256 Credential=")); ADD(authorization, credentials->access_key_id); ADD(authorization, VIEW("/"));
    ADD(authorization, (string){scope.data ? scope.data : "", scope.length}); ADD(authorization, VIEW(", SignedHeaders="));
    ADD(authorization, (string){signed_headers.data ? signed_headers.data : "", signed_headers.length}); ADD(authorization, VIEW(", Signature="));
    if (!code) ADD(authorization, VIEW(signature));
    if (!code) code = crucible_generated_header_set(storage, &request->headers, VIEW("Authorization"), (string){authorization.data, authorization.length}, issue);
#undef ADD
    if (secret.data) OPENSSL_cleanse(secret.data, secret.length);
    OPENSSL_cleanse(key, sizeof(key));
    free(canonical.data); free(signed_headers.data); free(scope.data); free(to_sign.data); free(secret.data); free(authorization.data);
    return code;
}

/* A caller-supplied header or query parameter for the scheme bypasses its provider. */
static bool preset(const crucible_generated_request *request, string kind, string name, string location, string prefix) {
    string value = {0};
    if (equal(kind, "apiKey")) {
        if (equal(location, "query")) {
            crucible_generated_url url;
            if (!crucible_generated_url_split(request->url, &url)) return false;
            size_t start = 0;
            for (size_t i = 0; i <= url.query.length; ++i) {
                if (i < url.query.length && url.query.data[i] != '&') continue;
                string pair = {url.query.data + start, i - start};
                start = i + 1;
                const char *separator = memchr(pair.data, '=', pair.length);
                if (separator && crucible_generated_string_equal((string){pair.data, (size_t)(separator - pair.data)}, name) && separator + 1 < pair.data + pair.length) return true;
            }
            return false;
        }
        if (!crucible_generated_header_get(request->headers, name, &value)) return false;
        if (!prefix.length) return value.length > 0;
    } else {
        if (!crucible_generated_header_get(request->headers, VIEW("authorization"), &value)) return false;
        if (equal(kind, "basic")) prefix = VIEW("Basic");
        else if (equal(kind, "bearer")) prefix = VIEW("Bearer");
        else if (equal(kind, "sigV4")) prefix = VIEW("AWS4-HMAC-SHA256");
        else return false;
    }
    return value.length > prefix.length + 1 && iequal((string){value.data, prefix.length}, prefix) && value.data[prefix.length] == ' ';
}
static status set_query(arena *storage, crucible_generated_request *request, string name, string value, error *issue) {
    crucible_generated_url url;
    if (!crucible_generated_url_split(request->url, &url)) return fail(issue, "Invalid request URL");
    buffer result = {0}; status code = OK;
    size_t base = url.query.data ? (size_t)(url.query.data - request->url.data) - 1 : request->url.length;
    code = append(&result, (string){request->url.data, base}, issue);
    bool first = true; size_t start = 0;
    for (size_t i = 0; !code && url.query.data && i <= url.query.length; ++i) {
        if (i < url.query.length && url.query.data[i] != '&') continue;
        string pair = {url.query.data + start, i - start};
        start = i + 1;
        const char *separator = memchr(pair.data, '=', pair.length);
        string key = {pair.data, separator ? (size_t)(separator - pair.data) : pair.length};
        if (!pair.length || crucible_generated_string_equal(key, name)) continue;
        code = append(&result, VIEW(first ? "?" : "&"), issue); first = false;
        if (!code) code = append(&result, pair, issue);
    }
    string escaped_name = {0}, escaped_value = {0};
    if (!code) code = crucible_generated_uri_escape(storage, name, &escaped_name, issue);
    if (!code) code = crucible_generated_uri_escape(storage, value, &escaped_value, issue);
    if (!code) code = append(&result, VIEW(first ? "?" : "&"), issue);
    if (!code) code = append(&result, escaped_name, issue);
    if (!code) code = append(&result, VIEW("="), issue);
    if (!code) code = append(&result, escaped_value, issue);
    if (!code) code = crucible_generated_arena_copy(storage, (string){result.data, result.length}, &request->url, issue);
    free(result.data); return code;
}
status crucible_generated_authenticate(crucible_generated_client *client, const yyjson_val *op, const yyjson_val *properties,
    const crucible_generated_scope *scope, arena *storage, crucible_generated_request *request, bool *credentials,
    crucible_generated_signing *signing, error *issue) {
    static const char bearer_id[] = "smithy.api#httpBearerAuth";
    const crucible_generated_client_config *config = &client->config;
    const yyjson_val *ids = get(op, "auth");
    const bool op_bearer = yyjson_get_bool((yyjson_val *)get(op, "bearer"));
    *credentials = false; *signing = (crucible_generated_signing){0};
    if (!yyjson_is_arr(ids)) {
        if (!op_bearer) return OK;
        TRY(crucible_generated_json_parse(storage, VIEW("[\"smithy.api#httpBearerAuth\"]"), &ids, issue));
    }
    const yyjson_val *choices = get(properties, "authSchemes");
    size_t index, count; yyjson_val *id;
    yyjson_arr_foreach((yyjson_val *)ids, index, count, id) {
        const yyjson_val *scheme = yyjson_obj_getn((yyjson_val *)get(client->schema, "authSchemes"), yyjson_get_str(id), yyjson_get_len(id));
        string kind = equal(text(id), bearer_id) ? VIEW("bearer") : text(get(scheme, "kind"));
        string signing_name = text(get(scheme, "signingName")), signing_region = {0};
        crucible_generated_sigv4_toggle double_encoding = crucible_generated_sigv4_default, normalize_path = crucible_generated_sigv4_default;
        if (yyjson_is_arr(choices)) {
            bool matched = false; size_t ci, cc; yyjson_val *choice;
            yyjson_arr_foreach((yyjson_val *)choices, ci, cc, choice) {
                if (!yyjson_is_str(get(choice, "name")) || !iequal(text(get(choice, "name")), kind)) continue;
                matched = true;
                if (yyjson_is_str(get(choice, "signingName"))) signing_name = text(get(choice, "signingName"));
                if (yyjson_is_str(get(choice, "signingRegion"))) signing_region = text(get(choice, "signingRegion"));
                if (yyjson_is_bool(get(choice, "disableDoubleEncoding"))) double_encoding = yyjson_get_bool(get(choice, "disableDoubleEncoding")) ? crucible_generated_sigv4_disabled : crucible_generated_sigv4_enabled;
                if (yyjson_is_bool(get(choice, "disableNormalizePath"))) normalize_path = yyjson_get_bool(get(choice, "disableNormalizePath")) ? crucible_generated_sigv4_disabled : crucible_generated_sigv4_enabled;
                break;
            }
            if (!matched) continue;
        }
        string name = text(get(scheme, "name")), location = text(get(scheme, "location")), prefix = text(get(scheme, "scheme"));
        if (preset(request, kind, name, location, prefix)) { *credentials = true; return OK; }
        bool present = false;
        if (equal(kind, "bearer") && config->token) {
            string token = {0};
            TRY(config->token(config->token_user, scope, storage, &present, &token, issue));
            TRY(crucible_generated_scope_check(scope, issue));
            if (present) {
                if (!token.data && token.length) return fail(issue, "Invalid bearer token view");
                string parts[] = {VIEW("Bearer "), token}, header;
                TRY(crucible_generated_concat(storage, parts, 2, &header, issue));
                TRY(crucible_generated_header_set(storage, &request->headers, VIEW("authorization"), header, issue));
                *credentials = true; return OK;
            }
        } else if (equal(kind, "basic") && config->basic_credentials) {
            crucible_generated_basic_credentials value = {0};
            TRY(config->basic_credentials(config->basic_user, scope, storage, &present, &value, issue));
            TRY(crucible_generated_scope_check(scope, issue));
            if (present) {
                if ((!value.username.data && value.username.length) || (!value.password.data && value.password.length)) return fail(issue, "Invalid basic credential view");
                if (value.username.length && memchr(value.username.data, ':', value.username.length)) return fail(issue, "Basic username cannot contain a colon");
                string joined, parts[] = {value.username, VIEW(":"), value.password};
                TRY(crucible_generated_concat(storage, parts, 3, &joined, issue));
                if (joined.length > (SIZE_MAX - 8) / 4 * 3) return fail(issue, "Basic credentials are too large");
                char *encoded = crucible_generated_arena_alloc(storage, 6 + (joined.length + 2) / 3 * 4 + 1, issue);
                if (!encoded) return OOM;
                memcpy(encoded, "Basic ", 6);
                crucible_generated_base64((const unsigned char *)joined.data, joined.length, encoded + 6);
                OPENSSL_cleanse((void *)joined.data, joined.length);
                TRY(crucible_generated_header_set(storage, &request->headers, VIEW("Authorization"), VIEW(encoded), issue));
                *credentials = true; return OK;
            }
        } else if (equal(kind, "apiKey") && config->api_key) {
            string key = {0};
            TRY(config->api_key(config->api_key_user, scope, storage, &present, &key, issue));
            TRY(crucible_generated_scope_check(scope, issue));
            if (present && key.length) {
                if (!key.data) return fail(issue, "Invalid API key view");
                if (equal(location, "query")) { TRY(set_query(storage, request, name, key, issue)); }
                else {
                    string value = key;
                    if (prefix.length) { string parts[] = {prefix, VIEW(" "), key}; TRY(crucible_generated_concat(storage, parts, 3, &value, issue)); }
                    TRY(crucible_generated_header_set(storage, &request->headers, name, value, issue));
                }
                *credentials = true; return OK;
            }
        } else if (equal(kind, "sigV4") && config->credentials) {
            aws_credentials identity = {0};
            TRY(config->credentials(config->credentials_user, scope, storage, &present, &identity, issue));
            TRY(crucible_generated_scope_check(scope, issue));
            if (present) {
                signing->sign = true;
                TRY(identity_copy(storage, &identity, &signing->identity, issue));
                signing->options.region = signing_region.length ? signing_region : config->region;
                signing->options.service = signing_name;
                signing->options.unsigned_payload = yyjson_get_bool((yyjson_val *)get(op, "unsignedPayload"));
                signing->options.double_encoding = double_encoding; signing->options.normalize_path = normalize_path;
                *credentials = true; return OK;
            }
        }
    }
    size_t declared = yyjson_arr_size((yyjson_val *)ids);
    bool legacy_bearer = declared == 1 && op_bearer && equal(text(yyjson_arr_get((yyjson_val *)ids, 0)), bearer_id);
    if (declared && !legacy_bearer && !yyjson_get_bool((yyjson_val *)get(op, "optionalAuth"))) return fail(issue, "No identity available for the modeled authentication schemes");
    return OK;
}
