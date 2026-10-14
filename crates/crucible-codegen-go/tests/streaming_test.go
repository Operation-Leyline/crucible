package stream

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

// Streaming blobs against a real loopback HTTP server.
const large = 256 << 20

type observed struct {
	attempts      atomic.Int64
	mu            sync.Mutex
	mode          string
	headers       http.Header
	contentLength int64
	chunked       bool
	received      string
	size          int64
	release       chan struct{}
	aborted       chan struct{}
	arrived       chan struct{}
}

func (o *observed) set(mode string) {
	o.mu.Lock()
	defer o.mu.Unlock()
	o.mode = mode
	o.release = make(chan struct{})
	o.aborted = make(chan struct{}, 1)
	o.arrived = make(chan struct{}, 1)
	o.attempts.Store(0)
}

// arrival is signalled when an upload's first body bytes reach the server.
func (o *observed) arrival() chan struct{} {
	o.mu.Lock()
	defer o.mu.Unlock()
	return o.arrived
}

func (o *observed) snapshot() (string, chan struct{}, chan struct{}) {
	o.mu.Lock()
	defer o.mu.Unlock()
	return o.mode, o.release, o.aborted
}

func (o *observed) last() (http.Header, int64, bool, string, int64) {
	o.mu.Lock()
	defer o.mu.Unlock()
	return o.headers, o.contentLength, o.chunked, o.received, o.size
}

type prefix struct {
	data    []byte
	count   int64
	arrived chan struct{}
}

func (p *prefix) Write(b []byte) (int, error) {
	if p.count == 0 && len(b) > 0 {
		select {
		case p.arrived <- struct{}{}:
		default:
		}
	}
	if len(p.data) < 1024 {
		p.data = append(p.data, b[:min(len(b), 1024-len(p.data))]...)
	}
	p.count += int64(len(b))
	return len(b), nil
}

// generated produces bytes on demand, so a large body never exists in memory.
type generated struct{ left int64 }

var block = bytes.Repeat([]byte{'x'}, 64<<10)

func (g *generated) Read(p []byte) (int, error) {
	if g.left == 0 {
		return 0, io.EOF
	}
	n := copy(p, block[:min(int64(len(block)), g.left)])
	g.left -= int64(n)
	return n, nil
}

// plain hides every method but Read: a one-shot reader of unknown length.
type plain struct{ io.Reader }

func handler(o *observed) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		mode, release, aborted := o.snapshot()
		signal := func() {
			select {
			case aborted <- struct{}{}:
			default:
			}
		}
		switch r.URL.Path {
		case "/upload", "/sized", "/store":
			attempt := o.attempts.Add(1)
			if mode == "early" {
				// Answer before reading any of the body, as a server rejecting an upload
				// does. Full duplex stops net/http draining the unread body before it
				// writes the response, which would wait on a client that sends nothing.
				if err := http.NewResponseController(w).EnableFullDuplex(); err != nil {
					panic(err)
				}
				// The test releases the answer once the client holds an open body.
				select {
				case <-release:
				case <-r.Context().Done():
					return
				}
				w.Header().Set("Content-Length", "0")
				w.WriteHeader(400)
				w.(http.Flusher).Flush()
				return
			}
			sink := &prefix{arrived: o.arrival()}
			if _, err := io.Copy(sink, r.Body); err != nil {
				signal()
			}
			o.mu.Lock()
			o.headers, o.contentLength, o.chunked = r.Header.Clone(), r.ContentLength, len(r.TransferEncoding) > 0 && r.TransferEncoding[0] == "chunked"
			o.received, o.size = string(sink.data), sink.count
			o.mu.Unlock()
			if mode == "503" || mode == "503-once" && attempt == 1 {
				w.WriteHeader(503)
				return
			}
			w.WriteHeader(204)
		case "/download":
			count := "3"
			if mode == "invalid" {
				count = "invalid"
			}
			w.Header().Set("x-count", count)
			if mode == "large" {
				w.Header().Set("Content-Length", strconv.Itoa(large))
			}
			w.WriteHeader(200)
			w.(http.Flusher).Flush()
			switch mode {
			case "ok":
				select {
				case <-release:
				case <-r.Context().Done():
					return
				}
				io.WriteString(w, "abc")
			case "hang":
				select {
				case <-release:
				case <-r.Context().Done():
					signal()
				}
			case "large":
				for sent := 0; sent < large; sent += len(block) {
					if _, err := w.Write(block); err != nil {
						return
					}
				}
			default:
				for i := 0; i < 16384; i++ {
					if _, err := w.Write(block); err != nil {
						signal()
						return
					}
					w.(http.Flusher).Flush()
				}
			}
		case "/fetch":
			checksum := "y/Q5Jg=="
			if mode == "bad" {
				checksum = "AAAAAA=="
			}
			w.Header().Set("x-amz-checksum-crc32", checksum)
			io.WriteString(w, "123456789")
		default:
			w.WriteHeader(404)
		}
	}
}

func setup(t *testing.T) (*observed, *StreamingClient, Config) {
	t.Helper()
	o := &observed{}
	o.set("ok")
	server := httptest.NewServer(handler(o))
	t.Cleanup(func() {
		_, release, _ := o.snapshot()
		select {
		case <-release:
		default:
			close(release)
		}
		server.CloseClientConnections()
		server.Close()
	})
	config := Config{Endpoint: server.URL, BaseDelay: time.Millisecond, MaxDelay: time.Millisecond, Timeout: 2 * time.Minute}
	client, err := NewStreamingClient(config)
	if err != nil {
		t.Fatal(err)
	}
	return o, client, config
}

func waitFor(t *testing.T, signal chan struct{}, what string) {
	t.Helper()
	select {
	case <-signal:
	case <-time.After(10 * time.Second):
		t.Fatal(what)
	}
}

func TestStreamingUploads(t *testing.T) {
	o, client, _ := setup(t)
	ctx := context.Background()

	// A plain reader is one-shot: sent once, chunked, even for a retryable operation.
	o.set("503")
	var service *ServiceError
	_, err := client.Upload(ctx, &UploadInput{Body: plain{strings.NewReader("one shot")}})
	if !errors.As(err, &service) || service.Metadata.Status != 503 || o.attempts.Load() != 1 {
		t.Fatal("one-shot dispatched more than once", err, o.attempts.Load())
	}
	if _, _, chunked, received, _ := o.last(); !chunked || received != "one shot" {
		t.Fatal("one-shot body", chunked, received)
	}
	oneShot := NewReaderStream(strings.NewReader("once"), 4)
	if oneShot.Replayable() || oneShot.Length() != 4 {
		t.Fatal("one-shot properties")
	}
	o.set("ok")
	if _, err := client.Upload(ctx, &UploadInput{Body: oneShot}); err != nil {
		t.Fatal(err)
	}
	if _, err := client.Upload(ctx, &UploadInput{Body: oneShot}); !errors.Is(err, ErrStreamConsumed) {
		t.Fatal("one-shot reused", err)
	}

	// A factory opens once per attempt and reproduces the bytes; its length is sent.
	o.set("503-once")
	opens := 0
	factory := NewByteStream(func() (io.ReadCloser, error) {
		opens++
		return io.NopCloser(strings.NewReader("same bytes")), nil
	}, 10)
	if _, err := client.Upload(ctx, &UploadInput{Body: factory}); err != nil {
		t.Fatal(err)
	}
	if _, length, chunked, received, _ := o.last(); opens != 2 || o.attempts.Load() != 2 || received != "same bytes" || length != 10 || chunked {
		t.Fatal("replay per attempt", opens, o.attempts.Load(), received, length)
	}

	// A seeker replays from its starting position, with the remaining length.
	o.set("503-once")
	seeker := strings.NewReader("skipreplayed")
	seeker.Seek(4, io.SeekStart)
	if _, err := client.Upload(ctx, &UploadInput{Body: seeker}); err != nil {
		t.Fatal(err)
	}
	if _, length, _, received, _ := o.last(); o.attempts.Load() != 2 || received != "replayed" || length != 8 {
		t.Fatal("seeker replay", received, length)
	}

	// Files are seekers: sized and rewound, and left open for their owner.
	o.set("ok")
	path := filepath.Join(t.TempDir(), "body.bin")
	os.WriteFile(path, []byte("file bytes"), 0o600)
	file, err := os.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	if _, err := client.Upload(ctx, &UploadInput{Body: file}); err != nil {
		t.Fatal(err)
	}
	if _, length, _, received, _ := o.last(); received != "file bytes" || length != 10 {
		t.Fatal("file upload", received, length)
	}
	if _, err := file.Seek(0, io.SeekStart); err != nil {
		t.Fatal("file closed by the client", err)
	}

	// A nil stream is an empty body.
	if _, err := client.Upload(ctx, &UploadInput{}); err != nil {
		t.Fatal(err)
	}
	if _, length, _, _, size := o.last(); length != 0 || size != 0 {
		t.Fatal("empty upload", length, size)
	}

	// requiresLength and @length are checked before sending, and while bytes flow.
	o.set("ok")
	if _, err := client.Sized(ctx, &SizedInput{Body: plain{strings.NewReader("abc")}}); !errors.Is(err, ErrStreamLength) {
		t.Fatal("unknown length accepted", err)
	}
	if _, err := client.Sized(ctx, &SizedInput{Body: strings.NewReader("abcd")}); !errors.Is(err, ErrStreamLength) {
		t.Fatal("modeled maximum ignored", err)
	}
	if o.attempts.Load() != 0 {
		t.Fatal("invalid lengths were sent")
	}
	if _, err := client.Sized(ctx, &SizedInput{Body: strings.NewReader("abc")}); err != nil {
		t.Fatal(err)
	}
	if _, length, _, received, _ := o.last(); length != 3 || received != "abc" {
		t.Fatal("sized upload", length, received)
	}
	for _, text := range []string{"ab", "abcd"} {
		_, err := client.Sized(ctx, &SizedInput{Body: NewReaderStream(plain{strings.NewReader(text)}, 3)})
		if !errors.Is(err, ErrStreamLength) {
			t.Fatal("declared length broken by", text, err)
		}
		var network *NetworkError
		if errors.As(err, &network) {
			t.Fatal("length failure reported as a network error")
		}
	}

	// A failing reader surfaces its own error.
	broken := errors.New("disk failed")
	_, err = client.Upload(ctx, &UploadInput{Body: io.MultiReader(strings.NewReader("partial"), &failing{broken})})
	if !errors.Is(err, broken) {
		t.Fatal("reader failure", err)
	}
}

type failing struct{ err error }

func (f *failing) Read([]byte) (int, error) { return 0, f.err }

func TestStreamingUploadCancellation(t *testing.T) {
	// One-shot pipes, and a replayable stream whose opened reader is a pipe.
	for _, name := range []string{"one-shot", "replayable"} {
		t.Run(name, func(t *testing.T) {
			o, client, _ := setup(t)
			reader, writer := io.Pipe()
			var body io.Reader = reader
			if name == "replayable" {
				body = NewByteStream(func() (io.ReadCloser, error) { return reader, nil }, -1)
			}
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			done := make(chan error, 1)
			go func() {
				_, err := client.Upload(ctx, &UploadInput{Body: body})
				done <- err
			}()
			if _, err := writer.Write([]byte("started")); err != nil {
				t.Fatal(err)
			}
			// Once the first chunk reaches the server the transport has flushed it,
			// and its next read of the pipe blocks until the pipe is closed. The
			// caller never writes again, so only cancellation can end that read:
			// net/http waits for it before Do returns.
			waitFor(t, o.arrival(), "the first chunk never reached the server")
			cancel()
			select {
			case err := <-done:
				if !errors.Is(err, context.Canceled) {
					t.Fatal("cancelled upload", err)
				}
			case <-time.After(10 * time.Second):
				t.Fatal("cancellation did not end a read blocked in the request stream")
			}
			if _, err := writer.Write([]byte("more")); !errors.Is(err, io.ErrClosedPipe) {
				t.Fatal("cancelled upload left the caller's stream open", err)
			}
		})
	}
}

func TestStreamingUploadEarlyResponse(t *testing.T) {
	// A response that arrives before the body is sent ends the call while the
	// transport is still blocked reading the stream: the call closes it on return,
	// so the transport never reads the caller's stream after the call has ended.
	for _, name := range []string{"one-shot", "replayable"} {
		t.Run(name, func(t *testing.T) {
			o, client, _ := setup(t)
			o.set("early")
			_, release, _ := o.snapshot()
			reader, writer := io.Pipe()
			var body io.Reader = reader
			if name == "replayable" {
				// The server answers only once the factory has opened the pipe: a
				// response that came first would leave the call nothing to close.
				var opened sync.Once
				body = NewByteStream(func() (io.ReadCloser, error) {
					opened.Do(func() { close(release) })
					return reader, nil
				}, -1)
			} else {
				// A one-shot pipe is the caller's own closer, closed whether or not it was read.
				close(release)
			}
			done := make(chan error, 1)
			go func() {
				_, err := client.Upload(context.Background(), &UploadInput{Body: body})
				done <- err
			}()
			select {
			case err := <-done:
				if err == nil {
					t.Fatal("rejected upload succeeded")
				}
			case <-time.After(10 * time.Second):
				t.Fatal("the early response did not end the call")
			}
			written := make(chan error, 1)
			go func() { _, err := writer.Write([]byte("late")); written <- err }()
			select {
			case err := <-written:
				if !errors.Is(err, io.ErrClosedPipe) {
					t.Fatal("rejected upload left the caller's stream open", err)
				}
			case <-time.After(10 * time.Second):
				t.Fatal("rejected upload left the caller's stream open: nothing closed or read it")
			}
		})
	}
}

func TestStreamingDownloads(t *testing.T) {
	o, client, config := setup(t)
	ctx := context.Background()

	// The response is returned at its headers; the body is read afterwards.
	o.set("ok")
	_, release, _ := o.snapshot()
	output, err := client.Download(ctx, Unit{})
	if err != nil || output.GetCount() != 3 {
		t.Fatal("headers", err)
	}
	close(release)
	data, err := io.ReadAll(output.Body)
	if err != nil || string(data) != "abc" {
		t.Fatal("download body", string(data), err)
	}
	output.Body.Close()

	// Closing an unread response, or closing mid-read, releases the connection.
	o.set("flood")
	_, _, aborted := o.snapshot()
	output, err = client.Download(ctx, Unit{})
	if err != nil {
		t.Fatal(err)
	}
	output.Body.Close()
	waitFor(t, aborted, "unread response was not released")
	o.set("flood")
	_, _, aborted = o.snapshot()
	output, _ = client.Download(ctx, Unit{})
	if n, err := output.Body.Read(make([]byte, 16)); n == 0 || err != nil {
		t.Fatal("partial read", n, err)
	}
	output.Body.Close()
	waitFor(t, aborted, "closed response was not released")
	if _, err := output.Body.Read(make([]byte, 16)); err == nil {
		t.Fatal("read after close")
	}

	// Closing from another goroutine ends a blocked read.
	o.set("hang")
	output, _ = client.Download(ctx, Unit{})
	failed := make(chan error, 1)
	go func() { _, err := output.Body.Read(make([]byte, 1)); failed <- err }()
	time.Sleep(200 * time.Millisecond)
	output.Body.Close()
	select {
	case err := <-failed:
		if err == nil {
			t.Fatal("blocked read succeeded")
		}
	case <-time.After(10 * time.Second):
		t.Fatal("close did not end a blocked read")
	}

	// The call's deadline still bounds reading after the operation returned.
	o.set("hang")
	timed := config
	timed.Timeout = 500 * time.Millisecond
	short, _ := NewStreamingClient(timed)
	output, err = short.Download(ctx, Unit{})
	if err != nil {
		t.Fatal(err)
	}
	started := time.Now()
	if _, err := io.ReadAll(output.Body); !errors.Is(err, context.DeadlineExceeded) || time.Since(started) > 5*time.Second {
		t.Fatal("deadline after return", err)
	}

	// Cancelling the caller's context ends reading too.
	o.set("hang")
	cancelled, cancel := context.WithCancel(ctx)
	output, _ = client.Download(cancelled, Unit{})
	time.AfterFunc(100*time.Millisecond, cancel)
	if _, err := io.ReadAll(output.Body); !errors.Is(err, context.Canceled) {
		t.Fatal("cancelled read", err)
	}

	// Invalid response headers release the unread body.
	o.set("invalid")
	_, _, aborted = o.snapshot()
	if _, err := client.Download(ctx, Unit{}); err == nil {
		t.Fatal("invalid header accepted")
	}
	waitFor(t, aborted, "invalid response body was not released")
}

func TestStreamingChecksumsAndDigests(t *testing.T) {
	o, client, config := setup(t)
	ctx := context.Background()

	// A streaming response checksum is verified at its end.
	validated := []string{}
	config.ChecksumValidated = func(operation, algorithm string) { validated = append(validated, operation+":"+algorithm) }
	checked, _ := NewStreamingClient(config)
	fetched, err := checked.Fetch(ctx, &FetchInput{})
	if err != nil || len(validated) != 0 {
		t.Fatal("validated before reading", err, validated)
	}
	if data, err := io.ReadAll(fetched.Body); err != nil || string(data) != "123456789" || strings.Join(validated, ",") != "Fetch:CRC32" {
		t.Fatal("streaming checksum", string(data), err, validated)
	}
	o.set("bad")
	fetched, _ = checked.Fetch(ctx, &FetchInput{})
	var mismatch *ChecksumError
	if _, err := io.ReadAll(fetched.Body); !errors.As(err, &mismatch) || mismatch.Algorithm != "CRC32" {
		t.Fatal("streaming checksum mismatch", err)
	}
	if _, err := fetched.Body.Read(make([]byte, 1)); !errors.As(err, &mismatch) {
		t.Fatal("mismatch is sticky", err)
	}

	// Request checksums are never computed by buffering a stream.
	o.set("ok")
	if _, err := client.Store(ctx, &StoreInput{Body: strings.NewReader("abc")}); err != nil {
		t.Fatal(err)
	}
	if headers, _, _, _, _ := o.last(); headers.Get("x-algorithm") != "" || headers.Get("x-amz-checksum-crc32") != "" {
		t.Fatal("default checksum for a stream", headers)
	}
	selected := &StoreInput{Body: strings.NewReader("abc")}
	selected.SetAlgorithm(AlgorithmCRC32)
	if _, err := client.Store(ctx, selected); !errors.Is(err, ErrStreamDigest) {
		t.Fatal("selected checksum computed from a stream", err)
	}
	selected = &StoreInput{Body: strings.NewReader("123456789")}
	selected.SetAlgorithm(AlgorithmCRC32)
	selected.SetChecksum("y/Q5Jg==")
	if _, err := client.Store(ctx, selected); err != nil {
		t.Fatal(err)
	}
	if headers, _, _, _, _ := o.last(); headers.Get("x-amz-checksum-crc32") != "y/Q5Jg==" || headers.Get("x-algorithm") != "CRC32" {
		t.Fatal("supplied streaming checksum", headers)
	}

	// A content digest is declared, never computed from a stream.
	digest := "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
	base, err := newClient(config, wireSchema, true)
	if err != nil {
		t.Fatal(err)
	}
	digesting := &StreamingClient{clientRuntime: base}
	opens := 0
	unsized := NewByteStream(func() (io.ReadCloser, error) { opens++; return io.NopCloser(strings.NewReader("abc")), nil }, -1)
	if _, err := digesting.Upload(ctx, &UploadInput{Body: unsized}); !errors.Is(err, ErrStreamDigest) || opens != 0 {
		t.Fatal("undeclared digest", err, opens)
	}
	if _, err := digesting.Upload(ctx, &UploadInput{Body: unsized.WithSHA256(strings.ToUpper(digest))}); err != nil {
		t.Fatal(err)
	}
	if headers, _, chunked, received, _ := o.last(); opens != 1 || headers.Get("x-amz-content-sha256") != digest || received != "abc" || !chunked {
		t.Fatal("declared digest", opens, headers, received)
	}
	if _, err := digesting.Upload(ctx, &UploadInput{Body: unsized.WithSHA256("not hex")}); err == nil {
		t.Fatal("invalid declared digest accepted")
	}

	// SigV4 signs a stream with its declared digest, or as UNSIGNED-PAYLOAD.
	identity := AWSCredentials{AccessKeyID: "AKIDEXAMPLE", SecretAccessKey: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY"}
	options := SigningOptions{Service: "service", Region: "us-east-1", Time: time.Date(2015, 8, 30, 12, 36, 0, 0, time.UTC)}
	signed := func(header string, unsigned bool) (*http.Request, *streamBody, error) {
		source, _ := newStreamSource(plain{strings.NewReader("abc")})
		body := newStreamBody(source, memberDescription{})
		request, _ := http.NewRequest("PUT", "https://example.amazonaws.com/upload", nil)
		body.install(request)
		if header != "" {
			request.Header.Set("X-Amz-Content-Sha256", header)
		}
		options := options
		options.UnsignedPayload = unsigned
		return request, body, SignV4(request, identity, options)
	}
	if _, body, err := signed("", false); !errors.Is(err, ErrStreamDigest) || body.opened.Load() != nil {
		t.Fatal("SigV4 read an undeclared stream", err)
	}
	request, body, err := signed(digest, false)
	if err != nil || request.Header.Get("X-Amz-Content-Sha256") != digest || !strings.Contains(request.Header.Get("Authorization"), "x-amz-content-sha256") || body.opened.Load() != nil {
		t.Fatal("SigV4 streaming digest", err, request.Header)
	}
	request, _, err = signed("", true)
	if err != nil || request.Header.Get("X-Amz-Content-Sha256") != "UNSIGNED-PAYLOAD" {
		t.Fatal("SigV4 unsigned stream", err)
	}
}

func TestIncrementalChecksumVectors(t *testing.T) {
	data, err := os.ReadFile("checksums.json")
	if err != nil {
		t.Fatal(err)
	}
	var vectors struct {
		InputUtf8 string
		Base64    map[string]string
	}
	if err := json.Unmarshal(data, &vectors); err != nil {
		t.Fatal(err)
	}
	for algorithm, expected := range vectors.Base64 {
		digest, err := newChecksumHash(algorithm)
		if err != nil {
			t.Fatal(err)
		}
		for i := range len(vectors.InputUtf8) {
			digest.Write([]byte{vectors.InputUtf8[i]})
		}
		whole, _ := checksumDigest(algorithm, []byte(vectors.InputUtf8))
		if actual, _ := checksumDigestOf(digest); actual != expected || whole != expected {
			t.Fatal("incremental", algorithm, actual, expected)
		}
	}
}

func checksumDigestOf(digest interface{ Sum([]byte) []byte }) (string, error) {
	return base64.StdEncoding.EncodeToString(digest.Sum(nil)), nil
}

// TestLargeStreamsBoundedMemory moves 256 MiB each way over loopback HTTP and
// samples the heap throughout: no body is ever held in memory.
func TestLargeStreamsBoundedMemory(t *testing.T) {
	o, client, _ := setup(t)
	ctx := context.Background()
	runtime.GC()
	var peak atomic.Uint64
	stop := make(chan struct{})
	sampled := make(chan struct{})
	go func() {
		defer close(sampled)
		var stats runtime.MemStats
		for {
			runtime.ReadMemStats(&stats)
			if stats.HeapInuse > peak.Load() {
				peak.Store(stats.HeapInuse)
			}
			select {
			case <-stop:
				return
			case <-time.After(10 * time.Millisecond):
			}
		}
	}()
	started := time.Now()
	o.set("ok")
	if _, err := client.Upload(ctx, &UploadInput{Body: &generated{left: large}}); err != nil {
		t.Fatal(err)
	}
	if _, _, chunked, _, size := o.last(); size != large || !chunked {
		t.Fatal("chunked upload of unknown length", size, chunked)
	}
	sized := NewByteStream(func() (io.ReadCloser, error) { return io.NopCloser(&generated{left: large}), nil }, large)
	if _, err := client.Upload(ctx, &UploadInput{Body: sized}); err != nil {
		t.Fatal(err)
	}
	if _, length, chunked, _, size := o.last(); size != large || length != large || chunked {
		t.Fatal("sized upload", size, length, chunked)
	}
	o.set("large")
	output, err := client.Download(ctx, Unit{})
	if err != nil {
		t.Fatal(err)
	}
	copied, err := io.Copy(io.Discard, output.Body)
	output.Body.Close()
	if err != nil || copied != large {
		t.Fatal("large download", copied, err)
	}
	close(stop)
	<-sampled
	if peak.Load() > 64<<20 {
		t.Fatalf("heap in use peaked at %d bytes for %d bytes each way", peak.Load(), large)
	}
	t.Logf("moved %d MiB each way in %s; peak heap in use %d KiB", large>>20, time.Since(started), peak.Load()>>10)
}
