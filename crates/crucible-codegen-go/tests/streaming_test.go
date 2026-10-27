package streams_test

import (
	"bufio"
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	streams "example.com/streams"
)

const item = "data: {\"ready\":\"first\"}\n\n"
const complete = "event: crucible.complete\ndata: {}\n\n"
const media = "text/event-stream; charset=utf-8; crucible-stream=1"

type clientFunc func(*http.Request) (*http.Response, error)

func (f clientFunc) Do(r *http.Request) (*http.Response, error) { return f(r) }

func TestSuccessfulHeadersAreNotReplayedWithTransportError(t *testing.T) {
	var calls int
	c := client(t, "http://127.0.0.1:1", func(c *streams.Config) {
		c.HTTPClient = clientFunc(func(r *http.Request) (*http.Response, error) {
			calls++
			return &http.Response{StatusCode: 200, Header: http.Header{"Content-Type": []string{media}}, Body: io.NopCloser(strings.NewReader(item))}, errors.New("transport failed after success headers")
		})
	})
	if _, err := c.Watch(context.Background(), streams.Unit{}); err == nil {
		t.Fatal("missing transport failure")
	}
	if calls != 1 {
		t.Fatal("replayed after successful headers")
	}
}

// cancelOnLast cancels the call as it returns its final chunk, so the frame
// completes from buffered bytes and fails parsing after the lifetime ended.
type cancelOnLast struct {
	chunks []string
	cancel context.CancelFunc
}

func (b *cancelOnLast) Read(p []byte) (int, error) {
	if len(b.chunks) == 0 {
		return 0, io.EOF
	}
	chunk := b.chunks[0]
	b.chunks = b.chunks[1:]
	if len(b.chunks) == 0 {
		b.cancel()
	}
	return copy(p, chunk), nil
}

func (b *cancelOnLast) Close() error { return nil }

func TestProtocolFailuresAfterCancellationReportContext(t *testing.T) {
	frames := map[string]string{
		"malformed-json":     "data: {bad\n",
		"invalid-item":       "data: {\"ready\":5}\n",
		"invalid-completion": "event: crucible.complete\ndata: []\n",
		"invalid-late-error": "event: crucible.error\ndata: {}\n",
		"unknown-event":      "event: future\ndata: {}\n",
	}
	for name, frame := range frames {
		t.Run(name, func(t *testing.T) {
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			c := client(t, "http://127.0.0.1:1", func(c *streams.Config) {
				c.HTTPClient = clientFunc(func(r *http.Request) (*http.Response, error) {
					return &http.Response{StatusCode: 200, Header: http.Header{"Content-Type": []string{media}}, Body: &cancelOnLast{chunks: []string{frame, "\n"}, cancel: cancel}}, nil
				})
			})
			events, err := c.Watch(ctx, streams.Unit{})
			if err != nil {
				t.Fatal(err)
			}
			defer events.Close()
			if events.Next() {
				t.Fatal("disclosed an item after cancellation")
			}
			if !errors.Is(events.Err(), context.Canceled) {
				t.Fatalf("cancellation must take precedence over %s: %v", name, events.Err())
			}
		})
	}
}

func client(t *testing.T, endpoint string, modify ...func(*streams.Config)) *streams.StreamsClient {
	t.Helper()
	config := streams.Config{Endpoint: endpoint, Timeout: 3 * time.Second, BaseDelay: time.Nanosecond}
	for _, f := range modify {
		f(&config)
	}
	c, err := streams.NewStreamsClient(config)
	if err != nil {
		t.Fatal(err)
	}
	return c
}

func TestPhysicalFrameBytesContiguousAndSplitCRLF(t *testing.T) {
	type profile struct {
		name, wire string
		valid      bool
		items      int
	}
	var cases []profile
	for _, delimiter := range []struct{ name, wire string }{{"lf", "\n"}, {"cr", "\r"}, {"crlf", "\r\n"}} {
		for _, size := range []int{256, 257} {
			suffix := delimiter.wire + `data: {"ready":"first"}` + strings.Repeat(delimiter.wire, 2)
			frame := "x:" + strings.Repeat("a", size-2-len(suffix)) + suffix
			if len(frame) != size {
				t.Fatal("incorrect physical boundary")
			}
			count := 0
			// The blank CR delivers immediately; an optional LF that exceeds
			// that completed frame's budget fails on the following Next.
			if size == 256 || delimiter.name == "crlf" {
				count = 1
			}
			cases = append(cases, profile{fmt.Sprintf("%s-%d", delimiter.name, size), frame + complete, size == 256, count})
		}
	}
	cases = append(cases, profile{"ignored-crlf-65", strings.Repeat("x:\r\n", 65) + item + complete, false, 0})
	for _, control := range []struct{ name, suffix string }{{"empty", "\r\n\r\n"}, {"terminal", "\r\nevent: crucible.complete\r\ndata: {}\r\n\r\n"}} {
		wire := "x:" + strings.Repeat("a", 257-2-len(control.suffix)) + control.suffix
		if control.name == "empty" {
			wire += item + complete
		}
		cases = append(cases, profile{control.name + "-crlf-257", wire, false, 0})
	}
	suffix := "\r\n" + `data: {"ready":"first"}` + "\r\n\r\n"
	frame := "x:" + strings.Repeat("a", 256-2-len(suffix)) + suffix
	cases = append(cases, profile{"consecutive-crlf-256", frame + frame + complete, true, 2})
	for _, split := range []bool{false, true} {
		for _, test := range cases {
			t.Run(fmt.Sprintf("%s/split-%v", test.name, split), func(t *testing.T) {
				var requests atomic.Int32
				disconnected := make(chan struct{})
				server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
					defer close(disconnected)
					requests.Add(1)
					w.Header().Set("Content-Type", media)
					if split {
						for _, b := range []byte(test.wire) {
							if _, err := w.Write([]byte{b}); err != nil {
								return
							}
							w.(http.Flusher).Flush()
							if b == '\r' {
								time.Sleep(time.Millisecond)
							}
						}
					} else {
						io.WriteString(w, test.wire)
					}
					if !test.valid {
						w.(http.Flusher).Flush()
						<-r.Context().Done()
					}
				}))
				defer server.Close()
				c := client(t, server.URL, func(c *streams.Config) { c.MaxStreamEventBytes = 256 })
				events, err := c.Watch(context.Background(), streams.Unit{})
				if err != nil {
					t.Fatal(err)
				}
				defer events.Close()
				count := 0
				for events.Next() {
					count++
				}
				var protocol *streams.StreamProtocolError
				if test.valid && events.Err() != nil {
					t.Fatal(events.Err())
				}
				if !test.valid && !errors.As(events.Err(), &protocol) {
					t.Fatalf("expected protocol failure: %v", events.Err())
				}
				if count != test.items || requests.Load() != 1 {
					t.Fatalf("items=%d expected=%d requests=%d", count, test.items, requests.Load())
				}
				if !test.valid {
					select {
					case <-disconnected:
					case <-time.After(time.Second):
						t.Fatal("response must close before helper cleanup")
					}
				}
			})
		}
	}
}

func TestSharedProfilesOverBytewiseNativeTCP(t *testing.T) {
	data, err := os.ReadFile(os.Getenv("CRUCIBLE_STREAM_VECTORS"))
	if err != nil {
		t.Fatal(err)
	}
	var profile struct {
		Cases []struct {
			Name, Wire, Hex string
			Valid           bool
			Items, Limit    int
		}
	}
	if err = json.Unmarshal(data, &profile); err != nil {
		t.Fatal(err)
	}
	if len(profile.Cases) != 25 {
		t.Fatal("shared profile must contain 25 vectors")
	}
	for _, test := range profile.Cases {
		t.Run(test.Name, func(t *testing.T) {
			wire := []byte(test.Wire)
			if test.Hex != "" {
				wire, err = hex.DecodeString(test.Hex)
				if err != nil {
					t.Fatal(err)
				}
			}
			listener, err := net.Listen("tcp", "127.0.0.1:0")
			if err != nil {
				t.Fatal(err)
			}
			defer listener.Close()
			done := make(chan struct{})
			go func() {
				defer close(done)
				connection, e := listener.Accept()
				if e != nil {
					return
				}
				defer connection.Close()
				request, e := http.ReadRequest(bufio.NewReader(connection))
				if e != nil {
					return
				}
				request.Body.Close()
				fmt.Fprintf(connection, "HTTP/1.1 200 OK\r\nContent-Type: %s\r\nContent-Length: %d\r\nConnection: close\r\n\r\n", media, len(wire))
				for _, b := range wire {
					if _, e = connection.Write([]byte{b}); e != nil {
						break
					}
				}
			}()
			c := client(t, "http://"+listener.Addr().String(), func(c *streams.Config) { c.MaxStreamEventBytes = test.Limit })
			events, err := c.Watch(context.Background(), streams.Unit{})
			if err != nil {
				t.Fatal(err)
			}
			defer events.Close()
			count := 0
			for events.Next() {
				count++
				if test.Name == "unknown-union" && events.Value().UnknownVariant.Name != "future" {
					t.Fatal("unknown union lost")
				}
			}
			if test.Valid && events.Err() != nil {
				t.Fatal(events.Err())
			}
			if !test.Valid {
				var protocol *streams.StreamProtocolError
				if !errors.As(events.Err(), &protocol) {
					t.Fatalf("expected protocol failure: %v", events.Err())
				}
			}
			if count != test.Items {
				t.Fatalf("got %d items; expected %d", count, test.Items)
			}
			<-done
		})
	}
}

func TestHeldDelimitersProgressBeforeClose(t *testing.T) {
	for _, delimiter := range []string{"\n", "\r\n", "\r"} {
		t.Run(fmt.Sprintf("%q", delimiter), func(t *testing.T) {
			disconnected := make(chan struct{})
			release := make(chan struct{})
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				w.Header().Set("Content-Type", media)
				io.WriteString(w, "data: {\"ready\":\"first\"}"+delimiter+delimiter)
				w.(http.Flusher).Flush()
				select {
				case <-r.Context().Done():
					close(disconnected)
				case <-release:
				}
			}))
			defer server.Close()
			defer close(release)
			events, err := client(t, server.URL).Watch(context.Background(), streams.Unit{})
			if err != nil {
				t.Fatal(err)
			}
			if !events.Next() || events.Value().GetReady() != "first" {
				t.Fatalf("first item before close: %v", events.Err())
			}
			events.Close()
			select {
			case <-disconnected:
			case <-time.After(time.Second):
				t.Fatal("Close failed to release held body BEFORE server cleanup")
			}
		})
	}
}

func TestOpeningRetriesAuthMiddlewareUnsafePostAndOversize(t *testing.T) {
	for _, post := range []bool{false, true} {
		t.Run(fmt.Sprint(post), func(t *testing.T) {
			var calls atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				if r.Header.Get("Authorization") != "Bearer token" || r.Header.Get("X-Test") != "middleware" {
					t.Error("auth/middleware missing")
				}
				w.Header().Set("Content-Type", media)
				if calls.Add(1) == 1 {
					w.WriteHeader(503)
					io.WriteString(w, strings.Repeat("x", 4096))
					return
				}
				io.WriteString(w, item+complete)
			}))
			defer server.Close()
			c := client(t, server.URL, func(c *streams.Config) {
				c.MaxStreamEventBytes = 256
				c.Token = func(context.Context) (string, error) { return "token", nil }
				c.Middleware = []func(context.Context, *http.Request) error{func(_ context.Context, r *http.Request) error { r.Header.Set("X-Test", "middleware"); return nil }}
			})
			var events *streams.ResponseStream[*streams.Progress]
			var err error
			if post {
				events, err = c.Publish(context.Background(), &streams.PublishInput{Topic: "send"})
				if err == nil {
					events.Close()
					t.Fatal("unsafe POST replayed")
				}
				if calls.Load() != 1 {
					t.Fatal("POST replay")
				}
			} else {
				events, err = c.Watch(context.Background(), streams.Unit{})
				if err != nil {
					t.Fatal(err)
				}
				defer events.Close()
				if !events.Next() {
					t.Fatal(events.Err())
				}
				if events.Next() || events.Err() != nil {
					t.Fatal(events.Err())
				}
				if calls.Load() != 2 {
					t.Fatal("eligible oversized opening error should retry")
				}
			}
		})
	}
}

type attemptBytes struct {
	streams.NopTelemetryHook
	bytes atomic.Int64
}

func (h *attemptBytes) AttemptFinished(event streams.AttemptFinishedEvent) {
	h.bytes.Store(event.ResponseBytes)
}

func TestOpeningErrorAtAndPastTheLimit(t *testing.T) {
	// A modelled opening error of exactly MaxStreamEventBytes keeps its fields;
	// one byte more is oversized, so even a valid JSON prefix is not decoded.
	document := `{"__type":"WorkFailed","message":"opening"}`
	for _, test := range []struct {
		name    string
		size    int
		decoded bool
	}{{"exact", 256, true}, {"oversized", 257, false}} {
		t.Run(test.name, func(t *testing.T) {
			body := document + strings.Repeat(" ", test.size-len(document))
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				w.Header().Set("Content-Type", "application/json")
				w.WriteHeader(503)
				io.WriteString(w, body)
			}))
			defer server.Close()
			hook := &attemptBytes{}
			c := client(t, server.URL, func(c *streams.Config) { c.MaxStreamEventBytes = 256; c.Telemetry = hook })
			_, err := c.Publish(context.Background(), &streams.PublishInput{Topic: "send"})
			var service *streams.ServiceError
			if !errors.As(err, &service) {
				t.Fatal(err)
			}
			if (service.Message == "opening") != test.decoded {
				t.Fatalf("message %q for a %d-byte body", service.Message, test.size)
			}
			// Telemetry reports the bytes read even when the body is discarded.
			if want := int64(test.size); hook.bytes.Load() != want {
				t.Fatalf("ResponseBytes %d, expected %d", hook.bytes.Load(), want)
			}
		})
	}
}

func TestLateErrorsAreDocumentsAndRetainMetadata(t *testing.T) {
	tests := []struct{ name, fields, reason string }{
		{"WorkFailed", `{"message":"late","reason":"broken"}`, "broken"},
		{"HeaderFailure", `{"message":"late","reason":"broken"}`, "broken"},
		{"PayloadFailure", `{"reason":"future"}`, "future"},
		{"DefaultReason", `{"message":"late"}`, "broken"},
		{"DefaultEmptyReason", `{"message":"late"}`, ""},
		{"FutureError", `{"message":"late"}`, ""},
		{"FutureError", `{"message":5}`, ""},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			var requests atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				requests.Add(1)
				w.Header().Set("Content-Type", media)
				w.Header().Set("x-request-id", "profile")
				w.Header().Set("X-Reason", "header-spoof")
				fmt.Fprintf(w, "%sevent: crucible.error\ndata: {\"type\":%q,\"status\":503,\"error\":%s}\n\n", item, test.name, test.fields)
			}))
			defer server.Close()
			events, err := client(t, server.URL).Watch(context.Background(), streams.Unit{})
			if err != nil {
				t.Fatal(err)
			}
			defer events.Close()
			if !events.Next() {
				t.Fatal(events.Err())
			}
			if events.Next() {
				t.Fatal("unexpected late item")
			}
			var service *streams.ServiceError
			if !errors.As(events.Err(), &service) {
				t.Fatal(events.Err())
			}
			if service.Name != test.name || service.Metadata.Status != 503 || service.Metadata.RequestID != "profile" {
				t.Fatal(service)
			}
			// A late error without a string message keeps the opening fallback.
			want := "HTTP 503"
			if strings.Contains(test.fields, `"message":"late"`) {
				want = "late"
			}
			if service.Message != want {
				t.Fatalf("message %q, expected %q", service.Message, want)
			}
			switch test.name {
			case "HeaderFailure":
				var e *streams.HeaderFailure
				if !errors.As(events.Err(), &e) || string(e.Reason) != test.reason {
					t.Fatal(events.Err())
				}
			case "PayloadFailure":
				var e *streams.PayloadFailure
				if !errors.As(events.Err(), &e) || string(e.Reason) != test.reason {
					t.Fatal(events.Err())
				}
			case "DefaultReason":
				var e *streams.DefaultReason
				if !errors.As(events.Err(), &e) || string(e.Reason) != test.reason {
					t.Fatal(events.Err())
				}
			case "DefaultEmptyReason":
				var e *streams.DefaultEmptyReason
				if !errors.As(events.Err(), &e) || string(e.Reason) != test.reason {
					t.Fatal(events.Err())
				}
			}
			if requests.Load() != 1 {
				t.Fatal("replay after headers")
			}
		})
	}
}

func fixture(t *testing.T, mode string) (string, func() [2]int) {
	t.Helper()
	binary := os.Getenv("CRUCIBLE_STREAM_FIXTURE")
	if binary == "" {
		t.Fatal("CRUCIBLE_STREAM_FIXTURE required")
	}
	command := exec.Command(binary, mode)
	pipe, err := command.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	if err = command.Start(); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { command.Process.Kill(); command.Wait() })
	// A fixture that hangs or exits before printing its endpoints fails this
	// test (the cleanup kills it) instead of blocking the whole run.
	started := make(chan string, 1)
	go func() {
		scanner := bufio.NewScanner(pipe)
		if scanner.Scan() {
			started <- scanner.Text()
		}
		close(started)
	}()
	var line string
	select {
	case text, ok := <-started:
		if !ok {
			t.Fatalf("Rust fixture %q exited before starting", mode)
		}
		line = text
	case <-time.After(30 * time.Second):
		t.Fatalf("Rust fixture %q printed no endpoints within 30s", mode)
	}
	urls := strings.Fields(line)
	if len(urls) != 2 {
		t.Fatal(line)
	}
	counts := func() (result [2]int) {
		response, e := http.Get(urls[1])
		if e != nil {
			t.Fatal(e)
		}
		defer response.Body.Close()
		if e = json.NewDecoder(response.Body).Decode(&result); e != nil {
			t.Fatal(e)
		}
		return
	}
	return urls[0], counts
}
func dropped(t *testing.T, counts func() [2]int) {
	t.Helper()
	until := time.Now().Add(2 * time.Second)
	for counts()[1] != 1 && time.Now().Before(until) {
		time.Sleep(10 * time.Millisecond)
	}
	if counts()[1] != 1 {
		t.Fatal("producer must drop BEFORE server cleanup")
	}
}

func TestRustGETPOSTUnaryAndProducerOwnership(t *testing.T) {
	for _, post := range []bool{false, true} {
		t.Run(fmt.Sprint(post), func(t *testing.T) {
			endpoint, counts := fixture(t, "hold")
			c := client(t, endpoint)
			var events *streams.ResponseStream[*streams.Progress]
			var err error
			if post {
				events, err = c.Publish(context.Background(), &streams.PublishInput{Topic: "hold"})
			} else {
				events, err = c.Watch(context.Background(), streams.Unit{})
			}
			if err != nil {
				t.Fatal(err)
			}
			defer events.Close()
			if !events.Next() || events.Value().GetReady() != "first" {
				t.Fatal(events.Err())
			}
			if counts()[1] != 0 {
				t.Fatal("producer finished early")
			}
			output, err := c.Read(context.Background(), streams.Unit{})
			if err != nil || output.Value != "unary" {
				t.Fatal(err)
			}
			events.Close()
			dropped(t, counts)
		})
	}
}

func TestRustCancellationBeforeConsumptionMidreadIdleQueuedAndDeadline(t *testing.T) {
	for _, mode := range []string{"before", "midread", "idle", "queued", "deadline"} {
		t.Run(mode, func(t *testing.T) {
			producer := "hold"
			if mode == "midread" {
				producer = "pending"
			}
			endpoint, counts := fixture(t, producer)
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			c := client(t, endpoint, func(c *streams.Config) {
				if mode == "deadline" {
					c.Timeout = 150 * time.Millisecond
				}
			})
			events, err := c.Watch(ctx, streams.Unit{})
			if err != nil {
				t.Fatal(err)
			}
			defer events.Close()
			if mode == "idle" {
				events.Close()
				dropped(t, counts)
				return
			}
			done := make(chan bool, 1)
			if mode == "midread" {
				go func() { done <- events.Next() }()
				time.Sleep(50 * time.Millisecond)
			}
			if mode == "queued" {
				time.Sleep(50 * time.Millisecond)
			}
			if mode == "deadline" {
				time.Sleep(200 * time.Millisecond)
			} else {
				cancel()
			}
			if mode == "midread" {
				select {
				case value := <-done:
					if value {
						t.Fatal("disclosed item after cancellation")
					}
				case <-time.After(time.Second):
					t.Fatal("cancel failed to interrupt read")
				}
			} else if events.Next() {
				t.Fatal("queued item disclosed after lifetime")
			}
			if !errors.Is(events.Err(), context.Canceled) && !errors.Is(events.Err(), context.DeadlineExceeded) {
				t.Fatal(events.Err())
			}
			dropped(t, counts)
		})
	}
}

func TestPrecancelBeforeAuthAndDeadlineDuringCredentials(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	c := client(t, "http://127.0.0.1:1", func(c *streams.Config) {
		c.Token = func(context.Context) (string, error) { t.Fatal("auth executed after precancel"); return "", nil }
	})
	if _, err := c.Watch(ctx, streams.Unit{}); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	c = client(t, "http://127.0.0.1:1", func(c *streams.Config) {
		c.Timeout = 20 * time.Millisecond
		c.Token = func(ctx context.Context) (string, error) { <-ctx.Done(); return "", ctx.Err() }
	})
	if _, err := c.Watch(context.Background(), streams.Unit{}); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal(err)
	}
}

func TestInterruptedHTTPBodyRetainsNetworkErrorAndNeverReplays(t *testing.T) {
	var requests atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests.Add(1)
		w.Header().Set("Content-Type", media)
		w.Header().Set("Content-Length", "9999")
		io.WriteString(w, item)
	}))
	defer server.Close()
	events, err := client(t, server.URL).Watch(context.Background(), streams.Unit{})
	if err != nil {
		t.Fatal(err)
	}
	defer events.Close()
	if !events.Next() {
		t.Fatal(events.Err())
	}
	if events.Next() {
		t.Fatal("unexpected extra item")
	}
	var network *streams.NetworkError
	if !errors.As(events.Err(), &network) {
		t.Fatal(events.Err())
	}
	if requests.Load() != 1 {
		t.Fatal("replayed after successful headers")
	}
}
