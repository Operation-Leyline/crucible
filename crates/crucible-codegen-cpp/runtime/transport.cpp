#include "transport.hpp"
#include <curl/curl.h>
#include <charconv>
#include <climits>
#include <atomic>
#include <cmath>
#include <cstring>
#include <exception>
#include <limits>
#include <mutex>
#include <string_view>
#ifdef __linux__
#include <cerrno>
#include <pthread.h>
#include <signal.h>
#endif

namespace crucible_generated::runtime {
namespace {
// OpenSSL can write to a closed socket even with libcurl's NOSIGNAL option.
// Block only this thread during owned network work; preserve the caller's mask.
class SigpipeBlock {
#ifdef __linux__
    sigset_t old_{};
    sigset_t pipe_{};
    bool consume_ = false;
public:
    SigpipeBlock() {
        sigemptyset(&pipe_);
        sigaddset(&pipe_, SIGPIPE);
        if (pthread_sigmask(SIG_BLOCK, &pipe_, &old_) != 0) throw std::runtime_error("Cannot block network SIGPIPE");
        sigset_t pending{};
        sigpending(&pending);
        consume_ = sigismember(&old_, SIGPIPE) == 0 && sigismember(&pending, SIGPIPE) == 0;
    }
    ~SigpipeBlock() {
        if (consume_) {
            const timespec immediate{};
            while (sigtimedwait(&pipe_, nullptr, &immediate) < 0 && errno == EINTR) {}
        }
        pthread_sigmask(SIG_SETMASK, &old_, nullptr);
    }
#endif
};
void initialize() {
    // Process lifetime initialization avoids racing another client's active handles.
    static const auto result = curl_global_init(CURL_GLOBAL_DEFAULT);
    if (result != CURLE_OK) throw NetworkError(curl_easy_strerror(result));
}
struct UrlDeleter { void operator()(CURLU* value) const { curl_url_cleanup(value); } };
struct CurlStringDeleter { void operator()(char* value) const { curl_free(value); } };
std::optional<std::string> part(CURLU* url, CURLUPart name) {
    char* raw = nullptr;
    const auto code = curl_url_get(url, name, &raw, 0);
    std::unique_ptr<char, CurlStringDeleter> owned(raw);
    if (code == CURLUE_OK) return std::string(raw);
    return std::nullopt;
}
bool token(std::string_view text) {
    if (text.empty()) return false;
    for (const unsigned char byte : text) if (!((byte >= 'a' && byte <= 'z') || (byte >= 'A' && byte <= 'Z') || (byte >= '0' && byte <= '9') || std::string_view("!#$%&'*+-.^_`|~").find(static_cast<char>(byte)) != std::string_view::npos)) return false;
    return true;
}
struct EasyDeleter { void operator()(CURL* value) const { curl_easy_cleanup(value); } };
struct ListDeleter { void operator()(curl_slist* value) const { curl_slist_free_all(value); } };
template<class T> void option(CURL* easy, CURLoption name, T value) {
    const auto code = curl_easy_setopt(easy, name, value);
    if (code != CURLE_OK) throw NetworkError(curl_easy_strerror(code));
}
void multi_check(CURLMcode code) { if (code != CURLM_OK) throw NetworkError(curl_multi_strerror(code)); }
// Received bytes a streaming body may hold before the transfer pauses until the reader drains them.
constexpr std::size_t stream_buffer_limit = 1 << 20;
struct Transfer {
    HttpResponse response;
    bool read_body = true;
    bool discarded = false;
    bool streaming = false;
    bool headers_done = false;
    bool paused = false;
    std::size_t offset = 0;
    std::shared_ptr<RequestBody> upload;
    std::exception_ptr failure;
    static size_t body(char* bytes, size_t size, size_t count, void* opaque) noexcept {
        auto& self = *static_cast<Transfer*>(opaque);
        try {
            if (size && count > (std::numeric_limits<size_t>::max)() / size) throw NetworkError("Response size overflow");
            if (self.streaming) {
                // Pull-based: hold at most one buffer of unread bytes, then let TCP apply backpressure.
                if (self.response.body.size() - self.offset >= stream_buffer_limit) { self.paused = true; return CURL_WRITEFUNC_PAUSE; }
                if (self.offset == self.response.body.size()) { self.response.body.clear(); self.offset = 0; }
            }
            self.response.body.append(bytes, size * count);
            return size * count;
        } catch (...) { self.failure = std::current_exception(); return 0; }
    }
    static size_t read(char* buffer, size_t size, size_t count, void* opaque) noexcept {
        auto& self = *static_cast<Transfer*>(opaque);
        try {
            if (size && count > (std::numeric_limits<size_t>::max)() / size) throw NetworkError("Upload size overflow");
            return self.upload->read(buffer, size * count);
        } catch (...) { self.failure = std::current_exception(); return CURL_READFUNC_ABORT; }
    }
    static size_t header(char* bytes, size_t size, size_t count, void* opaque) noexcept {
        auto& self = *static_cast<Transfer*>(opaque);
        try {
            if (size && count > (std::numeric_limits<size_t>::max)() / size) throw NetworkError("Header size overflow");
            const std::string_view line(bytes, size * count);
            if (line.starts_with("HTTP/")) {
                self.response = {};
                const auto space = line.find(' ');
                if (space == std::string_view::npos || space + 4 > line.size()) throw NetworkError("Invalid HTTP status");
                const auto status = line.substr(space + 1, 3);
                const auto parsed = std::from_chars(status.data(), status.data() + status.size(), self.response.status);
                if (parsed.ec != std::errc() || parsed.ptr != status.data() + status.size()) throw NetworkError("Invalid HTTP status");
            } else if (line == "\r\n" || line == "\n") {
                if (self.response.status >= 200) self.headers_done = true;
                if (!self.read_body && self.response.status >= 200 && self.response.status < 300) { self.discarded = true; return 0; }
            } else {
                const auto colon = line.find(':');
                if (colon != std::string_view::npos) {
                    auto value = line.substr(colon + 1);
                    while (!value.empty() && (value.front() == ' ' || value.front() == '\t')) value.remove_prefix(1);
                    while (!value.empty() && (value.back() == ' ' || value.back() == '\t' || value.back() == '\r' || value.back() == '\n')) value.remove_suffix(1);
                    const auto name = lower(std::string(line.substr(0, colon)));
                    const auto [item, inserted] = self.response.headers.emplace(name, value);
                    if (!inserted) item->second += ", " + std::string(value);
                }
            }
            return size * count;
        } catch (...) { self.failure = std::current_exception(); return 0; }
    }
};
using HeaderList = std::unique_ptr<curl_slist, ListDeleter>;
// Applies a request to an easy handle; the request, transfer, and header list must outlive the transfer.
HeaderList configure(CURL* easy, const HttpRequest& request, Transfer& transfer, const CallScope& scope) {
    option(easy, CURLOPT_URL, request.url.c_str());
    option(easy, CURLOPT_CUSTOMREQUEST, request.method.c_str());
    option(easy, CURLOPT_NOSIGNAL, 1L);
    option(easy, CURLOPT_FOLLOWLOCATION, 0L);
    option(easy, CURLOPT_MAXREDIRS, 0L);
    option(easy, CURLOPT_PATH_AS_IS, 1L);
    option(easy, CURLOPT_SUPPRESS_CONNECT_HEADERS, 1L);
    option(easy, CURLOPT_HTTP_CONTENT_DECODING, 0L);
#if LIBCURL_VERSION_NUM >= 0x075500
    option(easy, CURLOPT_PROTOCOLS_STR, "http,https");
#else
    option(easy, CURLOPT_PROTOCOLS, CURLPROTO_HTTP | CURLPROTO_HTTPS);
#endif
    const double milliseconds = std::ceil(scope.remaining() * 1000.0);
    option(easy, CURLOPT_TIMEOUT_MS, milliseconds >= static_cast<double>(LONG_MAX) ? LONG_MAX : (std::max)(1L, static_cast<long>(milliseconds)));
    option(easy, CURLOPT_WRITEFUNCTION, &Transfer::body);
    option(easy, CURLOPT_WRITEDATA, &transfer);
    option(easy, CURLOPT_HEADERFUNCTION, &Transfer::header);
    option(easy, CURLOPT_HEADERDATA, &transfer);
    if (request.body) {
        if (request.body->size() > static_cast<std::size_t>((std::numeric_limits<curl_off_t>::max)())) throw std::invalid_argument("Request body too large");
        option(easy, CURLOPT_POSTFIELDSIZE_LARGE, static_cast<curl_off_t>(request.body->size()));
        option(easy, CURLOPT_POSTFIELDS, request.body->data());
    } else if (request.stream) {
        // A known length is sent as Content-Length; an unknown one is chunked.
        const auto length = request.stream->length();
        if (length && *length > static_cast<std::uint64_t>((std::numeric_limits<curl_off_t>::max)())) throw std::invalid_argument("Request body too large");
        transfer.upload = request.stream;
        option(easy, CURLOPT_POST, 1L);
        option(easy, CURLOPT_READFUNCTION, &Transfer::read);
        option(easy, CURLOPT_READDATA, &transfer);
        option(easy, CURLOPT_POSTFIELDSIZE_LARGE, length ? static_cast<curl_off_t>(*length) : static_cast<curl_off_t>(-1));
    }
    if (request.method == "HEAD") option(easy, CURLOPT_NOBODY, 1L);
    HeaderList headers;
    auto append = [&](const std::string& text) { auto* next = curl_slist_append(headers.get(), text.c_str()); if (!next) throw std::bad_alloc(); (void)headers.release(); headers.reset(next); };
    for (const auto& [name, value] : request.headers) {
        const auto key = lower(name);
        // libcurl owns framing: Content-Length and Transfer-Encoding follow the body.
        if (key != "accept-encoding" && key != "expect" && key != "content-length" && key != "transfer-encoding") append(name + (value.empty() ? ";" : ": " + value));
    }
    append("Accept-Encoding: identity");
    append("Expect:");
    option(easy, CURLOPT_HTTPHEADER, headers.get());
    return headers;
}
CURLcode finished(CURLM* multi, CURL* easy) {
    CURLcode result = CURLE_FAILED_INIT;
    int pending = 0;
    while (const auto* message = curl_multi_info_read(multi, &pending)) if (message->msg == CURLMSG_DONE && message->easy_handle == easy) result = message->data.result;
    return result;
}
void raise(const Transfer& transfer, CURLcode result) {
    // The caller's stream failure or a callback error wins over the transport error it caused.
    if (transfer.failure) std::rethrow_exception(transfer.failure);
    if (result == CURLE_OPERATION_TIMEDOUT) throw DeadlineError();
    if (result != CURLE_OK) throw NetworkError(curl_easy_strerror(result));
}
// A streaming exchange on its own connection: returned at the headers, read on demand.
class CurlStream final : public ResponseBody {
    HttpRequest request_;
    CallScope scope_;
    CURLM* multi_ = nullptr;
    std::unique_ptr<CURL, EasyDeleter> easy_;
    HeaderList headers_;
    std::mutex mutex_;
    // Guards the handles against close() waking a reader while that reader releases them.
    std::mutex handles_;
    std::atomic<bool> closed_ = false;
    bool done_ = false;
    CURLcode result_ = CURLE_OK;
    void release() noexcept {
        std::lock_guard guard(handles_);
        if (!multi_) return;
        try { [[maybe_unused]] const SigpipeBlock signals; curl_multi_remove_handle(multi_, easy_.get()); easy_.reset(); curl_multi_cleanup(multi_); } catch (...) {}
        multi_ = nullptr;
    }
    // Drives the transfer once; returns false when it has finished.
    bool step() {
        scope_.check();
        [[maybe_unused]] const SigpipeBlock signals;
        int running = 0;
        multi_check(curl_multi_perform(multi_, &running));
        if (!running) { done_ = true; result_ = finished(multi_, easy_.get()); return false; }
        int ready = 0;
        multi_check(curl_multi_poll(multi_, nullptr, 0, 25, &ready));
        return true;
    }
public:
    Transfer transfer;
    CurlStream(const HttpRequest& request, const CallScope& scope) : request_(request), scope_(scope) {
        transfer.streaming = true;
        multi_ = curl_multi_init();
        if (!multi_) throw std::bad_alloc();
        easy_.reset(curl_easy_init());
        if (!easy_) { curl_multi_cleanup(multi_); multi_ = nullptr; throw std::bad_alloc(); }
    }
    ~CurlStream() override { release(); }
    CurlStream(const CurlStream&) = delete;
    CurlStream& operator=(const CurlStream&) = delete;
    void start() {
        headers_ = configure(easy_.get(), request_, transfer, scope_);
        multi_check(curl_multi_add_handle(multi_, easy_.get()));
        try {
            while (!transfer.headers_done && step()) {}
            if (transfer.failure) std::rethrow_exception(transfer.failure);
            if (!transfer.headers_done) { raise(transfer, result_); throw NetworkError("Missing HTTP response headers"); }
        } catch (...) { release(); throw; }
    }
    std::size_t read(char* buffer, std::size_t size) override {
        std::lock_guard lock(mutex_);
        for (;;) {
            if (closed_) { release(); throw StreamClosedError(); }
            try { scope_.check(); } catch (...) { release(); throw; }
            auto& data = transfer.response.body;
            if (transfer.offset < data.size()) {
                const auto count = (std::min)(size, data.size() - transfer.offset);
                std::memcpy(buffer, data.data() + transfer.offset, count);
                transfer.offset += count;
                if (transfer.offset == data.size() && transfer.paused && multi_) {
                    data.clear(); transfer.offset = 0; transfer.paused = false;
                    [[maybe_unused]] const SigpipeBlock signals;
                    curl_easy_pause(easy_.get(), CURLPAUSE_CONT);
                }
                return count;
            }
            if (done_ || !multi_) {
                const auto result = result_;
                release();
                raise(transfer, result);
                return 0;
            }
            try { (void)step(); } catch (...) { release(); throw; }
        }
    }
    void close() noexcept override {
        if (closed_.exchange(true)) return;
        { std::lock_guard guard(handles_); if (multi_) curl_multi_wakeup(multi_); }
        // A reader blocked in another thread notices the flag and releases the connection itself.
        std::unique_lock lock(mutex_, std::try_to_lock);
        if (lock.owns_lock()) release();
    }
};
}
std::string lower(std::string text) { for (char& byte : text) if (byte >= 'A' && byte <= 'Z') byte = static_cast<char>(byte + ('a' - 'A')); return text; }
std::string uri_encode(std::string_view text) {
    constexpr char hex[] = "0123456789ABCDEF";
    std::string output;
    for (const unsigned char byte : text) {
        if ((byte >= 'a' && byte <= 'z') || (byte >= 'A' && byte <= 'Z') || (byte >= '0' && byte <= '9') || byte == '-' || byte == '.' || byte == '_' || byte == '~') output += static_cast<char>(byte);
        else { output += '%'; output += hex[byte >> 4]; output += hex[byte & 15]; }
    }
    return output;
}
std::string uri_decode(std::string_view text) {
    std::string result;
    for (std::size_t i = 0; i < text.size(); ++i) {
        if (text[i] != '%') { result += text[i]; continue; }
        unsigned byte = 0;
        if (i + 2 >= text.size()) throw std::invalid_argument("Invalid percent-encoding");
        const auto parsed = std::from_chars(text.data() + i + 1, text.data() + i + 3, byte, 16);
        if (parsed.ec != std::errc() || parsed.ptr != text.data() + i + 3) throw std::invalid_argument("Invalid percent-encoding");
        result += static_cast<char>(byte);
        i += 2;
    }
    return result;
}
UrlParts split_url(std::string_view url) {
    UrlParts result;
    const auto scheme = url.find("://");
    if (scheme == std::string_view::npos) throw std::invalid_argument("Absolute URL required");
    result.scheme = lower(std::string(url.substr(0, scheme)));
    auto rest = url.substr(scheme + 3);
    if (const auto fragment = rest.find('#'); fragment != std::string_view::npos) rest = rest.substr(0, fragment);
    const auto end = rest.find_first_of("/?");
    result.authority = std::string(rest.substr(0, end));
    rest = end == std::string_view::npos ? std::string_view{} : rest.substr(end);
    const auto question = rest.find('?');
    result.path = std::string(rest.substr(0, question));
    if (question != std::string_view::npos) result.query = std::string(rest.substr(question + 1));
    return result;
}
void validate_url(const std::string& text, bool endpoint, bool credentials) {
    initialize();
    for (const unsigned char byte : text) if (byte <= 32 || byte == 127) throw std::invalid_argument("Invalid URL character");
    std::unique_ptr<CURLU, UrlDeleter> url(curl_url());
    if (!url) throw std::bad_alloc();
    if (curl_url_set(url.get(), CURLUPART_URL, text.c_str(), 0) != CURLUE_OK) throw std::invalid_argument("Invalid absolute HTTP(S) URL");
    const auto scheme = part(url.get(), CURLUPART_SCHEME);
    const auto host = part(url.get(), CURLUPART_HOST);
    if (!scheme || (*scheme != "http" && *scheme != "https") || !host || host->empty() || part(url.get(), CURLUPART_USER) || part(url.get(), CURLUPART_PASSWORD)) throw std::invalid_argument("Absolute HTTP(S) URL without credentials required");
    if (part(url.get(), CURLUPART_FRAGMENT) || (endpoint && part(url.get(), CURLUPART_QUERY))) throw std::invalid_argument("Endpoint cannot contain query or fragment");
    const auto hostname = lower(*host);
    if (credentials && *scheme != "https" && hostname != "localhost" && hostname != "127.0.0.1" && hostname != "[::1]") throw std::invalid_argument("Credentials require HTTPS or loopback HTTP");
}
void validate_request(const HttpRequest& request) {
    if (!token(request.method)) throw std::invalid_argument("Invalid HTTP method");
    if (request.method == "HEAD" && ((request.body && !request.body->empty()) || request.stream)) throw std::invalid_argument("libcurl HEAD requests cannot carry a body");
    if (request.body && request.stream) throw std::invalid_argument("A request has either a body or a stream");
    bool credentials = false;
    for (const auto& [name, value] : request.headers) {
        if (!token(name) || value.find_first_of(std::string("\r\n\0", 3)) != std::string::npos) throw std::invalid_argument("Invalid HTTP header");
        if (lower(name) == "authorization") credentials = true;
    }
    validate_url(request.url, false, credentials);
}
struct CurlTransport::State {
    std::mutex mutex;
    CURLM* multi = nullptr;
    bool closed = false;
    State() { initialize(); if (!(curl_version_info(CURLVERSION_NOW)->features & CURL_VERSION_ASYNCHDNS)) throw std::runtime_error("libcurl asynchronous DNS support is required"); multi = curl_multi_init(); if (!multi) throw std::bad_alloc(); }
    ~State() {
        if (multi) {
            // A failed signal-mask setup cannot safely perform TLS teardown.
            try { [[maybe_unused]] const SigpipeBlock signals; curl_multi_cleanup(multi); } catch (...) {}
        }
    }
};
CurlTransport::CurlTransport() : state_(std::make_unique<State>()) {}
CurlTransport::~CurlTransport() = default;
void CurlTransport::close() { std::lock_guard lock(state_->mutex); [[maybe_unused]] const SigpipeBlock signals; state_->closed = true; if (state_->multi) { curl_multi_cleanup(state_->multi); state_->multi = nullptr; } }
HttpResponse CurlTransport::send(const HttpRequest& request, bool read_body, const CallScope& scope) {
    scope.check();
    validate_request(request);
    std::unique_lock lock(state_->mutex, std::defer_lock);
    while (!lock.try_lock()) scope.wait(0.01);
    scope.check();
    if (state_->closed) throw std::logic_error("Transport is closed");
    [[maybe_unused]] const SigpipeBlock signals;
    std::unique_ptr<CURL, EasyDeleter> easy(curl_easy_init());
    if (!easy) throw std::bad_alloc();
    Transfer transfer;
    transfer.read_body = read_body;
    const auto headers = configure(easy.get(), request, transfer, scope);
    multi_check(curl_multi_add_handle(state_->multi, easy.get()));
    struct Remove { CURLM* multi; CURL* easy; ~Remove() { curl_multi_remove_handle(multi, easy); } } remove{state_->multi, easy.get()};
    int running = 0;
    do {
        scope.check();
        multi_check(curl_multi_perform(state_->multi, &running));
        if (!running) break;
        int ready = 0;
        multi_check(curl_multi_poll(state_->multi, nullptr, 0, 25, &ready));
    } while (running);
    scope.check();
    const auto result = finished(state_->multi, easy.get());
    if (transfer.discarded && result == CURLE_WRITE_ERROR && !transfer.failure) return std::move(transfer.response);
    raise(transfer, result);
    if (transfer.response.status < 100) throw NetworkError("Missing HTTP response status");
    return std::move(transfer.response);
}
HttpResponse CurlTransport::open(const HttpRequest& request, const CallScope& scope) {
    scope.check();
    validate_request(request);
    {
        std::lock_guard lock(state_->mutex);
        if (state_->closed) throw std::logic_error("Transport is closed");
    }
    // A dedicated connection: the caller may hold the body open while the client sends other requests.
    auto stream = std::make_shared<CurlStream>(request, scope);
    stream->start();
    HttpResponse response;
    response.status = stream->transfer.response.status;
    response.headers = stream->transfer.response.headers;
    response.stream = std::move(stream);
    return response;
}
HttpResponse Transport::open(const HttpRequest& request, const CallScope& scope) {
    auto buffered = request;
    if (buffered.stream) { buffered.body = std::string{}; char chunk[65536]; while (const auto count = buffered.stream->read(chunk, sizeof(chunk))) buffered.body->append(chunk, count); buffered.stream.reset(); }
    auto response = send(buffered, true, scope);
    response.stream = std::make_shared<MemoryBody>(std::move(response.body));
    response.body.clear();
    return response;
}
}
