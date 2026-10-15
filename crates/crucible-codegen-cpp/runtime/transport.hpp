#pragma once
#include "call_scope.hpp"
#include "types.hpp"
#include <map>
#include <memory>
#include <optional>
#include <string>

namespace crucible_generated::runtime {
/// One HTTP attempt.
struct HttpRequest {
    HttpRequest() = default;
    HttpRequest(std::string url, std::string method, Headers headers = {}, std::optional<std::string> body = std::nullopt)
        : url(std::move(url)), method(std::move(method)), headers(std::move(headers)), body(std::move(body)) {}
    /// Absolute request URL.
    std::string url;
    /// HTTP method.
    std::string method;
    /// Case-insensitive HTTP headers.
    Headers headers;
    /// Optional buffered request body.
    std::optional<std::string> body;
};
/// HTTP status, headers, and a buffered response body.
struct HttpResponse {
    HttpResponse() = default;
    HttpResponse(int status, Headers headers = {}, std::string body = {})
        : status(status), headers(std::move(headers)), body(std::move(body)) {}
    /// HTTP response status code.
    int status = 0;
    /// Case-insensitive HTTP headers.
    Headers headers;
    /// Buffered response body.
    std::string body;
};
/// HTTP transport interface; implementations must honor CallScope and support concurrent calls if shared.
class Transport {
public:
    virtual ~Transport() = default;
    /// Sends one attempt; read_body chooses whether to buffer the response body.
    virtual HttpResponse send(const HttpRequest& request, bool read_body, const CallScope& scope) = 0;
    /// Releases transport resources; stop using the transport after closing.
    virtual void close() {}
};
/// libcurl transport with pooled buffered requests.
class CurlTransport final : public Transport {
    struct State;
    std::unique_ptr<State> state_;
public:
    CurlTransport();
    ~CurlTransport() override;
    CurlTransport(const CurlTransport&) = delete;
    CurlTransport& operator=(const CurlTransport&) = delete;
    HttpResponse send(const HttpRequest& request, bool read_body, const CallScope& scope) override;
    void close() override;
};
/// Folds ASCII letters to lowercase.
[[nodiscard]] std::string lower(std::string text);
/// Validates endpoints and the final middleware URL, including bearer-token transport.
void validate_url(const std::string& url, bool endpoint, bool credentials = false);
/// Rejects invalid method, URL, or headers.
void validate_request(const HttpRequest& request);
/// RFC 3986 percent-encoding of every byte except unreserved characters.
[[nodiscard]] std::string uri_encode(std::string_view text);
}
