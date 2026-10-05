#include "client_runtime.hpp"
#include <openssl/evp.h>
#include <openssl/rand.h>
#include <array>
#include <charconv>
#include <random>
#include <set>

namespace crucible_generated::runtime {
namespace {
bool string(const Json& value) { return std::holds_alternative<std::string>(value.value()); }
std::string unescape(std::string_view text) {
    std::string result;
    for (std::size_t i = 0; i < text.size(); ++i) {
        unsigned byte = 0;
        if (text[i] == '%' && i + 2 < text.size()) {
            const auto parsed = std::from_chars(text.data() + i + 1, text.data() + i + 3, byte, 16);
            if (parsed.ec == std::errc() && parsed.ptr == text.data() + i + 3) { result += static_cast<char>(byte); i += 2; continue; }
        }
        result += text[i];
    }
    return result;
}
std::string escape(const std::string& text) {
    validate_utf8(text);
    constexpr char hex[] = "0123456789ABCDEF";
    std::string output;
    for (const unsigned char byte : text) {
        if ((byte >= 'a' && byte <= 'z') || (byte >= 'A' && byte <= 'Z') || (byte >= '0' && byte <= '9') || byte == '-' || byte == '.' || byte == '_' || byte == '~') output += static_cast<char>(byte);
        else { output += '%'; output += hex[byte >> 4]; output += hex[byte & 15]; }
    }
    return output;
}
std::string hex(const unsigned char* data, std::size_t size) {
    constexpr char alphabet[] = "0123456789abcdef";
    std::string result;
    for (std::size_t i = 0; i < size; ++i) { result += alphabet[data[i] >> 4]; result += alphabet[data[i] & 15]; }
    return result;
}
std::string uuid() {
    std::array<unsigned char, 16> bytes{};
    if (RAND_bytes(bytes.data(), static_cast<int>(bytes.size())) != 1) throw std::runtime_error("Secure random generation failed");
    bytes[6] = static_cast<unsigned char>((bytes[6] & 15) | 64);
    bytes[8] = static_cast<unsigned char>((bytes[8] & 63) | 128);
    const auto text = hex(bytes.data(), bytes.size());
    return text.substr(0, 8) + "-" + text.substr(8, 4) + "-" + text.substr(12, 4) + "-" + text.substr(16, 4) + "-" + text.substr(20);
}
std::string sha256(const std::string& body) {
    std::array<unsigned char, EVP_MAX_MD_SIZE> bytes{};
    unsigned length = 0;
    if (EVP_Digest(body.data(), body.size(), bytes.data(), &length, EVP_sha256(), nullptr) != 1) throw std::runtime_error("SHA256 failed");
    return hex(bytes.data(), length);
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
std::string payload_target(const Schema& schema, std::string_view target, const Json& bindings) { return schema.shape(target).at("members").at(bindings.at("payload").array()[0].string()).at("target").string(); }
Json payload_value(const Schema& schema, std::string_view target, const Json& bindings, const std::string& body) {
    const auto* kind = bindings.find("payloadKind");
    if (kind && kind->string() == "string") { validate_utf8(body); return Json(body); }
    if (kind && kind->string() == "document") return body.empty() ? Json() : document(schema, payload_target(schema, target, bindings), Json::parse(body), false);
    return Json(base64(blob(body)));
}
std::optional<std::string> get(const Headers& headers, const std::string& name) { const auto found = headers.find(name); return found == headers.end() ? std::nullopt : std::optional<std::string>(found->second); }
}
ClientRuntime::ClientRuntime(ClientConfig config, const Schema& schema, ErrorFactory errors, std::string endpoint, bool digest)
    : config_(std::move(config)), schema_(&schema), errors_(std::move(errors)), endpoint_(config_.endpoint.value_or(std::move(endpoint))), transport_(config_.transport ? config_.transport : std::make_shared<CurlTransport>()), digest_(digest) {
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
        const auto text = bound(found->second);
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
        while ((start = path.find(placeholder, start)) != std::string::npos) { path.replace(start, placeholder.size(), encoded); start += encoded.size(); }
    }
    std::set<std::string, std::less<>> reserved_query;
    if (const auto question = path.find('?'); question != std::string::npos) {
        std::size_t start = question + 1;
        while (start < path.size()) {
            const auto end = path.find('&', start);
            const auto pair = path.substr(start, end == std::string::npos ? end : end - start);
            reserved_query.insert(unescape(pair.substr(0, pair.find('='))));
            if (end == std::string::npos) break;
            start = end + 1;
        }
    }
    for (const auto& binding : bindings.at("query").array()) {
        const auto& pair = binding.array();
        reserved_query.insert(pair[1].string());
        const auto found = values.find(pair[0].string());
        if (found == values.end() || found->second.is_null()) continue;
        auto append = [&](const Json& value) { path += path.find('?') == std::string::npos ? '?' : '&'; path += escape(pair[1].string()) + "=" + escape(bound(value)); };
        if (std::holds_alternative<Json::Array>(found->second.value())) for (const auto& value : found->second.array()) append(value);
        else append(found->second);
    }
    if (const auto* member = bindings.find("queryParams")) {
        const auto found = values.find(member->string());
        if (found != values.end() && !found->second.is_null()) for (const auto& [key, value] : found->second.object()) {
            if (reserved_query.contains(key)) continue;
            auto append = [&](const Json& item) { if (!item.is_null()) { path += path.find('?') == std::string::npos ? '?' : '&'; path += escape(key) + "=" + escape(bound(item)); } };
            if (const auto* items = std::get_if<Json::Array>(&value.value())) for (const auto& item : *items) append(item);
            else append(value);
        }
    }
    HttpRequest request{endpoint_ + path, operation.at("method").string(), {}, std::nullopt};
    for (const auto& [key, value] : config_.headers) request.headers.insert_or_assign(lower(key), value);
    for (const auto& binding : bindings.at("headers").array()) { const auto& pair = binding.array(); const auto found = values.find(pair[0].string()); if (found != values.end() && !found->second.is_null()) request.headers.insert_or_assign(lower(pair[1].string()), bound(media(found->second, bindings, pair[0].string(), true))); }
    std::set<std::string, std::less<>> explicit_headers;
    for (const auto& binding : bindings.at("headers").array()) explicit_headers.insert(lower(binding.array()[1].string()));
    if (const auto* prefixes = bindings.find("prefixHeaders")) for (const auto& binding : prefixes->array()) {
        const auto& pair = binding.array(); const auto found = values.find(pair[0].string());
        if (found == values.end() || found->second.is_null()) continue;
        for (const auto& [key, value] : found->second.object()) {
            const auto name = lower(pair[1].string() + key);
            if (!explicit_headers.contains(name)) request.headers.insert_or_assign(name, bound(media(value, bindings, pair[0].string(), true)));
        }
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
            request.headers.insert_or_assign("content-type", payload.array()[1].string());
        }
    }
    validate_request(request);
    return request;
}
Json ClientRuntime::apply_headers(const std::string& target, const Json& bindings, const HttpResponse& response, const Json& body) const {
    auto object = body.object();
    const auto& members = schema_->shape(target).at("members");
    std::set<std::string, std::less<>> explicit_headers;
    for (const auto& binding : bindings.at("headers").array()) { const auto& pair = binding.array(); explicit_headers.insert(lower(pair[1].string())); const auto found = response.headers.find(lower(pair[1].string())); if (found != response.headers.end()) object.insert_or_assign(pair[0].string(), media(header(*schema_, members.at(pair[0].string()), found->second), bindings, pair[0].string(), false)); }
    if (const auto* prefixes = bindings.find("prefixHeaders")) for (const auto& binding : prefixes->array()) {
        const auto& pair = binding.array(); const auto prefix = lower(pair[1].string()); Json::Object values;
        for (const auto& [name, text] : response.headers) if (lower(name).starts_with(prefix) && !explicit_headers.contains(lower(name))) values.emplace(lower(name).substr(prefix.size()), media(Json(text), bindings, pair[0].string(), false));
        object.insert_or_assign(pair[0].string(), Json(std::move(values)));
    }
    if (const auto* member = bindings.find("responseCode")) object.insert_or_assign(member->string(), Json(static_cast<std::int64_t>(response.status)));
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
    const auto* name = body.find("__type");
    const Json header_name(get(response.headers, "x-crucible-error").value_or(""));
    if (!header_name.string().empty()) name = &header_name;
    if (name && !string(*name)) name = nullptr;
    std::vector<std::string> matches;
    for (const auto& error : operation.at("errors").array()) { const auto& shape = schema_->shape(error.string()); if (name ? (shape.at("error").find("wireCode") ? shape.at("error").at("wireCode").string() : shape.at("name").string()) == name->string() : shape.at("error").at("status").number().integer() == response.status) matches.push_back(error.string()); }
    const Json* shape = matches.size() == 1 ? &schema_->shape(matches[0]) : nullptr;
    const Json* traits = shape ? &shape->at("error") : nullptr;
    const auto* message = body.find("message");
    ErrorMetadata metadata;
    metadata.name = name ? name->string() : shape ? shape->at("name").string() : "UnknownError";
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
    return {metadata, std::make_exception_ptr(ServiceError(metadata))};
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
    const auto prepared = prepare(op, input);
    const bool safe = op.at("safe").boolean();
    for (unsigned attempt = 1;; ++attempt) {
        scope.check();
        auto request = prepared;
        if (op.at("bearer").boolean() && config_.token) { const auto token = config_.token(); scope.check(); if (token) request.headers.insert_or_assign("authorization", "Bearer " + *token); }
        for (const auto& middleware : config_.middleware) { scope.check(); request = middleware(std::move(request)); }
        scope.check();
        Headers normalized;
        for (const auto& [key, value] : request.headers) normalized.insert_or_assign(lower(key), value);
        request.headers = std::move(normalized);
        validate_request(request);
        if (digest_) { request.headers.erase("x-amz-content-sha256"); if (request.body || request.method == "POST" || request.method == "PUT" || request.method == "PATCH") request.headers.insert_or_assign("x-amz-content-sha256", sha256(request.body.value_or(""))); }
        const auto& bindings = op.at("response");
        HttpResponse response;
        try { response = transport_->send(request, !bindings.at("body").array().empty() || !bindings.at("payload").is_null(), scope); }
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
