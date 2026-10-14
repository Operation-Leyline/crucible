package sdk

import (
    "context"
    "crypto/md5"
    "crypto/sha1"
    "crypto/sha256"
    "crypto/sha512"
    "crypto/subtle"
    "encoding/base64"
    "encoding/hex"
    "errors"
    "fmt"
    "hash"
    "hash/crc32"
    "hash/crc64"
    "io"
    "net/http"
    "strings"
    "sync"
    "sync/atomic"
)

// ErrStreamLength reports a request stream whose bytes break its declared
// length, its modeled @length bounds, or a @requiresLength member.
var ErrStreamLength = errors.New("stream length violates its declaration")

// ErrStreamConsumed reports a one-shot ByteStream passed to a second call.
var ErrStreamConsumed = errors.New("one-shot stream was already opened by an earlier call")

// ErrStreamDigest reports a stream that must be hashed before sending. Streams
// are never buffered to compute a digest or checksum: declare it instead.
var ErrStreamDigest = errors.New("streaming payload requires a precomputed digest")

var errStreamClosed = errors.New("read on closed response stream")

// ByteStream is a streaming blob request body with an explicit replay rule,
// length, and optional declared SHA-256. A plain io.Reader works too: an
// io.ReadSeeker replays from its starting position on every attempt, and any
// other reader is one-shot.
type ByteStream struct {
    open   func() (io.ReadCloser, error)
    reader io.Reader
    length int64
    sha256 string
    used   *atomic.Bool
    direct io.ReadCloser
}

// NewByteStream returns a replayable stream: open is called once per attempt,
// like http.Request.GetBody, and must yield the same bytes each time. A
// negative length means unknown, so the body is sent chunked.
func NewByteStream(open func() (io.ReadCloser, error), length int64) *ByteStream {
    return &ByteStream{open: open, length: max(length, -1), used: new(atomic.Bool)}
}

// NewReaderStream returns a one-shot stream over reader, which is read at most
// once and never retried. A negative length means unknown.
func NewReaderStream(reader io.Reader, length int64) *ByteStream {
    return &ByteStream{reader: reader, length: max(length, -1), used: new(atomic.Bool)}
}

// WithSHA256 declares the hex SHA-256 of the stream's bytes, sent as
// x-amz-content-sha256 and used to sign it, so the stream is never hashed.
func (s *ByteStream) WithSHA256(digest string) *ByteStream {
    copied := *s
    copied.sha256 = strings.ToLower(digest)
    copied.direct = nil
    return &copied
}

// Length returns the declared length, or -1 when it is unknown.
func (s *ByteStream) Length() int64 { return s.length }

// Replayable reports whether the stream can be reopened for a retry.
func (s *ByteStream) Replayable() bool { return s.open != nil }

// Read reads the stream directly, opening it on first use; a one-shot stream
// read this way can no longer be sent.
func (s *ByteStream) Read(p []byte) (int, error) {
    if s.direct == nil {
        reader, err := s.acquire()
        if err != nil { return 0, err }
        s.direct = reader
    }
    return s.direct.Read(p)
}

// Close closes a reader opened by Read, and a one-shot reader that is an io.Closer.
func (s *ByteStream) Close() error {
    if s.direct != nil { return s.direct.Close() }
    if closer, ok := s.reader.(io.Closer); ok && s.open == nil { return closer.Close() }
    return nil
}

func (s *ByteStream) acquire() (io.ReadCloser, error) {
    if s.open != nil {
        reader, err := s.open()
        if err == nil && reader == nil { err = errors.New("stream factory returned no reader") }
        return reader, err
    }
    if s.used.Swap(true) { return nil, ErrStreamConsumed }
    if closer, ok := s.reader.(io.ReadCloser); ok { return closer, nil }
    return io.NopCloser(s.reader), nil
}

// streamSource resolves a caller's value into per-attempt bodies.
type streamSource struct {
    length     int64
    replayable bool
    sha256     string
    open       func() (io.ReadCloser, error)
    oneShot    io.Closer
    shared     bool
    closeOnce  sync.Once
}

func newStreamSource(value any) (*streamSource, error) {
    switch v := value.(type) {
    case nil:
        return nil, nil
    case *ByteStream:
        if v == nil { return nil, nil }
        if v.sha256 != "" {
            if decoded, err := hex.DecodeString(v.sha256); err != nil || len(decoded) != sha256.Size { return nil, errors.New("declared SHA-256 must be 64 hexadecimal characters") }
        }
        source := &streamSource{length: v.length, replayable: v.open != nil, sha256: v.sha256, open: v.acquire}
        if v.open == nil {
            if v.used.Load() { return nil, ErrStreamConsumed }
            source.oneShot, _ = v.reader.(io.Closer)
        }
        return source, nil
    case io.ReadSeeker:
        if start, err := v.Seek(0, io.SeekCurrent); err == nil {
            if end, err := v.Seek(0, io.SeekEnd); err == nil {
                if _, err := v.Seek(start, io.SeekStart); err != nil { return nil, err }
                // The caller owns a seekable reader: it is rewound per attempt, never closed.
                return &streamSource{length: end - start, replayable: true, shared: true, open: func() (io.ReadCloser, error) {
                    if _, err := v.Seek(start, io.SeekStart); err != nil { return nil, err }
                    return io.NopCloser(v), nil
                }}, nil
            }
        }
        // A seeker that cannot seek (a pipe behind *os.File) is one-shot.
        return oneShotSource(v), nil
    case io.Reader:
        return oneShotSource(v), nil
    default:
        return nil, fmt.Errorf("a streaming blob requires an io.Reader, not %T", value)
    }
}

func oneShotSource(reader io.Reader) *streamSource {
    length := int64(-1)
    if sized, ok := reader.(interface{ Len() int }); ok { length = int64(sized.Len()) }
    used := false
    source := &streamSource{length: length}
    source.oneShot, _ = reader.(io.Closer)
    source.open = func() (io.ReadCloser, error) {
        if used { return nil, ErrStreamConsumed }
        used = true
        if closer, ok := reader.(io.ReadCloser); ok { return closer, nil }
        return io.NopCloser(reader), nil
    }
    return source
}

// release closes a one-shot reader at the end of a call, as net/http closes a
// request body, which also ends a read blocked in it.
func (s *streamSource) release() {
    if s != nil && s.oneShot != nil { s.closeOnce.Do(func() { s.oneShot.Close() }) }
}

func (s *streamSource) check(member memberDescription, requiresLength bool) error {
    if requiresLength && s.length < 0 { return fmt.Errorf("%w: the operation requires a known length", ErrStreamLength) }
    if s.length >= 0 && member.Length != nil {
        if member.Length.Min != nil && s.length < *member.Length.Min { return fmt.Errorf("%w: %d bytes is below the modeled minimum %d", ErrStreamLength, s.length, *member.Length.Min) }
        if member.Length.Max != nil && s.length > *member.Length.Max { return fmt.Errorf("%w: %d bytes is above the modeled maximum %d", ErrStreamLength, s.length, *member.Length.Max) }
    }
    return nil
}

// streamBody is one attempt's request body. It opens its source on first read,
// counts bytes against the declared and modeled lengths, and records the first
// failure so a broken stream surfaces as its own error, never a retried NetworkError.
type streamBody struct {
    source   *streamSource
    min, max int64
    mu       sync.Mutex
    opened   atomic.Pointer[io.ReadCloser]
    count    int64
    closed   atomic.Bool
    failure  atomic.Pointer[error]
}

func newStreamBody(source *streamSource, member memberDescription) *streamBody {
    body := &streamBody{source: source, min: -1, max: -1}
    if member.Length != nil {
        if member.Length.Min != nil { body.min = *member.Length.Min }
        if member.Length.Max != nil { body.max = *member.Length.Max }
    }
    return body
}

func (b *streamBody) fail(err error) error {
    b.failure.CompareAndSwap(nil, &err)
    return *b.failure.Load()
}

func (b *streamBody) err() error {
    if value := b.failure.Load(); value != nil { return *value }
    return nil
}

func (b *streamBody) Read(p []byte) (int, error) {
    b.mu.Lock()
    defer b.mu.Unlock()
    if err := b.err(); err != nil { return 0, err }
    if b.closed.Load() { return 0, errors.New("request stream closed") }
    reader := b.opened.Load()
    if reader == nil {
        opened, err := b.source.open()
        if err != nil { return 0, b.fail(err) }
        reader = &opened
        b.opened.Store(reader)
        // Close may have run while the source was opening.
        if b.closed.Load() { opened.Close(); return 0, errors.New("request stream closed") }
    }
    n, err := (*reader).Read(p)
    b.count += int64(n)
    length := b.source.length
    if length >= 0 && b.count > length { return 0, b.fail(fmt.Errorf("%w: the stream produced more than its declared %d bytes", ErrStreamLength, length)) }
    if b.max >= 0 && b.count > b.max { return 0, b.fail(fmt.Errorf("%w: the stream exceeded the modeled maximum %d bytes", ErrStreamLength, b.max)) }
    if err == io.EOF {
        if length >= 0 && b.count != length { return n, b.fail(fmt.Errorf("%w: the stream ended after %d of its declared %d bytes", ErrStreamLength, b.count, length)) }
        if b.min >= 0 && b.count < b.min { return n, b.fail(fmt.Errorf("%w: the stream ended below the modeled minimum %d bytes", ErrStreamLength, b.min)) }
        return n, io.EOF
    }
    if err != nil { return n, b.fail(err) }
    return n, nil
}

// Close may run while a transport goroutine is blocked in Read, so it closes
// the opened reader without waiting, which ends that read. A rewound seeker is
// shared between attempts and the caller's, so every Close, not only the first
// of racing ones, waits for an in-flight read: none returns while one runs.
func (b *streamBody) Close() error {
    if b.source.shared { b.mu.Lock(); defer b.mu.Unlock() }
    if b.closed.Swap(true) { return nil }
    if b.source.oneShot != nil { b.source.release(); return nil }
    if reader := b.opened.Load(); reader != nil { return (*reader).Close() }
    return nil
}

// install makes body the request's body with the declared length.
func (b *streamBody) install(request *http.Request) {
    request.GetBody = nil
    if b.source.length == 0 {
        request.Body, request.ContentLength = http.NoBody, 0
        b.closed.Store(true)
        return
    }
    request.Body, request.ContentLength = b, b.source.length
}

func streamDigestHeader(request *http.Request) error {
    if _, ok := request.Body.(*streamBody); ok && request.Header.Get("X-Amz-Content-Sha256") == "" {
        return fmt.Errorf("%w: declare it with ByteStream.WithSHA256 or set x-amz-content-sha256", ErrStreamDigest)
    }
    return nil
}

func newChecksumHash(algorithm string) (hash.Hash, error) {
    switch algorithm {
    case "CRC32": return crc32.NewIEEE(), nil
    case "CRC32C": return crc32.New(crc32cTable), nil
    case "CRC64NVME": return crc64.New(crc64nvmeTable), nil
    case "MD5": return md5.New(), nil
    case "SHA1": return sha1.New(), nil
    case "SHA256": return sha256.New(), nil
    case "SHA512": return sha512.New(), nil
    }
    return nil, fmt.Errorf("unsupported checksum algorithm: %s", algorithm)
}

// responseStream is a streaming blob response body. It is returned at the
// headers; the call's deadline and context keep governing every read, and a
// response checksum is verified incrementally, failing the final Read.
type responseStream struct {
    body      io.ReadCloser
    ctx       context.Context
    releases  []func()
    hash      hash.Hash
    algorithm string
    expected  string
    validated func()
    mu        sync.Mutex
    done      bool
    closed    atomic.Bool
    failure   error
    once      sync.Once
}

func (s *responseStream) release() { s.once.Do(func() { for _, release := range s.releases { release() } }) }

func (s *responseStream) Read(p []byte) (int, error) {
    if s.closed.Load() { return 0, errStreamClosed }
    s.mu.Lock()
    defer s.mu.Unlock()
    if s.failure != nil { return 0, s.failure }
    if s.done { return 0, io.EOF }
    n, err := s.body.Read(p)
    if s.hash != nil && n > 0 { s.hash.Write(p[:n]) }
    if err == io.EOF {
        s.done = true
        if s.hash != nil {
            actual := base64.StdEncoding.EncodeToString(s.hash.Sum(nil))
            if subtle.ConstantTimeCompare([]byte(actual), []byte(s.expected)) != 1 {
                s.failure = &ChecksumError{Algorithm: s.algorithm}
                s.release()
                return n, s.failure
            }
            if s.validated != nil { s.validated() }
        }
        s.release()
        return n, io.EOF
    }
    if err != nil {
        if s.closed.Load() { err = errStreamClosed } else if cause := s.ctx.Err(); cause != nil { err = cause }
        s.failure = err
        s.release()
    }
    return n, err
}

// Close releases the connection and the call's deadline; it may be called
// from another goroutine to end a blocked Read.
func (s *responseStream) Close() error {
    if s.closed.Swap(true) { return nil }
    s.release()
    return s.body.Close()
}

func (c *clientRuntime) streamResponse(ctx context.Context, release func(), name string, op operationDescription, response *http.Response, checksum checksumSettings) (map[string]any, error) {
    values := map[string]any{}
    if err := c.responseBindings(op.Output, op.Response, response, values); err != nil { return nil, err }
    stream := &responseStream{body: response.Body, ctx: ctx, releases: []func(){release}}
    for _, algorithm := range checksum.Response {
        expected := response.Header.Get("X-Amz-Checksum-" + strings.ToLower(algorithm))
        if expected == "" { continue }
        digest, err := newChecksumHash(algorithm)
        if err != nil { return nil, err }
        stream.hash, stream.algorithm, stream.expected = digest, algorithm, expected
        if callback := c.config.ChecksumValidated; callback != nil { stream.validated = func() { callback(name, algorithm) } }
        break
    }
    values[c.schema.Shapes[op.Output].wireName(op.Response.Payload[0])] = stream
    return values, nil
}

// closeStreams releases every response stream in decoded wire values that
// will not reach the caller.
func closeStreams(values map[string]any) {
    for _, value := range values { if stream, ok := value.(*responseStream); ok { stream.Close() } }
}

// keepStreams hands release to the response streams in values, reporting
// whether any took it.
func keepStreams(values map[string]any, release func()) bool {
    kept := false
    for _, value := range values {
        if stream, ok := value.(*responseStream); ok { stream.releases = append(stream.releases, release); kept = true }
    }
    return kept
}
