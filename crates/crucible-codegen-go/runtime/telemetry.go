package sdk

// Client telemetry: a dependency-free hook with redacted events. Events never
// carry bodies, credentials, or the values of sensitive members.
// docs/sdk-runtime-policy.md is the contract.

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"log"
	"net/http"
	"net/url"
	"reflect"
	"strings"
	"time"
	"unicode"
)

// TelemetryHook observes every call of a client: OperationStarted, then an
// AttemptStarted and AttemptFinished pair per attempt that reached the
// transport, then OperationFinished. Concurrent calls report concurrently, so
// an implementation must be safe for concurrent use. A hook that panics is
// logged and never changes a call's result, retries or timing. Embed
// NopTelemetryHook to implement only some of the methods.
type TelemetryHook interface {
	OperationStarted(OperationStartedEvent)
	AttemptStarted(AttemptStartedEvent)
	AttemptFinished(AttemptFinishedEvent)
	OperationFinished(OperationFinishedEvent)
}

// NopTelemetryHook ignores every event.
type NopTelemetryHook struct{}

// OperationStarted observes the start of a logical operation.
func (NopTelemetryHook) OperationStarted(OperationStartedEvent)   {}
// AttemptStarted observes an outgoing request attempt after sensitive headers and targets are redacted.
func (NopTelemetryHook) AttemptStarted(AttemptStartedEvent)       {}
// AttemptFinished observes the result of a request attempt and its retry decision.
func (NopTelemetryHook) AttemptFinished(AttemptFinishedEvent)     {}
// OperationFinished observes the completion of a logical operation.
func (NopTelemetryHook) OperationFinished(OperationFinishedEvent) {}

// OperationStartedEvent identifies the start of a call using a shared invocation identifier and monotonic time.
type OperationStartedEvent struct {
	// InvocationID is shared by one call's events.
	InvocationID string
	// Service is the service shape ID.
	Service   string
	Operation string
	Time      time.Time
}

// AttemptStartedEvent describes one outgoing attempt with redacted target and headers.
type AttemptStartedEvent struct {
	InvocationID string
	Service      string
	Operation    string
	// Attempt is 1-based.
	Attempt int
	Method  string
	// Endpoint is the scheme, host, port and base path; never user information.
	Endpoint string
	// Target is the path and query as sent, with sensitive and credential values redacted.
	Target string
	// Headers are the request headers as sent, lowercased, with sensitive and
	// credential values redacted.
	Headers map[string]string
	// RequestBytes is the Content-Length sent, or -1 for a chunked body.
	RequestBytes int64
	Time         time.Time
}

// AttemptFinishedEvent records status, safe error classification, byte counts, latency and retry decision.
type AttemptFinishedEvent struct {
	InvocationID string
	Service      string
	Operation    string
	Attempt      int
	// Status is the HTTP status, or 0 when no response arrived.
	Status int
	// Error is a modeled or wire error's name or the error's type name; never a
	// message. It is empty when the attempt succeeded.
	Error     string
	RequestID string
	// ResponseBytes is the bytes read,
	// or -1 when unknown or no response arrived.
	ResponseBytes int64
	Latency       time.Duration
	// RetryReason is set only when another attempt follows: "throttling",
	// "transient", "timeout" or "network".
	RetryReason string
	// RetryDelay is the wait before that attempt.
	RetryDelay time.Duration
	Time       time.Time
}

// OperationFinishedEvent records the final call status, safe error classification, total attempts and latency.
type OperationFinishedEvent struct {
	InvocationID string
	Service      string
	Operation    string
	Attempts     int
	// Status is the last attempt's HTTP status, or 0.
	Status  int
	Error   string
	Latency time.Duration
	Time    time.Time
}

const redacted = "***"

// Always redacted, whatever the model says.
var (
	credentialHeaders = map[string]bool{"authorization": true, "proxy-authorization": true, "cookie": true, "set-cookie": true}
)

func notify(event string, deliver func()) {
	defer func() {
		if recovered := recover(); recovered != nil {
			log.Printf("crucible: telemetry hook %s panicked: %v", event, recovered)
		}
	}()
	deliver()
}

// redactHeaders lowercases names and redacts credential and sensitive values.
func redactHeaders(headers http.Header, sensitive map[string]bool) map[string]string {
	result := make(map[string]string, len(headers))
	for name, values := range headers {
		lower := strings.ToLower(name)
		value := strings.Join(values, ", ")
		if credentialHeaders[lower] || sensitive[lower] {
			value = redacted
		}
		result[lower] = value
	}
	return result
}

// redactQuery keeps the query as sent, replacing sensitive values. Names compare case-insensitively: redacting too much is the safe failure.
func redactQuery(raw string, sensitive map[string]bool) string {
	if raw == "" {
		return ""
	}
	parts := strings.Split(raw, "&")
	for i, part := range parts {
		name, _, _ := strings.Cut(part, "=")
		decoded, err := url.QueryUnescape(name)
		if err != nil {
			decoded = name
		}
		lower := strings.ToLower(decoded)
		if sensitive[lower] {
			parts[i] = name + "=" + redacted
		}
	}
	return strings.Join(parts, "&")
}

// shownEndpoint is scheme, host, port and base path; never user information.
func shownEndpoint(endpoint string) string {
	parsed, err := url.Parse(endpoint)
	if err != nil {
		return ""
	}
	return parsed.Scheme + "://" + parsed.Host + parsed.EscapedPath()
}

// errorName is a modeled or wire error's name, or the error's type name.
func errorName(err error) string {
	if err == nil {
		return ""
	}
	var service *ServiceError
	switch {
	case errors.As(err, &service):
		return service.Name
	case errors.Is(err, context.Canceled):
		return "Canceled"
	case errors.Is(err, context.DeadlineExceeded):
		return "DeadlineExceeded"
	}
	for {
		kind := reflect.TypeOf(err)
		for kind.Kind() == reflect.Pointer {
			kind = kind.Elem()
		}
		name := kind.Name()
		if name == "wrapError" {
			if inner := errors.Unwrap(err); inner != nil {
				err = inner
				continue
			}
		}
		if name == "" || !unicode.IsUpper([]rune(name)[0]) {
			return "error"
		}
		return name
	}
}

// callTrace is one call's telemetry. Without a hook it only counts attempts.
type callTrace struct {
	runtime      *clientRuntime
	hook         TelemetryHook
	operation    string
	id           string
	attempts     int
	status       int
	start        time.Time
	attemptStart time.Time
}

func (c *clientRuntime) startTrace(operation string) *callTrace {
	trace := &callTrace{runtime: c, hook: c.config.Telemetry, operation: operation}
	if trace.hook == nil {
		return trace
	}
	id := make([]byte, 16)
	_, _ = rand.Read(id)
	trace.id = hex.EncodeToString(id)
	trace.start = c.config.Clock.Now()
	event := OperationStartedEvent{InvocationID: trace.id, Service: c.schema.Service, Operation: operation, Time: trace.start}
	notify("OperationStarted", func() { trace.hook.OperationStarted(event) })
	return trace
}

func (t *callTrace) attemptStarted(request *http.Request, endpoint, displayPath string, sensitiveHeaders, sensitiveQuery map[string]bool) {
	t.attempts++
	if t.hook == nil {
		return
	}
	t.attemptStart = t.runtime.config.Clock.Now()
	target := displayPath
	if query := redactQuery(request.URL.RawQuery, sensitiveQuery); query != "" {
		target += "?" + query
	}
	length := request.ContentLength
	if length == 0 && request.Body != nil && request.Body != http.NoBody {
		length = -1
	}
	event := AttemptStartedEvent{InvocationID: t.id, Service: t.runtime.schema.Service, Operation: t.operation, Attempt: t.attempts,
		Method: request.Method, Endpoint: shownEndpoint(endpoint), Target: target,
		Headers: redactHeaders(request.Header, sensitiveHeaders), RequestBytes: length, Time: t.attemptStart}
	notify("AttemptStarted", func() { t.hook.AttemptStarted(event) })
}

// attemptFinished reports an attempt; reason is empty unless another attempt follows.
func (t *callTrace) attemptFinished(response *http.Response, err error, size int64, reason string, delay time.Duration) {
	t.status = 0
	if response != nil {
		t.status = response.StatusCode
	}
	if t.hook == nil {
		return
	}
	now := t.runtime.config.Clock.Now()
	requestID := ""
	var service *ServiceError
	if errors.As(err, &service) {
		requestID = service.Metadata.RequestID
	} else if response != nil {
		requestID = response.Header.Get("X-Request-Id")
		if requestID == "" {
			requestID = response.Header.Get("X-Amzn-Requestid")
		}
	}
	if reason == "" {
		delay = 0
	}
	event := AttemptFinishedEvent{InvocationID: t.id, Service: t.runtime.schema.Service, Operation: t.operation, Attempt: t.attempts,
		Status: t.status, Error: errorName(err), RequestID: requestID, ResponseBytes: size, Latency: now.Sub(t.attemptStart),
		RetryReason: reason, RetryDelay: delay, Time: now}
	notify("AttemptFinished", func() { t.hook.AttemptFinished(event) })
}

func (t *callTrace) finished(err error) {
	if t.hook == nil {
		return
	}
	now := t.runtime.config.Clock.Now()
	event := OperationFinishedEvent{InvocationID: t.id, Service: t.runtime.schema.Service, Operation: t.operation, Attempts: t.attempts,
		Status: t.status, Error: errorName(err), Latency: now.Sub(t.start), Time: now}
	notify("OperationFinished", func() { t.hook.OperationFinished(event) })
}
