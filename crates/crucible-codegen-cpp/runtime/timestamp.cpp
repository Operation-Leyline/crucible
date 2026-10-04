#include "timestamp.hpp"
#include <algorithm>
#include <array>
#include <charconv>
#include <iomanip>
#include <regex>
#include <sstream>

namespace crucible_generated::runtime {
namespace {
constexpr std::array<std::string_view, 7> weekdays = {"Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"};
constexpr std::array<std::string_view, 12> months = {"Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"};
unsigned natural(std::string_view text) {
    unsigned value = 0;
    const auto [end, error] = std::from_chars(text.data(), text.data() + text.size(), value);
    if (error != std::errc() || end != text.data() + text.size()) throw std::invalid_argument("Invalid timestamp component");
    return value;
}
std::int64_t utc(int year, unsigned month, unsigned day, unsigned hour, unsigned minute, unsigned second) {
    const auto date = std::chrono::year(year) / month / day;
    if (year < 1 || year > 9999 || !date.ok() || hour > 23 || minute > 59 || second > 59) throw std::invalid_argument("Invalid timestamp components");
    const auto days = std::chrono::sys_days(date).time_since_epoch().count();
    return static_cast<std::int64_t>(days) * 86400 + hour * 3600 + minute * 60 + second;
}
struct Parts {
    int year;
    unsigned month, day, hour, minute, second, weekday;
};
Parts split(std::int64_t seconds) {
    if (seconds < utc(1, 1, 1, 0, 0, 0) || seconds > utc(9999, 12, 31, 23, 59, 59)) throw std::invalid_argument("Text timestamp year out of range");
    auto days = seconds / 86400;
    auto remaining = seconds % 86400;
    if (remaining < 0) { --days; remaining += 86400; }
    const auto date = std::chrono::sys_days(std::chrono::days(days));
    const auto calendar = std::chrono::year_month_day(date);
    return {int(calendar.year()), unsigned(calendar.month()), unsigned(calendar.day()), static_cast<unsigned>(remaining / 3600), static_cast<unsigned>((remaining % 3600) / 60), static_cast<unsigned>(remaining % 60), std::chrono::weekday(date).c_encoding()};
}
std::string fraction(std::uint32_t nanos) {
    auto text = std::to_string(nanos);
    text.insert(0, 9 - text.size(), '0');
    while (text.back() == '0') text.pop_back();
    return text;
}
}
Timestamp::Timestamp(std::int64_t seconds, std::uint32_t nanoseconds) : seconds_(seconds), nanoseconds_(nanoseconds) {
    if (nanoseconds >= 1000000000) throw std::invalid_argument("Nanoseconds out of range");
}
Timestamp Timestamp::from_epoch(const Number& number) {
    auto scaled = number.scaled(9);
    const bool negative = scaled.front() == '-';
    if (negative) scaled.erase(0, 1);
    if (scaled.size() <= 9) scaled.insert(0, 10 - scaled.size(), '0');
    const auto digits = scaled.size() - 9;
    const auto whole = scaled.substr(0, digits);
    const auto remainder = natural(std::string_view(scaled).substr(digits));
    auto seconds = Number((negative ? "-" : "") + whole).integer();
    if (negative && remainder > 0) {
        if (seconds == INT64_MIN) throw std::invalid_argument("Timestamp out of range");
        --seconds;
    }
    return Timestamp(seconds, negative && remainder > 0 ? 1000000000 - remainder : remainder);
}
Number Timestamp::epoch() const {
    if (nanoseconds_ == 0) return Number(std::to_string(seconds_));
    if (seconds_ >= 0) return Number(std::to_string(seconds_) + '.' + fraction(nanoseconds_));
    return Number('-' + std::to_string(-(seconds_ + 1)) + '.' + fraction(1000000000 - nanoseconds_));
}
Timestamp Timestamp::parse(std::string_view text) {
    static const std::regex pattern(R"(^([0-9]{4})-([0-9]{2})-([0-9]{2})T([0-9]{2}):([0-9]{2}):([0-9]{2})(?:\.([0-9]{1,9}))?(Z|[+-][0-9]{2}:[0-9]{2})$)");
    const std::string input(text);
    std::smatch match;
    if (!std::regex_match(input, match, pattern)) throw std::invalid_argument("Invalid date-time");
    auto seconds = utc(static_cast<int>(natural(match[1].str())), natural(match[2].str()), natural(match[3].str()), natural(match[4].str()), natural(match[5].str()), natural(match[6].str()));
    const auto zone = match[8].str();
    if (zone != "Z") {
        const auto hours = natural(std::string_view(zone).substr(1, 2));
        const auto minutes = natural(std::string_view(zone).substr(4, 2));
        if (hours > 23 || minutes > 59) throw std::invalid_argument("Invalid date-time offset");
        const auto offset = static_cast<std::int64_t>(hours * 3600 + minutes * 60);
        seconds += zone[0] == '-' ? offset : -offset;
    }
    auto nanos = match[7].str();
    nanos.append(9 - nanos.size(), '0');
    (void)split(seconds);
    return Timestamp(seconds, natural(nanos));
}
std::string Timestamp::date_time() const {
    const auto p = split(seconds_);
    std::ostringstream out;
    out.imbue(std::locale::classic());
    out << std::setfill('0') << std::setw(4) << p.year << '-' << std::setw(2) << p.month << '-' << std::setw(2) << p.day << 'T' << std::setw(2) << p.hour << ':' << std::setw(2) << p.minute << ':' << std::setw(2) << p.second;
    if (nanoseconds_ != 0) out << '.' << fraction(nanoseconds_);
    out << 'Z';
    return out.str();
}
std::string Timestamp::http_date() const {
    const auto p = split(seconds_);
    std::ostringstream out;
    out.imbue(std::locale::classic());
    out << weekdays[p.weekday] << ", " << std::setfill('0') << std::setw(2) << p.day << ' ' << months[p.month - 1] << ' ' << std::setw(4) << p.year << ' ' << std::setw(2) << p.hour << ':' << std::setw(2) << p.minute << ':' << std::setw(2) << p.second << " GMT";
    return out.str();
}
Timestamp Timestamp::from_http_date(std::string_view text) {
    if (text.size() != 29 || text.substr(3, 2) != ", " || text[7] != ' ' || text[11] != ' ' || text[16] != ' ' || text[19] != ':' || text[22] != ':' || text.substr(25) != " GMT") throw std::invalid_argument("Invalid HTTP date");
    const auto month = std::find(months.begin(), months.end(), text.substr(8, 3));
    if (month == months.end()) throw std::invalid_argument("Invalid HTTP month");
    const auto seconds = utc(static_cast<int>(natural(text.substr(12, 4))), static_cast<unsigned>(month - months.begin() + 1), natural(text.substr(5, 2)), natural(text.substr(17, 2)), natural(text.substr(20, 2)), natural(text.substr(23, 2)));
    Timestamp result(seconds);
    if (result.http_date() != text) throw std::invalid_argument("Invalid HTTP date weekday");
    return result;
}
}
