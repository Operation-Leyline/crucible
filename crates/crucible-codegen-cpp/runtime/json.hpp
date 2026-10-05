#pragma once
#include <cstdint>
#include <map>
#include <stdexcept>
#include <string>
#include <string_view>
#include <variant>
#include <utility>
#include <vector>

namespace crucible_generated::runtime {
class Number {
    std::string text_;
public:
    explicit Number(std::string text);
    [[nodiscard]] const std::string& text() const noexcept { return text_; }
    [[nodiscard]] std::string scaled(unsigned places) const;
    [[nodiscard]] std::int64_t integer(std::int64_t minimum = INT64_MIN, std::int64_t maximum = INT64_MAX) const;
    [[nodiscard]] double floating() const;
    [[nodiscard]] std::pair<std::string, std::string> canonical() const;
};

class BigInteger {
    Number number_;
public:
    explicit BigInteger(std::string text) : number_(std::move(text)) { if (number_.canonical().second.starts_with('-')) throw std::invalid_argument("Expected integral number"); }
    [[nodiscard]] const Number& number() const noexcept { return number_; }
    [[nodiscard]] const std::string& text() const noexcept { return number_.text(); }
    bool operator==(const BigInteger& other) const { return number_.canonical() == other.number_.canonical(); }
};
class BigDecimal {
    Number number_;
public:
    explicit BigDecimal(std::string text) : number_(std::move(text)) {}
    [[nodiscard]] const Number& number() const noexcept { return number_; }
    [[nodiscard]] const std::string& text() const noexcept { return number_.text(); }
    bool operator==(const BigDecimal& other) const { return number_.canonical() == other.number_.canonical(); }
};

class Json {
public:
    using Array = std::vector<Json>;
    using Object = std::map<std::string, Json, std::less<>>;
    using Value = std::variant<std::nullptr_t, bool, Number, std::string, Array, Object>;
private:
    Value value_;
public:
    Json() : value_(nullptr) {}
    explicit Json(std::nullptr_t) : value_(nullptr) {}
    explicit Json(bool value) : value_(value) {}
    explicit Json(std::int64_t value) : value_(Number(std::to_string(value))) {}
    explicit Json(double value);
    explicit Json(Number value) : value_(std::move(value)) {}
    explicit Json(std::string value) : value_(std::move(value)) {}
    explicit Json(const char* value) : value_(std::string(value)) {}
    explicit Json(Array value) : value_(std::move(value)) {}
    explicit Json(Object value) : value_(std::move(value)) {}
    [[nodiscard]] const Value& value() const noexcept { return value_; }
    [[nodiscard]] bool is_null() const noexcept { return std::holds_alternative<std::nullptr_t>(value_); }
    [[nodiscard]] const std::string& string() const;
    [[nodiscard]] const Number& number() const;
    [[nodiscard]] bool boolean() const;
    [[nodiscard]] const Array& array() const;
    [[nodiscard]] const Object& object() const;
    [[nodiscard]] const Json& at(std::string_view key) const;
    [[nodiscard]] const Json* find(std::string_view key) const;
    [[nodiscard]] static Json parse(std::string_view text);
    [[nodiscard]] std::string dump(unsigned depth = 0) const;
};

void validate_utf8(std::string_view text);
[[nodiscard]] bool equivalent(const Json& left, const Json& right);
}
