#include "json.hpp"
#include "timestamp.hpp"
#include <iostream>
#include <limits>

using namespace crucible_generated::runtime;
void check(bool condition, const char* message) { if (!condition) throw std::runtime_error(message); }
template<class Action> void rejects(Action action) { try { action(); } catch (const std::invalid_argument&) { return; } throw std::runtime_error("Expected invalid_argument"); }
int main() {
    const auto json = Json::parse(R"({"integer":9223372036854775807,"decimal":1.000000000000000000001,"nul\u0000key":{},"list":[]})");
    check(json.at("integer").number().integer() == INT64_MAX, "Exact int64");
    check(json.at("decimal").number().text() == "1.000000000000000000001", "Exact decimal");
    check(json.at(std::string("nul\0key", 7)).object().empty(), "Null-byte object key");
    check(Json::parse(json.dump()).at("list").array().empty(), "JSON round trip");
    for (const auto* invalid : {R"("\udc00")", R"("\ud800")", "[1,]", "01", "1e", R"({"x":})", "truefalse", "NaN", "\"\xff\""}) rejects([&] { (void)Json::parse(invalid); });
    check(Json::parse(R"("\uD83D\uDE00")").string().size() == 4, "Surrogate pair");
    check(Number("-9223372036854775808.0").integer() == INT64_MIN, "Exact negative bound");
    rejects([] { (void)Number("9223372036854775808").integer(); });
    rejects([] { (void)Number("1.01").integer(); });
    rejects([] { (void)Number("1e10001").integer(); });
    rejects([] { (void)Json(std::numeric_limits<double>::infinity()); });
    for (const std::int64_t seconds : {INT64_MIN, std::int64_t(-1), std::int64_t(0), INT64_MAX}) {
        const Timestamp value(seconds, 999999999);
        check(Timestamp::from_epoch(value.epoch()) == value, "Exact epoch round trip");
    }
    check(Timestamp::from_epoch(Number("-0.000000001")).date_time() == "1969-12-31T23:59:59.999999999Z", "Negative fractional epoch");
    check(Timestamp::parse("1582-10-10T00:00:00Z").date_time() == "1582-10-10T00:00:00Z", "Gregorian calendar");
    check(Timestamp::parse("2026-10-01T02:30:00+02:30").date_time() == "2026-10-01T00:00:00Z", "Timezone offset");
    check(Timestamp::from_http_date("Thu, 01 Jan 1970 00:00:00 GMT").seconds() == 0, "HTTP date");
    rejects([] { (void)Timestamp::parse("1500-02-29T00:00:00Z"); });
    rejects([] { (void)Timestamp::parse("2026-02-30T00:00:00Z"); });
    rejects([] { (void)Timestamp::parse("2026-01-01T24:00:00Z"); });
    rejects([] { (void)Timestamp::from_http_date("Fri, 01 Jan 1970 00:00:00 GMT"); });
    rejects([] { (void)Timestamp::from_epoch(Number("0.0000000001")); });
    std::cout << "C++ exact JSON and timestamp checks passed\n";
}
