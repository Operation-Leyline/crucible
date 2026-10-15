package sdk

// Retry modes: the shared retry quota and adaptive client-side rate limiting.
// The constants and formulas follow the AWS SDKs' standard and adaptive retry
// modes; docs/sdk-runtime-policy.md is the cross-language contract.

import (
	"context"
	"math"
	mathrand "math/rand/v2"
	"sync"
	"time"
)

// RetryMode selects how a client limits its retries. Every mode retries the
// same failures with the same backoff; the modes only add limits.
type RetryMode string

const (
	// RetryModeLegacy, the default, retries eligible failures without a quota.
	RetryModeLegacy RetryMode = "legacy"
	// RetryModeStandard adds a retry quota shared by every call of the client.
	RetryModeStandard RetryMode = "standard"
	// RetryModeAdaptive adds client-side send-rate limiting after throttling.
	RetryModeAdaptive RetryMode = "adaptive"
)

// Clock supplies the time used for retry backoff and rate limiting and
// telemetry. Now must be monotonic. A call's own
// deadline always uses the real clock, so a fake Clock never changes a timeout.
type Clock interface {
	Now() time.Time
	// Sleep waits for d, returning early with ctx's error when ctx ends.
	Sleep(ctx context.Context, d time.Duration) error
}

type systemClock struct{}

// Now returns monotonic elapsed seconds.
func (systemClock) Now() time.Time { return time.Now() }
// Sleep waits for a delay unless ctx is canceled.
func (systemClock) Sleep(ctx context.Context, d time.Duration) error {
	if d <= 0 {
		return ctx.Err()
	}
	timer := time.NewTimer(d)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-timer.C:
		return nil
	}
}

// Retry quota (standard and adaptive modes).
const (
	initialRetryTokens = 500
	retryCost          = 5
	retryTimeoutCost   = 10
	noRetryIncrement   = 1
)

// Adaptive rate limiting.
const (
	minFillRate        = 0.5
	minCapacity        = 1.0
	rateSmoothing      = 0.8
	timeBucketRange    = 0.5
	cubicBeta          = 0.7
	cubicScale         = 0.4
	maxRateAdjustScale = 2.0
)

// retryQuota bounds how much of a client's traffic may be retries.
type retryQuota struct {
	mu        sync.Mutex
	max       int
	available int
}

func newRetryQuota() *retryQuota {
	return &retryQuota{max: initialRetryTokens, available: initialRetryTokens}
}

// acquire takes the cost of one retry, reporting false (and spending nothing)
// when the quota cannot pay for it.
func (q *retryQuota) acquire(timeout bool) (int, bool) {
	cost := retryCost
	if timeout {
		cost = retryTimeoutCost
	}
	q.mu.Lock()
	defer q.mu.Unlock()
	if cost > q.available {
		return 0, false
	}
	q.available -= cost
	return cost, true
}

// release refunds a successful call's last retry cost, or the small increment
// when it never retried (lastCost 0).
func (q *retryQuota) release(lastCost int) {
	amount := lastCost
	if amount == 0 {
		amount = noRetryIncrement
	}
	q.mu.Lock()
	defer q.mu.Unlock()
	q.available = min(q.max, q.available+amount)
}

// cubicCalculator applies CUBIC congestion control to the send rate.
type cubicCalculator struct{ wMax, k, lastFail float64 }

func newCubicCalculator(startingMaxRate, startTime float64) *cubicCalculator {
	c := &cubicCalculator{wMax: startingMaxRate, lastFail: startTime}
	c.k = c.zeroPoint()
	return c
}
func (c *cubicCalculator) zeroPoint() float64 {
	return math.Cbrt(c.wMax * (1 - cubicBeta) / cubicScale)
}
func (c *cubicCalculator) success(timestamp float64) float64 {
	d := timestamp - c.lastFail - c.k
	return cubicScale*d*d*d + c.wMax
}
func (c *cubicCalculator) throttle(rate, timestamp float64) float64 {
	c.wMax = rate
	c.k = c.zeroPoint()
	c.lastFail = timestamp
	return rate * cubicBeta
}

// rateClocker measures the rate responses arrive, smoothed over half-second buckets.
type rateClocker struct {
	measured   float64
	count      int
	lastBucket float64
}

func (r *rateClocker) record(now float64) float64 {
	bucket := math.Floor(now/timeBucketRange) * timeBucketRange
	r.count++
	if bucket > r.lastBucket {
		rate := float64(r.count) / (bucket - r.lastBucket)
		r.measured = rate*rateSmoothing + r.measured*(1-rateSmoothing)
		r.count = 0
		r.lastBucket = bucket
	}
	return r.measured
}

// sendTokenBucket holds client-side send tokens; disabled until the first throttle.
type sendTokenBucket struct {
	fillRate    float64
	maxCapacity float64
	capacity    float64
	last        float64
	refilled    bool
	enabled     bool
}

func (b *sendTokenBucket) refill(now float64) {
	if b.refilled {
		b.capacity = math.Min(b.maxCapacity, b.capacity+(now-b.last)*b.fillRate)
	}
	b.last, b.refilled = now, true
}

func (b *sendTokenBucket) setRate(rate, now float64) {
	// Tokens earned at the old rate are credited before the rate changes.
	b.refill(now)
	b.fillRate = math.Max(rate, minFillRate)
	b.maxCapacity = math.Max(rate, minCapacity)
	b.capacity = math.Min(b.capacity, b.maxCapacity)
}

// reserve takes one token, possibly on credit, returning how long to wait for it.
// Taking it before waiting gives concurrent callers first-come order.
func (b *sendTokenBucket) reserve(now float64) float64 {
	if !b.enabled {
		return 0
	}
	b.refill(now)
	b.capacity--
	if b.capacity >= 0 {
		return 0
	}
	return -b.capacity / b.fillRate
}

func (b *sendTokenBucket) giveBack() {
	if b.enabled {
		b.capacity = math.Min(b.maxCapacity, b.capacity+1)
	}
}

// adaptiveRateLimiter measures the response rate and, after throttling,
// limits the send rate with CUBIC.
type adaptiveRateLimiter struct {
	clocker *rateClocker
	cubic   *cubicCalculator
	bucket  *sendTokenBucket
}

func newAdaptiveRateLimiter(now float64) *adaptiveRateLimiter {
	return &adaptiveRateLimiter{
		clocker: &rateClocker{lastBucket: math.Floor(now)},
		cubic:   newCubicCalculator(0, now),
		bucket:  &sendTokenBucket{fillRate: minFillRate, maxCapacity: minCapacity},
	}
}

func (l *adaptiveRateLimiter) beforeSend(now float64) float64 { return l.bucket.reserve(now) }

func (l *adaptiveRateLimiter) afterAttempt(throttled bool, now float64) {
	measured := l.clocker.record(now)
	var rate float64
	if throttled {
		rate = measured
		if l.bucket.enabled {
			rate = math.Min(measured, l.bucket.fillRate)
		}
		rate = l.cubic.throttle(rate, now)
		l.bucket.enabled = true
	} else {
		rate = l.cubic.success(now)
	}
	l.bucket.setRate(math.Min(rate, maxRateAdjustScale*measured), now)
}

// retryStrategy is one client's retry state, shared by all of its calls.
type retryStrategy struct {
	clock  Clock
	origin time.Time
	// jitter is the full-jitter factor in [0, 1); tests replace it.
	jitter  func() float64
	mu      sync.Mutex
	quota   *retryQuota
	limiter *adaptiveRateLimiter
}

func newRetryStrategy(mode RetryMode, clock Clock) *retryStrategy {
	s := &retryStrategy{clock: clock, origin: clock.Now(), jitter: mathrand.Float64}
	if mode != RetryModeLegacy {
		s.quota = newRetryQuota()
	}
	if mode == RetryModeAdaptive {
		s.limiter = newAdaptiveRateLimiter(0)
	}
	return s
}

// now is the injected clock's reading, in seconds since the client was created.
func (s *retryStrategy) now() float64 { return s.clock.Now().Sub(s.origin).Seconds() }

func seconds(value float64) time.Duration {
	if value >= float64(math.MaxInt64)/float64(time.Second) {
		return time.Duration(math.MaxInt64)
	}
	return time.Duration(math.Round(value * float64(time.Second)))
}

// sendDelay reserves a send token before an attempt and returns how long to wait for it.
func (s *retryStrategy) sendDelay() time.Duration {
	if s.limiter == nil {
		return 0
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	return seconds(s.limiter.beforeSend(s.now()))
}

// cancelSend returns a reserved send token whose attempt never happened.
func (s *retryStrategy) cancelSend() {
	if s.limiter != nil {
		s.mu.Lock()
		defer s.mu.Unlock()
		s.limiter.bucket.giveBack()
	}
}

// afterAttempt records an attempt that reached the transport.
func (s *retryStrategy) afterAttempt(throttled bool) {
	if s.limiter != nil {
		s.mu.Lock()
		defer s.mu.Unlock()
		s.limiter.afterAttempt(throttled, s.now())
	}
}

// acquireRetry reports whether a retry may proceed and the quota it consumed.
func (s *retryStrategy) acquireRetry(timeout bool) (bool, int) {
	if s.quota == nil {
		return true, 0
	}
	cost, granted := s.quota.acquire(timeout)
	return granted, cost
}

func (s *retryStrategy) succeeded(lastCost int) {
	if s.quota != nil {
		s.quota.release(lastCost)
	}
}
