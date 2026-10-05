"""Retry modes: the shared retry quota and adaptive client-side rate limiting.

The constants and formulas follow the AWS SDKs' standard and adaptive retry
modes; docs/sdk-runtime-policy.md is the cross-language contract.
"""
from __future__ import annotations

import asyncio
import math
import random
import threading
import time
from typing import Callable

RETRY_MODES = ("legacy", "standard", "adaptive")

# Retry quota (standard and adaptive modes).
INITIAL_RETRY_TOKENS = 500
RETRY_COST = 5
RETRY_TIMEOUT_COST = 10
NO_RETRY_INCREMENT = 1

# Adaptive rate limiting.
MIN_FILL_RATE = 0.5
MIN_CAPACITY = 1.0
SMOOTHING = 0.8
TIME_BUCKET_RANGE = 0.5
BETA = 0.7
SCALE_CONSTANT = 0.4
MAX_RATE_ADJUST_SCALE = 2.0


class SystemClock:
    """Monotonic seconds and sleeps. Tests inject a clock with the same methods."""

    def now(self) -> float:
        return time.monotonic()

    def sleep(self, seconds: float) -> None:
        time.sleep(seconds)

    async def asleep(self, seconds: float) -> None:
        await asyncio.sleep(seconds)


class RetryQuota:
    """A token bucket bounding how much of a client's traffic may be retries."""

    def __init__(self, capacity: int = INITIAL_RETRY_TOKENS) -> None:
        self.max_capacity = capacity
        self.available = capacity
        self._lock = threading.Lock()

    def acquire(self, timeout: bool) -> int | None:
        """The cost of one retry, or None when the quota cannot pay for it."""
        cost = RETRY_TIMEOUT_COST if timeout else RETRY_COST
        with self._lock:
            if cost > self.available:
                return None
            self.available -= cost
            return cost

    def release(self, last_retry_cost: int | None) -> None:
        """A successful response refunds the call's last retry, or adds a small increment."""
        with self._lock:
            amount = NO_RETRY_INCREMENT if last_retry_cost is None else last_retry_cost
            self.available = min(self.max_capacity, self.available + amount)


class CubicCalculator:
    """CUBIC congestion control applied to the client's send rate."""

    def __init__(self, starting_max_rate: float, start_time: float) -> None:
        self.w_max = starting_max_rate
        self.k = self._zero_point()
        self.last_fail = start_time

    def _zero_point(self) -> float:
        return (self.w_max * (1 - BETA) / SCALE_CONSTANT) ** (1 / 3)

    def success(self, timestamp: float) -> float:
        return SCALE_CONSTANT * (timestamp - self.last_fail - self.k) ** 3 + self.w_max

    def throttle(self, rate: float, timestamp: float) -> float:
        self.w_max = rate
        self.k = self._zero_point()
        self.last_fail = timestamp
        return rate * BETA


class RateClocker:
    """A smoothed measurement of the rate responses are received, in half-second buckets."""

    def __init__(self, now: float) -> None:
        self.measured = 0.0
        self._count = 0
        self._last_bucket: float = math.floor(now)

    def record(self, now: float) -> float:
        scale = 1 / TIME_BUCKET_RANGE
        bucket = math.floor(now * scale) / scale
        self._count += 1
        if bucket > self._last_bucket:
            rate = self._count / (bucket - self._last_bucket)
            self.measured = rate * SMOOTHING + self.measured * (1 - SMOOTHING)
            self._count = 0
            self._last_bucket = bucket
        return self.measured


class SendTokenBucket:
    """Client-side send tokens. Disabled until the first throttling response."""

    def __init__(self) -> None:
        self.fill_rate = MIN_FILL_RATE
        self.max_capacity = MIN_CAPACITY
        self.capacity = 0.0
        self.last: float | None = None
        self.enabled = False

    def _refill(self, now: float) -> None:
        if self.last is not None:
            self.capacity = min(self.max_capacity, self.capacity + (now - self.last) * self.fill_rate)
        self.last = now

    def set_rate(self, rate: float, now: float) -> None:
        # Tokens earned at the old rate are credited before the rate changes.
        self._refill(now)
        self.fill_rate = max(rate, MIN_FILL_RATE)
        self.max_capacity = max(rate, MIN_CAPACITY)
        self.capacity = min(self.capacity, self.max_capacity)

    def reserve(self, now: float) -> float:
        """Take one token, possibly on credit; the result is how long to wait for it."""
        if not self.enabled:
            return 0.0
        self._refill(now)
        self.capacity -= 1
        return 0.0 if self.capacity >= 0 else -self.capacity / self.fill_rate

    def give_back(self) -> None:
        if self.enabled:
            self.capacity = min(self.max_capacity, self.capacity + 1)


class AdaptiveRateLimiter:
    """Measures the response rate and, after throttling, limits the send rate with CUBIC."""

    def __init__(self, now: float) -> None:
        self.clocker = RateClocker(now)
        self.cubic = CubicCalculator(0.0, now)
        self.bucket = SendTokenBucket()

    def before_send(self, now: float) -> float:
        return self.bucket.reserve(now)

    def after_attempt(self, throttled: bool, now: float) -> None:
        measured = self.clocker.record(now)
        if throttled:
            rate = min(measured, self.bucket.fill_rate) if self.bucket.enabled else measured
            new_rate = self.cubic.throttle(rate, now)
            self.bucket.enabled = True
        else:
            new_rate = self.cubic.success(now)
        self.bucket.set_rate(min(new_rate, MAX_RATE_ADJUST_SCALE * measured), now)


class RetryStrategy:
    """One client's retry state: the mode, its quota and its rate limiter, shared by all calls."""

    def __init__(self, mode: str, clock: object, *, base_delay: float, max_delay: float,
                 jitter: Callable[[], float] = random.random) -> None:
        if mode not in RETRY_MODES:
            raise ValueError("retry_mode must be 'legacy', 'standard' or 'adaptive'")
        self.mode = mode
        self.clock = clock
        self._base_delay = base_delay
        self._max_delay = max_delay
        self._jitter = jitter
        self._lock = threading.Lock()
        self.quota = RetryQuota() if mode != "legacy" else None
        self.limiter = AdaptiveRateLimiter(clock.now()) if mode == "adaptive" else None  # type: ignore[attr-defined]

    def send_delay(self) -> float:
        if self.limiter is None:
            return 0.0
        with self._lock:
            return self.limiter.before_send(self.clock.now())  # type: ignore[attr-defined]

    def cancel_send(self) -> None:
        if self.limiter is not None:
            with self._lock:
                self.limiter.bucket.give_back()

    def after_attempt(self, throttled: bool) -> None:
        if self.limiter is not None:
            with self._lock:
                self.limiter.after_attempt(throttled, self.clock.now())  # type: ignore[attr-defined]

    def acquire_retry(self, timeout: bool) -> tuple[bool, int | None]:
        """Whether a retry may proceed, and the quota it consumed."""
        if self.quota is None:
            return True, None
        cost = self.quota.acquire(timeout)
        return cost is not None, cost

    def succeeded(self, last_retry_cost: int | None) -> None:
        if self.quota is not None:
            self.quota.release(last_retry_cost)

    def backoff(self, attempt: int) -> float:
        """Full-jitter exponential backoff after the given (1-based) failed attempt."""
        return self._jitter() * min(self._max_delay, self._base_delay * 2 ** min(attempt - 1, 30))
