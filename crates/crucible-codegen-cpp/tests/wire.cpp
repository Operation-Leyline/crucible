#include "wire_client/client.hpp"
#include <iostream>
#include <source_location>

void check(bool condition, std::source_location where = std::source_location::current()) { if (!condition) throw std::runtime_error("Wire check failed at line " + std::to_string(where.line())); }
struct Fake : wire_sdk::runtime::Transport {
    bool failure;
    explicit Fake(bool failure) : failure(failure) {}
    wire_sdk::runtime::HttpResponse send(const wire_sdk::runtime::HttpRequest& request, bool read, const wire_sdk::runtime::CallScope&) override {
        check(read);
        if (request.url.ends_with("/document")) {
            check(request.headers.at("content-type") == "application/json" && wire_sdk::runtime::Json::parse(*request.body).at("name").string() == "record");
            return {200, {}, *request.body};
        }
        check(request.headers.at("content-type") == "text/plain" && *request.body == "h\xc3\xa9llo");
        check(request.headers.at("x-meta") == "eyJhIjoxfQ==, aGVsbG8=" && request.headers.at("x-extra-trace") == "aGVsbG8=");
        return failure ? wire_sdk::runtime::HttpResponse{422, {{"x-crucible-error", "TextError"}}, "invalid text"} : wire_sdk::runtime::HttpResponse{200, {{"x-meta", "eyJhIjoxfQ==, aGVsbG8="}, {"x-extra-trace", "aGVsbG8="}}, *request.body};
    }
    void close() override {}
};
int run() {
    wire_sdk::runtime::ClientConfig config;
    config.endpoint = "http://127.0.0.1"; config.transport = std::make_shared<Fake>(false);
    wire_sdk::models::TextInput input;
    input.text = "h\xc3\xa9llo"; input.meta = std::vector<std::string>{"{\"a\":1}", "hello"}; input.extras = std::map<std::string,std::string>{{"trace","hello"}};
    const auto output = wire_sdk::WireClient(config).text(input);
    check(*output.text == *input.text && *output.meta == *input.meta && *output.extras == *input.extras);
    wire_sdk::models::Record record; record.name = "record";
    wire_sdk::models::DocumentInput document; document.document = record;
    check(*wire_sdk::WireClient(config).document(document).document->name == "record");
    config.transport = std::make_shared<Fake>(true);
    try { (void)wire_sdk::WireClient(config).text(input); throw std::runtime_error("Expected TextError"); }
    catch (const wire_sdk::models::TextError& error) { check(*error.details == "invalid text" && error.metadata().status == 422); }
    std::cout << "C++ payloads and media headers passed\n";
    return 0;
}
int main() {
    try { return run(); }
    catch (const std::exception& error) { std::cerr << error.what() << "\n"; return 1; }
}
