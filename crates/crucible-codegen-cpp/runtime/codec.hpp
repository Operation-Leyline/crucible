#pragma once
#include "types.hpp"
#include <cmath>
#include <concepts>
#include <limits>

namespace crucible_generated::runtime {
class Schema;
class Context {
    const Schema* schema_;
    const Json* shape_;
    std::string_view format_;
    unsigned depth_;
public:
    Context(const Schema& schema, const Json& shape, std::string_view format = {}, unsigned depth = 0);
    [[nodiscard]] const Json& shape() const noexcept { return *shape_; }
    [[nodiscard]] std::string_view format() const;
    [[nodiscard]] Context child(const Json& member) const;
    [[nodiscard]] Context member(std::string_view name) const;
};
class Schema {
    Json description_;
public:
    explicit Schema(Json description) : description_(std::move(description)) {}
    [[nodiscard]] const Json& description() const noexcept { return description_; }
    [[nodiscard]] const Json& shape(std::string_view target) const { return description_.at("shapes").at(target); }
    [[nodiscard]] Context context(std::string_view target) const { return Context(*this, shape(target)); }
};

template<class T> struct Codec;
template<std::signed_integral T> struct Codec<T> {
    static T decode(const Json& value, const Context&) { return static_cast<T>(value.number().integer((std::numeric_limits<T>::min)(), (std::numeric_limits<T>::max)())); }
    static Json encode(T value, const Context&) { return Json(static_cast<std::int64_t>(value)); }
};
template<std::floating_point T> struct Codec<T> {
    static T decode(const Json& value, const Context&) { const auto result = static_cast<T>(value.number().floating()); if (!std::isfinite(result)) throw std::invalid_argument("Float out of range"); return result; }
    static Json encode(T value, const Context&) { return Json(static_cast<double>(value)); }
};
template<> struct Codec<bool> {
    static bool decode(const Json& value, const Context&) { return value.boolean(); }
    static Json encode(bool value, const Context&) { return Json(value); }
};
template<> struct Codec<std::string> {
    static std::string decode(const Json& value, const Context&) { validate_utf8(value.string()); return value.string(); }
    static Json encode(const std::string& value, const Context&) { validate_utf8(value); return Json(value); }
};
template<> struct Codec<Json> {
    static Json decode(const Json& value, const Context&) { return value; }
    static Json encode(const Json& value, const Context&) { return value; }
};
template<> struct Codec<Unit> {
    static Unit decode(const Json& value, const Context&) { if (!value.object().empty()) throw std::invalid_argument("Expected unit"); return {}; }
    static Json encode(Unit, const Context&) { return Json(Json::Object{}); }
};
template<> struct Codec<Blob> {
    static Blob decode(const Json& value, const Context&) { return unbase64(value.string()); }
    static Json encode(const Blob& value, const Context&) { return Json(base64(value)); }
};
template<> struct Codec<Timestamp> {
    static Timestamp decode(const Json& value, const Context& context) { const auto format = context.format(); return format == "epoch-seconds" ? Timestamp::from_epoch(value.number()) : format == "http-date" ? Timestamp::from_http_date(value.string()) : Timestamp::parse(value.string()); }
    static Json encode(const Timestamp& value, const Context& context) { const auto format = context.format(); return format == "epoch-seconds" ? Json(value.epoch()) : format == "http-date" ? Json(value.http_date()) : Json(value.date_time()); }
};
template<class T> struct Codec<Box<T>> {
    static Box<T> decode(const Json& value, const Context& context) { return Box<T>(Codec<T>::decode(value, context)); }
    static Json encode(const Box<T>& value, const Context& context) { return Codec<T>::encode(value.value(), context); }
};
template<class T> struct Codec<std::vector<T>> {
    static std::vector<T> decode(const Json& value, const Context& context) {
        const auto child = context.child(context.shape().at("member"));
        std::vector<T> result;
        result.reserve(value.array().size());
        for (const auto& item : value.array()) result.push_back(Codec<T>::decode(item, child));
        return result;
    }
    static Json encode(const std::vector<T>& value, const Context& context) {
        const auto child = context.child(context.shape().at("member"));
        Json::Array result;
        result.reserve(value.size());
        for (const auto& item : value) result.push_back(Codec<T>::encode(item, child));
        return Json(std::move(result));
    }
};
template<class K, class V> struct Codec<std::map<K, V>> {
    static std::map<K, V> decode(const Json& value, const Context& context) {
        const auto key = context.child(context.shape().at("key"));
        const auto child = context.child(context.shape().at("value"));
        std::map<K, V> result;
        for (const auto& [name, item] : value.object()) result.emplace(Codec<K>::decode(Json(name), key), Codec<V>::decode(item, child));
        return result;
    }
    static Json encode(const std::map<K, V>& value, const Context& context) {
        const auto key = context.child(context.shape().at("key"));
        const auto child = context.child(context.shape().at("value"));
        Json::Object result;
        for (const auto& [name, item] : value) result.emplace(Codec<K>::encode(name, key).string(), Codec<V>::encode(item, child));
        return Json(std::move(result));
    }
};

template<class T> T read(const Json& object, const Context& context, std::string_view name) {
    const auto& descriptor = context.shape().at("members").at(name);
    const auto* value = object.find(name);
    if (!value) { value = &descriptor.at("default"); if (value->is_null()) throw std::invalid_argument("Missing required member: " + std::string(name)); }
    return Codec<T>::decode(*value, context.child(descriptor));
}
template<class T> Field<T> optional(const Json& object, const Context& context, std::string_view name) {
    const auto& descriptor = context.shape().at("members").at(name);
    const auto* value = object.find(name);
    if (!value) { value = &descriptor.at("default"); if (value->is_null()) return {}; }
    if (value->is_null()) return Field<T>::null();
    return Field<T>::of(Codec<T>::decode(*value, context.child(descriptor)));
}
template<class T> void put(Json::Object& object, const Context& context, const std::string& name, const T& value) { object.emplace(name, Codec<T>::encode(value, context.member(name))); }
template<class T> void put(Json::Object& object, const Context& context, const std::string& name, const Field<T>& value) {
    if (value.is_set()) object.emplace(name, value.is_null() ? Json() : Codec<T>::encode(value.value(), context.member(name)));
}
[[nodiscard]] Json header(const Schema& schema, const Json& member, std::string_view text);
[[nodiscard]] std::string bound(const Json& value);
}
