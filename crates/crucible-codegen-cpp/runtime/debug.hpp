#pragma once
#include "codec.hpp"
#include <sstream>

namespace crucible_generated::runtime {
/** The marker used in diagnostic representations of sensitive data. */
inline constexpr const char* redacted = "*** Sensitive Data Redacted ***";

/** Formats a native value without serializing it or consuming streaming bodies. */
template<class T> std::string debug_value(const T& value);
/** Joins named diagnostic fields; omission and explicit null remain distinct. */
class Debug {
    std::string text_;
    bool first_ = true;
public:
    explicit Debug(std::string name) : text_(std::move(name) + "(") {}
    template<class T> Debug& field(const char* name, const T& value, bool sensitive = false) {
        if constexpr (detail::is_field<T>) { if (!value.is_set()) return *this; }
        if (!first_) text_ += ", ";
        first_ = false;
        text_ += name; text_ += "=";
        text_ += sensitive ? redacted : debug_value(value);
        return *this;
    }
    [[nodiscard]] std::string finish() const { return text_ + ")"; }
};

template<class T> std::string debug_value(const T& value) {
    if constexpr (requires { debug_string(value); }) return debug_string(value);
    else if constexpr (std::is_convertible_v<T, std::string_view>) return Json(std::string(value)).dump();
    else if constexpr (std::is_same_v<T, bool>) return value ? "true" : "false";
    else if constexpr (std::is_arithmetic_v<T>) { std::ostringstream out; out << +value; return out.str(); }
    else if constexpr (std::is_same_v<T, Unit>) return "Unit()";
    else if constexpr (std::is_same_v<T, Json>) return value.dump();
    else if constexpr (std::is_same_v<T, Timestamp>) return "Timestamp(" + value.epoch().text() + ")";
    else if constexpr (requires { value.text(); }) return value.text();
    else if constexpr (std::is_same_v<T, ByteStream>) return "ByteStream(<stream>)";
    else if constexpr (std::is_same_v<T, Blob>) { std::string out = "["; for (auto byte : value) { if (out.size() > 1) out += ", "; out += std::to_string(std::to_integer<unsigned>(byte)); } return out + "]"; }
    else if constexpr (detail::is_field<T>) return value.is_set() ? (value.is_null() ? "null" : debug_value(*value)) : "absent";
    else if constexpr (requires { value.has_value(); }) return value ? debug_value(*value) : "null";
    else if constexpr (requires { value.value(); }) { try { return debug_value(value.value()); } catch (const std::invalid_argument&) { return "null"; } }
    else if constexpr (requires { typename T::mapped_type; }) {
        std::string out = "{"; for (const auto& [key, item] : value) { if (out.size() > 1) out += ", "; out += debug_value(key) + ": " + debug_value(item); } return out + "}";
    } else {
        std::string out = "["; for (const auto& item : value) { if (out.size() > 1) out += ", "; out += debug_value(item); } return out + "]";
    }
}
}
