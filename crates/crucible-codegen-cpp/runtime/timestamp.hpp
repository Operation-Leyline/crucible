#pragma once
#include "json.hpp"
#include <chrono>

namespace crucible_generated::runtime {
class Timestamp {
    std::int64_t seconds_;
    std::uint32_t nanoseconds_;
public:
    explicit Timestamp(std::int64_t seconds = 0, std::uint32_t nanoseconds = 0);
    [[nodiscard]] std::int64_t seconds() const noexcept { return seconds_; }
    [[nodiscard]] std::uint32_t nanoseconds() const noexcept { return nanoseconds_; }
    [[nodiscard]] static Timestamp from_epoch(const Number& number);
    [[nodiscard]] static Timestamp parse(std::string_view text);
    [[nodiscard]] static Timestamp from_http_date(std::string_view text);
    [[nodiscard]] std::string date_time() const;
    [[nodiscard]] std::string http_date() const;
    [[nodiscard]] Number epoch() const;
    bool operator==(const Timestamp&) const = default;
};
}
