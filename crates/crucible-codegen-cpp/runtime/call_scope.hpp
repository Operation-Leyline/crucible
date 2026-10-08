#pragma once
#include <algorithm>
#include <chrono>
#include <cmath>
#include <condition_variable>
#include <mutex>
#include <stdexcept>
#include <stop_token>

namespace crucible_generated::runtime {
/// The total call deadline expired.
class DeadlineError : public std::runtime_error { public: DeadlineError() : std::runtime_error("Call deadline exceeded") {} };
/// The caller requested cancellation.
class CancelledError : public std::runtime_error { public: CancelledError() : std::runtime_error("Call cancelled") {} };
/// Transport or response-body network failure.
class NetworkError : public std::runtime_error { public: using std::runtime_error::runtime_error; };
/// A monotonic total call deadline and stop token shared across credentials, retries, and body reads.
class CallScope {
    using Clock = std::chrono::steady_clock;
    Clock::time_point deadline_;
    std::stop_token cancellation_;
public:
    /// Starts a scope with a finite positive timeout in seconds.
    explicit CallScope(double timeout, std::stop_token cancellation = {}) : cancellation_(cancellation) {
        if (!std::isfinite(timeout) || timeout <= 0) throw std::invalid_argument("Timeout must be finite and positive");
        const auto now = Clock::now();
        const auto maximum = std::chrono::duration<double>((Clock::time_point::max)() - now).count();
        deadline_ = timeout >= maximum ? (Clock::time_point::max)() : now + std::chrono::duration_cast<Clock::duration>(std::chrono::duration<double>(timeout));
    }
    /// Throws CancelledError or DeadlineError when the call cannot continue.
    void check() const { if (cancellation_.stop_requested()) throw CancelledError(); if (Clock::now() >= deadline_) throw DeadlineError(); }
    /// Returns remaining seconds; throws if cancelled or expired.
    [[nodiscard]] double remaining() const { check(); return std::chrono::duration<double>(deadline_ - Clock::now()).count(); }
    /// Waits for a delay while respecting cancellation and the total deadline.
    void wait(double seconds) const {
        if (!std::isfinite(seconds) || seconds < 0) throw std::invalid_argument("Invalid wait duration");
        check();
        const auto now = Clock::now();
        const auto available = std::chrono::duration<double>(deadline_ - now).count();
        const auto until = seconds >= available ? deadline_ : now + std::chrono::duration_cast<Clock::duration>(std::chrono::duration<double>(seconds));
        std::mutex mutex;
        std::condition_variable_any changed;
        std::unique_lock lock(mutex);
        changed.wait_until(lock, cancellation_, until, [] { return false; });
        check();
    }
    /// Waits on shared state (such as an in-flight credential refresh) within this call's limits.
    template<class Predicate> void wait_until(std::condition_variable_any& changed, std::unique_lock<std::mutex>& lock, Predicate ready) const {
        while (!ready()) {
            if (cancellation_.stop_requested() || Clock::now() >= deadline_) { lock.unlock(); check(); }
            (void)changed.wait_until(lock, cancellation_, (std::min)(deadline_, Clock::now() + std::chrono::milliseconds(50)), ready);
        }
    }
    /// Returns the caller stop token.
    [[nodiscard]] std::stop_token cancellation() const noexcept { return cancellation_; }
};
}
