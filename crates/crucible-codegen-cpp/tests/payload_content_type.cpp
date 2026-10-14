// Payload Content-Type precedence: a modelled Content-Type member, then the payload's
// media type, then a client-wide default header. Compiled once per protocol:
// PAYLOAD_HEADER and PAYLOAD_NAMESPACE name the generated package.
#include PAYLOAD_HEADER
#include <iostream>
#include <memory>
#include <optional>
#include <stdexcept>
#include <string>

namespace sdk = PAYLOAD_NAMESPACE;
namespace rt = PAYLOAD_NAMESPACE::runtime;

void check(bool condition, const std::string& message) { if (!condition) throw std::runtime_error("Payload Content-Type check failed: " + message); }
struct Fake : rt::Transport {
    std::string expected;
    explicit Fake(std::string expected) : expected(std::move(expected)) {}
    // Streams reach send() buffered, through Transport::open's default.
    rt::HttpResponse send(const rt::HttpRequest& request, bool, const rt::CallScope&) override {
        check(request.body && *request.body == "PNG", "body");
        std::size_t seen = 0;
        for (const auto& [name, value] : request.headers) {
            std::string lowered;
            for (const char c : name) lowered += c >= 'A' && c <= 'Z' ? static_cast<char>(c + 32) : c;
            if (lowered == "content-type") { ++seen; check(value == expected, "Content-Type " + value + " != " + expected); }
        }
        check(seen == 1, "exactly one Content-Type header");
        return {200, {}, {}};
    }
};
int run() {
    for (const bool streaming : {false, true}) {
        for (const std::optional<std::string>& member : {std::optional<std::string>{}, std::optional<std::string>{"image/png"}}) {
            for (const bool configured : {false, true}) {
                rt::ClientConfig config;
                config.endpoint = "http://127.0.0.1"; config.max_attempts = 1;
                config.transport = std::make_shared<Fake>(member.value_or("application/octet-stream"));
                if (configured) config.headers.insert_or_assign("Content-Type", "image/jpeg");
                const sdk::PayloadBindingsClient client(config);
                if (streaming) {
                    sdk::models::StreamInput input;
                    input.data = rt::ByteStream::from_string("PNG");
                    if (member) input.content_type = *member;
                    (void)client.upload_stream(input);
                } else {
                    sdk::models::UploadInput input;
                    input.data = rt::Blob{std::byte{'P'}, std::byte{'N'}, std::byte{'G'}};
                    if (member) input.content_type = *member;
                    (void)client.upload(input);
                }
            }
        }
    }
    std::cout << "C++ payload Content-Type precedence passed\n";
    return 0;
}
int main() {
    try { return run(); }
    catch (const std::exception& error) { std::cerr << error.what() << "\n"; return 1; }
}
