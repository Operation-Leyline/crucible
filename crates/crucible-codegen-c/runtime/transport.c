#ifndef _WIN32
#define _POSIX_C_SOURCE 200809L
#endif
#include "http.h"
#include "internal.h"
#include <curl/curl.h>
#include <limits.h>
#include <math.h>
#include <stdlib.h>
#include <string.h>
#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
typedef SRWLOCK mutex;
static INIT_ONCE once = INIT_ONCE_STATIC_INIT;
#else
#include <pthread.h>
#include <signal.h>
#include <errno.h>
typedef pthread_mutex_t mutex;
static pthread_once_t once = PTHREAD_ONCE_INIT;
#endif

static CURLcode initialized = CURLE_FAILED_INIT;
#ifdef _WIN32
static BOOL CALLBACK initialize(PINIT_ONCE init, PVOID parameter, PVOID *context) {
    (void)init; (void)parameter; (void)context;
    initialized = curl_global_init(CURL_GLOBAL_DEFAULT); return TRUE;
}
#else
static void initialize(void) { initialized = curl_global_init(CURL_GLOBAL_DEFAULT); }
#endif
static crucible_generated_status ready(crucible_generated_error *error) {
#ifdef _WIN32
    if (!InitOnceExecuteOnce(&once, initialize, NULL, NULL)) return crucible_generated_fail(error, crucible_generated_network, "Cannot initialize libcurl");
#else
    if (pthread_once(&once, initialize) != 0) return crucible_generated_fail(error, crucible_generated_network, "Cannot initialize libcurl");
#endif
    return initialized == CURLE_OK ? crucible_generated_ok : crucible_generated_fail(error, crucible_generated_network, curl_easy_strerror(initialized));
}
typedef struct {
#ifdef __linux__
    sigset_t old, pipe;
    bool consume;
#else
    unsigned char unused;
#endif
} signals;
static bool block(signals *state) {
#ifdef __linux__
    sigemptyset(&state->pipe); sigaddset(&state->pipe, SIGPIPE);
    if (pthread_sigmask(SIG_BLOCK, &state->pipe, &state->old) != 0) return false;
    sigset_t pending;
    sigpending(&pending);
    state->consume = sigismember(&state->old, SIGPIPE) == 0 && sigismember(&pending, SIGPIPE) == 0;
#else
    (void)state;
#endif
    return true;
}
static void unblock(signals *state) {
#ifdef __linux__
    if (state->consume) {
        struct timespec immediate = {0, 0};
        while (sigtimedwait(&state->pipe, NULL, &immediate) < 0 && errno == EINTR) {}
    }
    (void)pthread_sigmask(SIG_SETMASK, &state->old, NULL);
#else
    (void)state;
#endif
}
static bool token(crucible_generated_string text) {
    if (!text.data || !text.length) return false;
    for (size_t i = 0; i < text.length; ++i) {
        unsigned char c = (unsigned char)text.data[i];
        if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || (c && strchr("!#$%&'*+-.^_`|~", c)))) return false;
    }
    return true;
}
static bool part(CURLU *url, CURLUPart field) {
    char *value = NULL;
    bool present = curl_url_get(url, field, &value, 0) == CURLUE_OK;
    curl_free(value); return present;
}
crucible_generated_status crucible_generated_validate_url(crucible_generated_string text, bool endpoint, bool credentials, crucible_generated_error *error) {
    crucible_generated_status status = ready(error);
    if (status) return status;
    if (!text.data || text.length == SIZE_MAX) return crucible_generated_fail(error, crucible_generated_invalid, "Invalid URL view");
    for (size_t i = 0; i < text.length; ++i) if ((unsigned char)text.data[i] <= 32 || text.data[i] == 127) return crucible_generated_fail(error, crucible_generated_invalid, "Invalid URL character");
    char *copy = malloc(text.length + 1);
    CURLU *url = curl_url();
    if (!copy || !url) { free(copy); curl_url_cleanup(url); return crucible_generated_fail(error, crucible_generated_out_of_memory, "URL allocation failed"); }
    memcpy(copy, text.data, text.length); copy[text.length] = 0;
    char *scheme = NULL, *host = NULL;
    bool valid = curl_url_set(url, CURLUPART_URL, copy, 0) == CURLUE_OK;
    valid = valid && curl_url_get(url, CURLUPART_SCHEME, &scheme, 0) == CURLUE_OK && curl_url_get(url, CURLUPART_HOST, &host, 0) == CURLUE_OK;
    valid = valid && (!strcmp(scheme, "http") || !strcmp(scheme, "https")) && *host && !part(url, CURLUPART_USER) && !part(url, CURLUPART_PASSWORD) && !part(url, CURLUPART_FRAGMENT) && !(endpoint && part(url, CURLUPART_QUERY));
    if (valid && credentials && strcmp(scheme, "https")) {
        for (char *c = host; *c; ++c) if (*c >= 'A' && *c <= 'Z') *c = (char)(*c + 32);
        valid = !strcmp(host, "localhost") || !strcmp(host, "127.0.0.1") || !strcmp(host, "[::1]");
    }
    free(copy); curl_free(scheme); curl_free(host); curl_url_cleanup(url);
    return valid ? crucible_generated_ok : crucible_generated_fail(error, crucible_generated_invalid, "Absolute HTTP(S) URL without credentials or fragment required; bearer credentials require HTTPS or loopback");
}
crucible_generated_status crucible_generated_validate_request(const crucible_generated_request *request, crucible_generated_error *error) {
    if (!request || !token(request->method) || (request->headers.count && !request->headers.items) || (request->body.length && !request->body.data)) return crucible_generated_fail(error, crucible_generated_invalid, "Invalid HTTP request");
    if (crucible_generated_string_equal(request->method, crucible_generated_string_view("HEAD")) && request->has_body && request->body.length) return crucible_generated_fail(error, crucible_generated_invalid, "libcurl HEAD requests cannot carry a body");
    for (size_t i = 0; i < request->headers.count; ++i) {
        const crucible_generated_header *header = &request->headers.items[i];
        if (!token(header->name) || (!header->value.data && header->value.length)) return crucible_generated_fail(error, crucible_generated_invalid, "Invalid HTTP header");
        for (size_t j = 0; j < header->value.length; ++j) if (!header->value.data[j] || header->value.data[j] == '\r' || header->value.data[j] == '\n') return crucible_generated_fail(error, crucible_generated_invalid, "Invalid HTTP header value");
    }
    return crucible_generated_validate_url(request->url, false, crucible_generated_header_get(request->headers, crucible_generated_string_view("authorization"), NULL), error);
}
struct crucible_generated_curl { mutex mutex; CURLM *multi; };
crucible_generated_status crucible_generated_curl_new(crucible_generated_curl **output, crucible_generated_error *error) {
    if (!output) return crucible_generated_fail(error, crucible_generated_invalid, "Transport output is required");
    *output = NULL;
    crucible_generated_status status = ready(error);
    if (status) return status;
    if (!(curl_version_info(CURLVERSION_NOW)->features & CURL_VERSION_ASYNCHDNS)) return crucible_generated_fail(error, crucible_generated_invalid, "libcurl asynchronous DNS support is required");
    crucible_generated_curl *state = calloc(1, sizeof(*state));
    if (!state) return crucible_generated_fail(error, crucible_generated_out_of_memory, "Transport allocation failed");
#ifdef _WIN32
    InitializeSRWLock(&state->mutex);
#else
    if (pthread_mutex_init(&state->mutex, NULL)) { free(state); return crucible_generated_fail(error, crucible_generated_network, "Mutex initialization failed"); }
#endif
    state->multi = curl_multi_init();
    if (!state->multi) { crucible_generated_curl_free(state); return crucible_generated_fail(error, crucible_generated_out_of_memory, "Transport allocation failed"); }
    *output = state; return crucible_generated_ok;
}
void crucible_generated_curl_free(crucible_generated_curl *state) {
    if (!state) return;
    /* Callers must finish active calls before freeing the owning client. */
    signals mask;
    if (state->multi && block(&mask)) { curl_multi_cleanup(state->multi); unblock(&mask); }
#ifndef _WIN32
    (void)pthread_mutex_destroy(&state->mutex);
#endif
    free(state);
}
typedef struct {
    crucible_generated_arena *arena;
    crucible_generated_response response;
    crucible_generated_error error;
    unsigned char *body;
    size_t length, capacity;
    bool read_body, discarded;
} transfer;
static size_t body(char *data, size_t size, size_t count, void *user) {
    transfer *self = user;
    if ((size && count > SIZE_MAX / size) || size * count > SIZE_MAX - self->length) { crucible_generated_fail(&self->error, crucible_generated_out_of_memory, "Response size overflow"); return 0; }
    const size_t length = size * count, total = self->length + length;
    if (total > self->capacity) {
        size_t capacity = self->capacity > SIZE_MAX / 2 ? total : self->capacity * 2;
        if (capacity < total) capacity = total;
        unsigned char *next = realloc(self->body, capacity);
        if (!next) { crucible_generated_fail(&self->error, crucible_generated_out_of_memory, "Response allocation failed"); return 0; }
        self->body = next; self->capacity = capacity;
    }
    if (length) memcpy(self->body + self->length, data, length);
    self->length = total; return length;
}
static size_t header(char *data, size_t size, size_t count, void *user) {
    transfer *self = user;
    if (size && count > SIZE_MAX / size) { crucible_generated_fail(&self->error, crucible_generated_out_of_memory, "Header size overflow"); return 0; }
    const size_t length = size * count;
    if (length >= 5 && !memcmp(data, "HTTP/", 5)) {
        self->response = (crucible_generated_response){0}; self->length = 0;
        char *space = memchr(data, ' ', length);
        if (!space || (size_t)(data + length - space) < 4 || space[1] < '1' || space[1] > '5' || space[2] < '0' || space[2] > '9' || space[3] < '0' || space[3] > '9') { crucible_generated_fail(&self->error, crucible_generated_network, "Invalid HTTP status"); return 0; }
        self->response.status = (space[1] - '0') * 100 + (space[2] - '0') * 10 + space[3] - '0';
    } else if ((length == 2 && !memcmp(data, "\r\n", 2)) || (length == 1 && data[0] == '\n')) {
        if (!self->read_body && self->response.status >= 200 && self->response.status < 300) { self->discarded = true; return 0; }
    } else {
        char *colon = memchr(data, ':', length);
        if (colon) {
            crucible_generated_string name = {data, (size_t)(colon - data)}, value = {colon + 1, (size_t)(data + length - colon - 1)}, old;
            while (value.length && (*value.data == ' ' || *value.data == '\t')) { ++value.data; --value.length; }
            while (value.length && strchr(" \t\r\n", value.data[value.length - 1])) --value.length;
            if (crucible_generated_header_get(self->response.headers, name, &old)) {
                if (old.length > SIZE_MAX - 2 || value.length > SIZE_MAX - old.length - 2) { crucible_generated_fail(&self->error, crucible_generated_out_of_memory, "Header size overflow"); return 0; }
                char *joined = crucible_generated_arena_alloc(self->arena, old.length + 2 + value.length, &self->error);
                if (!joined) return 0;
                if (old.length) memcpy(joined, old.data, old.length);
                memcpy(joined + old.length, ", ", 2);
                if (value.length) memcpy(joined + old.length + 2, value.data, value.length);
                value = (crucible_generated_string){joined, old.length + 2 + value.length};
            }
            if (crucible_generated_header_set(self->arena, &self->response.headers, name, value, &self->error)) return 0;
        }
    }
    return length;
}

crucible_generated_status crucible_generated_curl_send(void *user, const crucible_generated_request *request, bool read_body,
    const crucible_generated_scope *scope, crucible_generated_arena *arena, crucible_generated_response *response, crucible_generated_error *error) {
    crucible_generated_curl *state = user;
    if (!state || !arena || !response) return crucible_generated_fail(error, crucible_generated_invalid, "Transport, arena, and response are required");
    crucible_generated_status status = crucible_generated_scope_check(scope, error);
    if (status) return status;
    status = crucible_generated_validate_request(request, error);
    if (status) return status;
    for (;;) {
#ifdef _WIN32
        if (TryAcquireSRWLockExclusive(&state->mutex)) break;
#else
        const int lock = pthread_mutex_trylock(&state->mutex);
        if (!lock) break;
        if (lock != EBUSY) return crucible_generated_fail(error, crucible_generated_network, "Transport lock failed");
#endif
        status = crucible_generated_scope_wait(scope, 0.01, error);
        if (status) return status;
    }
    signals mask;
    const bool blocked = block(&mask);
    CURL *easy = NULL;
    struct curl_slist *headers = NULL;
    bool attached = false;
    transfer self = {0}; self.arena = arena; self.read_body = read_body;
    if (!blocked) { status = crucible_generated_fail(error, crucible_generated_network, "Cannot block network SIGPIPE"); goto done; }
    status = crucible_generated_scope_check(scope, error);
    if (status) goto done;
    easy = curl_easy_init();
    if (!easy) { status = crucible_generated_fail(error, crucible_generated_out_of_memory, "Transport allocation failed"); goto done; }
    crucible_generated_string url = {0}, method = {0};
    status = crucible_generated_arena_copy(arena, request->url, &url, error);
    if (!status) status = crucible_generated_arena_copy(arena, request->method, &method, error);
    if (status) goto done;
#define OPTION(name, value) do { CURLcode code = curl_easy_setopt(easy, name, value); if (code != CURLE_OK) { status = crucible_generated_fail(error, crucible_generated_network, curl_easy_strerror(code)); goto done; } } while (0)
#define MULTI(call) do { CURLMcode code = (call); if (code != CURLM_OK) { status = crucible_generated_fail(error, crucible_generated_network, curl_multi_strerror(code)); goto done; } } while (0)
    OPTION(CURLOPT_URL, url.data); OPTION(CURLOPT_CUSTOMREQUEST, method.data);
    OPTION(CURLOPT_NOSIGNAL, 1L); OPTION(CURLOPT_FOLLOWLOCATION, 0L); OPTION(CURLOPT_MAXREDIRS, 0L);
    OPTION(CURLOPT_PATH_AS_IS, 1L); OPTION(CURLOPT_SUPPRESS_CONNECT_HEADERS, 1L); OPTION(CURLOPT_HTTP_CONTENT_DECODING, 0L);
#if LIBCURL_VERSION_NUM >= 0x075500
    OPTION(CURLOPT_PROTOCOLS_STR, "http,https");
#else
    OPTION(CURLOPT_PROTOCOLS, CURLPROTO_HTTP | CURLPROTO_HTTPS);
#endif
    double milliseconds = ceil(crucible_generated_scope_remaining(scope) * 1000);
    OPTION(CURLOPT_TIMEOUT_MS, milliseconds >= (double)LONG_MAX ? LONG_MAX : milliseconds < 1 ? 1L : (long)milliseconds);
    OPTION(CURLOPT_WRITEFUNCTION, body); OPTION(CURLOPT_WRITEDATA, &self);
    OPTION(CURLOPT_HEADERFUNCTION, header); OPTION(CURLOPT_HEADERDATA, &self);
    if (request->has_body) {
        if (request->body.length > INT64_MAX) { status = crucible_generated_fail(error, crucible_generated_invalid, "Request body too large"); goto done; }
        OPTION(CURLOPT_POSTFIELDSIZE_LARGE, (curl_off_t)request->body.length);
        OPTION(CURLOPT_POSTFIELDS, request->body.data ? (const char *)request->body.data : "");
    }
    if (!strcmp(method.data, "HEAD")) { OPTION(CURLOPT_NOBODY, 1L); }
    for (size_t i = 0; i < request->headers.count + 2; ++i) {
        const char *line;
        if (i >= request->headers.count) line = i == request->headers.count ? "Accept-Encoding: identity" : "Expect:";
        else {
            crucible_generated_header item = request->headers.items[i];
            crucible_generated_headers one = {&item, 1};
            if (crucible_generated_header_get(one, crucible_generated_string_view("accept-encoding"), NULL) || crucible_generated_header_get(one, crucible_generated_string_view("expect"), NULL)) continue;
            if (item.name.length > SIZE_MAX - 3 || item.value.length > SIZE_MAX - item.name.length - 3) { status = crucible_generated_fail(error, crucible_generated_invalid, "Header size overflow"); goto done; }
            char *buffer = crucible_generated_arena_alloc(arena, item.name.length + item.value.length + 3, error);
            if (!buffer) { status = crucible_generated_out_of_memory; goto done; }
            memcpy(buffer, item.name.data, item.name.length);
            buffer[item.name.length] = item.value.length ? ':' : ';';
            if (item.value.length) { buffer[item.name.length + 1] = ' '; memcpy(buffer + item.name.length + 2, item.value.data, item.value.length); }
            line = buffer;
        }
        struct curl_slist *next = curl_slist_append(headers, line);
        if (!next) { status = crucible_generated_fail(error, crucible_generated_out_of_memory, "Header allocation failed"); goto done; }
        headers = next;
    }
    OPTION(CURLOPT_HTTPHEADER, headers);
    MULTI(curl_multi_add_handle(state->multi, easy)); attached = true;
    int running;
    do {
        status = crucible_generated_scope_check(scope, error);
        if (status) goto done;
        MULTI(curl_multi_perform(state->multi, &running));
        if (running) { int pending; MULTI(curl_multi_poll(state->multi, NULL, 0, 25, &pending)); }
    } while (running);
    status = crucible_generated_scope_check(scope, error);
    if (status) goto done;
    if (self.error.status) { status = self.error.status; if (error) *error = self.error; goto done; }
    CURLcode result = CURLE_FAILED_INIT;
    int pending;
    CURLMsg *message;
    while ((message = curl_multi_info_read(state->multi, &pending))) if (message->msg == CURLMSG_DONE && message->easy_handle == easy) result = message->data.result;
    if (!(self.discarded && result == CURLE_WRITE_ERROR) && result != CURLE_OK) { status = crucible_generated_fail(error, result == CURLE_OPERATION_TIMEDOUT ? crucible_generated_deadline : crucible_generated_network, curl_easy_strerror(result)); goto done; }
    if (self.response.status < 100) { status = crucible_generated_fail(error, crucible_generated_network, "Missing HTTP response status"); goto done; }
    if (self.body) {
        status = crucible_generated_arena_own(arena, self.body, free, error);
        if (status) goto done;
        self.response.body = (crucible_generated_blob){self.body, self.length}; self.body = NULL;
    }
    *response = self.response;
done:
    if (attached) (void)curl_multi_remove_handle(state->multi, easy);
    curl_easy_cleanup(easy); curl_slist_free_all(headers); free(self.body);
    if (blocked) unblock(&mask);
#ifdef _WIN32
    ReleaseSRWLockExclusive(&state->mutex);
#else
    (void)pthread_mutex_unlock(&state->mutex);
#endif
    return status;
#undef OPTION
#undef MULTI
}
