#pragma once
// Shared helpers for feature tests against tools/test-cpp.py's scripted loopback server.
// Define SCRIPTED_RUNTIME as the generated runtime namespace before including.
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <iterator>
#include <stdexcept>
#include <string>

namespace rt = SCRIPTED_RUNTIME;
inline void check(bool condition, const std::string& message) { if (!condition) throw std::runtime_error("Check failed: " + message); }
template<class E = std::exception, class F> std::string rejects(F action, const std::string& message) {
    try { action(); } catch (const E& error) { return error.what(); }
    throw std::runtime_error("Expected exception: " + message);
}
inline std::string env(const char* name) { const auto* value = std::getenv(name); if (!value) throw std::runtime_error(std::string("Missing ") + name); return value; }
inline std::string read_file(const std::string& path) { std::ifstream stream(path, std::ios::binary); if (!stream) throw std::runtime_error("Missing file " + path); return std::string(std::istreambuf_iterator<char>(stream), {}); }
inline std::string text(const rt::Blob& value) { std::string result; for (const auto byte : value) result += static_cast<char>(byte); return result; }
inline rt::Blob blob(const std::string& value) { rt::Blob result; for (const unsigned char byte : value) result.push_back(static_cast<std::byte>(byte)); return result; }
inline rt::Json reply(int status, std::string body = "{}", rt::Json::Object headers = {}, double delay = 0) {
    rt::Json::Object result{{"status", rt::Json(static_cast<std::int64_t>(status))}, {"body", rt::Json(std::move(body))}, {"headers", rt::Json(std::move(headers))}};
    if (delay > 0) result.emplace("delay", rt::Json(delay));
    return rt::Json(std::move(result));
}
struct Scripted {
    std::string url = env("SCRIPTED_URL");
    rt::CurlTransport transport;
    // Replaces the response queue (served in order, then `fallback`) and clears recorded requests.
    void script(rt::Json::Array responses, rt::Json fallback = rt::Json()) {
        rt::Json::Object spec{{"responses", rt::Json(std::move(responses))}};
        if (!fallback.is_null()) spec.emplace("default", std::move(fallback));
        const auto response = transport.send({url + "/__script", "POST", {{"content-type", "application/json"}}, rt::Json(std::move(spec)).dump()}, true, rt::CallScope(10));
        check(response.status == 204, "script accepted");
    }
    // Returns and clears the recorded requests: method, path, lower-case headers, base64 body.
    rt::Json::Array requests() { return rt::Json::parse(transport.send({url + "/__requests", "GET", {}, std::nullopt}, true, rt::CallScope(10)).body).array(); }
    rt::Json last() { auto all = requests(); check(!all.empty(), "a request was recorded"); return all.back(); }
};
inline std::string header(const rt::Json& request, const std::string& name) { const auto* value = request.at("headers").find(name); return value ? value->string() : std::string{}; }
inline bool has_header(const rt::Json& request, const std::string& name) { return request.at("headers").find(name) != nullptr; }
inline std::string body(const rt::Json& request) { return text(rt::unbase64(request.at("body").string())); }
