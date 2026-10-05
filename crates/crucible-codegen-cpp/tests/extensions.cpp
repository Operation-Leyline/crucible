#include "dynamic_client/client.hpp"
#include "types_client/models.hpp"
#include "types_client/schema.hpp"
#include <iostream>

void check(bool condition, const char* message) { if (!condition) throw std::runtime_error(message); }
template<class F> void rejects(F work) { try { work(); } catch (const std::invalid_argument&) { return; } throw std::runtime_error("Accepted invalid value"); }
struct Fake : dynamic::runtime::Transport {
    dynamic::runtime::HttpResponse response;
    explicit Fake(dynamic::runtime::HttpResponse response) : response(std::move(response)) {}
    dynamic::runtime::HttpResponse send(const dynamic::runtime::HttpRequest& request, bool read, const dynamic::runtime::CallScope&) override {
        check(request.method == "POST", "method");
        check(request.url == "http://127.0.0.1/files/folder/a%20b?empty=&tag=hello%20world&tag=%2F", "URI");
        check(request.headers.at("x-meta-trace") == "abc", "prefix");
        check(dynamic::runtime::Json::parse(*request.body).at("wire-value").string() == "hello", "jsonName");
        check(read, "read body");
        return response;
    }
    void close() override {}
};
int main() {
    dynamic::runtime::ClientConfig config;
    config.endpoint = "http://127.0.0.1";
    config.transport = std::make_shared<Fake>(dynamic::runtime::HttpResponse{206, {{"X-Meta-Trace", "abc"}}, "{\"wire-value\":\"result\",\"value\":\"ignored\"}"});
    dynamic::models::Input input("folder/a b");
    input.query = std::map<std::string, std::vector<std::string>>{{"tag", {"hello world", "/"}}, {"empty", {""}}};
    input.metadata_value = std::map<std::string, std::string>{{"trace", "abc"}};
    input.value = "hello";
    auto output = dynamic::DynamicClient(config).get(input);
    check(*output.status == 206 && output.metadata_value->at("trace") == "abc" && *output.value == "result", "output");
    config.transport = std::make_shared<Fake>(dynamic::runtime::HttpResponse{422, {{"x-crucible-error", "Problem"}, {"x-meta-reason", "invalid"}}, "{}"});
    try { (void)dynamic::DynamicClient(config).get(input); throw std::runtime_error("Expected Problem"); }
    catch (const dynamic::models::Problem& error) { check(*error.status == 422 && error.details->at("reason") == "invalid", "error"); }
    for (const auto* key : {"a/../b", "./a", "a/."}) rejects([&] { (void)dynamic::DynamicClient(config).get(dynamic::models::Input(key)); });
    using namespace types::runtime;
    const auto context = types::schema().context("example.types#Output");
    const auto node = Json::parse("{\"mode\":123,\"names\":[\"a\",\"b\"],\"sparseNames\":[null,\"x\"],\"sparseMap\":{\"absent\":null,\"future\":123}}");
    const auto model = Codec<types::models::Output>::decode(node, context);
    check(model.integer.text() == "12345678901234567890123456789012345678901234567890", "integer");
    check(model.decimal.text() == "12345678901234567890.12345678901234567890123456789", "decimal");
    check(model.mode->value == 123 && model.sparse_map->at("future")->value == 123, "future enum");
    check(!model.sparse_names->at(0) && model.sparse_map->contains("absent") && !model.sparse_map->at("absent"), "sparse");
    check(equivalent(Codec<types::models::Output>::encode(model, context).at("sparseMap"), node.at("sparseMap")), "roundtrip");
    rejects([&] { (void)Codec<std::vector<std::string>>::decode(Json::parse("[\"a\",\"a\"]"), types::schema().context("example.types#Names")); });
    rejects([&] { (void)Codec<std::vector<std::string>>::decode(Json::parse("[null]"), types::schema().context("example.types#Names")); });
    rejects([&] { (void)Codec<types::models::Mode>::decode(Json(Number("2147483648")), types::schema().context("example.types#Mode")); });
    check(BigDecimal("1e999999999999999999999999") == BigDecimal("10e999999999999999999999998"), "exact equality");
    check(!(BigDecimal("1.000000000000000000001") == BigDecimal("1")), "exact inequality");
    rejects([] { (void)BigInteger("1.000000000000000000001"); });
    (void)Codec<std::vector<BigInteger>>::decode(Json::parse("[18446744073709551616,18446744073709551617]"), types::schema().context("example.types#Integers"));
    (void)Codec<std::vector<BigDecimal>>::decode(Json::parse("[1.00000000000000000001,1.00000000000000000002]"), types::schema().context("example.types#Decimals"));
    rejects([&] { (void)Codec<std::vector<BigInteger>>::decode(Json::parse("[1,1.0]"), types::schema().context("example.types#Integers")); });
    rejects([&] { (void)Codec<std::vector<BigDecimal>>::decode(Json::parse("[1,1.0]"), types::schema().context("example.types#Decimals")); });
    std::cout << "C++ dynamic bindings and exact extended types passed\n";
}
