#pragma once
#include "call_scope.hpp"
#include "types.hpp"
#include <map>
#include <memory>
#include <optional>
#include <string>

namespace crucible_generated::runtime {
struct HttpRequest {
    std::string url;
    std::string method;
    Headers headers;
    std::optional<std::string> body;
};
struct HttpResponse {
    int status = 0;
    Headers headers;
    std::string body;
};
class Transport {
public:
    virtual ~Transport() = default;
    virtual HttpResponse send(const HttpRequest& request, bool read_body, const CallScope& scope) = 0;
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
