#include "weather_client/models.hpp"
#include "weather_client/schema.hpp"
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <iterator>

using namespace crucible_generated;
using namespace crucible_generated::runtime;
using namespace crucible_generated::models;

static void check(bool condition, const std::string& message) { if (!condition) throw std::runtime_error(message); }
static std::pair<std::string, long> number(std::string text) {
    const bool negative = text.front() == '-';
    if (negative) text.erase(0, 1);
    const auto exponent = text.find_first_of("eE");
    long power = exponent == std::string::npos ? 0 : std::stol(text.substr(exponent + 1));
    text = text.substr(0, exponent);
    const auto dot = text.find('.');
    if (dot != std::string::npos) { power -= static_cast<long>(text.size() - dot - 1); text.erase(dot, 1); }
    const auto first = text.find_first_not_of('0');
    if (first == std::string::npos) return {"0", 0};
    text.erase(0, first);
    while (text.back() == '0') { text.pop_back(); ++power; }
    return {negative ? "-" + text : text, power};
}
static bool equal(const Json& a, const Json& b) {
    if (a.value().index() != b.value().index()) return false;
    if (a.is_null()) return true;
    if (std::holds_alternative<Number>(a.value())) return number(a.number().text()) == number(b.number().text());
    if (std::holds_alternative<std::string>(a.value())) return a.string() == b.string();
    if (std::holds_alternative<bool>(a.value())) return a.boolean() == b.boolean();
    if (std::holds_alternative<Json::Array>(a.value())) {
        if (a.array().size() != b.array().size()) return false;
        for (std::size_t i = 0; i < a.array().size(); ++i) if (!equal(a.array()[i], b.array()[i])) return false;
    } else {
        if (a.object().size() != b.object().size()) return false;
        for (const auto& [key, value] : a.object()) { const auto* other = b.find(key); if (!other || !equal(value, *other)) return false; }
    }
    return true;
}
template<class T> static std::string state(const Field<T>& value) { return !value.is_set() ? "absent" : value.is_null() ? "null" : "value"; }
template<class T> static void run(const Json& row) {
    const auto context = schema().context(row.at("target").string());
    const auto wire = Json::parse(row.at("wire").string());
    if (row.find("reject")) {
        bool rejected = false;
        try { (void)Codec<T>::decode(wire, context); } catch (const std::invalid_argument&) { rejected = true; }
        check(rejected, "Invalid wire value was accepted"); return;
    }
    const auto native = Codec<T>::decode(wire, context);
    const auto& expect = row.at("expect");
    check(equal(Codec<T>::encode(native, context), Json::parse(expect.at("json").string())), "Exact structural JSON mismatch");
    if constexpr (std::is_same_v<T, CitySummary>) {
        if (const auto* states = expect.find("states")) for (const auto& [member, wanted] : states->object()) {
            check(member == "population" || member == "updatedAt" || member == "createdAt", "Unknown observed member");
            const auto actual = member == "population" ? "value" : member == "updatedAt" ? state(native.updated_at) : state(native.created_at);
            check(actual == wanted.string(), "Native presence mismatch: " + member);
        }
        if (const auto* instant = expect.find("instant")) {
            const auto& member = instant->at("member").string();
            check(member == "updatedAt" || member == "createdAt", "Unknown timestamp member");
            const auto& value = (member == "updatedAt" ? native.updated_at : native.created_at).value();
            check(std::to_string(value.seconds()) == instant->at("seconds").string() && value.nanoseconds() == instant->at("nanoseconds").number().integer(), "Native timestamp mismatch");
        }
    } else if constexpr (std::is_same_v<T, Blob>) {
        if (const auto* wanted = expect.find("blobHex")) {
            const char* digits = "0123456789abcdef"; std::string hex;
            for (const auto byte : native) { const auto value = std::to_integer<unsigned>(byte); hex += digits[value >> 4]; hex += digits[value & 15]; }
            check(hex == wanted->string(), "Native blob mismatch");
        }
    }
}
int main() {
    const auto* path = std::getenv("SERIALIZATION_CASES"); check(path != nullptr, "Missing corpus path");
    std::ifstream stream(path); check(static_cast<bool>(stream), "Cannot open corpus");
    const auto rows = Json::parse(std::string(std::istreambuf_iterator<char>(stream), {}));
    check(rows.array().size() >= 15, "Incomplete corpus");
    for (const auto& row : rows.array()) {
        try {
            const auto& target = row.at("target").string();
            if (target == "example.weather#CitySummary") run<CitySummary>(row);
            else if (target == "example.weather#Photo") run<Blob>(row);
            else if (target == "example.weather#Sky") run<Sky>(row);
            else if (target == "example.weather#Location") run<Location>(row);
            else if (target == "example.weather#Forecast") run<std::map<Sky, Json>>(row);
            else throw std::runtime_error("Unknown corpus target: " + target);
        } catch (const std::exception& error) { std::cerr << row.at("id").string() << ": " << error.what() << '\n'; return 1; }
    }
    std::cout << "C++ shared serialization corpus passed\n";
}
