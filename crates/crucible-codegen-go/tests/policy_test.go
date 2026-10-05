package policy

// Runtime policy: retry quota, adaptive rate limiting, endpoint discovery and
// telemetry. The shared vectors are tests/conformance/{retry-quota,
// adaptive-retry,endpoint-discovery,telemetry}.json, run against a client
// generated from crates/crucible-smithy/tests/fixtures/runtime-policy.json.

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log"
	"math"
	"net/http"
	"os"
	"reflect"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

const endpoint = "https://records.example"

var epoch = time.Unix(1_700_000_000, 0)

// fakeClock starts at 0 and advances only when told to or when slept on.
type fakeClock struct {
	mu      sync.Mutex
	elapsed time.Duration
	sleeps  []time.Duration
}

func (c *fakeClock) Now() time.Time {
	c.mu.Lock()
	defer c.mu.Unlock()
	return epoch.Add(c.elapsed)
}
func (c *fakeClock) Sleep(ctx context.Context, d time.Duration) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.sleeps = append(c.sleeps, d)
	c.elapsed += d
	return ctx.Err()
}
func (c *fakeClock) advance(seconds float64) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.elapsed += time.Duration(math.Round(seconds * float64(time.Second)))
}
func (c *fakeClock) slept() []time.Duration {
	c.mu.Lock()
	defer c.mu.Unlock()
	return append([]time.Duration(nil), c.sleeps...)
}

type doFunc func(*http.Request) (*http.Response, error)

func (f doFunc) Do(r *http.Request) (*http.Response, error) { return f(r) }

// timeoutError is a transport timeout, as net/http reports one.
type timeoutError struct{}

func (timeoutError) Error() string   { return "i/o timeout" }
func (timeoutError) Timeout() bool   { return true }
func (timeoutError) Temporary() bool { return true }

func reply(status int, body any, headers map[string]any) *http.Response {
	if body == nil {
		body = map[string]any{}
	}
	data, _ := json.Marshal(body)
	header := http.Header{}
	for name, value := range headers {
		header.Set(name, fmt.Sprint(value))
	}
	return &http.Response{StatusCode: status, Header: header, Body: io.NopCloser(bytes.NewReader(data)), ContentLength: int64(len(data))}
}

// respond answers a vector's response spec: a status and JSON body, or a transport timeout.
func respond(spec map[string]any) (*http.Response, error) {
	if spec["transport"] == "timeout" {
		return nil, timeoutError{}
	}
	headers, _ := spec["headers"].(map[string]any)
	return reply(int(spec["status"].(float64)), spec["json"], headers), nil
}

func load(t *testing.T, name string, out any) {
	t.Helper()
	data, err := os.ReadFile(name)
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(data, out); err != nil {
		t.Fatal(err)
	}
}

func newRecords(t *testing.T, config Config) *RecordsClient {
	t.Helper()
	if config.Endpoint == "" {
		config.Endpoint = endpoint
	}
	if config.APIKey == nil {
		config.APIKey = func(context.Context) (string, error) { return "key", nil }
	}
	client, err := NewRecordsClient(config)
	if err != nil {
		t.Fatal(err)
	}
	// Backoff disabled, as every vector assumes.
	client.retry.jitter = func() float64 { return 0 }
	return client
}

// invoke calls an operation through its typed method with a vector's input.
func invoke(ctx context.Context, client *RecordsClient, name string, input map[string]any) error {
	switch name {
	case "GetRecord":
		value := &GetRecordInput{}
		if err := decodeModel(wireSchema, "example.policy#GetRecordInput", input, value); err != nil {
			return err
		}
		_, err := client.GetRecord(ctx, value)
		return err
	case "PutRecord":
		value := &PutRecordInput{}
		if err := decodeModel(wireSchema, "example.policy#PutRecordInput", input, value); err != nil {
			return err
		}
		_, err := client.PutRecord(ctx, value)
		return err
	case "ListRecords":
		_, err := client.ListRecords(ctx, Unit{})
		return err
	}
	return fmt.Errorf("unknown operation %s", name)
}

func outcome(err error) string {
	if err == nil {
		return "success"
	}
	if errors.Is(err, ErrEndpointDiscovery) {
		return "endpoint-discovery"
	}
	var service *ServiceError
	if errors.As(err, &service) {
		return service.Name
	}
	return "unexpected " + err.Error()
}

func near(a, b float64) bool { return math.Abs(a-b) <= 1e-6 }

func TestQuotaVectors(t *testing.T) {
	var vectors struct {
		Constants map[string]int
		Quota     []struct {
			Name    string
			Initial *int
			Steps   []map[string]any
		}
		Calls []struct {
			Name  string
			Mode  RetryMode
			Steps []struct {
				Repeat    int
				Responses []string
				Attempts  int
				Success   bool
				Available *int
			}
		}
	}
	load(t, "retry-quota.json", &vectors)
	if got := (map[string]int{"initialRetryTokens": initialRetryTokens, "retryCost": retryCost, "retryTimeoutCost": retryTimeoutCost, "noRetryIncrement": noRetryIncrement}); !reflect.DeepEqual(got, vectors.Constants) {
		t.Fatalf("constants %v != %v", got, vectors.Constants)
	}
	for _, vector := range vectors.Quota {
		t.Run(vector.Name, func(t *testing.T) {
			quota := newRetryQuota()
			if vector.Initial != nil {
				quota.available = *vector.Initial
			}
			for _, step := range vector.Steps {
				if kind, ok := step["acquire"]; ok {
					if _, granted := quota.acquire(kind == "timeout"); granted != step["granted"] {
						t.Fatalf("%v: granted %v", step, granted)
					}
				} else {
					cost, _ := step["release"].(float64)
					quota.release(int(cost))
				}
				if quota.available != int(step["available"].(float64)) {
					t.Fatalf("%v: available %d", step, quota.available)
				}
			}
		})
	}
	for _, vector := range vectors.Calls {
		t.Run(vector.Name, func(t *testing.T) {
			var script []string
			client := newRecords(t, Config{RetryMode: vector.Mode, Clock: &fakeClock{}, HTTPClient: doFunc(func(r *http.Request) (*http.Response, error) {
				if len(script) == 0 {
					t.Fatal("unexpected request")
				}
				token := script[0]
				script = script[1:]
				switch token {
				case "ok":
					return reply(200, nil, nil), nil
				case "unavailable":
					return reply(503, map[string]any{"__type": "Unavailable"}, nil), nil
				case "throttled":
					return reply(429, map[string]any{"__type": "Throttled"}, nil), nil
				case "denied":
					return reply(400, map[string]any{"__type": "Denied"}, nil), nil
				default:
					return nil, timeoutError{}
				}
			})})
			for _, step := range vector.Steps {
				for range max(step.Repeat, 1) {
					script = append([]string(nil), step.Responses...)
					_, err := client.ListRecords(context.Background(), Unit{})
					if sent := len(step.Responses) - len(script); sent != step.Attempts {
						t.Fatalf("%+v: %d attempts", step, sent)
					}
					var network *NetworkError
					var service *ServiceError
					if (err == nil) != step.Success || err != nil && !errors.As(err, &network) && !errors.As(err, &service) {
						t.Fatalf("%+v: %v", step, err)
					}
				}
				if step.Available == nil {
					if client.retry.quota != nil {
						t.Fatal("legacy mode has no quota")
					}
				} else if client.retry.quota.available != *step.Available {
					t.Fatalf("%+v: available %d", step, client.retry.quota.available)
				}
			}
		})
	}
}

func TestAdaptiveVectors(t *testing.T) {
	var vectors struct {
		Constants    map[string]float64
		CubicSuccess struct {
			LastMaxRate, LastThrottleTime float64
			Expect                        []struct{ Time, Rate float64 }
		}
		CubicThrottle struct {
			LastMaxRate, LastThrottleTime float64
			Steps                         []struct {
				Time, Rate float64
				Throttled  bool
			}
		}
		RateUpdates []struct {
			Time, MeasuredRate, FillRate float64
			Throttled, Enabled           bool
		}
		SendDelays []struct {
			Time, Delay, FillRate float64
			Send, Throttled       bool
		}
	}
	load(t, "adaptive-retry.json", &vectors)
	constants := map[string]float64{"minFillRate": minFillRate, "minCapacity": minCapacity, "smoothing": rateSmoothing, "timeBucketRange": timeBucketRange,
		"beta": cubicBeta, "scaleConstant": cubicScale, "maxRateAdjustScale": maxRateAdjustScale}
	if !reflect.DeepEqual(constants, vectors.Constants) {
		t.Fatalf("constants %v != %v", constants, vectors.Constants)
	}
	cubic := newCubicCalculator(vectors.CubicSuccess.LastMaxRate, vectors.CubicSuccess.LastThrottleTime)
	for _, step := range vectors.CubicSuccess.Expect {
		if rate := cubic.success(step.Time); !near(rate, step.Rate) {
			t.Fatalf("cubic success at %v: %v != %v", step.Time, rate, step.Rate)
		}
	}
	cubic = newCubicCalculator(vectors.CubicThrottle.LastMaxRate, vectors.CubicThrottle.LastThrottleTime)
	last := 0.0
	for _, step := range vectors.CubicThrottle.Steps {
		rate := cubic.success(step.Time)
		if step.Throttled {
			rate = cubic.throttle(last, step.Time)
		}
		if !near(rate, step.Rate) {
			t.Fatalf("cubic at %v: %v != %v", step.Time, rate, step.Rate)
		}
		last = rate
	}
	limiter := newAdaptiveRateLimiter(0)
	for _, step := range vectors.RateUpdates {
		limiter.afterAttempt(step.Throttled, step.Time)
		if !near(limiter.clocker.measured, step.MeasuredRate) || !near(limiter.bucket.fillRate, step.FillRate) || limiter.bucket.enabled != step.Enabled {
			t.Fatalf("rate update at %v: measured %v fill %v enabled %v", step.Time, limiter.clocker.measured, limiter.bucket.fillRate, limiter.bucket.enabled)
		}
	}
	limiter = newAdaptiveRateLimiter(0)
	for _, step := range vectors.SendDelays {
		if step.Send {
			if delay := limiter.beforeSend(step.Time); !near(delay, step.Delay) {
				t.Fatalf("send at %v: delay %v != %v", step.Time, delay, step.Delay)
			}
		} else {
			limiter.afterAttempt(step.Throttled, step.Time)
			if !near(limiter.bucket.fillRate, step.FillRate) {
				t.Fatalf("attempt at %v: fill %v != %v", step.Time, limiter.bucket.fillRate, step.FillRate)
			}
		}
	}
}

func TestAdaptiveClientWaitsForSendTokensAfterThrottling(t *testing.T) {
	clock := &fakeClock{}
	statuses := []int{200, 429, 200}
	client := newRecords(t, Config{RetryMode: RetryModeAdaptive, Clock: clock, HTTPClient: doFunc(func(*http.Request) (*http.Response, error) {
		clock.advance(0.01) // each response arrives a little later
		status := statuses[0]
		statuses = statuses[1:]
		if status == 429 {
			return reply(status, map[string]any{"__type": "Throttled"}, nil), nil
		}
		return reply(status, nil, nil), nil
	})})
	if _, err := client.ListRecords(context.Background(), Unit{}); err != nil {
		t.Fatal(err)
	}
	if len(clock.slept()) != 0 {
		t.Fatal("no send delay before the first throttle")
	}
	if _, err := client.ListRecords(context.Background(), Unit{}); err != nil {
		t.Fatal(err)
	}
	// The retry after the throttle waited for a send token (the backoff itself is zero).
	sleeps := clock.slept()
	if len(sleeps) != 2 || sleeps[0] != 0 || sleeps[1] <= 0 || !client.retry.limiter.bucket.enabled {
		t.Fatal(sleeps)
	}
}

func TestStandardAndLegacyNeverRateLimit(t *testing.T) {
	for _, mode := range []RetryMode{"", RetryModeLegacy, RetryModeStandard} {
		clock := &fakeClock{}
		client := newRecords(t, Config{RetryMode: mode, Clock: clock, HTTPClient: doFunc(func(*http.Request) (*http.Response, error) {
			return reply(429, map[string]any{"__type": "Throttled"}, nil), nil
		})})
		_, err := client.ListRecords(context.Background(), Unit{})
		var throttled *Throttled
		if !errors.As(err, &throttled) || client.retry.limiter != nil || !reflect.DeepEqual(clock.slept(), []time.Duration{0, 0}) {
			t.Fatalf("%q: %v %v", mode, err, clock.slept())
		}
	}
}

func TestSendDelayPastTheDeadlineFailsAndReturnsTheToken(t *testing.T) {
	clock := &fakeClock{}
	client := newRecords(t, Config{RetryMode: RetryModeAdaptive, Clock: clock, Timeout: time.Second, HTTPClient: doFunc(func(*http.Request) (*http.Response, error) {
		t.Fatal("sent despite the rate limit")
		return nil, nil
	})})
	bucket := client.retry.limiter.bucket
	bucket.enabled = true // a fill rate of 0.5/s makes the next token 2 s away
	_, err := client.ListRecords(context.Background(), Unit{})
	if !errors.Is(err, context.DeadlineExceeded) || bucket.capacity != 0 || len(clock.slept()) != 0 {
		t.Fatalf("%v capacity %v", err, bucket.capacity)
	}
}

func TestInvalidSettings(t *testing.T) {
	if _, err := NewRecordsClient(Config{Endpoint: endpoint, RetryMode: "aggressive"}); err == nil || !strings.Contains(err.Error(), "RetryMode") {
		t.Fatal(err)
	}
	if _, err := NewRecordsClient(Config{Endpoint: endpoint, EndpointDiscovery: "yes"}); err == nil || !strings.Contains(err.Error(), "EndpointDiscovery") {
		t.Fatal(err)
	}
}

func TestBackoffUsesTheInjectedClockAndRetryAfter(t *testing.T) {
	clock := &fakeClock{}
	replies := []*http.Response{reply(503, map[string]any{"__type": "Unavailable"}, map[string]any{"Retry-After": "2"}),
		reply(503, map[string]any{"__type": "Unavailable"}, nil), reply(200, nil, nil)}
	client := newRecords(t, Config{RetryMode: RetryModeStandard, Clock: clock, BaseDelay: 500 * time.Millisecond, MaxDelay: 750 * time.Millisecond,
		HTTPClient: doFunc(func(*http.Request) (*http.Response, error) { next := replies[0]; replies = replies[1:]; return next, nil })})
	client.retry.jitter = func() float64 { return 1 }
	if _, err := client.ListRecords(context.Background(), Unit{}); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(clock.slept(), []time.Duration{2 * time.Second, 750 * time.Millisecond}) {
		t.Fatal(clock.slept())
	}
}

type exchange struct {
	Method   string
	URL      string
	JSON     any
	Response map[string]any
}

// script serves exchanges in order, failing the test on any mismatch.
type script struct {
	t         *testing.T
	mu        sync.Mutex
	exchanges []exchange
}

func (s *script) Do(r *http.Request) (*http.Response, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if len(s.exchanges) == 0 {
		s.t.Errorf("unexpected request %s %s", r.Method, r.URL)
		return reply(500, nil, nil), nil
	}
	next := s.exchanges[0]
	s.exchanges = s.exchanges[1:]
	if r.Method != next.Method || r.URL.String() != next.URL {
		s.t.Errorf("request %s %s, expected %s %s", r.Method, r.URL, next.Method, next.URL)
	}
	if next.JSON != nil {
		data, _ := io.ReadAll(r.Body)
		var body any
		if err := json.Unmarshal(data, &body); err != nil || !reflect.DeepEqual(body, next.JSON) {
			s.t.Errorf("body %s, expected %v", data, next.JSON)
		}
	}
	if r.Header.Get("X-Api-Key") != "key" {
		s.t.Error("discovery and discovered calls are authenticated")
	}
	return respond(next.Response)
}

func (s *script) load(exchanges []exchange) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.exchanges = append([]exchange(nil), exchanges...)
}

func (s *script) remaining() int {
	s.mu.Lock()
	defer s.mu.Unlock()
	return len(s.exchanges)
}

func TestEndpointDiscoveryScenarios(t *testing.T) {
	var vectors struct {
		Scenarios []struct {
			Name              string
			EndpointDiscovery *bool
			MaxAttempts       int
			Steps             []struct {
				Advance   *float64
				Call      string
				Input     map[string]any
				Exchanges []exchange
				Outcome   string
			}
		}
	}
	load(t, "endpoint-discovery.json", &vectors)
	modes := map[bool]EndpointDiscoveryMode{true: EndpointDiscoveryEnabled, false: EndpointDiscoveryDisabled}
	for _, scenario := range vectors.Scenarios {
		t.Run(scenario.Name, func(t *testing.T) {
			clock := &fakeClock{}
			server := &script{t: t}
			mode := EndpointDiscoveryAuto
			if scenario.EndpointDiscovery != nil {
				mode = modes[*scenario.EndpointDiscovery]
			}
			client := newRecords(t, Config{Clock: clock, HTTPClient: server, EndpointDiscovery: mode, MaxAttempts: max(scenario.MaxAttempts, 0)})
			for _, step := range scenario.Steps {
				if step.Advance != nil {
					clock.advance(*step.Advance)
					continue
				}
				server.load(step.Exchanges)
				err := invoke(context.Background(), client, step.Call, step.Input)
				if got := outcome(err); got != step.Outcome {
					t.Fatalf("%s: outcome %s, expected %s", step.Call, got, step.Outcome)
				}
				if left := server.remaining(); left != 0 {
					t.Fatalf("%s: %d expected requests were not sent", step.Call, left)
				}
			}
		})
	}
}

func endpointsReply() *http.Response {
	return reply(200, map[string]any{"Endpoints": []any{map[string]any{"Address": "t1.records.example", "CachePeriodInMinutes": 1}}}, nil)
}

func TestConcurrentCallersShareOneDiscovery(t *testing.T) {
	var discoveries atomic.Int32
	started, release := make(chan struct{}), make(chan struct{})
	var once sync.Once
	client := newRecords(t, Config{HTTPClient: doFunc(func(r *http.Request) (*http.Response, error) {
		if r.URL.Path == "/endpoints" {
			discoveries.Add(1)
			once.Do(func() { close(started) })
			<-release
			return endpointsReply(), nil
		}
		if r.URL.Host != "t1.records.example" {
			t.Errorf("sent to %s", r.URL.Host)
		}
		return reply(200, nil, nil), nil
	})})
	var group sync.WaitGroup
	errs := make(chan error, 8)
	for range 8 {
		group.Add(1)
		go func() {
			defer group.Done()
			_, err := client.GetRecord(context.Background(), &GetRecordInput{Table: "t1", Key: "k"})
			errs <- err
		}()
	}
	<-started
	time.Sleep(100 * time.Millisecond)
	close(release)
	group.Wait()
	close(errs)
	for err := range errs {
		if err != nil {
			t.Fatal(err)
		}
	}
	if discoveries.Load() != 1 {
		t.Fatalf("%d discoveries", discoveries.Load())
	}
}

func TestAFailedSharedDiscoveryFailsEveryWaitingRequiredCall(t *testing.T) {
	started, release := make(chan struct{}), make(chan struct{})
	var once sync.Once
	client := newRecords(t, Config{HTTPClient: doFunc(func(*http.Request) (*http.Response, error) {
		once.Do(func() { close(started) })
		<-release
		return reply(400, map[string]any{"__type": "Denied"}, nil), nil
	})})
	var group sync.WaitGroup
	outcomes := make(chan string, 4)
	for range 4 {
		group.Add(1)
		go func() {
			defer group.Done()
			_, err := client.GetRecord(context.Background(), &GetRecordInput{Table: "t1", Key: "k"})
			outcomes <- outcome(err)
		}()
	}
	<-started
	time.Sleep(100 * time.Millisecond)
	close(release)
	group.Wait()
	close(outcomes)
	for result := range outcomes {
		if result != "Denied" {
			t.Fatal(result)
		}
	}
}

func TestSharedDiscoverySurvivesTheCancelledCallerThatStartedIt(t *testing.T) {
	var discoveries atomic.Int32
	started, release := make(chan struct{}), make(chan struct{})
	client := newRecords(t, Config{HTTPClient: doFunc(func(r *http.Request) (*http.Response, error) {
		if r.URL.Path == "/endpoints" {
			discoveries.Add(1)
			close(started)
			<-release
			if err := r.Context().Err(); err != nil {
				t.Error("the shared discovery was cancelled with its first caller")
			}
			return endpointsReply(), nil
		}
		return reply(200, map[string]any{"value": r.URL.Path}, nil), nil
	})})
	ctx, cancel := context.WithCancel(context.Background())
	first := make(chan error, 1)
	go func() {
		_, err := client.GetRecord(ctx, &GetRecordInput{Table: "t1", Key: "first"})
		first <- err
	}()
	<-started
	type result struct {
		index int
		value string
		err   error
	}
	results := make(chan result, 6)
	for i := range 6 {
		go func() {
			output, err := client.GetRecord(context.Background(), &GetRecordInput{Table: "t1", Key: fmt.Sprintf("k%d", i)})
			results <- result{i, output.GetValue(), err}
		}()
	}
	time.Sleep(50 * time.Millisecond)
	// The caller that started discovery goes away; the others still get its result.
	cancel()
	if err := <-first; !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	close(release)
	for range 6 {
		r := <-results
		if r.err != nil || r.value != fmt.Sprintf("/records/t1/k%d", r.index) {
			t.Fatal(r)
		}
	}
	if discoveries.Load() != 1 {
		t.Fatalf("%d discoveries", discoveries.Load())
	}
}

func TestAWaiterStopsAtItsOwnDeadline(t *testing.T) {
	started, release := make(chan struct{}), make(chan struct{})
	var discoveries atomic.Int32
	client := newRecords(t, Config{HTTPClient: doFunc(func(r *http.Request) (*http.Response, error) {
		if r.URL.Path == "/endpoints" {
			if discoveries.Add(1) == 1 {
				close(started)
			}
			<-release
			return endpointsReply(), nil
		}
		return reply(200, nil, nil), nil
	})})
	leader := make(chan error, 1)
	go func() {
		_, err := client.GetRecord(context.Background(), &GetRecordInput{Table: "t1", Key: "k"})
		leader <- err
	}()
	<-started
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	if _, err := client.GetRecord(ctx, &GetRecordInput{Table: "t1", Key: "k"}); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal(err)
	}
	close(release)
	if err := <-leader; err != nil {
		t.Fatal(err)
	}
	if _, err := client.GetRecord(context.Background(), &GetRecordInput{Table: "t1", Key: "k"}); err != nil || discoveries.Load() != 1 {
		t.Fatal(err, discoveries.Load())
	}
}

// recorder keeps every telemetry event, in order, from any goroutine.
type recorder struct {
	mu     sync.Mutex
	events []any
}

func (r *recorder) add(event any) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.events = append(r.events, event)
}
func (r *recorder) OperationStarted(e OperationStartedEvent)   { r.add(e) }
func (r *recorder) AttemptStarted(e AttemptStartedEvent)       { r.add(e) }
func (r *recorder) AttemptFinished(e AttemptFinishedEvent)     { r.add(e) }
func (r *recorder) OperationFinished(e OperationFinishedEvent) { r.add(e) }

func nullable[T comparable](value, none T) any {
	if value == none {
		return nil
	}
	return value
}

// record is an event as the shared vectors spell it.
func record(event any) (map[string]any, time.Time) {
	var fields map[string]any
	var at time.Time
	switch e := event.(type) {
	case OperationStartedEvent:
		fields = map[string]any{"event": "operationStarted", "invocationId": e.InvocationID, "service": e.Service, "operation": e.Operation}
		at = e.Time
	case AttemptStartedEvent:
		fields = map[string]any{"event": "attemptStarted", "invocationId": e.InvocationID, "service": e.Service, "operation": e.Operation,
			"attempt": e.Attempt, "method": e.Method, "endpoint": e.Endpoint, "endpointSource": e.EndpointSource, "target": e.Target,
			"headers": e.Headers, "requestBytes": nullable(e.RequestBytes, -1)}
		at = e.Time
	case AttemptFinishedEvent:
		fields = map[string]any{"event": "attemptFinished", "invocationId": e.InvocationID, "service": e.Service, "operation": e.Operation,
			"attempt": e.Attempt, "status": nullable(e.Status, 0), "error": nullable(e.Error, ""), "requestId": nullable(e.RequestID, ""),
			"responseBytes": nullable(e.ResponseBytes, -1), "latency": e.Latency.Seconds(), "retryReason": nullable(e.RetryReason, ""),
			"retryDelay": e.RetryDelay.Seconds()}
		at = e.Time
	case OperationFinishedEvent:
		fields = map[string]any{"event": "operationFinished", "invocationId": e.InvocationID, "service": e.Service, "operation": e.Operation,
			"attempts": e.Attempts, "status": nullable(e.Status, 0), "error": nullable(e.Error, ""), "latency": e.Latency.Seconds()}
		at = e.Time
	}
	fields["time"] = at.Format(time.RFC3339Nano)
	data, _ := json.Marshal(fields)
	var normalized map[string]any
	_ = json.Unmarshal(data, &normalized)
	return normalized, at
}

func matches(expected, actual any) bool {
	if expected == "*" {
		return actual != nil
	}
	if object, ok := expected.(map[string]any); ok {
		fields, ok := actual.(map[string]any)
		if !ok {
			return false
		}
		for key, value := range object {
			item, present := fields[key]
			if !present || !matches(value, item) {
				return false
			}
		}
		return true
	}
	return reflect.DeepEqual(expected, actual)
}

func TestTelemetryScenarios(t *testing.T) {
	var vectors struct {
		Scenarios []struct {
			Name      string
			Call      string
			Input     map[string]any
			Responses []map[string]any
			Secrets   []string
			Events    []map[string]any
		}
	}
	load(t, "telemetry.json", &vectors)
	for _, scenario := range vectors.Scenarios {
		t.Run(scenario.Name, func(t *testing.T) {
			hook := &recorder{}
			responses := scenario.Responses
			client := newRecords(t, Config{Telemetry: hook, Clock: &fakeClock{}, APIKey: func(context.Context) (string, error) { return "api-key-secret", nil },
				HTTPClient: doFunc(func(*http.Request) (*http.Response, error) {
					next := responses[0]
					responses = responses[1:]
					return respond(next)
				})})
			_ = invoke(context.Background(), client, scenario.Call, scenario.Input)
			records := make([]map[string]any, len(hook.events))
			var previous time.Time
			ids := map[any]bool{}
			for i, event := range hook.events {
				var at time.Time
				records[i], at = record(event)
				if at.Before(previous) {
					t.Fatal("event times go backwards")
				}
				previous = at
				if records[i]["operation"] == scenario.Call {
					ids[records[i]["invocationId"]] = true
				}
			}
			text, _ := json.MarshalIndent(records, "", " ")
			if len(records) != len(scenario.Events) {
				t.Fatalf("%d events, expected %d:\n%s", len(records), len(scenario.Events), text)
			}
			for i, expected := range scenario.Events {
				if !matches(expected, records[i]) {
					t.Fatalf("event %d: %v\n!=\n%v", i, expected, records[i])
				}
			}
			for _, secret := range scenario.Secrets {
				if bytes.Contains(text, []byte(secret)) {
					t.Fatalf("%q leaked:\n%s", secret, text)
				}
			}
			if len(ids) != 1 {
				t.Fatalf("one invocation id per call, got %v", ids)
			}
		})
	}
}

type brokenHook struct{ NopTelemetryHook }

func (brokenHook) AttemptStarted(AttemptStartedEvent) { panic("hook bug") }

func TestAFailingHookNeverChangesTheOutcome(t *testing.T) {
	var logged bytes.Buffer
	log.SetOutput(&logged)
	defer log.SetOutput(os.Stderr)
	client := newRecords(t, Config{Telemetry: brokenHook{}, HTTPClient: doFunc(func(*http.Request) (*http.Response, error) {
		return reply(200, map[string]any{"count": 2}, nil), nil
	})})
	output, err := client.ListRecords(context.Background(), Unit{})
	if err != nil || output.GetCount() != 2 || !strings.Contains(logged.String(), "telemetry hook AttemptStarted panicked: hook bug") {
		t.Fatal(err, logged.String())
	}
}

func TestLatencyAndBytesUseTheInjectedClock(t *testing.T) {
	clock := &fakeClock{}
	hook := &recorder{}
	client := newRecords(t, Config{Telemetry: hook, Clock: clock, HTTPClient: doFunc(func(*http.Request) (*http.Response, error) {
		clock.advance(0.25)
		return reply(200, map[string]any{"count": 7}, nil), nil
	})})
	if _, err := client.ListRecords(context.Background(), Unit{}); err != nil {
		t.Fatal(err)
	}
	finished := hook.events[2].(AttemptFinishedEvent)
	operation := hook.events[3].(OperationFinishedEvent)
	if finished.Latency != 250*time.Millisecond || finished.ResponseBytes != int64(len(`{"count":7}`)) || operation.Latency != 250*time.Millisecond {
		t.Fatal(finished, operation)
	}
}

func TestTelemetryIsOptional(t *testing.T) {
	client := newRecords(t, Config{HTTPClient: doFunc(func(*http.Request) (*http.Response, error) { return reply(200, nil, nil), nil })})
	if _, err := client.PutRecord(context.Background(), &PutRecordInput{Table: "t1", Secret: Some[Secret]("s")}); err != nil {
		t.Fatal(err)
	}
	if len(client.credentialHeaders) != 1 || !client.credentialHeaders["x-api-key"] {
		t.Fatal("per-call sensitive names must not leak into the client's credential set", client.credentialHeaders)
	}
}
