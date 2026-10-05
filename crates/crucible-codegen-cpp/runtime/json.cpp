#include "json.hpp"
#include <charconv>
#include <cmath>
#include <limits>

namespace crucible_generated::runtime {
namespace {
bool digit(char c) { return c >= '0' && c <= '9'; }
std::size_t number_end(std::string_view text, std::size_t pos) {
    auto fail = [] { throw std::invalid_argument("Invalid JSON number"); };
    if (pos < text.size() && text[pos] == '-') ++pos;
    if (pos == text.size()) fail();
    if (text[pos] == '0') ++pos;
    else {
        if (!digit(text[pos])) fail();
        while (pos < text.size() && digit(text[pos])) ++pos;
    }
    if (pos < text.size() && text[pos] == '.') {
        ++pos;
        if (pos == text.size() || !digit(text[pos])) fail();
        while (pos < text.size() && digit(text[pos])) ++pos;
    }
    if (pos < text.size() && (text[pos] == 'e' || text[pos] == 'E')) {
        ++pos;
        if (pos < text.size() && (text[pos] == '+' || text[pos] == '-')) ++pos;
        if (pos == text.size() || !digit(text[pos])) fail();
        while (pos < text.size() && digit(text[pos])) ++pos;
    }
    return pos;
}
void append_utf8(std::string& result, std::uint32_t point) {
    if (point < 0x80) result += static_cast<char>(point);
    else if (point < 0x800) {
        result += static_cast<char>(0xc0 | (point >> 6));
        result += static_cast<char>(0x80 | (point & 0x3f));
    } else if (point < 0x10000) {
        result += static_cast<char>(0xe0 | (point >> 12));
        result += static_cast<char>(0x80 | ((point >> 6) & 0x3f));
        result += static_cast<char>(0x80 | (point & 0x3f));
    } else {
        result += static_cast<char>(0xf0 | (point >> 18));
        result += static_cast<char>(0x80 | ((point >> 12) & 0x3f));
        result += static_cast<char>(0x80 | ((point >> 6) & 0x3f));
        result += static_cast<char>(0x80 | (point & 0x3f));
    }
}
class Parser {
    std::string_view text_;
    std::size_t pos_ = 0;
    void whitespace() { while (pos_ < text_.size() && (text_[pos_] == ' ' || text_[pos_] == '\t' || text_[pos_] == '\n' || text_[pos_] == '\r')) ++pos_; }
    bool take(char c) { if (pos_ < text_.size() && text_[pos_] == c) { ++pos_; return true; } return false; }
    void require(char c) { if (!take(c)) throw std::invalid_argument("Invalid JSON punctuation"); }
    std::uint32_t hex() {
        if (text_.size() - pos_ < 4) throw std::invalid_argument("Truncated Unicode escape");
        std::uint32_t result = 0;
        for (int index = 0; index < 4; ++index) {
            const char c = text_[pos_++];
            const int value = digit(c) ? c - '0' : c >= 'a' && c <= 'f' ? c - 'a' + 10 : c >= 'A' && c <= 'F' ? c - 'A' + 10 : -1;
            if (value < 0) throw std::invalid_argument("Invalid Unicode escape");
            result = result * 16 + static_cast<unsigned>(value);
        }
        return result;
    }
    std::string string() {
        require('"');
        std::string result;
        while (pos_ < text_.size()) {
            char c = text_[pos_++];
            if (c == '"') { validate_utf8(result); return result; }
            if (static_cast<unsigned char>(c) < 32) throw std::invalid_argument("Unescaped JSON control character");
            if (c != '\\') { result += c; continue; }
            if (pos_ == text_.size()) throw std::invalid_argument("Truncated JSON escape");
            c = text_[pos_++];
            switch (c) {
            case '"': case '\\': case '/': result += c; break;
            case 'b': result += '\b'; break;
            case 'f': result += '\f'; break;
            case 'n': result += '\n'; break;
            case 'r': result += '\r'; break;
            case 't': result += '\t'; break;
            case 'u': {
                auto point = hex();
                if (point >= 0xd800 && point <= 0xdbff) {
                    require('\\'); require('u');
                    const auto low = hex();
                    if (low < 0xdc00 || low > 0xdfff) throw std::invalid_argument("Invalid Unicode surrogate pair");
                    point = 0x10000 + ((point - 0xd800) << 10) + low - 0xdc00;
                } else if (point >= 0xdc00 && point <= 0xdfff) throw std::invalid_argument("Lone Unicode surrogate");
                append_utf8(result, point); break;
            }
            default: throw std::invalid_argument("Invalid JSON escape");
            }
        }
        throw std::invalid_argument("Unterminated JSON string");
    }
    Json value(unsigned depth) {
        if (depth > 255) throw std::invalid_argument("JSON nesting limit");
        whitespace();
        if (pos_ == text_.size()) throw std::invalid_argument("Missing JSON value");
        const char c = text_[pos_];
        if (c == '"') return Json(string());
        if (take('[')) {
            Json::Array items;
            whitespace();
            if (take(']')) return Json(std::move(items));
            do { items.push_back(value(depth + 1)); whitespace(); if (take(']')) return Json(std::move(items)); require(','); } while (true);
        }
        if (take('{')) {
            Json::Object items;
            whitespace();
            if (take('}')) return Json(std::move(items));
            do { whitespace(); auto key = string(); whitespace(); require(':'); items.insert_or_assign(std::move(key), value(depth + 1)); whitespace(); if (take('}')) return Json(std::move(items)); require(','); } while (true);
        }
        for (const auto word : {std::string_view("null"), std::string_view("true"), std::string_view("false")}) {
            if (text_.substr(pos_, word.size()) == word) { pos_ += word.size(); return word == "null" ? Json() : Json(word == "true"); }
        }
        const auto end = number_end(text_, pos_);
        auto number = Number(std::string(text_.substr(pos_, end - pos_)));
        pos_ = end;
        return Json(std::move(number));
    }
public:
    explicit Parser(std::string_view text) : text_(text) {}
    Json parse() { auto result = value(0); whitespace(); if (pos_ != text_.size()) throw std::invalid_argument("Trailing JSON input"); return result; }
};
std::string quote(std::string_view text) {
    validate_utf8(text);
    constexpr char hex[] = "0123456789abcdef";
    std::string result = "\"";
    for (const auto raw : text) {
        const auto c = static_cast<unsigned char>(raw);
        if (c == '"' || c == '\\') { result += '\\'; result += raw; }
        else if (c < 32) { result += "\\u00"; result += hex[c >> 4]; result += hex[c & 15]; }
        else result += raw;
    }
    return result + '"';
}
}

void validate_utf8(std::string_view text) {
    std::size_t index = 0;
    while (index < text.size()) {
        auto c = static_cast<unsigned char>(text[index++]);
        if (c < 0x80) continue;
        const unsigned length = c >= 0xc2 && c <= 0xdf ? 1 : c >= 0xe0 && c <= 0xef ? 2 : c >= 0xf0 && c <= 0xf4 ? 3 : 0;
        if (length == 0 || text.size() - index < length) throw std::invalid_argument("Invalid UTF-8");
        std::uint32_t point = c & (0x7fU >> (length + 1));
        for (unsigned count = 0; count < length; ++count) {
            c = static_cast<unsigned char>(text[index++]);
            if ((c & 0xc0) != 0x80) throw std::invalid_argument("Invalid UTF-8 continuation");
            point = (point << 6) | (c & 0x3f);
        }
        if ((length == 1 && point < 0x80) || (length == 2 && point < 0x800) || (length == 3 && point < 0x10000) || (point >= 0xd800 && point <= 0xdfff) || point > 0x10ffff) throw std::invalid_argument("Invalid UTF-8 scalar");
    }
}

Number::Number(std::string text) : text_(std::move(text)) {
    if (number_end(text_, 0) != text_.size()) throw std::invalid_argument("Invalid JSON number");
}
std::string Number::scaled(unsigned places) const {
    if (places > 10000) throw std::invalid_argument("Numeric scale limit");
    const bool negative = text_[0] == '-';
    const auto exponent_at = text_.find_first_of("eE");
    const auto mantissa = text_.substr(negative ? 1 : 0, exponent_at == std::string::npos ? std::string::npos : exponent_at - (negative ? 1 : 0));
    int exponent = 0;
    if (exponent_at != std::string::npos) {
        auto raw = std::string_view(text_).substr(exponent_at + 1);
        if (raw.front() == '+') raw.remove_prefix(1);
        const auto [end, error] = std::from_chars(raw.data(), raw.data() + raw.size(), exponent);
        if (error != std::errc() || end != raw.data() + raw.size() || exponent < -10000 || exponent > 10000) throw std::invalid_argument("Numeric exponent limit");
    }
    std::string digits;
    std::size_t fraction = 0;
    bool decimal = false;
    for (const auto c : mantissa) { if (c == '.') decimal = true; else { digits += c; if (decimal) ++fraction; } }
    if (fraction > 1000000) throw std::invalid_argument("Numeric precision limit");
    const auto shift = static_cast<long long>(places) + exponent - static_cast<long long>(fraction);
    if (shift >= 0) digits.append(static_cast<std::size_t>(shift), '0');
    else {
        auto count = static_cast<std::size_t>(-shift);
        while (count > 0 && !digits.empty() && digits.back() == '0') { digits.pop_back(); --count; }
        if (count > 0 && digits.find_first_not_of('0') != std::string::npos) throw std::invalid_argument("Numeric precision exceeds target");
        if (count > 0) digits = "0";
    }
    const auto first = digits.find_first_not_of('0');
    if (first == std::string::npos) return "0";
    digits.erase(0, first);
    return (negative ? "-" : "") + digits;
}
std::int64_t Number::integer(std::int64_t minimum, std::int64_t maximum) const {
    const auto text = scaled(0);
    std::int64_t result = 0;
    const auto [end, error] = std::from_chars(text.data(), text.data() + text.size(), result);
    if (error != std::errc() || end != text.data() + text.size() || result < minimum || result > maximum) throw std::invalid_argument("Integer out of range");
    return result;
}
double Number::floating() const {
    double result = 0;
    const auto [end, error] = std::from_chars(text_.data(), text_.data() + text_.size(), result);
    if (error != std::errc() || end != text_.data() + text_.size() || !std::isfinite(result)) throw std::invalid_argument("Float out of range");
    return result;
}
Json::Json(double value) {
    if (!std::isfinite(value)) throw std::invalid_argument("Non-finite JSON number");
    char buffer[64];
    const auto [end, error] = std::to_chars(buffer, buffer + sizeof(buffer), value, std::chars_format::general, std::numeric_limits<double>::max_digits10);
    if (error != std::errc()) throw std::invalid_argument("Cannot encode JSON number");
    value_ = Number(std::string(buffer, end));
}
const std::string& Json::string() const { if (const auto* value = std::get_if<std::string>(&value_)) return *value; throw std::invalid_argument("Expected string"); }
const Number& Json::number() const { if (const auto* value = std::get_if<Number>(&value_)) return *value; throw std::invalid_argument("Expected number"); }
bool Json::boolean() const { if (const auto* value = std::get_if<bool>(&value_)) return *value; throw std::invalid_argument("Expected boolean"); }
const Json::Array& Json::array() const { if (const auto* value = std::get_if<Array>(&value_)) return *value; throw std::invalid_argument("Expected list"); }
const Json::Object& Json::object() const { if (const auto* value = std::get_if<Object>(&value_)) return *value; throw std::invalid_argument("Expected object"); }
const Json& Json::at(std::string_view key) const { const auto* value = find(key); if (!value) throw std::invalid_argument("Missing JSON member: " + std::string(key)); return *value; }
const Json* Json::find(std::string_view key) const { const auto& map = object(); const auto found = map.find(key); return found == map.end() ? nullptr : &found->second; }
Json Json::parse(std::string_view text) { return Parser(text).parse(); }
std::string Json::dump(unsigned depth) const {
    if (depth > 255) throw std::invalid_argument("JSON nesting limit");
    if (is_null()) return "null";
    if (const auto* value = std::get_if<bool>(&value_)) return *value ? "true" : "false";
    if (const auto* value = std::get_if<Number>(&value_)) return value->text();
    if (const auto* value = std::get_if<std::string>(&value_)) return quote(*value);
    std::string result;
    if (const auto* values = std::get_if<Array>(&value_)) {
        result = "[";
        for (const auto& value : *values) { if (result.size() > 1) result += ','; result += value.dump(depth + 1); }
        return result + ']';
    }
    result = "{";
    for (const auto& [key, value] : object()) { if (result.size() > 1) result += ','; result += quote(key) + ':' + value.dump(depth + 1); }
    return result + '}';
}
}
