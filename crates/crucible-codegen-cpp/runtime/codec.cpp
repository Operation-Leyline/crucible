#include "codec.hpp"
#include <algorithm>

namespace crucible_generated::runtime {
namespace {
std::string_view trim(std::string_view text) {
    while (!text.empty() && (text.front() == ' ' || text.front() == '\t')) text.remove_prefix(1);
    while (!text.empty() && (text.back() == ' ' || text.back() == '\t')) text.remove_suffix(1);
    return text;
}
}
Context::Context(const Schema& schema, const Json& shape, std::string_view format, unsigned depth) : schema_(&schema), shape_(&shape), format_(format), depth_(depth) {
    if (depth > 255) throw std::invalid_argument("Model nesting limit");
}
std::string_view Context::format() const {
    if (!format_.empty()) return format_;
    const auto* format = shape_->find("format");
    return format ? format->string() : std::string_view{};
}
Context Context::child(const Json& member) const {
    const auto* format = member.find("format");
    return Context(*schema_, schema_->shape(member.at("target").string()), format && !format->is_null() ? format->string() : std::string_view{}, depth_ + 1);
}
Context Context::member(std::string_view name) const { return child(shape_->at("members").at(name)); }
Json document(const Schema& schema, std::string_view target, const Json& value, bool encode) {
    if (value.is_null()) return value;
    const auto& shape = schema.shape(target);
    const auto& kind = shape.at("kind").string();
    if (kind == "structure" || kind == "union") {
        Json::Object result;
        std::vector<std::string> known, wire_names;
        for (const auto& [name, member] : shape.at("members").object()) {
            const auto* renamed = member.find("jsonName");
            const auto& wire = renamed ? renamed->string() : name;
            const auto& source = encode ? name : wire;
            known.push_back(source); wire_names.push_back(wire);
            if (const auto* item = value.find(source)) result.emplace(encode ? wire : name, document(schema, member.at("target").string(), *item, encode));
        }
        if (kind == "union") for (const auto& [name, item] : value.object()) {
            if (std::find(known.begin(), known.end(), name) != known.end()) continue;
            if (encode && std::find(wire_names.begin(), wire_names.end(), name) != wire_names.end()) throw std::invalid_argument("Unknown variant name is declared");
            result.emplace(name, item);
        }
        return Json(std::move(result));
    }
    if (kind == "list") {
        Json::Array result;
        for (const auto& item : value.array()) result.push_back(document(schema, shape.at("member").at("target").string(), item, encode));
        return Json(std::move(result));
    }
    if (kind == "map") {
        Json::Object result;
        for (const auto& [name, item] : value.object()) result.emplace(name, document(schema, shape.at("value").at("target").string(), item, encode));
        return Json(std::move(result));
    }
    return value;
}
std::string base64(const Blob& bytes) {
    constexpr std::string_view alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    std::string result;
    for (std::size_t index = 0; index < bytes.size(); index += 3) {
        const auto a = std::to_integer<unsigned>(bytes[index]);
        const auto b = index + 1 < bytes.size() ? std::to_integer<unsigned>(bytes[index + 1]) : 0;
        const auto c = index + 2 < bytes.size() ? std::to_integer<unsigned>(bytes[index + 2]) : 0;
        result += alphabet[a >> 2];
        result += alphabet[((a & 3) << 4) | (b >> 4)];
        result += index + 1 < bytes.size() ? alphabet[((b & 15) << 2) | (c >> 6)] : '=';
        result += index + 2 < bytes.size() ? alphabet[c & 63] : '=';
    }
    return result;
}
Blob unbase64(std::string_view text) {
    constexpr std::string_view alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    if (text.size() % 4 != 0) throw std::invalid_argument("Expected padded base64");
    Blob result;
    for (std::size_t index = 0; index < text.size(); index += 4) {
        unsigned word = 0;
        unsigned padding = 0;
        for (unsigned part = 0; part < 4; ++part) {
            const char c = text[index + part];
            word <<= 6;
            if (c == '=') { if (part < 2 || index + 4 != text.size()) throw std::invalid_argument("Invalid base64 padding"); ++padding; }
            else { const auto value = alphabet.find(c); if (value == std::string_view::npos || padding != 0) throw std::invalid_argument("Invalid base64"); word |= static_cast<unsigned>(value); }
        }
        result.push_back(static_cast<std::byte>(word >> 16));
        if (padding < 2) result.push_back(static_cast<std::byte>((word >> 8) & 255));
        if (padding == 0) result.push_back(static_cast<std::byte>(word & 255));
    }
    if (base64(result) != text) throw std::invalid_argument("Noncanonical base64");
    return result;
}
Json header(const Schema& schema, const Json& member, std::string_view text) {
    const auto& shape = schema.shape(member.at("target").string());
    const auto& kind = shape.at("kind").string();
    if (kind == "list") {
        Json::Array result;
        if (trim(text).empty()) return Json(std::move(result));
        std::size_t start = 0;
        bool quoted = false, escaped = false;
        auto append = [&](std::size_t end) {
            const auto part = trim(text.substr(start, end - start));
            const auto value = !part.empty() && part.front() == '"' ? Json::parse(part).string() : std::string(part);
            result.push_back(header(schema, shape.at("member"), value));
        };
        for (std::size_t index = 0; index < text.size(); ++index) {
            const char c = text[index];
            if (c == ',' && !quoted) { append(index); start = index + 1; }
            else if (escaped) escaped = false;
            else if (c == '\\' && quoted) escaped = true;
            else if (c == '"') quoted = !quoted;
        }
        if (quoted) throw std::invalid_argument("Unterminated quoted header");
        append(text.size());
        return Json(std::move(result));
    }
    if (kind == "byte" || kind == "short" || kind == "integer" || kind == "long" || kind == "intEnum" || kind == "bigInteger" || kind == "bigDecimal" || kind == "float" || kind == "double" || kind == "boolean" || (kind == "timestamp" && member.at("format").string() == "epoch-seconds")) return Json::parse(text);
    return Json(std::string(text));
}
std::string bound(const Json& value) {
    if (const auto* text = std::get_if<std::string>(&value.value())) return *text;
    if (const auto* number = std::get_if<Number>(&value.value())) return number->text();
    if (const auto* boolean = std::get_if<bool>(&value.value())) return *boolean ? "true" : "false";
    if (const auto* values = std::get_if<Json::Array>(&value.value())) {
        std::string result;
        bool first = true;
        for (const auto& item : *values) {
            if (!first) result += ", ";
            first = false;
            const auto text = bound(item);
            result += text.empty() || text.find_first_of(",\"\\") != std::string::npos || trim(text) != text ? Json(text).dump() : text;
        }
        return result;
    }
    throw std::invalid_argument("Expected bound scalar");
}
}
