#include "client_runtime.hpp"
#include <openssl/crypto.h>
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
std::string escape(const std::string& text) { validate_utf8(text); return uri_encode(text); }
std::string uuid() {
    std::array<unsigned char, 16> bytes{};
    if (RAND_bytes(bytes.data(), static_cast<int>(bytes.size())) != 1) throw std::runtime_error("Secure random generation failed");
    bytes[6] = static_cast<unsigned char>((bytes[6] & 15) | 64);
    bytes[8] = static_cast<unsigned char>((bytes[8] & 63) | 128);
    const auto text = hex(std::string_view(reinterpret_cast<const char*>(bytes.data()), bytes.size()));
    return text.substr(0, 8) + "-" + text.substr(8, 4) + "-" + text.substr(12, 4) + "-" + text.substr(16, 4) + "-" + text.substr(20);
}
std::string sha256(const std::string& body) { return hex(digest_bytes("SHA256", body)); }
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
std::string protocol(const Schema& schema) { const auto* value = schema.description().find("protocol"); return value ? value->string() : std::string{}; }
bool aws_json(const std::string& protocol) { return protocol == "awsRestJson1" || protocol == "awsJson1_0" || protocol == "awsJson1_1"; }
bool iequals(std::string_view left, std::string_view right) { return left.size() == right.size() && lower(std::string(left)) == lower(std::string(right)); }
bool flag(const Json& object, std::string_view name) { const auto* value = object.find(name); return value && value->boolean(); }
ChecksumSettings checksum_settings(const Json& description, Json::Object& values, const ClientConfig& config, bool streaming) {
    ChecksumSettings settings;
    if (description.at("kind").string() == "md5") { settings.algorithm = "MD5"; settings.header = "content-md5"; return settings; }
    const auto* algorithm_member = description.find("requestAlgorithmMember");
    if (algorithm_member && !algorithm_member->is_null()) {
        const auto& member = algorithm_member->string();
        const auto found = values.find(member);
        std::string algorithm = found != values.end() && string(found->second) ? found->second.string() : std::string{};
        // A stream gets no implicit checksum: it is never buffered to hash it.
        if (algorithm.empty() && (flag(description, "requestRequired") || (config.request_checksum_calculation != ChecksumPolicy::when_required && !streaming))) {
            algorithm = "CRC32";
            values.insert_or_assign(member, Json(algorithm));
        }
        if (!algorithm.empty()) {
            if (!checksum_supported(algorithm)) throw std::invalid_argument("Unsupported checksum algorithm: " + algorithm);
            settings.algorithm = algorithm;
            settings.header = "x-amz-checksum-" + lower(algorithm);
        }
    } else if (flag(description, "requestRequired")) { settings.algorithm = "MD5"; settings.header = "content-md5"; }
    const auto* mode_member = description.find("validationModeMember");
    if (mode_member && !mode_member->is_null()) {
        const auto& member = mode_member->string();
        const auto found = values.find(member);
        std::string mode = found != values.end() && string(found->second) ? found->second.string() : std::string{};
        if (mode.empty() && config.response_checksum_validation != ChecksumPolicy::when_required) { mode = "ENABLED"; values.insert_or_assign(member, Json(mode)); }
        if (mode == "ENABLED") for (const auto& algorithm : description.at("responseAlgorithms").array()) if (checksum_supported(algorithm.string())) settings.response.push_back(algorithm.string());
    }
    return settings;
}
// A caller-supplied credential for a scheme is honored without invoking that scheme's provider.
bool preset(const HttpRequest& request, const std::string& kind, const Json& scheme) {
    std::string value = get(request.headers, "authorization").value_or(""), prefix;
    if (kind == "basic") prefix = "Basic";
    else if (kind == "bearer") prefix = "Bearer";
    else if (kind == "sigV4") prefix = "AWS4-HMAC-SHA256";
    else if (kind == "apiKey") {
        const auto& name = scheme.at("name").string();
        if (scheme.at("location").string() == "query") {
            const auto query = split_url(request.url).query;
            std::size_t start = 0;
            while (start < query.size()) {
                const auto end = query.find('&', start);
                const auto pair = query.substr(start, end == std::string::npos ? std::string::npos : end - start);
                const auto equals = pair.find('=');
                if (unescape(pair.substr(0, equals)) == name && equals != std::string::npos && equals + 1 < pair.size()) return true;
                if (end == std::string::npos) break;
                start = end + 1;
            }
            return false;
        }
        value = get(request.headers, name).value_or("");
        const auto* configured = scheme.find("scheme");
        prefix = configured && !configured->is_null() ? configured->string() : std::string{};
        if (prefix.empty()) return !value.empty();
    } else return false;
    return value.size() > prefix.size() + 1 && iequals(std::string_view(value).substr(0, prefix.size() + 1), prefix + " ");
}
}
WaiterError::WaiterError(std::string name, unsigned attempts, Json response, std::exception_ptr cause, Json reason, const std::string& message)
    : std::runtime_error("Waiter " + name + " failed after " + std::to_string(attempts) + " attempts: " + message), name_(std::move(name)), attempts_(attempts), response_(std::move(response)), reason_(std::move(reason)), cause_(std::move(cause)) {}
ClientRuntime::ClientRuntime(ClientConfig config, const Schema& schema, ErrorFactory errors, std::string endpoint, bool digest)
    : config_(std::move(config)), schema_(&schema), errors_(std::move(errors)), endpoint_(config_.endpoint.value_or(std::move(endpoint))), transport_(config_.transport ? config_.transport : std::make_shared<CurlTransport>()), digest_(digest) {
    (void)scope();
    if (config_.max_attempts == 0 || !std::isfinite(config_.base_delay) || config_.base_delay < 0 || !std::isfinite(config_.max_delay) || config_.max_delay < 0) throw std::invalid_argument("Invalid retry limits");
    validate_endpoint_parameters(config_.endpoint_parameters);
    validate_endpoint_parameters(config_.endpoint_builtins);
    // Endpoint rules can derive the whole endpoint, so a client endpoint is then optional.
    if (!(endpoint_.empty() && schema_->description().find("endpointRules"))) validate_url(endpoint_, true);
    while (endpoint_.ends_with('/')) endpoint_.pop_back();
}
void ClientRuntime::close() { if (!closed_.exchange(true) && !config_.transport) transport_->close(); }
ClientRuntime::Prepared ClientRuntime::prepare(const std::string& name, const Json& operation, const Json& input, bool streaming) const {
    Prepared prepared;
    auto values = input.object();
    if (const auto* checksum = operation.find("checksum")) prepared.checksum = checksum_settings(*checksum, values, config_, streaming);
    std::string endpoint = endpoint_;
    const auto* rules = schema_->description().find("endpointRules");
    const auto* host_prefix = operation.find("endpoint");
    if (rules || host_prefix) {
        const auto& target = operation.at("input");
        const auto view = query_model(*schema_, target.is_null() ? std::string_view{} : std::string_view(target.string()), Json(values));
        if (rules) {
            auto builtins = config_.endpoint_builtins;
            if (!builtins.contains("AWS::Region") && config_.region) builtins.emplace("AWS::Region", Json(*config_.region));
            if (!builtins.contains("SDK::Endpoint") && config_.endpoint && !config_.endpoint->empty()) builtins.emplace("SDK::Endpoint", Json(*config_.endpoint));
            prepared.endpoint = resolve_endpoint(*rules, operation.find("endpointParameters"), view, config_.endpoint_parameters, builtins);
            endpoint = prepared.endpoint->url;
        }
        if (host_prefix && !config_.disable_host_prefix) endpoint = prefix_endpoint(endpoint, *host_prefix, view);
        while (endpoint.ends_with('/')) endpoint.pop_back();
        validate_url(endpoint, true);
    }
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
        const auto value = retime(found->second, binding_format(bindings, pair[0].string()));
        auto append = [&](const Json& item) { path += path.find('?') == std::string::npos ? '?' : '&'; path += escape(pair[1].string()) + "=" + escape(bound(item)); };
        if (std::holds_alternative<Json::Array>(value.value())) for (const auto& item : value.array()) append(item);
        else append(value);
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
    HttpRequest request{endpoint + path, operation.at("method").string(), {}, std::nullopt};
    for (const auto& [key, value] : config_.headers) request.headers.insert_or_assign(lower(key), value);
    if (prepared.endpoint) for (const auto& [key, items] : prepared.endpoint->headers) {
        std::string joined;
        for (const auto& item : items) { if (!joined.empty()) joined += ", "; joined += item; }
        request.headers.insert_or_assign(lower(key), joined);
    }
    for (const auto& binding : bindings.at("headers").array()) {
        const auto& pair = binding.array();
        const auto found = values.find(pair[0].string());
        if (found != values.end() && !found->second.is_null()) request.headers.insert_or_assign(lower(pair[1].string()), bound(media(retime(found->second, binding_format(bindings, pair[0].string())), bindings, pair[0].string(), true)));
    }
    std::set<std::string, std::less<>> explicit_headers;
    for (const auto& binding : bindings.at("headers").array()) explicit_headers.insert(lower(binding.array()[1].string()));
    if (const auto* prefixes = bindings.find("prefixHeaders")) for (const auto& binding : prefixes->array()) {
        const auto& pair = binding.array(); const auto found = values.find(pair[0].string());
        if (found == values.end() || found->second.is_null()) continue;
        for (const auto& [key, value] : found->second.object()) {
            const auto header_name = lower(pair[1].string() + key);
            if (!explicit_headers.contains(header_name)) request.headers.insert_or_assign(header_name, bound(media(value, bindings, pair[0].string(), true)));
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
    const auto wire = protocol(*schema_);
    if (wire == "awsJson1_0" || wire == "awsJson1_1") {
        // AWS JSON RPC always sends an object body and names the operation in X-Amz-Target.
        if (!request.body) request.body = "{}";
        request.headers.insert_or_assign("content-type", wire == "awsJson1_0" ? "application/x-amz-json-1.0" : "application/x-amz-json-1.1");
        const auto& service = schema_->description().at("service").string();
        request.headers.insert_or_assign("x-amz-target", service.substr(service.rfind('#') + 1) + "." + name);
    }
    validate_request(request);
    prepared.request = std::move(request);
    return prepared;
}
std::optional<SigningOptions> ClientRuntime::authenticate(const Json& operation, HttpRequest& request, const std::optional<ResolvedEndpoint>& endpoint, const CallScope& scope, bool& credentials, std::optional<AwsCredentials>& identity) const {
    static const std::string bearer_id = "smithy.api#httpBearerAuth";
    const bool bearer = operation.at("bearer").boolean();
    std::vector<std::string> ids;
    if (const auto* auth = operation.find("auth")) for (const auto& id : auth->array()) ids.push_back(id.string());
    else if (bearer) ids.push_back(bearer_id);
    const auto* schemes = schema_->description().find("authSchemes");
    for (const auto& id : ids) {
        Json::Object scheme = id == bearer_id ? Json::Object{{"kind", Json("bearer")}} : schemes->at(id).object();
        const auto kind = scheme.at("kind").string();
        if (endpoint) {
            if (const auto found = endpoint->properties.find("authSchemes"); found != endpoint->properties.end() && std::holds_alternative<Json::Array>(found->second.value())) {
                bool matched = false;
                for (const auto& choice : found->second.array()) {
                    if (!std::holds_alternative<Json::Object>(choice.value())) continue;
                    const auto* choice_name = choice.find("name");
                    if (!choice_name || !string(*choice_name) || !iequals(choice_name->string(), kind)) continue;
                    matched = true;
                    for (const auto* key : {"signingName", "signingRegion", "disableDoubleEncoding", "disableNormalizePath"}) if (const auto* value = choice.find(key)) scheme.insert_or_assign(key, *value);
                    break;
                }
                if (!matched) continue;
            }
        }
        const Json description(scheme);
        if (preset(request, kind, description)) { credentials = true; return std::nullopt; }
        if (kind == "bearer" && config_.token) {
            const auto token = config_.token();
            scope.check();
            if (token && !token->empty()) { request.headers.insert_or_assign("authorization", "Bearer " + *token); credentials = true; return std::nullopt; }
        } else if (kind == "basic" && config_.basic_credentials) {
            const auto value = config_.basic_credentials();
            scope.check();
            if (value) {
                if (value->username.find(':') != std::string::npos) throw std::invalid_argument("Basic username cannot contain a colon");
                request.headers.insert_or_assign("authorization", "Basic " + base64(blob(value->username + ":" + value->password)));
                credentials = true;
                return std::nullopt;
            }
        } else if (kind == "apiKey" && config_.api_key) {
            const auto value = config_.api_key();
            scope.check();
            if (value && !value->empty()) {
                const auto& header = description.at("name").string();
                if (description.at("location").string() == "query") {
                    request.url += request.url.find('?') == std::string::npos ? '?' : '&';
                    request.url += uri_encode(header) + "=" + uri_encode(*value);
                } else {
                    const auto* prefix = description.find("scheme");
                    request.headers.insert_or_assign(lower(header), (prefix && !prefix->is_null() && !prefix->string().empty() ? prefix->string() + " " : std::string{}) + *value);
                }
                credentials = true;
                return std::nullopt;
            }
        } else if (kind == "sigV4" && config_.credentials) {
            auto value = config_.credentials(scope);
            scope.check();
            if (value) {
                SigningOptions options;
                const auto* region = description.find("signingRegion");
                options.region = region && string(*region) ? region->string() : config_.region.value_or("");
                const auto* service = description.find("signingName");
                options.service = service && string(*service) ? service->string() : std::string{};
                options.unsigned_payload = flag(operation, "unsignedPayload");
                if (const auto* option = description.find("disableDoubleEncoding"); option && !option->is_null()) options.disable_double_encoding = option->boolean();
                if (const auto* option = description.find("disableNormalizePath"); option && !option->is_null()) options.disable_normalize_path = option->boolean();
                identity = std::move(value);
                credentials = true;
                return options;
            }
        }
    }
    if (!ids.empty() && !(ids.size() == 1 && ids.front() == bearer_id && bearer) && !flag(operation, "optionalAuth")) throw std::invalid_argument("No identity available for modeled authentication schemes");
    return std::nullopt;
}
Json ClientRuntime::apply_headers(const std::string& target, const Json& bindings, const HttpResponse& response, const Json& body) const {
    auto object = body.object();
    const auto& members = schema_->shape(target).at("members");
    std::set<std::string, std::less<>> explicit_headers;
    for (const auto& binding : bindings.at("headers").array()) { const auto& pair = binding.array(); explicit_headers.insert(lower(pair[1].string())); const auto found = response.headers.find(lower(pair[1].string())); if (found != response.headers.end()) object.insert_or_assign(pair[0].string(), media(header(*schema_, members.at(pair[0].string()), found->second, binding_format(bindings, pair[0].string())), bindings, pair[0].string(), false)); }
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
    const bool aws = aws_json(protocol(*schema_));
    std::optional<std::string> name;
    if (const auto* type = body.find("__type"); type && string(*type)) name = type->string();
    if (const auto header_name = get(response.headers, "x-crucible-error"); header_name && !header_name->empty()) name = header_name;
    if (aws) {
        // AWS JSON discriminators may be qualified shape IDs and may carry a ":" suffix.
        if (const auto* code = body.find("code"); code && string(*code)) name = code->string();
        if (const auto header_name = get(response.headers, "x-amzn-errortype"); header_name && !header_name->empty()) name = header_name;
        if (name) {
            *name = name->substr(0, name->find(':'));
            if (const auto hash = name->rfind('#'); hash != std::string::npos) *name = name->substr(hash + 1);
            if (name->empty()) name.reset();
        }
    }
    std::vector<std::string> matches;
    for (const auto& error : operation.at("errors").array()) { const auto& shape = schema_->shape(error.string()); if (name ? (shape.at("error").find("wireCode") ? shape.at("error").at("wireCode").string() : shape.at("name").string()) == *name : shape.at("error").at("status").number().integer() == response.status) matches.push_back(error.string()); }
    const Json* shape = matches.size() == 1 ? &schema_->shape(matches[0]) : nullptr;
    const Json* traits = shape ? &shape->at("error") : nullptr;
    const auto* message = body.find("message");
    if (aws && !(message && string(*message))) message = body.find("Message");
    ErrorMetadata metadata;
    metadata.name = name ? *name : shape ? shape->at("name").string() : "UnknownError";
    metadata.message = message && string(*message) ? message->string() : "HTTP " + std::to_string(response.status);
    metadata.status = response.status;
    metadata.headers = response.headers;
    metadata.request_id = get(response.headers, "x-request-id");
    if (aws) { if (auto id = get(response.headers, "x-amzn-requestid")) metadata.request_id = id; else if (auto amz = get(response.headers, "x-amz-request-id")) metadata.request_id = amz; }
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
namespace {
// A streaming response body that verifies a response checksum as bytes arrive.
// A mismatch fails the final read and every read after it.
class VerifiedBody final : public ResponseBody {
    std::shared_ptr<ResponseBody> inner_;
    std::optional<IncrementalChecksum> checksum_;
    std::string expected_;
    std::function<void()> validated_;
    std::exception_ptr failure_;
    bool done_ = false;
public:
    VerifiedBody(std::shared_ptr<ResponseBody> inner, std::optional<IncrementalChecksum> checksum, std::string expected, std::function<void()> validated)
        : inner_(std::move(inner)), checksum_(std::move(checksum)), expected_(std::move(expected)), validated_(std::move(validated)) {}
    std::size_t read(char* buffer, std::size_t size) override {
        if (failure_) std::rethrow_exception(failure_);
        if (done_ || size == 0) return 0;
        std::size_t count = 0;
        try { count = inner_->read(buffer, size); }
        catch (...) { failure_ = std::current_exception(); throw; }
        if (checksum_ && count) checksum_->update(std::string_view(buffer, count));
        if (count == 0) {
            done_ = true;
            if (checksum_) {
                Blob raw;
                for (const unsigned char byte : checksum_->finish()) raw.push_back(static_cast<std::byte>(byte));
                const auto actual = base64(raw);
                if (actual.size() != expected_.size() || CRYPTO_memcmp(actual.data(), expected_.data(), actual.size()) != 0) {
                    failure_ = std::make_exception_ptr(ChecksumError(checksum_->algorithm()));
                    std::rethrow_exception(failure_);
                }
                if (validated_) validated_();
            }
        }
        return count;
    }
    void close() noexcept override { inner_->close(); }
};
std::optional<std::uint64_t> length_bound(const Json& member, const char* name) {
    const auto* length = member.find("length");
    const auto* value = length ? length->find(name) : nullptr;
    if (!value || value->is_null()) return std::nullopt;
    const auto number = value->number().integer();
    return number < 0 ? std::optional<std::uint64_t>(0) : std::optional<std::uint64_t>(static_cast<std::uint64_t>(number));
}
std::shared_ptr<detail::StreamSource> resolve_source(const std::optional<ByteStream>& body) {
    if (!body || body->empty()) return nullptr;
    if (body->source()) {
        if (!body->source()->replayable && body->source()->used->load()) throw StreamConsumedError();
        return body->source();
    }
    // A response stream passed as a request body is read through once.
    auto source = std::make_shared<detail::StreamSource>();
    source->open = [response = body->response()] { return StreamReader([response](char* buffer, std::size_t size) { return response->read(buffer, size); }); };
    return source;
}
}
ClientRuntime::CallResult ClientRuntime::exchange(const std::string& name, const Json& input, const CallScope& scope, std::optional<ByteStream> body) const {
    scope.check();
    if (closed_) throw std::logic_error("Client is closed");
    const auto& op = operation(name);
    const auto& request_bindings = op.at("request");
    const auto& response_bindings = op.at("response");
    const bool streaming_request = flag(request_bindings, "streaming");
    const bool streaming_response = flag(response_bindings, "streaming");
    std::shared_ptr<detail::StreamSource> source;
    Json member(Json::Object{});
    if (streaming_request) {
        member = schema_->shape(op.at("input").string()).at("members").at(request_bindings.at("payload").array()[0].string());
        source = resolve_source(body);
        const auto minimum = length_bound(member, "min"), maximum = length_bound(member, "max");
        if (source) {
            // Lengths are checked before anything is sent, and again as bytes flow.
            if (flag(request_bindings, "requiresLength") && !source->length) throw StreamLengthError("The operation requires a stream of known length");
            if (source->length && minimum && *source->length < *minimum) throw StreamLengthError(std::to_string(*source->length) + " bytes is below the modeled minimum of " + std::to_string(*minimum));
            if (source->length && maximum && *source->length > *maximum) throw StreamLengthError(std::to_string(*source->length) + " bytes is above the modeled maximum of " + std::to_string(*maximum));
        } else if (minimum && *minimum > 0) throw StreamLengthError("An absent stream is below the modeled minimum");
    }
    const auto prepared = prepare(name, op, input, source != nullptr);
    const bool safe = op.at("safe").boolean();
    // A one-shot stream is read once, so its operation is never retried.
    const unsigned attempts = source && !source->replayable ? 1U : config_.max_attempts;
    for (unsigned attempt = 1;; ++attempt) {
        scope.check();
        auto request = prepared.request;
        std::shared_ptr<AttemptBody> upload;
        if (source) {
            upload = std::make_shared<AttemptBody>(source, length_bound(member, "min"), length_bound(member, "max"), scope);
            if (source->length && *source->length == 0) {
                // The transport never pulls a zero-length body, so validate its EOF here.
                char extra = 0;
                (void)upload->read(&extra, 1);
                request.body = std::string{};
            } else request.stream = upload;
            if (!body->sha256().empty()) request.headers.insert_or_assign("x-amz-content-sha256", body->sha256());
        }
        bool credentials = false;
        std::optional<AwsCredentials> identity;
        const auto signing = authenticate(op, request, prepared.endpoint, scope, credentials, identity);
        for (const auto& middleware : config_.middleware) { scope.check(); request = middleware(std::move(request)); }
        scope.check();
        Headers normalized;
        for (const auto& [key, value] : request.headers) normalized.insert_or_assign(lower(key), value);
        request.headers = std::move(normalized);
        validate_request(request);
        if (credentials) validate_url(request.url, false, true);
        if (request.stream) {
            // Streams are never buffered to hash them: digests and checksums are declared.
            if (digest_ && !request.headers.contains("x-amz-content-sha256")) throw StreamDigestError("Declare the stream's SHA-256 with ByteStream::with_sha256 or an x-amz-content-sha256 header");
            if (!prepared.checksum.header.empty() && !request.headers.contains(prepared.checksum.header)) throw StreamDigestError("Supply the " + prepared.checksum.header + " header for a streaming request");
        } else {
            if (digest_) { request.headers.erase("x-amz-content-sha256"); if (request.body || request.method == "POST" || request.method == "PUT" || request.method == "PATCH") request.headers.insert_or_assign("x-amz-content-sha256", sha256(request.body.value_or(""))); }
            const auto& checksum_header = prepared.checksum.header;
            if (!checksum_header.empty() && !request.headers.contains(checksum_header)) request.headers.insert_or_assign(checksum_header, checksum(prepared.checksum.algorithm, request.body.value_or("")));
        }
        if (signing) { sign_v4(request, *identity, *signing); validate_request(request); }
        HttpResponse response;
        try {
            if (streaming_request || streaming_response) {
                response = transport_->open(request, scope);
                if (!response.stream) throw NetworkError("Transport returned no response stream");
            } else response = transport_->send(request, !response_bindings.at("body").array().empty() || !response_bindings.at("payload").is_null() || !prepared.checksum.response.empty(), scope);
        }
        catch (const NetworkError&) { scope.check(); if (!safe || attempt >= attempts) throw; scope.wait(retry_delay(attempt, std::nullopt)); continue; }
        scope.check();
        Headers headers;
        for (const auto& [key, value] : response.headers) headers.insert_or_assign(lower(key), value);
        response.headers = std::move(headers);
        const bool ok = response.status >= 200 && response.status < 300;
        if (ok && streaming_response) {
            std::optional<IncrementalChecksum> verifier;
            std::string expected;
            std::function<void()> validated;
            for (const auto& algorithm : prepared.checksum.response) {
                const auto value = get(response.headers, "x-amz-checksum-" + lower(algorithm));
                if (!value) continue;
                verifier.emplace(algorithm);
                expected = *value;
                if (config_.checksum_validated) validated = [callback = config_.checksum_validated, name, algorithm] { callback(name, algorithm); };
                break;
            }
            auto stream = response.stream;
            // Header decoding failures release the unread body with the stream's last owner.
            auto value = apply_headers(op.at("output").string(), response_bindings, response, Json(Json::Object{{response_bindings.at("payload").array()[0].string(), Json(Json::Object{})}}));
            return {std::move(value), ByteStream(std::make_shared<VerifiedBody>(std::move(stream), std::move(verifier), std::move(expected), std::move(validated)))};
        }
        if (response.stream) {
            try { response.body = drain(*response.stream); }
            catch (const NetworkError&) { scope.check(); if (!safe || attempt >= attempts) throw; scope.wait(retry_delay(attempt, std::nullopt)); continue; }
            response.stream.reset();
        }
        if (ok) {
            for (const auto& algorithm : prepared.checksum.response) {
                const auto expected = get(response.headers, "x-amz-checksum-" + lower(algorithm));
                if (!expected) continue;
                const auto actual = checksum(algorithm, response.body);
                if (expected->size() != actual.size() || CRYPTO_memcmp(expected->data(), actual.data(), actual.size()) != 0) throw ChecksumError(algorithm);
                if (config_.checksum_validated) config_.checksum_validated(name, algorithm);
                break;
            }
            return {success(op, response), std::nullopt};
        }
        const auto [metadata, error] = failure(op, response);
        const bool transient = response.status == 500 || response.status == 502 || response.status == 503 || response.status == 504;
        if (!(metadata.throttling || (safe && (metadata.retryable || transient))) || attempt >= attempts) std::rethrow_exception(error);
        scope.wait(retry_delay(attempt, get(response.headers, "retry-after")));
    }
}
namespace {
std::pair<std::string, Json> waiter_state(const Json& description, const Json& input, const Json& output, const std::exception_ptr& failure) {
    std::optional<std::string> error_name;
    if (failure) { try { std::rethrow_exception(failure); } catch (const ServiceError& error) { error_name = error.metadata().name; } catch (...) {} }
    for (const auto& acceptor : description.at("acceptors").array()) {
        const auto& matcher = acceptor.at("matcher");
        const auto& kind = matcher.at("kind").string();
        bool matches = false;
        if (kind == "success") matches = (failure == nullptr) == matcher.at("expected").boolean();
        else if (kind == "errorType") {
            if (error_name) { const auto& wanted = matcher.at("name").string(); matches = *error_name == wanted || *error_name == wanted.substr(wanted.rfind('#') == std::string::npos ? 0 : wanted.rfind('#') + 1); }
        } else if (!failure) {
            const auto source = kind == "inputOutput" ? Json(Json::Object{{"input", input}, {"output", output}}) : output;
            const auto result = query(matcher.at("path").at("expression"), source);
            const auto& expected = matcher.at("expected").string();
            const auto& comparator = matcher.at("comparator").string();
            if (comparator == "stringEquals") matches = string(result) && result.string() == expected;
            else if (comparator == "booleanEquals") matches = std::holds_alternative<bool>(result.value()) && result.boolean() == (expected == "true");
            else if (const auto* items = std::get_if<Json::Array>(&result.value())) {
                bool all = !items->empty(), any = false;
                for (const auto& item : *items) { const bool equal = string(item) && item.string() == expected; all = all && equal; any = any || equal; }
                matches = comparator == "allStringEquals" ? all : comparator == "anyStringEquals" && any;
            }
        }
        if (matches) {
            const auto* message = acceptor.find("message");
            return {acceptor.at("state").string(), message && !message->is_null() ? query(message->at("expression"), output) : Json()};
        }
    }
    return {failure ? "failure" : "retry", Json()};
}
std::string describe(const Json& reason, const std::exception_ptr& cause) {
    if (!reason.is_null()) return std::holds_alternative<std::string>(reason.value()) ? reason.string() : reason.dump();
    if (cause) { try { std::rethrow_exception(cause); } catch (const std::exception& error) { return error.what(); } catch (...) {} }
    return "failure acceptor matched";
}
}
std::optional<ClientRuntime::CallResult> ClientRuntime::wait_json(const std::string& name, const std::string& waiter, const Json& input, double timeout, const WaiterOptions& options, std::stop_token stop, const std::function<Json(const Json&)>& view, const std::optional<ByteStream>& body) const {
    const auto& op = operation(name);
    const auto& description = op.at("waiters").at(waiter);
    const double minimum = options.min_delay.value_or(description.at("minDelay").number().floating());
    const double maximum = options.max_delay.value_or(description.at("maxDelay").number().floating());
    if (!std::isfinite(timeout) || timeout <= 0 || !std::isfinite(minimum) || minimum < 0 || !std::isfinite(maximum) || maximum < minimum) throw std::invalid_argument("Waiter deadline and ordered delays must be valid");
    const auto& input_target = op.at("input");
    const auto& output_target = op.at("output");
    const auto input_view = query_model(*schema_, input_target.is_null() ? std::string_view{} : std::string_view(input_target.string()), input);
    const CallScope overall(timeout, stop);
    unsigned attempts = 0;
    Json response;
    std::exception_ptr failure;
    const auto expired = [&] { try { overall.check(); return false; } catch (const DeadlineError&) { return true; } };
    const auto timed_out = [&] { return WaiterTimeoutError(waiter, attempts, response, failure, Json(), "deadline exceeded"); };
    thread_local std::mt19937 engine(std::random_device{}());
    for (;;) {
        if (expired()) throw timed_out();
        ++attempts;
        failure = nullptr;
        response = Json();
        Json output;
        std::optional<CallResult> result;
        try {
            const CallScope attempt((std::min)(config_.timeout, overall.remaining()), stop);
            // Response streams of attempts that are not returned are released with their result.
            result = exchange(name, input, attempt, body);
            output = query_model(*schema_, output_target.is_null() ? std::string_view{} : std::string_view(output_target.string()), view(result->value));
            response = output;
        } catch (const CancelledError&) { throw; }
        catch (const std::exception&) { failure = std::current_exception(); result.reset(); }
        if (expired()) throw timed_out();
        std::pair<std::string, Json> state;
        try { state = waiter_state(description, input_view, output, failure); }
        catch (const std::invalid_argument&) { throw WaiterError(waiter, attempts, response, std::current_exception(), Json(), "acceptor evaluation failed"); }
        if (state.first == "success") return result;
        if (state.first == "failure") throw WaiterError(waiter, attempts, response, failure, state.second, describe(state.second, failure));
        const double ceiling = (std::min)(maximum, minimum * std::ldexp(1.0, static_cast<int>((std::min)(attempts - 1, 62U))));
        double delay = minimum + std::uniform_real_distribution<double>(0.0, 1.0)(engine) * (ceiling - minimum);
        double remaining = 0;
        try { remaining = overall.remaining(); } catch (const DeadlineError&) { throw timed_out(); }
        if (remaining <= minimum) delay = remaining;
        else if (remaining - delay <= minimum) delay = remaining - minimum;
        try { overall.wait(delay); } catch (const DeadlineError&) { throw timed_out(); }
    }
}
}
