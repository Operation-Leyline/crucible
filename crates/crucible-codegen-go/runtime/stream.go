package sdk

import (
	"bufio"
	"bytes"
	"context"
	"errors"
	"io"
	"mime"
	"net/http"
	"strings"
	"sync"
	"unicode/utf8"
)

// StreamProtocolError reports an invalid or interrupted response-stream profile.
// It deliberately excludes response data from its message.
type StreamProtocolError struct{ Message string }

func (e *StreamProtocolError) Error() string { return "response stream: " + e.Message }
func streamProtocol(message string) error    { return &StreamProtocolError{Message: message} }

func validateStreamProfile(value string) error {
	media, params, err := mime.ParseMediaType(value)
	if err != nil || media != "text/event-stream" || params["crucible-stream"] != "1" {
		return streamProtocol("unsupported media type or profile")
	}
	if charset := params["charset"]; charset != "" && !strings.EqualFold(charset, "utf-8") {
		return streamProtocol("unsupported charset")
	}
	return nil
}

type streamOpening struct {
	response *http.Response
	ctx      context.Context
	cancel   context.CancelFunc
}

// ResponseStream owns a single HTTP response. Iterate with Next, Value and Err,
// and always defer Close, including when abandoning it before the first Next.
// Next is single-consumer; Close may be called concurrently to interrupt a read.
type ResponseStream[T any] struct {
	Metadata      ResponseMetadata
	client        *clientRuntime
	op            operationDescription
	ctx           context.Context
	cancel        context.CancelFunc
	response      *http.Response
	reader        *bufio.Reader
	trace         *callTrace
	once          sync.Once
	mu            sync.Mutex
	reading       bool
	closed        bool
	err           error
	value         T
	line          []byte
	data          []byte
	event         string
	frameBytes    int
	skipLF        bool
	crFrameBytes  int
	first         bool
	terminal      bool
	terminalError error
}

func openResponseStream[T any](c *clientRuntime, ctx context.Context, name string, input any) (*ResponseStream[T], error) {
	if c.schema.Operations[name].ResponseMode != "server-stream" {
		return nil, errors.New("operation is not a response stream")
	}
	trace := c.startTrace(name)
	values, err := encodeModel(c.schema, c.schema.Operations[name].Input, input)
	opening := &streamOpening{}
	if err == nil {
		_, err = c.sendMode(ctx, name, values, trace, opening)
	}
	if err != nil {
		trace.finished(err)
		return nil, err
	}
	response := opening.response
	stream := &ResponseStream[T]{client: c, op: c.schema.Operations[name], ctx: opening.ctx, cancel: opening.cancel, response: response,
		reader: bufio.NewReaderSize(response.Body, 4096), trace: trace, first: true,
		Metadata: ResponseMetadata{Status: response.StatusCode, RequestID: response.Header.Get("x-request-id"), Headers: response.Header.Clone()}}
	context.AfterFunc(stream.ctx, func() { stream.finish(stream.ctx.Err()) })
	return stream, nil
}

func (s *ResponseStream[T]) finish(err error) {
	s.once.Do(func() {
		s.mu.Lock()
		s.closed = true
		s.err = err
		s.mu.Unlock()
		s.cancel()
		s.response.Body.Close()
		s.trace.finished(err)
	})
}

// Close stops consumption and releases this response without closing the HTTP client.
func (s *ResponseStream[T]) Close() error { s.finish(context.Canceled); return nil }

// Value is the last value returned by a successful Next.
func (s *ResponseStream[T]) Value() T { s.mu.Lock(); defer s.mu.Unlock(); return s.value }

// Err returns the terminal failure, or nil following a complete profile and EOF.
func (s *ResponseStream[T]) Err() error { s.mu.Lock(); defer s.mu.Unlock(); return s.err }

// Next advances one value. Cancellation and deadlines take precedence over
// buffered data, including the final check after synchronous model decoding.
func (s *ResponseStream[T]) Next() bool {
	s.mu.Lock()
	if s.closed {
		s.mu.Unlock()
		return false
	}
	if s.reading {
		s.mu.Unlock()
		s.finish(streamProtocol("concurrent reads"))
		return false
	}
	s.reading = true
	s.mu.Unlock()
	defer func() { s.mu.Lock(); s.reading = false; s.mu.Unlock() }()
	for {
		if err := s.ctx.Err(); err != nil {
			s.finish(err)
			return false
		}
		event, data, err := s.frame()
		if err != nil {
			if e := s.ctx.Err(); e != nil {
				err = e
			} else if err == io.EOF {
				if s.terminal {
					err = s.terminalError
				} else {
					err = streamProtocol("EOF without terminal")
				}
			} else {
				var protocol *StreamProtocolError
				if !errors.As(err, &protocol) {
					err = &NetworkError{Cause: err}
				}
			}
			s.finish(err)
			return false
		}
		raw, err := parseJSON(data)
		if err != nil {
			s.finish(streamProtocol("malformed JSON"))
			return false
		}
		switch event {
		case "", "message":
			var value T
			object, ok := raw.(map[string]any)
			if !ok || decodeModel(s.client.schema, s.op.Output, object, &value) != nil {
				s.finish(streamProtocol("invalid item"))
				return false
			}
			s.mu.Lock()
			err = s.ctx.Err()
			if err == nil && !s.closed {
				s.value = value
				s.mu.Unlock()
				return true
			}
			s.mu.Unlock()
			if err != nil {
				s.finish(err)
			}
			return false
		case "crucible.complete":
			object, ok := raw.(map[string]any)
			if !ok || len(object) != 0 {
				s.finish(streamProtocol("invalid completion"))
				return false
			}
			s.terminal = true
		case "crucible.error":
			late, err := s.decodeLateError(raw, data)
			if err != nil {
				s.finish(err)
				return false
			}
			s.terminal, s.terminalError = true, late
		default:
			s.finish(streamProtocol("unknown event"))
			return false
		}
	}
}

// frame reads on demand, holding at most one bounded frame and bufio's fixed
// transport buffer. A CR ends a line immediately; a subsequent LF is swallowed.
func (s *ResponseStream[T]) frame() (string, []byte, error) {
	for {
		if err := s.ctx.Err(); err != nil {
			return "", nil, err
		}
		b, err := s.reader.ReadByte()
		if err != nil {
			if err == io.EOF && (len(s.line) != 0 || s.frameBytes != 0) {
				return "", nil, streamProtocol("incomplete frame")
			}
			return "", nil, err
		}
		if s.skipLF {
			s.skipLF = false
			if b == '\n' {
				// Charge the LF to the CR's physical frame, even when a
				// blank CR has already reset frameBytes for the next frame.
				if s.crFrameBytes+1 > s.client.config.MaxStreamEventBytes {
					return "", nil, streamProtocol("frame exceeds byte limit")
				}
				if s.frameBytes != 0 {
					s.frameBytes++
				}
				continue
			}
		}
		if s.terminal {
			return "", nil, streamProtocol("bytes after terminal")
		}
		s.frameBytes++
		if s.frameBytes > s.client.config.MaxStreamEventBytes {
			return "", nil, streamProtocol("frame exceeds byte limit")
		}
		if b != '\n' && b != '\r' {
			s.line = append(s.line, b)
			continue
		}
		s.skipLF = b == '\r'
		s.crFrameBytes = s.frameBytes
		line := s.line
		s.line = nil
		if !utf8.Valid(line) {
			return "", nil, streamProtocol("invalid UTF-8")
		}
		if s.first {
			s.first = false
			line = bytes.TrimPrefix(line, []byte{0xef, 0xbb, 0xbf})
		}
		if len(line) == 0 {
			event, data := s.event, s.data
			s.event, s.data, s.frameBytes = "", nil, 0
			if data != nil {
				return event, bytes.TrimSuffix(data, []byte{'\n'}), nil
			}
			if event != "" && event != "message" {
				return "", nil, streamProtocol("control without data")
			}
			continue
		}
		if line[0] == ':' {
			continue
		}
		field, value, _ := bytes.Cut(line, []byte{':'})
		value = bytes.TrimPrefix(value, []byte{' '})
		switch string(field) {
		case "event":
			s.event = string(value)
		case "data":
			s.data = append(s.data, value...)
			s.data = append(s.data, '\n')
		}
	}
}

func (s *ResponseStream[T]) decodeLateError(raw any, data []byte) (error, error) {
	envelope, ok := raw.(map[string]any)
	if !ok {
		return nil, streamProtocol("invalid error envelope")
	}
	name, ok := envelope["type"].(string)
	if !ok || name == "" {
		return nil, streamProtocol("invalid error type")
	}
	status, ok := envelope["status"].(interface{ Int64() (int64, error) })
	if !ok {
		return nil, streamProtocol("invalid error status")
	}
	code, err := status.Int64()
	if err != nil || code < 400 || code > 599 {
		return nil, streamProtocol("invalid error status")
	}
	fields, ok := envelope["error"].(map[string]any)
	if !ok {
		return nil, streamProtocol("invalid error fields")
	}
	message, _ := fields["message"].(string)
	metadata := &ServiceError{Name: name, Message: message, Metadata: ResponseMetadata{Status: int(code), RequestID: s.Metadata.RequestID, Headers: s.Metadata.Headers.Clone(), Body: bytes.Clone(data)}}
	for _, target := range s.op.Errors {
		shape := s.client.schema.Shapes[target]
		if shape.errorCode() != name {
			continue
		}
		metadata.Retryable, metadata.Throttling = shape.Error.Retryable, shape.Error.Throttling
		result := modeledError(target, fields, metadata)
		if result == metadata {
			return nil, streamProtocol("invalid modeled error")
		}
		return result, nil
	}
	return metadata, nil
}
