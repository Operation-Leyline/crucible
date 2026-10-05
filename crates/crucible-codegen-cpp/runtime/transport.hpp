#pragma once
#include "call_scope.hpp"
#include "stream.hpp"
#include "types.hpp"
#include <map>
#include <memory>
#include <optional>
#include <string>

namespace crucible_generated::runtime {
struct HttpRequest {
    HttpRequest() = default;
    HttpRequest(std::string url, std::string method, Headers headers = {}, std::optional<std::string> body = std::nullopt, std::shared_ptr<RequestBody> stream = nullptr)
        : url(std::move(url)), method(std::move(method)), headers(std::move(headers)), body(std::move(body)), stream(std::move(stream)) {}
    std::string url;
    std::string method;
    Headers headers;
    std::optional<std::string> body;
    // A streaming request body, read once as it is sent; never set together with body.
    std::shared_ptr<RequestBody> stream;
};
struct HttpResponse {
    HttpResponse() = default;
    HttpResponse(int status, Headers headers = {}, std::string body = {}, std::shared_ptr<ResponseBody> stream = nullptr)
        : status(status), headers(std::move(headers)), body(std::move(body)), stream(std::move(stream)) {}
    int status = 0;
    Headers headers;
    std::string body;
    // Set by open(): the body, still on the connection, instead of body.
    std::shared_ptr<ResponseBody> stream;
};
class Transport {
public:
    virtual ~Transport() = default;
    virtual HttpResponse send(const HttpRequest& request, bool read_body, const CallScope& scope) = 0;
    // Sends a request that may carry a streaming body and returns at the response headers,
    // with the body in HttpResponse::stream. The default buffers both bodies through send().
    virtual HttpResponse open(const HttpRequest& request, const CallScope& scope);
    virtual void close() {}
};
class CurlTransport final : public Transport {
    struct State;
    std::unique_ptr<State> state_;
public:
    CurlTransport();
    ~CurlTransport() override;
    CurlTransport(const CurlTransport&) = delete;
    CurlTransport& operator=(const CurlTransport&) = delete;
    HttpResponse send(const HttpRequest& request, bool read_body, const CallScope& scope) override;
    // Streams both bodies on a dedicated connection that the response body releases.
    HttpResponse open(const HttpRequest& request, const CallScope& scope) override;
    void close() override;
};
[[nodiscard]] std::string lower(std::string text);
// Validates endpoints and the final middleware URL, including credential transport.
void validate_url(const std::string& url, bool endpoint, bool credentials = false);
void validate_request(const HttpRequest& request);
// RFC 3986 percent-encoding of every byte except unreserved characters.
[[nodiscard]] std::string uri_encode(std::string_view text);
// Percent-decoding; malformed escapes are rejected.
[[nodiscard]] std::string uri_decode(std::string_view text);
struct UrlParts {
    std::string scheme;
    std::string authority;
    std::string path;
    std::string query;
};
// Splits an already validated absolute URL without decoding it.
[[nodiscard]] UrlParts split_url(std::string_view url);
}
