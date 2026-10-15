#include "client_runtime.hpp"
#include <openssl/rand.h>
#include <array>
#include <charconv>
#include <random>
#include <set>

namespace crucible_generated::runtime {
namespace {
bool string(const Json& value) { return std::holds_alternative<std::string>(value.value()); }
std::string escape(const std::string& text) { validate_utf8(text); return uri_encode(text); }
std::string hex(std::string_view bytes) {
    constexpr char digits[] = "0123456789abcdef";
    std::string result;
    result.reserve(bytes.size() * 2);
    for (const unsigned char byte : bytes) { result += digits[byte >> 4]; result += digits[byte & 15]; }
    return result;
}
std::string uuid() {
    std::array<unsigned char, 16> bytes{};
    if (RAND_bytes(bytes.data(), static_cast<int>(bytes.size())) != 1) throw std::runtime_error("Secure random generation failed");
    bytes[6] = static_cast<unsigned char>((bytes[6] & 15) | 64);
    bytes[8] = static_cast<unsigned char>((bytes[8] & 63) | 128);
    const auto text = hex(std::string_view(reinterpret_cast<const char*>(bytes.data()), bytes.size()));
    return text.substr(0, 8) + "-" + text.substr(8, 4) + "-" + text.substr(12, 4) + "-" + text.substr(16, 4) + "-" + text.substr(20);
}
Blob blob(std::string_view text) { Blob result; result.reserve(text.size()); for (const unsigned char byte : text) result.push_back(static_cast<std::byte>(byte)); return result; }
std::string bytes(const Blob& data) { return data.empty() ? std::string{} : std::string(reinterpret_cast<const char*>(data.data()), data.size()); }
Json media(const Json& value, const Json& bindings, std::string_view member, bool encode) {
    const auto* encodings = bindings.find("headerEncodings");
    const auto* encoding = encodings ? encodings->find(member) : nullptr;
    if (!encoding || encoding->string() != "base64") return value;
    if (const auto* list = std::get_if<Json::Array>(&value.value())) { Json::Array result; for (const auto& item : *list) result.push_back(media(item, bindings, member, encode)); return Json(std::move(result)); }
    const auto& text = value.string();
    if (encode) return Json(base64(blob(text)));
    const auto decoded = unbase64(text); const auto result = bytes(decoded); validate_utf8(result); return Json(result);
}
// HTTP bindings may use a different timestamp format than the member's document encoding.
std::string_view binding_format(const Json& bindings, std::string_view member) {
    const auto* formats = bindings.find("timestampFormats");
    const auto* format = formats ? formats->find(member) : nullptr;
    return format ? std::string_view(format->string()) : std::string_view{};
}
Json retime(const Json& value, std::string_view format) {
    if (format.empty() || value.is_null()) return value;
    if (const auto* list = std::get_if<Json::Array>(&value.value())) { Json::Array result; for (const auto& item : *list) result.push_back(retime(item, format)); return Json(std::move(result)); }
    Timestamp time;
    if (std::holds_alternative<Number>(value.value())) time = Timestamp::from_epoch(value.number());
    else { try { time = Timestamp::parse(value.string()); } catch (const std::invalid_argument&) { time = Timestamp::from_http_date(value.string()); } }
    return format == "epoch-seconds" ? Json(time.epoch()) : format == "http-date" ? Json(time.http_date()) : Json(time.date_time());
}
std::string payload_target(const Schema& schema, std::string_view target, const Json& bindings) { return schema.shape(target).at("members").at(bindings.at("payload").array()[0].string()).at("target").string(); }
Json payload_value(const Schema& schema, std::string_view target, const Json& bindings, const std::string& body) {
    const auto* kind = bindings.find("payloadKind");
    if (kind && kind->string() == "string") { validate_utf8(body); return Json(body); }
    if (kind && kind->string() == "document") return body.empty() ? Json() : document(schema, payload_target(schema, target, bindings), Json::parse(body), false);
    return Json(base64(blob(body)));
}
std::optional<std::string> get(const Headers& headers, const std::string& name) { const auto found = headers.find(name); return found == headers.end() ? std::nullopt : std::optional<std::string>(found->second); }
bool iequals(std::string_view left, std::string_view right) { return left.size() == right.size() && lower(std::string(left)) == lower(std::string(right)); }
// A caller-supplied bearer Authorization is honored without invoking the token provider.
bool has_bearer(const HttpRequest& request) {
    const auto value = get(request.headers, "authorization").value_or("");
    return value.size() > 7 && iequals(std::string_view(value).substr(0, 7), "Bearer ");
}
}
ClientRuntime::ClientRuntime(ClientConfig config, const Schema& schema, ErrorFactory errors, std::string endpoint)
    : config_(std::move(config)), schema_(&schema), errors_(std::move(errors)), endpoint_(config_.endpoint.value_or(std::move(endpoint))), transport_(config_.transport ? config_.transport : std::make_shared<CurlTransport>()) {
    (void)scope();
    if (config_.max_attempts == 0 || !std::isfinite(config_.base_delay) || config_.base_delay < 0 || !std::isfinite(config_.max_delay) || config_.max_delay < 0) throw std::invalid_argument("Invalid retry limits");
    validate_url(endpoint_, true);
    while (endpoint_.ends_with('/')) endpoint_.pop_back();
}
void ClientRuntime::close() { if (!closed_.exchange(true) && !config_.transport) transport_->close(); }
HttpRequest ClientRuntime::prepare(const Json& operation, const Json& input) const {
    auto values = input.object();
    const auto& token = operation.at("token");
    if (!token.is_null()) { const auto found = values.find(token.string()); if (found == values.end() || found->second.is_null()) values.insert_or_assign(token.string(), Json(uuid())); }
    const auto& bindings = operation.at("request");
    auto path = operation.at("uri").string();
    for (const auto& member : bindings.at("labels").array()) {
        const auto found = values.find(member.string());
        if (found == values.end() || found->second.is_null()) throw std::invalid_argument("Missing label: " + member.string());
        const auto text = bound(retime(found->second, binding_format(bindings, member.string())));
        if (text.empty()) throw std::invalid_argument("Empty label: " + member.string());
        bool greedy = false;
        if (const auto* labels = operation.find("greedyLabels")) for (const auto& label : labels->array()) if (label.string() == member.string()) greedy = true;
        const auto placeholder = "{" + member.string() + (greedy ? "+}" : "}");
        std::string encoded;
        std::size_t segment = 0;
        do {
            const auto end = greedy ? text.find('/', segment) : std::string::npos;
            const auto part = text.substr(segment, end == std::string::npos ? end : end - segment);
            if (part == "." || part == "..") throw std::invalid_argument("Dot-segment HTTP label: " + member.string());
            encoded += escape(part);
            if (end == std::string::npos) break;
            encoded += '/'; segment = end + 1;
        } while (true);
        std::size_t start = 0;
        std::size_t pos = path.find(placeholder, start);
        if (pos != std::string::npos) {
            std::string new_path;
            new_path.reserve(path.size());
            do {
                new_path.append(path, start, pos - start);
                new_path.append(encoded);
                start = pos + placeholder.size();
                pos = path.find(placeholder, start);
            } while (pos != std::string::npos);
            new_path.append(path, start, std::string::npos);
            path = std::move(new_path);
        }
    }
    for (const auto& binding : bindings.at("query").array()) {
        const auto& pair = binding.array();
        const auto found = values.find(pair[0].string());
        if (found == values.end() || found->second.is_null()) continue;
        const auto value = retime(found->second, binding_format(bindings, pair[0].string()));
        auto append = [&](const Json& item) { path += path.find('?') == std::string::npos ? '?' : '&'; path += escape(pair[1].string()) + "=" + escape(bound(item)); };
        if (std::holds_alternative<Json::Array>(value.value())) for (const auto& item : value.array()) append(item);
        else append(value);
    }
    HttpRequest request{endpoint_ + path, operation.at("method").string(), {}, std::nullopt};
    for (const auto& [key, value] : config_.headers) request.headers.insert_or_assign(lower(key), value);
    // A Content-Type bound to an input member outranks the payload's media
    // type, which outranks client header defaults.
    bool modelled_content_type = false;
    for (const auto& binding : bindings.at("headers").array()) {
        const auto& pair = binding.array();
        const auto found = values.find(pair[0].string());
        if (found == values.end() || found->second.is_null()) continue;
        const auto header_name = lower(pair[1].string());
        request.headers.insert_or_assign(header_name, bound(media(retime(found->second, binding_format(bindings, pair[0].string())), bindings, pair[0].string(), true)));
        if (header_name == "content-type") modelled_content_type = true;
    }
    if (!bindings.at("body").array().empty()) {
        Json::Object object;
        for (const auto& member : bindings.at("body").array()) { const auto found = values.find(member.string()); if (found != values.end()) object.emplace(found->first, found->second); }
        request.body = document(*schema_, operation.at("input").string(), Json(std::move(object)), true).dump();
        request.headers.insert_or_assign("content-type", "application/json");
    }
    const auto& payload = bindings.at("payload");
    if (!payload.is_null()) {
        const auto found = values.find(payload.array()[0].string());
        if (found != values.end() && !found->second.is_null()) {
            const auto* kind = bindings.find("payloadKind");
            if (kind && kind->string() == "string") request.body = found->second.string();
            else if (kind && kind->string() == "document") request.body = document(*schema_, payload_target(*schema_, operation.at("input").string(), bindings), found->second, true).dump();
            else request.body = bytes(unbase64(found->second.string()));
            if (!modelled_content_type) request.headers.insert_or_assign("content-type", payload.array()[1].string());
        }
    }
    // The release this client was generated for (docs/api-versions-clients.md).
    if (const auto* api_version = schema_->description().find("apiVersion"))
        request.headers.insert_or_assign("crucible-version", api_version->string());
    validate_request(request);
    return request;
}
bool ClientRuntime::authenticate(const Json& operation, HttpRequest& request, const CallScope& scope) const {
    if (!operation.at("bearer").boolean()) return false;
    if (has_bearer(request)) return true;
    if (!config_.token) return false;
    const auto token = config_.token();
    scope.check();
    if (!token || token->empty()) return false;
    request.headers.insert_or_assign("authorization", "Bearer " + *token);
    return true;
}
Json ClientRuntime::apply_headers(const std::string& target, const Json& bindings, const HttpResponse& response, const Json& body) const {
    auto object = body.object();
    const auto& members = schema_->shape(target).at("members");
    for (const auto& binding : bindings.at("headers").array()) { const auto& pair = binding.array(); const auto found = response.headers.find(lower(pair[1].string())); if (found != response.headers.end()) object.insert_or_assign(pair[0].string(), media(header(*schema_, members.at(pair[0].string()), found->second, binding_format(bindings, pair[0].string())), bindings, pair[0].string(), false)); }
    return Json(std::move(object));
}
Json ClientRuntime::success(const Json& operation, const HttpResponse& response) const {
    const auto& bindings = operation.at("response");
    const auto& payload = bindings.at("payload");
    Json body(Json::Object{});
    if (!payload.is_null()) {
        body = Json(Json::Object{{payload.array()[0].string(), payload_value(*schema_, operation.at("output").string(), bindings, response.body)}});
    } else if (!bindings.at("body").array().empty() && !response.body.empty()) body = document(*schema_, operation.at("output").string(), Json::parse(response.body), false);
    (void)body.object();
    const auto& output = operation.at("output");
    return output.is_null() ? body : apply_headers(output.string(), bindings, response, body);
}
std::pair<ErrorMetadata, std::exception_ptr> ClientRuntime::failure(const Json& operation, const HttpResponse& response) const {
    Json body(Json::Object{});
    try { auto decoded = Json::parse(response.body); (void)decoded.object(); body = std::move(decoded); } catch (const std::invalid_argument&) {}
    std::optional<std::string> name;
    if (const auto* type = body.find("__type"); type && string(*type)) name = type->string();
    if (const auto header_name = get(response.headers, "x-crucible-error"); header_name && !header_name->empty()) name = header_name;
    std::vector<std::string> matches;
    for (const auto& error : operation.at("errors").array()) { const auto& shape = schema_->shape(error.string()); if (name ? (shape.at("error").find("wireCode") ? shape.at("error").at("wireCode").string() : shape.at("name").string()) == *name : shape.at("error").at("status").number().integer() == response.status) matches.push_back(error.string()); }
    const Json* shape = matches.size() == 1 ? &schema_->shape(matches[0]) : nullptr;
    const Json* traits = shape ? &shape->at("error") : nullptr;
    const auto* message = body.find("message");
    ErrorMetadata metadata;
    metadata.name = name ? *name : shape ? shape->at("name").string() : "UnknownError";
    metadata.message = message && string(*message) ? message->string() : "HTTP " + std::to_string(response.status);
    metadata.status = response.status;
    metadata.headers = response.headers;
    metadata.request_id = get(response.headers, "x-request-id");
    if (!metadata.request_id) { const auto* id = body.find("requestId"); if (id && string(*id)) metadata.request_id = id->string(); }
    metadata.retryable = traits && traits->at("retryable").boolean();
    metadata.throttling = traits && traits->at("throttling").boolean();
    if (shape) {
        try {
            const auto* payload = traits->find("payload");
            auto modeled = payload ? Json(Json::Object{{payload->array()[0].string(), payload_value(*schema_, matches[0], *traits, response.body)}}) : document(*schema_, matches[0], body, false);
            const auto error = errors_(matches[0], apply_headers(matches[0], *traits, response, modeled), metadata); if (error) return {metadata, error};
        }
        catch (const std::invalid_argument&) { /* Preserve metadata if modeled members are malformed. */ }
    }
    return {metadata, std::make_exception_ptr(ServiceException(metadata))};
}
double ClientRuntime::retry_delay(unsigned attempt, const std::optional<std::string>& value) const {
    if (value) {
        double seconds = 0;
        const auto parsed = std::from_chars(value->data(), value->data() + value->size(), seconds);
        if (parsed.ec == std::errc() && parsed.ptr == value->data() + value->size() && std::isfinite(seconds) && seconds >= 0) return seconds;
        try { const auto date = Timestamp::from_http_date(*value); const auto now = std::chrono::duration<double>(std::chrono::system_clock::now().time_since_epoch()).count(); return (std::max)(0.0, static_cast<double>(date.seconds()) - now); } catch (const std::invalid_argument&) {}
    }
    thread_local std::mt19937 engine(std::random_device{}());
    return (std::min)(config_.max_delay, std::ldexp(config_.base_delay, static_cast<int>((std::min)(attempt - 1, 30U)))) * std::uniform_real_distribution<double>(0.0, 1.0)(engine);
}
Json ClientRuntime::call(const std::string& name, const Json& input, const CallScope& scope) const {
    scope.check();
    if (closed_) throw std::logic_error("Client is closed");
    const auto& op = operation(name);
    const auto& response_bindings = op.at("response");
    const auto prepared = prepare(op, input);
    const bool safe = op.at("safe").boolean();
    const bool read_body = !response_bindings.at("body").array().empty() || !response_bindings.at("payload").is_null();
    for (unsigned attempt = 1;; ++attempt) {
        scope.check();
        auto request = prepared;
        const bool credentials = authenticate(op, request, scope);
        for (const auto& middleware : config_.middleware) { scope.check(); request = middleware(std::move(request)); }
        scope.check();
        Headers normalized;
        for (const auto& [key, value] : request.headers) normalized.insert_or_assign(lower(key), value);
        request.headers = std::move(normalized);
        validate_request(request);
        if (credentials) validate_url(request.url, false, true);
        HttpResponse response;
        try { response = transport_->send(request, read_body, scope); }
        catch (const NetworkError&) { scope.check(); if (!safe || attempt >= config_.max_attempts) throw; scope.wait(retry_delay(attempt, std::nullopt)); continue; }
        scope.check();
        Headers headers;
        for (const auto& [key, value] : response.headers) headers.insert_or_assign(lower(key), value);
        response.headers = std::move(headers);
        if (response.status >= 200 && response.status < 300) return success(op, response);
        const auto [metadata, error] = failure(op, response);
        const bool transient = response.status == 500 || response.status == 502 || response.status == 503 || response.status == 504;
        if (!(metadata.throttling || (safe && (metadata.retryable || transient))) || attempt >= config_.max_attempts) std::rethrow_exception(error);
        scope.wait(retry_delay(attempt, get(response.headers, "retry-after")));
    }
}
}
