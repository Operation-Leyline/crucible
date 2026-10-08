#pragma once
#include "call_scope.hpp"
#include "stream.hpp"
#include "types.hpp"
#include <map>
#include <memory>
#include <optional>
#include <string>

namespace crucible_generated::runtime {
/// One HTTP attempt; body and stream are mutually exclusive.
struct HttpRequest {
    HttpRequest() = default;
    HttpRequest(std::string url, std::string method, Headers headers = {}, std::optional<std::string> body = std::nullopt, std::shared_ptr<RequestBody> stream = nullptr)
        : url(std::move(url)), method(std::move(method)), headers(std::move(headers)), body(std::move(body)), stream(std::move(stream)) {}
    /// Absolute request URL.
    std::string url;
    /// HTTP method.
    std::string method;
    /// Case-insensitive HTTP headers.
    Headers headers;
    /// Optional buffered request body.
    std::optional<std::string> body;
    /// A streaming request body, read once as it is sent; never set together with body.
    std::shared_ptr<RequestBody> stream;
};
/// HTTP status, headers, and a buffered or streaming response body.
struct HttpResponse {
    HttpResponse() = default;
    HttpResponse(int status, Headers headers = {}, std::string body = {}, std::shared_ptr<ResponseBody> stream = nullptr)
        : status(status), headers(std::move(headers)), body(std::move(body)), stream(std::move(stream)) {}
    /// HTTP response status code.
    int status = 0;
    /// Case-insensitive HTTP headers.
    Headers headers;
    /// Buffered response body.
    std::string body;
    /// Set by open(): the body, still on the connection, instead of body.
    std::shared_ptr<ResponseBody> stream;
};
/// HTTP transport interface; implementations must honor CallScope and support concurrent calls if shared.
class Transport {
public:
    virtual ~Transport() = default;
    /// Sends one attempt; read_body chooses whether to buffer the response body.
    virtual HttpResponse send(const HttpRequest& request, bool read_body, const CallScope& scope) = 0;
    /// Sends a request that may carry a streaming body and returns at the response headers,
    /// with the body in HttpResponse::stream. The default buffers both bodies through send().
    virtual HttpResponse open(const HttpRequest& request, const CallScope& scope);
    /// Releases transport resources; stop using the transport after closing.
    virtual void close() {}
};
/// libcurl transport with pooled buffered requests and dedicated streaming connections.
class CurlTransport final : public Transport {
    struct State;
    std::unique_ptr<State> state_;
public:
    CurlTransport();
    ~CurlTransport() override;
    CurlTransport(const CurlTransport&) = delete;
    CurlTransport& operator=(const CurlTransport&) = delete;
    HttpResponse send(const HttpRequest& request, bool read_body, const CallScope& scope) override;
    /// Streams both bodies on a dedicated connection that the response body releases.
    HttpResponse open(const HttpRequest& request, const CallScope& scope) override;
    void close() override;
};
/// Folds ASCII letters to lowercase.
[[nodiscard]] std::string lower(std::string text);
/// Validates endpoints and the final middleware URL, including credential transport.
void validate_url(const std::string& url, bool endpoint, bool credentials = false);
/// Rejects invalid method, URL, headers, or conflicting body sources.
void validate_request(const HttpRequest& request);
/// RFC 3986 percent-encoding of every byte except unreserved characters.
[[nodiscard]] std::string uri_encode(std::string_view text);
/// Percent-decoding; malformed escapes are rejected.
[[nodiscard]] std::string uri_decode(std::string_view text);
/// Raw components of a validated absolute URL.
struct UrlParts {
    std::string scheme;
    std::string authority;
    std::string path;
    std::string query;
};
/// Splits an already validated absolute URL without decoding it.
[[nodiscard]] UrlParts split_url(std::string_view url);
}
