#include "endpoints.hpp"
#include "query.hpp"
#include "transport.hpp"
#include <charconv>

namespace crucible_generated::runtime {
namespace {
[[noreturn]] void fail(const std::string& message) { throw EndpointError(message); }
const std::string& text(const Json& value) { if (!std::holds_alternative<std::string>(value.value())) fail("Endpoint rules expected a string"); return value.string(); }
const Json::Array& array(const Json& value) { if (!std::holds_alternative<Json::Array>(value.value())) fail("Endpoint rules expected an array"); return value.array(); }
const Json::Object& object(const Json& value) { if (!std::holds_alternative<Json::Object>(value.value())) fail("Endpoint rules expected an object"); return value.object(); }
bool boolean(const Json& value) { if (!std::holds_alternative<bool>(value.value())) fail("Endpoint function requires a boolean"); return value.boolean(); }
long long integer(const Json& value) {
    if (!std::holds_alternative<Number>(value.value())) fail("Endpoint function requires an integer");
    try { return value.number().integer(); } catch (const std::invalid_argument&) { fail("Endpoint integer out of range"); }
}
bool label(std::string_view text) {
    if (text.empty() || text.size() > 63) return false;
    const auto alnum = [](char c) { return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9'); };
    if (!alnum(text.front()) || !alnum(text.back())) return false;
    for (const char c : text) if (!alnum(c) && c != '-') return false;
    return true;
}
bool ipv4(std::string_view text) {
    int parts = 0;
    std::size_t start = 0;
    for (;;) {
        const auto end = text.find('.', start);
        const auto part = text.substr(start, end == std::string_view::npos ? std::string_view::npos : end - start);
        unsigned value = 0;
        const auto parsed = std::from_chars(part.data(), part.data() + part.size(), value);
        if (part.empty() || part.size() > 3 || parsed.ec != std::errc() || parsed.ptr != part.data() + part.size() || value > 255) return false;
        ++parts;
        if (end == std::string_view::npos) break;
        start = end + 1;
    }
    return parts == 4;
}
bool ipv6(std::string_view text) {
    if (text.find(':') == std::string_view::npos) return false;
    for (const char c : text) if (!((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F') || c == ':' || c == '.')) return false;
    return true;
}
Json attribute(Json value, const Json& path) {
    Json::Array steps;
    if (std::holds_alternative<std::string>(path.value())) {
        const auto& source = path.string();
        std::size_t start = 0;
        for (;;) {
            const auto end = source.find('.', start);
            const auto part = source.substr(start, end == std::string::npos ? std::string::npos : end - start);
            const auto bracket = part.find('[');
            if (bracket != 0) steps.push_back(Json(Json::Object{{"kind", Json("member")}, {"name", Json(part.substr(0, bracket))}}));
            if (bracket != std::string::npos) {
                if (!part.ends_with(']')) fail("Invalid endpoint attribute index");
                const auto digits = part.substr(bracket + 1, part.size() - bracket - 2);
                long long index = 0;
                const auto parsed = std::from_chars(digits.data(), digits.data() + digits.size(), index);
                if (digits.empty() || parsed.ec != std::errc() || parsed.ptr != digits.data() + digits.size()) fail("Invalid endpoint attribute index");
                steps.push_back(Json(Json::Object{{"kind", Json("index")}, {"index", Json(static_cast<std::int64_t>(index))}}));
            }
            if (end == std::string::npos) break;
            start = end + 1;
        }
    } else steps = array(path);
    for (const auto& step : steps) {
        if (text(step.at("kind")) == "member") {
            if (!std::holds_alternative<Json::Object>(value.value())) return Json();
            const auto* item = value.find(text(step.at("name")));
            value = item ? *item : Json();
        } else {
            if (!std::holds_alternative<Json::Array>(value.value())) return Json();
            const auto& items = value.array();
            auto index = integer(step.at("index"));
            if (index < 0) index += static_cast<long long>(items.size());
            if (index < 0 || index >= static_cast<long long>(items.size())) return Json();
            value = Json(items[static_cast<std::size_t>(index)]);
        }
        if (value.is_null()) return value;
    }
    return value;
}
Json expression(const Json& node, const Json::Object& values) {
    const auto& kind = text(node.at("kind"));
    if (kind == "reference") { const auto found = values.find(text(node.at("name"))); return found == values.end() ? Json() : found->second; }
    if (kind == "boolean" || kind == "integer") return node.at("value");
    if (kind == "array") { Json::Array result; for (const auto& item : array(node.at("values"))) result.push_back(expression(item, values)); return Json(std::move(result)); }
    if (kind == "object") { Json::Object result; for (const auto& [key, item] : object(node.at("values"))) result.emplace(key, expression(item, values)); return Json(std::move(result)); }
    if (kind == "call") { Json::Array arguments; for (const auto& item : array(node.at("arguments"))) arguments.push_back(expression(item, values)); return endpoint_function(text(node.at("function")), arguments); }
    if (kind == "string") {
        std::string result;
        for (const auto& part : array(node.at("parts"))) {
            Json value;
            if (text(part.at("kind")) == "literal") value = part.at("value");
            else { const auto found = values.find(text(part.at("name"))); const auto* path = part.find("path"); value = attribute(found == values.end() ? Json() : found->second, path ? *path : Json(Json::Array{})); }
            if (const auto* item = std::get_if<std::string>(&value.value())) result += *item;
            else if (const auto* flag = std::get_if<bool>(&value.value())) result += *flag ? "true" : "false";
            else if (const auto* number = std::get_if<Number>(&value.value())) result += number->text();
            else fail("Endpoint template references a missing or non-scalar value");
        }
        return Json(result);
    }
    fail("Unknown endpoint expression");
}
std::optional<ResolvedEndpoint> rules(const Json::Array& nodes, const Json::Object& parameters) {
    for (const auto& rule : nodes) {
        auto scope = parameters;
        bool matched = true;
        for (const auto& condition : array(rule.at("conditions"))) {
            Json::Array arguments;
            for (const auto& argument : array(condition.at("arguments"))) arguments.push_back(expression(argument, scope));
            auto value = endpoint_function(text(condition.at("function")), arguments);
            if (value.is_null() || (std::holds_alternative<bool>(value.value()) && !value.boolean())) { matched = false; break; }
            if (const auto* assign = condition.find("assign"); assign && !assign->is_null()) scope.insert_or_assign(text(*assign), std::move(value));
        }
        if (!matched) continue;
        const auto& result = rule.at("result");
        const auto& kind = text(result.at("kind"));
        if (kind == "tree") { if (auto nested = rules(array(result.at("rules")), scope)) return nested; continue; }
        if (kind == "error") fail(text(expression(result.at("message"), scope)));
        if (kind == "endpoint") {
            ResolvedEndpoint resolved;
            resolved.url = text(expression(result.at("url"), scope));
            if (parse_endpoint(resolved.url).is_null()) fail("Endpoint rule returned an invalid URL");
            if (const auto* headers = result.find("headers")) for (const auto& [name, items] : object(*headers)) for (const auto& item : array(items)) resolved.headers[name].push_back(text(expression(item, scope)));
            if (const auto* properties = result.find("properties")) for (const auto& [name, item] : object(*properties)) resolved.properties.emplace(name, expression(item, scope));
            return resolved;
        }
        fail("Unknown endpoint rule result");
    }
    return std::nullopt;
}
}
bool valid_host_label(const std::string& value, bool subdomains) {
    if (!subdomains) return label(value);
    std::size_t start = 0;
    for (;;) {
        const auto end = value.find('.', start);
        if (!label(std::string_view(value).substr(start, end == std::string::npos ? std::string::npos : end - start))) return false;
        if (end == std::string::npos) return true;
        start = end + 1;
    }
}
Json parse_endpoint(const std::string& url) {
    for (const unsigned char c : url) if (c <= 32 || c == 127 || c == '?' || c == '#') return Json();
    const auto separator = url.find("://");
    if (separator == std::string::npos) return Json();
    const auto scheme = lower(url.substr(0, separator));
    if (scheme != "http" && scheme != "https") return Json();
    const auto rest = url.substr(separator + 3);
    const auto slash = rest.find('/');
    const auto authority = rest.substr(0, slash);
    const auto path = slash == std::string::npos ? std::string("/") : rest.substr(slash);
    if (authority.empty() || authority.find('@') != std::string::npos) return Json();
    std::string host, port;
    bool ip = false;
    if (authority.front() == '[') {
        const auto close = authority.find(']');
        if (close == std::string::npos) return Json();
        host = authority.substr(1, close - 1);
        if (!ipv6(host)) return Json();
        ip = true;
        const auto after = authority.substr(close + 1);
        if (!after.empty()) { if (after.front() != ':') return Json(); port = after.substr(1); }
    } else {
        const auto colon = authority.find(':');
        host = authority.substr(0, colon);
        if (colon != std::string::npos) port = authority.substr(colon + 1);
        if (port.find(':') != std::string::npos) return Json();
        ip = ipv4(host);
        auto name = host;
        if (name.ends_with('.')) name.pop_back();
        if (!ip && !valid_host_label(name, true)) return Json();
    }
    auto normalized = lower(host);
    if (normalized.find(':') != std::string::npos) normalized = "[" + normalized + "]";
    if (!port.empty()) {
        unsigned number = 0;
        const auto parsed = std::from_chars(port.data(), port.data() + port.size(), number);
        if (parsed.ec != std::errc() || parsed.ptr != port.data() + port.size() || number > 65535) return Json();
        if (!((scheme == "https" && number == 443) || (scheme == "http" && number == 80))) normalized += ":" + port;
    }
    return Json(Json::Object{{"scheme", Json(scheme)}, {"authority", Json(normalized)}, {"path", Json(path)},
        {"normalizedPath", Json(path.ends_with('/') ? path : path + "/")}, {"isIp", Json(ip)}});
}
Json endpoint_function(const std::string& name, const Json::Array& arguments) {
    const auto argument = [&](std::size_t index) -> const Json& { if (index >= arguments.size()) fail("Endpoint function arity"); return arguments[index]; };
    if (name == "isSet") return Json(!argument(0).is_null());
    if (name == "not") return Json(!boolean(argument(0)));
    if (name == "booleanEquals") return Json(std::holds_alternative<bool>(argument(0).value()) && std::holds_alternative<bool>(argument(1).value()) && argument(0).boolean() == argument(1).boolean());
    if (name == "stringEquals") return Json(std::holds_alternative<std::string>(argument(0).value()) && std::holds_alternative<std::string>(argument(1).value()) && argument(0).string() == argument(1).string());
    if (name == "getAttr") return attribute(argument(0), argument(1));
    if (name == "substring") {
        const auto& value = text(argument(0));
        const auto start = integer(argument(1)), end = integer(argument(2));
        for (const unsigned char c : value) if (c > 127) return Json();
        const auto size = static_cast<long long>(value.size());
        if (start < 0 || end < start || end > size) return Json();
        if (boolean(argument(3))) return Json(value.substr(static_cast<std::size_t>(size - end), static_cast<std::size_t>(end - start)));
        return Json(value.substr(static_cast<std::size_t>(start), static_cast<std::size_t>(end - start)));
    }
    if (name == "isValidHostLabel") return Json(valid_host_label(text(argument(0)), boolean(argument(1))));
    if (name == "parseUrl") return parse_endpoint(text(argument(0)));
    if (name == "uriEncode") return Json(uri_encode(text(argument(0))));
    fail("Unknown endpoint function: " + name);
}
void validate_endpoint_parameters(const Json::Object& values) {
    for (const auto& [name, value] : values) {
        if (value.is_null() || std::holds_alternative<std::string>(value.value()) || std::holds_alternative<bool>(value.value())) continue;
        if (const auto* items = std::get_if<Json::Array>(&value.value())) {
            for (const auto& item : *items) if (!std::holds_alternative<std::string>(item.value())) throw std::invalid_argument("Endpoint parameter arrays require strings: " + name);
            continue;
        }
        throw std::invalid_argument("Endpoint parameters require strings, booleans, or string arrays: " + name);
    }
}
ResolvedEndpoint resolve_endpoint(const Json& ruleset, const Json* sources, const Json& input, const Json::Object& client, const Json::Object& builtins) {
    try {
        Json::Object parameters;
        for (const auto& [name, parameter] : object(ruleset.at("parameters"))) {
            Json value;
            const auto* candidates = sources ? sources->find(name) : nullptr;
            if (candidates) for (const auto& source : array(*candidates)) {
                const auto& kind = text(source.at("kind"));
                if (kind == "static" || kind == "default") value = source.at("value");
                else if (kind == "inputMember") { if (std::holds_alternative<Json::Object>(input.value())) { const auto* item = input.find(text(source.at("member"))); if (item) value = *item; } }
                else if (kind == "inputPath") value = query(source.at("path").at("expression"), input);
                else if (kind == "client") { const auto found = client.find(text(source.at("name"))); if (found != client.end()) value = found->second; }
                else if (kind == "builtIn") { const auto found = builtins.find(text(source.at("name"))); if (found != builtins.end()) value = found->second; }
                if (!value.is_null()) break;
            }
            const auto* required = parameter.find("required");
            if (value.is_null() && required && required->boolean()) fail("Required endpoint parameter missing: " + name);
            if (!value.is_null()) {
                const auto& kind = text(parameter.at("kind"));
                bool valid = false;
                if (kind == "string") valid = std::holds_alternative<std::string>(value.value());
                else if (kind == "boolean") valid = std::holds_alternative<bool>(value.value());
                else if (kind == "stringArray" && std::holds_alternative<Json::Array>(value.value())) {
                    valid = true;
                    for (const auto& item : value.array()) valid = valid && std::holds_alternative<std::string>(item.value());
                }
                if (!valid) fail("Endpoint parameter has the wrong type: " + name);
            }
            parameters.insert_or_assign(name, std::move(value));
        }
        if (auto result = rules(array(ruleset.at("rules")), parameters)) return std::move(*result);
        fail("No endpoint rule matched");
    } catch (const EndpointError&) { throw; }
    catch (const std::invalid_argument& error) { throw EndpointError(error.what()); }
}
std::string prefix_endpoint(const std::string& endpoint, const Json& description, const Json& input) {
    std::string prefix;
    for (const auto& part : array(description.at("parts"))) {
        if (text(part.at("kind")) == "literal") { prefix += text(part.at("value")); continue; }
        const auto* value = std::holds_alternative<Json::Object>(input.value()) ? input.find(text(part.at("member"))) : nullptr;
        if (!value || !std::holds_alternative<std::string>(value->value()) || !valid_host_label(value->string(), false)) fail("Invalid endpoint host label");
        prefix += value->string();
    }
    const auto separator = endpoint.find("://");
    if (separator == std::string::npos) fail("Invalid endpoint");
    const auto start = separator + 3;
    const auto end = endpoint.find_first_of("/?#", start);
    const auto authority = endpoint.substr(start, end == std::string::npos ? std::string::npos : end - start);
    const auto colon = authority.rfind(':');
    const bool bracketed = !authority.empty() && authority.front() == '[';
    const auto hostname = bracketed || colon == std::string::npos ? authority : authority.substr(0, colon);
    const auto port = bracketed || colon == std::string::npos ? std::string{} : authority.substr(colon);
    const auto host = prefix + hostname;
    if (bracketed || host.size() > 253 || !valid_host_label(host, true)) fail("Invalid expanded endpoint hostname");
    return endpoint.substr(0, start) + host + port + (end == std::string::npos ? std::string{} : endpoint.substr(end));
}
}
