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
}
