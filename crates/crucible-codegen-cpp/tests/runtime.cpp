#include "weather_client/client.hpp"
#include "weather_client/schema.hpp"
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <iterator>
#include <thread>

using namespace crucible_generated;
using namespace crucible_generated::runtime;
using namespace crucible_generated::models;
void check(bool condition, const std::string& message) { if (!condition) throw std::runtime_error(message); }
template<class E = std::invalid_argument, class F> E rejects(F action) { try { action(); } catch (const E& error) { return error; } throw std::runtime_error("Expected exception"); }
std::string env(const char* name) { const auto* value = std::getenv(name); if (!value) throw std::runtime_error(std::string("Missing ") + name); return value; }
Json fixture(const char* name) { std::ifstream stream(env(name)); if (!stream) throw std::runtime_error("Missing fixture"); return Json::parse(std::string(std::istreambuf_iterator<char>(stream), {})); }
struct Fake : Transport {
    std::function<HttpResponse(unsigned, const HttpRequest&)> handler;
    std::vector<HttpRequest> requests;
    bool closed = false;
    explicit Fake(std::function<HttpResponse(unsigned, const HttpRequest&)> handler) : handler(std::move(handler)) {}
    HttpResponse send(const HttpRequest& request, bool, const CallScope&) override { requests.push_back(request); return handler(static_cast<unsigned>(requests.size()), request); }
    void close() override { closed = true; }
};
ClientConfig config(const std::shared_ptr<Transport>& transport = {}) { ClientConfig result; result.endpoint = "https://example.invalid"; result.transport = transport; result.base_delay = 0; return result; }
std::string bytes(const Blob& value) { std::string result; for (const auto byte : value) result += static_cast<char>(byte); return result; }
int main() {
    {
        const auto model = Codec<models::DebugModel>::decode(Json::parse(R"({"label":"visible","secret":"SECRET-MEMBER","ticket":"SECRET-TARGET","nested":{"label":"nested-visible","secret":"SECRET-NESTED"},"items":[{"secret":"SECRET-LIST"}],"values":["SECRET-ELEMENT"],"choice":{"secret":"SECRET-UNION"},"enumeration":"SECRET-ENUM"})"), schema().context("example.weather#DebugModel"));
        const auto text = debug_string(model);
        check(text.starts_with("DebugModel(") && text.find("DebugNested(") != std::string::npos && text.find("visible") != std::string::npos, "Recursive debug formatting");
        check(text.find("Sensitive Data Redacted") != std::string::npos && text.find("SECRET-") == std::string::npos, "Sensitive debug redaction");
        check(Codec<models::DebugModel>::encode(model, schema().context("example.weather#DebugModel")).dump().find("SECRET-MEMBER") != std::string::npos, "Debug does not alter serialization");
    }
    {
        auto fake = std::make_shared<Fake>([](unsigned, const HttpRequest& request) {
            check(!request.headers.contains("authorization"), "Public operation has no bearer header");
            return HttpResponse{204, {}, ""};
        });
        auto settings = config(fake);
        settings.token = []() -> std::optional<std::string> { throw std::runtime_error("Public operation requested credentials"); };
        (void)WeatherClient(settings).public_ping();
        for (const auto* method : {"POST", "PUT", "PATCH", "GET", "DELETE"}) {
            auto transport = std::make_shared<Fake>([method](unsigned, const HttpRequest& request) {
                check(!request.body && request.method == method, "Bodiless request");
                return HttpResponse{200, {}, "{}"};
            });
            auto options = config(transport);
            options.middleware.push_back([method](HttpRequest request) { request.method = method; return request; });
            (void)WeatherClient(options).retry_safe();
            check(transport->requests.size() == 1, "One bodiless request");
        }
        for (const auto* label : {".", ".."}) {
            fake = std::make_shared<Fake>([](unsigned, const HttpRequest&) -> HttpResponse { throw std::runtime_error("Dot label reached transport"); });
            (void)rejects([&] { (void)WeatherClient(config(fake)).get_city(GetCityInput(label)); });
            check(fake->requests.empty(), "Dot label rejected before dispatch");
        }
    }
    for (auto original : {Field<bool>{}, Field<bool>::null(), Field<bool>(false), Field<bool>(true)}) {
        Field<bool> copied(original), assigned; assigned = original;
        check(copied.is_set() == original.is_set() && copied.is_null() == original.is_null(), "Bool copy preserves presence");
        check(assigned.is_set() == original.is_set() && assigned.is_null() == original.is_null(), "Bool assignment preserves presence");
        if (original) check(*copied == *original && *assigned == *original, "Bool copy preserves false and true");
    }
    (void)rejects([] { validate_request({"https://example.invalid", "GET\r\nX: y", {}, {}}); });
    (void)rejects([] { validate_request({"https://example.invalid", "GET", {{"x", "a\r\nb"}}, {}}); });
    (void)rejects([] { validate_url("http://example.invalid", false, true); });
    (void)rejects([] { validate_url("https://user:password@example.invalid", false); });
    (void)rejects([] { validate_url("https://example.invalid?x", true); });
    (void)rejects([] { validate_request({"https://example.invalid", "HEAD", {}, "abc"}); });
    CreateCityInput input("C++", Field<Location>::of(Location{LocationUnknown{"future", Json::parse(R"({"long":9223372036854775807})")}}));
    const auto context = schema().context("example.weather#CreateCityInput");
    const auto wire = Codec<CreateCityInput>::encode(input, context);
    check(!wire.find("clientToken"), "Absent input");
    const auto copy = Codec<CreateCityInput>::decode(wire, context);
    check(std::get<LocationUnknown>(copy.location.value().value).value.at("long").number().integer() == INT64_MAX, "Unknown union exact document");
    input.client_token = "direct assignment";
    check(input.client_token && *input.client_token == "direct assignment" && input.client_token->size() == 17, "Native optional-style field access");
    input.client_token.reset();
    check(!input.client_token && !input.client_token.is_set() && input.client_token.value_or("fallback") == "fallback", "Field reset and fallback");
    input.client_token = Field<std::string>::null();
    check(Codec<CreateCityInput>::encode(input, context).at("clientToken").is_null(), "Explicit null input");
    const Schema simple(Json::parse(R"({"shapes":{"doc":{"kind":"document"},"long":{"kind":"long"},"float":{"kind":"float"},"double":{"kind":"double"},"str":{"kind":"string"},"list":{"kind":"list","member":{"target":"str"}},"parent":{"kind":"structure","members":{"doc":{"target":"doc","default":null},"count":{"target":"long","default":9223372036854775807},"optional":{"target":"long","default":2}}}}})"));
    const auto parent = simple.context("parent");
    check(read<Json>(Json::parse(R"({"doc":null})"), parent, "doc").is_null(), "Required document null");
    (void)rejects([&] { (void)read<Json>(Json(Json::Object{}), parent, "doc"); });
    check(read<std::int64_t>(Json(Json::Object{}), parent, "count") == INT64_MAX, "Exact default");
    (void)rejects([&] { (void)read<std::int64_t>(Json::parse(R"({"count":null})"), parent, "count"); });
    check(optional<std::int64_t>(Json(Json::Object{}), parent, "optional").value() == 2, "Optional default");
    check(optional<std::int64_t>(Json::parse(R"({"optional":null})"), parent, "optional").is_null(), "Null overrides default");
    const auto header_cases = fixture("HEADER_CASES");
    for (const auto& row : header_cases.array()) {
        const auto descriptor = Json::parse(R"({"target":"list"})");
        check(header(simple, descriptor, row.at("wire").string()).dump() == row.at("values").dump(), "Shared header fixture");
        check(header(simple, descriptor, bound(row.at("values"))).dump() == row.at("values").dump(), "Header round trip");
    }
    for (const auto* target : {"float", "double"}) {
        const auto descriptor = Json::parse(std::string(R"({"target":")") + target + R"("})");
        for (const auto* text : {"NaN", "Infinity", "-Infinity"}) check(header(simple, descriptor, text).string() == text, std::string("Non-finite ") + target + " header " + text);
        check(header(simple, descriptor, "1.5").dump() == "1.5", "Finite float header");
    }
    const auto retry_cases = fixture("RETRY_CASES");
    for (const auto& row : retry_cases.array()) {
        auto fake = std::make_shared<Fake>([&](unsigned attempt, const HttpRequest&) {
            Json::Object body;
            if (const auto* error = row.find("error"); error && !error->is_null()) body.emplace("__type", *error);
            return HttpResponse{attempt <= row.at("failures").number().integer() ? static_cast<int>(row.at("status").number().integer()) : 200, {{"x-amzn-errortype", "Throttled"}}, Json(std::move(body)).dump()};
        });
        auto settings = config(fake);
        settings.max_attempts = static_cast<unsigned>(row.at("maxAttempts").number().integer());
        WeatherClient client(settings);
        bool success = false;
        try { if (row.at("safe").boolean()) (void)client.retry_safe(); else (void)client.retry_unsafe(); success = true; } catch (const ServiceError&) {}
        check(success == row.at("success").boolean() && fake->requests.size() == static_cast<std::size_t>(row.at("attempts").number().integer()), "Retry fixture " + row.at("name").string());
        client.close();
        check(!fake->closed, "Caller transport ownership");
    }
    auto fake = std::make_shared<Fake>([](unsigned, const HttpRequest& request) {
        check(request.url.find("a%2Fb%20%2A%3F%23%C3%A9") != std::string::npos, "Label escaping");
        check(request.url.find("at=2026-10-01T00%3A00%3A00Z") != std::string::npos, "Query timestamp");
        return HttpResponse{200, {{"X-Request-Id", "bound"}}, R"({"city":{"id":"x","name":"n"}})"};
    });
    check(WeatherClient(config(fake)).get_city(GetCityInput("a/b *?#é", Field<Timestamp>::of(Timestamp::parse("2026-10-01T00:00:00Z")))).request_id.value() == "bound", "Bound response header");
    fake = std::make_shared<Fake>([](unsigned, const HttpRequest&) { return HttpResponse{429, {{"x-request-id", "header"}}, R"({"__type":"ThrottlingError","message":42})"}; });
    auto settings = config(fake); settings.max_attempts = 2;
    const auto malformed = rejects<WeatherException>([&] { (void)WeatherClient(settings).ping(); });
    check(malformed.metadata().throttling && malformed.metadata().request_id == "header" && fake->requests.size() == 2, "Malformed modeled error retains metadata");
    fake = std::make_shared<Fake>([](unsigned, const HttpRequest&) { return HttpResponse{200, {}, "{bad"}; });
    (void)rejects([&] { (void)WeatherClient(config(fake)).get_city(GetCityInput("x")); });
    check(fake->requests.size() == 1, "Malformed success never retries");
    fake = std::make_shared<Fake>([](unsigned, const HttpRequest&) { return HttpResponse{200, {}, R"({"cities":[],"nextToken":"same"})"}; });
    auto pages = WeatherClient(config(fake)).list_cities_pages(ListCitiesInput());
    check(fake->requests.empty(), "Lazy pagination");
    (void)pages.next(); (void)pages.next();
    (void)rejects([&] { (void)pages.next(); });
    check(fake->requests.size() == 2, "Pagination cycle");
    fake = std::make_shared<Fake>([](unsigned, const HttpRequest&) { return HttpResponse{503, {{"retry-after", "2147484"}}, "{}"}; });
    settings = config(fake); settings.timeout = 0.05;
    (void)rejects<DeadlineError>([&] { (void)WeatherClient(settings).retry_safe(); });
    check(fake->requests.size() == 1, "Retry deadline");
    std::stop_source stopped; stopped.request_stop();
    fake = std::make_shared<Fake>([](unsigned, const HttpRequest&) { return HttpResponse{200, {}, "{}"}; });
    (void)rejects<CancelledError>([&] { (void)WeatherClient(config(fake)).ping(stopped.get_token()); });
    check(fake->requests.empty(), "Precancelled call");
    fake = std::make_shared<Fake>([](unsigned, const HttpRequest& request) {
        check(request.headers.at("authorization") == "Bearer middleware", "Case-insensitive middleware assignment");
        check(request.headers.at("CONTENT-TYPE") == "custom/type", "Case-insensitive content type assignment");
        return HttpResponse{201, {}, R"({"city":{"id":"x","name":"n"}})"};
    });
    settings = config(fake);
    settings.token = [] { return "provider"; };
    settings.middleware.push_back([](HttpRequest request) { request.headers["Authorization"] = "Bearer middleware"; request.headers["Content-Type"] = "custom/type"; return request; });
    (void)WeatherClient(settings).create_city(CreateCityInput("middleware"));
    settings = config(); settings.endpoint = env("WEATHER_URL"); settings.token = [] { return "weather-token"; };
    WeatherClient client(settings);
    const auto city = client.create_city(CreateCityInput("C++ city")).city;
    check(client.get_city(GetCityInput(city.id)).city.name == "C++ city", "Real create/read");
    const Blob photo{std::byte{0}, std::byte{128}, std::byte{255}};
    (void)client.put_city_photo(PutCityPhotoInput(city.id, photo));
    check(client.get_city_photo(GetCityPhotoInput(city.id)).photo.value() == photo, "Real raw payload");
    check(client.list_cities_pages(ListCitiesInput({}, 1)).next().has_value(), "Real pagination");
    (void)rejects<ConflictError>([&] { (void)client.create_city(CreateCityInput("C++ city")); });
    (void)client.ping(); client.close();
    const auto endpoint = env("FIXTURE_URL") + "/native/";
    for (const auto* path : {"partial", "bad-length"}) {
        settings = config(); settings.endpoint = endpoint + path;
        WeatherClient native(settings);
        check(bytes(native.get_city_photo(GetCityPhotoInput("x")).photo.value()) == "complete", "Network body retry");
        native.close();
    }
    settings = config(); settings.endpoint = endpoint + "not-modified"; settings.max_attempts = 1;
    check(rejects<WeatherException>([&] { (void)WeatherClient(settings).get_city(GetCityInput("x")); }).metadata().status == 304, "Bodyless 304");
    settings = config(); settings.endpoint = endpoint + "empty-header"; settings.headers = {{"x-empty", ""}};
    (void)WeatherClient(settings).ping();
    settings = config(); settings.endpoint = endpoint + "stall"; settings.timeout = 0.15;
    auto start = std::chrono::steady_clock::now();
    (void)rejects<DeadlineError>([&] { (void)WeatherClient(settings).get_city(GetCityInput("x")); });
    check(std::chrono::duration<double>(std::chrono::steady_clock::now() - start).count() < 1.5, "Stalled body deadline");
    settings.timeout = 5;
    std::stop_source cancellation;
    std::jthread cancel([&] { std::this_thread::sleep_for(std::chrono::milliseconds(100)); cancellation.request_stop(); });
    start = std::chrono::steady_clock::now();
    (void)rejects<CancelledError>([&] { (void)WeatherClient(settings).get_city(GetCityInput("x"), cancellation.get_token()); });
    check(std::chrono::duration<double>(std::chrono::steady_clock::now() - start).count() < 1.5, "Stalled body cancellation");
    settings = config(); settings.endpoint = endpoint + "discard"; settings.timeout = 0.5;
    (void)WeatherClient(settings).ping();
    settings = config(); settings.endpoint = endpoint + "redirect"; settings.max_attempts = 1;
    check(rejects<WeatherException>([&] { (void)WeatherClient(settings).ping(); }).metadata().status == 302, "Redirect refusal");
    unsigned token = 0;
    settings = config(); settings.endpoint = endpoint + "echo"; settings.token = [&] { return std::to_string(++token); };
    (void)WeatherClient(settings).create_city(CreateCityInput("x"));
    check(token == 2, "Real refreshed retry credential");
    token = 0; settings.endpoint = endpoint + "cache";
    WeatherClient cache(settings);
    check(cache.get_city(GetCityInput("c")).city.name == "Bearer 1", "First credential");
    check(cache.get_city(GetCityInput("c")).city.name == "Bearer 2", "No cached credential response");
    cache.close();
    std::cout << "C++ native models, shared fixtures, and real HTTP passed\n";
}
