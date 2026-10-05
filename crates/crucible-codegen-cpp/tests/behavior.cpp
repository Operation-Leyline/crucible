#include "behavior_client/client.hpp"
#define SCRIPTED_RUNTIME behavior_sdk::runtime
#include "scripted.hpp"
#include <functional>

namespace sdk = behavior_sdk;
using sdk::models::Input;

struct Fake : rt::Transport {
    std::function<rt::HttpResponse(const rt::HttpRequest&)> handler;
    std::vector<rt::HttpRequest> requests;
    explicit Fake(std::function<rt::HttpResponse(const rt::HttpRequest&)> handler) : handler(std::move(handler)) {}
    rt::HttpResponse send(const rt::HttpRequest& request, bool, const rt::CallScope&) override { requests.push_back(request); return handler(request); }
};
std::string host(const std::string& url) { return rt::split_url(url).authority; }
std::string header(const rt::HttpRequest& request, const std::string& name) { const auto found = request.headers.find(name); return found == request.headers.end() ? std::string{} : found->second; }

void generated_endpoint_rules() {
    auto fake = std::make_shared<Fake>([](const rt::HttpRequest& request) {
        check(header(request, "x-region") == "local", "endpoint rule header");
        if (request.url.ends_with("/public")) { check(!request.headers.contains("authorization"), "anonymous optional operation"); return rt::HttpResponse{200, {}, ""}; }
        check(header(request, "authorization").find("/local/example/aws4_request") != std::string::npos, "endpoint signing region and name: " + header(request, "authorization"));
        check(header(request, "x-amz-content-sha256") == "UNSIGNED-PAYLOAD", "unsigned payload");
        return rt::HttpResponse{200, {}, R"({"wire-status":"READY"})"};
    });
    rt::ClientConfig config;
    config.transport = fake;
    config.region = "local";
    config.credentials = [](const rt::CallScope&) { return std::optional<rt::AwsCredentials>(rt::AwsCredentials{"key", "secret", "", std::nullopt}); };
    sdk::ServiceClient client(config);
    const auto output = client.read(Input("customer"));
    check(output.status && *output.status == "READY", "decoded output");
    check(host(fake->requests.back().url) == "customer.local.example.test", "resolved endpoint with host prefix: " + fake->requests.back().url);
    for (const auto* label : {"a.b", "-bad", "x/y", "x@evil", "", "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"}) {
        (void)rejects<rt::EndpointError>([&] { (void)client.read(Input(std::string(label))); }, std::string("invalid host label ") + label);
    }
    check(fake->requests.size() == 1, "invalid host labels never dispatch");
    const auto ready = client.wait_until_ready(Input("customer"), 5, rt::WaiterOptions{0.0, 0.0});
    check(ready && *ready->status == "READY", "waiter over endpoint rules");
    config.disable_host_prefix = true;
    (void)sdk::ServiceClient(config).read(Input("ignored"));
    check(host(fake->requests.back().url) == "local.example.test", "host prefix can be disabled");
    config.region = "missing";
    check(rejects<rt::EndpointError>([&] { (void)sdk::ServiceClient(config).read(Input("customer")); }, "rule error").find("Unknown region missing") != std::string::npos, "endpoint rule error message");
    config.region.reset();
    check(rejects<rt::EndpointError>([&] { (void)sdk::ServiceClient(config).read(Input("customer")); }, "missing region").find("Region") != std::string::npos, "required endpoint parameter");
    config.endpoint_builtins = {{"AWS::Region", rt::Json("local")}};
    (void)sdk::ServiceClient(config).public_value();
    check(host(fake->requests.back().url) == "local.example.test" && !fake->requests.back().headers.contains("authorization"), "built-in override and optional anonymous operation");
    config.endpoint_builtins = {{"AWS::Region", rt::Json(std::int64_t{1})}};
    (void)rejects<std::invalid_argument>([&] { (void)sdk::ServiceClient(config); }, "endpoint parameter types");
}

void parameter_precedence() {
    // Parsing avoids MSVC's excessive instantiation of nested initializer lists.
    const auto rules = rt::Json::parse(R"({
        "parameters": {
            "Name": {"kind": "string", "required": true},
            "Flag": {"kind": "boolean", "required": true}
        },
        "rules": [{
            "conditions": [{"function": "isSet", "arguments": [{"kind": "reference", "name": "Flag"}], "assign": null}],
            "result": {
                "kind": "endpoint",
                "url": {"kind": "string", "parts": [{"kind": "literal", "value": "https://example.test/base"}]},
                "headers": {"name": [{"kind": "reference", "name": "Name"}]},
                "properties": {"flag": {"kind": "reference", "name": "Flag"}}
            }
        }]
    })");
    const auto sources = rt::Json::parse(R"({"Name":[{"kind":"inputMember","member":"name"},{"kind":"client","name":"name"},{"kind":"default","value":"default"}],"Flag":[{"kind":"builtIn","name":"Flag"}]})");
    const rt::Json::Object flag{{"Flag", rt::Json(false)}};
    auto result = rt::resolve_endpoint(rules, &sources, rt::Json::parse(R"({"name":"input"})"), {{"name", rt::Json("client")}}, flag);
    check(result.url == "https://example.test/base" && result.headers.at("name") == std::vector<std::string>{"input"}, "input member wins");
    check(!result.properties.at("flag").boolean(), "false is a present boolean");
    check(rt::resolve_endpoint(rules, &sources, rt::Json(rt::Json::Object{}), {{"name", rt::Json("client")}}, flag).headers.at("name")[0] == "client", "client parameter");
    check(rt::resolve_endpoint(rules, &sources, rt::Json(rt::Json::Object{}), {}, flag).headers.at("name")[0] == "default", "default parameter");
    check(rejects<rt::EndpointError>([&] { (void)rt::resolve_endpoint(rules, &sources, rt::Json(rt::Json::Object{}), {}, {}); }, "missing").find("missing") != std::string::npos, "required parameter missing");
    check(rejects<rt::EndpointError>([&] { (void)rt::resolve_endpoint(rules, &sources, rt::Json(rt::Json::Object{}), {}, {{"Flag", rt::Json(std::int64_t{0})}}); }, "type").find("wrong type") != std::string::npos, "parameter type");
}

void standard_functions() {
    check(rt::parse_endpoint("https://example.test:443/a").at("authority").string() == "example.test", "default port removed");
    check(rt::parse_endpoint("https://[::1]:8443").at("authority").string() == "[::1]:8443", "IPv6 authority");
    check(rt::parse_endpoint("https://[::1]").at("isIp").boolean() && rt::parse_endpoint("http://127.0.0.1/x").at("isIp").boolean(), "IP detection");
    check(rt::parse_endpoint("https://example.test/a").at("normalizedPath").string() == "/a/", "normalized path");
    for (const auto* value : {"file:///path", "https://user:password@example.test", "https://example.test?q=x", "https://example.test#x", "https://example.test:99999", "https://evil\n.example", "https://bad_host.test"}) check(rt::parse_endpoint(value).is_null(), std::string("invalid endpoint accepted: ") + value);
    const auto call = [](const char* name, rt::Json::Array arguments) { return rt::endpoint_function(name, arguments); };
    check(call("substring", {rt::Json("abcdef"), rt::Json(std::int64_t{1}), rt::Json(std::int64_t{3}), rt::Json(true)}).string() == "de", "reverse substring");
    check(call("substring", {rt::Json("abcdef"), rt::Json(std::int64_t{1}), rt::Json(std::int64_t{3}), rt::Json(false)}).string() == "bc", "substring");
    check(call("substring", {rt::Json("\xc3\xa9"), rt::Json(std::int64_t{0}), rt::Json(std::int64_t{1}), rt::Json(false)}).is_null(), "non-ASCII substring");
    check(call("substring", {rt::Json("a"), rt::Json(std::int64_t{0}), rt::Json(std::int64_t{2}), rt::Json(false)}).is_null(), "substring bounds");
    check(call("uriEncode", {rt::Json("a/b +\xc3\xa9")}).string() == "a%2Fb%20%2B%C3%A9", "URI encoding");
    check(call("getAttr", {rt::Json::parse(R"({"items":["a","b"]})"), rt::Json("items[1]")}).string() == "b", "getAttr path");
    check(call("getAttr", {rt::Json::parse(R"({"items":["a","b"]})"), rt::Json("items[5]")}).is_null(), "getAttr missing index");
    check(call("isSet", {rt::Json(false)}).boolean(), "false is set");
    check(!call("isValidHostLabel", {rt::Json("x.y"), rt::Json(false)}).boolean() && call("isValidHostLabel", {rt::Json("x.y"), rt::Json(true)}).boolean(), "host labels");
    check(call("booleanEquals", {rt::Json(true), rt::Json(true)}).boolean() && !call("stringEquals", {rt::Json("a"), rt::Json(true)}).boolean(), "equality functions");
    check(call("not", {rt::Json(false)}).boolean(), "not");
    check(call("parseUrl", {rt::Json("https://example.test/path")}).at("path").string() == "/path", "parseUrl");
    check(rt::prefix_endpoint("https://example.test:8443/base", rt::Json::parse(R"({"parts":[{"kind":"label","member":"account"},{"kind":"literal","value":"."}]})"), rt::Json::parse(R"({"account":"tenant"})")) == "https://tenant.example.test:8443/base", "host prefix keeps port and path");
}

int main() {
    try {
        generated_endpoint_rules();
        parameter_precedence();
        standard_functions();
        std::cout << "C++ endpoint rules and host prefixes passed\n";
    } catch (const std::exception& error) {
        std::cerr << error.what() << "\n";
        return 1;
    }
}
