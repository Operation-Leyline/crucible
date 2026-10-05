#pragma once
#include "json.hpp"
#include "timestamp.hpp"
#include <cstddef>
#include <concepts>
#include <type_traits>
#include <memory>
#include <optional>
#include <utility>

namespace crucible_generated::runtime {
using Blob = std::vector<std::byte>;
struct Unit {};
struct HeaderNameLess {
    bool operator()(std::string_view left, std::string_view right) const noexcept {
        const auto fold = [](unsigned char c) { return c >= 'A' && c <= 'Z' ? static_cast<unsigned char>(c + ('a' - 'A')) : c; };
        for (std::size_t i = 0; i < left.size() && i < right.size(); ++i) {
            const auto a = fold(static_cast<unsigned char>(left[i]));
            const auto b = fold(static_cast<unsigned char>(right[i]));
            if (a != b) return a < b;
        }
        return left.size() < right.size();
    }
};
using Headers = std::map<std::string, std::string, HeaderNameLess>;

template<class T> class Field;
namespace detail {
template<class T> inline constexpr bool is_field = false;
template<class T> inline constexpr bool is_field<Field<T>> = true;
}

template<class T> class Field {
    bool set_ = false;
    std::optional<T> value_;
public:
    Field() = default;
    // A Field's explicit bool conversion describes presence, not its contained value.
    template<class U> requires (!detail::is_field<std::remove_cvref_t<U>> && std::constructible_from<T, U&&>)
    Field(U&& value) : set_(true), value_(std::in_place, std::forward<U>(value)) {}
    template<class U> requires (!detail::is_field<std::remove_cvref_t<U>> && std::constructible_from<T, U&&>)
    Field& operator=(U&& value) { value_ = T(std::forward<U>(value)); set_ = true; return *this; }
    [[nodiscard]] explicit operator bool() const noexcept { return value_.has_value(); }
    [[nodiscard]] const T& operator*() const { return value(); }
    [[nodiscard]] T& operator*() { return value(); }
    [[nodiscard]] const T* operator->() const { return &value(); }
    [[nodiscard]] T* operator->() { return &value(); }
    void reset() noexcept { value_.reset(); set_ = false; }
    template<class U> [[nodiscard]] T value_or(U&& fallback) const { return value_.value_or(std::forward<U>(fallback)); }
    [[nodiscard]] static Field null() { Field result; result.set_ = true; return result; }
    [[nodiscard]] static Field of(T value) { Field result; result.set_ = true; result.value_ = std::move(value); return result; }
    [[nodiscard]] bool is_set() const noexcept { return set_; }
    [[nodiscard]] bool is_null() const noexcept { return set_ && !value_; }
    [[nodiscard]] const T& value() const { return value_.value(); }
    [[nodiscard]] T& value() { return value_.value(); }
};

// Recursive model edges own their value; copies never alias a caller's graph.
template<class T> class Box {
    std::unique_ptr<T> value_;
public:
    Box() = default;
    [[nodiscard]] const T& operator*() const { return value(); }
    [[nodiscard]] T& operator*() { return value(); }
    [[nodiscard]] const T* operator->() const { return &value(); }
    [[nodiscard]] T* operator->() { return &value(); }
    Box(T value) : value_(std::make_unique<T>(std::move(value))) {}
    Box(const Box& other) : value_(other.value_ ? std::make_unique<T>(*other.value_) : nullptr) {}
    Box(Box&&) noexcept = default;
    Box& operator=(const Box& other) { if (this != &other) value_ = other.value_ ? std::make_unique<T>(*other.value_) : nullptr; return *this; }
    Box& operator=(Box&&) noexcept = default;
    [[nodiscard]] const T& value() const { if (!value_) throw std::invalid_argument("Empty recursive model box"); return *value_; }
    [[nodiscard]] T& value() { if (!value_) throw std::invalid_argument("Empty recursive model box"); return *value_; }
};

struct ErrorMetadata {
    std::string name = "UnknownError";
    std::string message;
    int status = 0;
    std::optional<std::string> request_id;
    Headers headers;
    bool retryable = false;
    bool throttling = false;
};
class ServiceError : public std::runtime_error {
    ErrorMetadata metadata_;
public:
    explicit ServiceError(ErrorMetadata metadata = {}) : std::runtime_error(metadata.message), metadata_(std::move(metadata)) {}
    [[nodiscard]] const ErrorMetadata& metadata() const noexcept { return metadata_; }
};

[[nodiscard]] std::string base64(const Blob& bytes);
[[nodiscard]] Blob unbase64(std::string_view text);
}
