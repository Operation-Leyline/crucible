package sdk

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"math/big"
	mathrand "math/rand/v2"
	"net"
	"net/http"
	"net/url"
	"reflect"
	"strconv"
	"strings"
	"time"
)

// Optional distinguishes absent members, explicit JSON null, and present values.
type Optional[T any] struct {
	Value T
	Set   bool
	Null  bool
}

func (Optional[T]) isOptional()       {}
func Some[T any](value T) Optional[T] { return Optional[T]{Value: value, Set: true} }
func Null[T any]() Optional[T]        { return Optional[T]{Set: true, Null: true} }

type optionalField interface{ isOptional() }
type Unit struct{}
type UnknownUnion struct {
	Name  string
	Value any
}

type ResponseMetadata struct {
	Status    int
	RequestID string
	Headers   http.Header
	Body      []byte
}
type ServiceError struct {
	Name       string
	Message    string
	Metadata   ResponseMetadata
	Retryable  bool
	Throttling bool
}

func (e *ServiceError) Error() string { return e.Name + ": " + e.Message }

type NetworkError struct{ Cause error }

func (e *NetworkError) Error() string { return "transport failed: " + e.Cause.Error() }
func (e *NetworkError) Unwrap() error { return e.Cause }

type HTTPClient interface {
	Do(*http.Request) (*http.Response, error)
}
type Config struct {
	Endpoint    string
	HTTPClient  HTTPClient
	Token       func(context.Context) (string, error)
	Headers     http.Header
	MaxAttempts int
	Timeout     time.Duration
	BaseDelay   time.Duration
	MaxDelay    time.Duration
	Middleware  []func(context.Context, *http.Request) error
}
type clientRuntime struct {
	config Config
	schema *schemaDescription
	digest bool
}
type memberDescription struct {
	Target    string
	Attribute string
	Format    string
	Required  bool
	Default   any
}
type errorDescription struct {
	Status     int
	Retryable  bool
	Throttling bool
	Headers    [][2]string
}
type shapeDescription struct {
	Name    string
	Kind    string
	Format  string
	Member  memberDescription
	Key     memberDescription
	Value   memberDescription
	Members map[string]memberDescription
	Error   *errorDescription
}
type bindings struct {
	Labels  []string
	Query   [][2]string
	Headers [][2]string
	Body    []string
	// Payload is the member that is the whole body, raw, and its media type.
	Payload []string
}
type operationDescription struct {
	Bearer   bool
	Method   string
	URI      string
	Input    string
	Output   string
	Request  bindings
	Response bindings
	Errors   []string
	Safe     bool
	Token    string
}
type schemaDescription struct {
	Shapes     map[string]shapeDescription
	Operations map[string]operationDescription
	Bearer     bool
}

func mustSchema(text string) *schemaDescription {
	var schema schemaDescription
	decoder := json.NewDecoder(strings.NewReader(text))
	decoder.UseNumber()
	if err := decoder.Decode(&schema); err != nil {
		panic(err)
	}
	return &schema
}
func newClient(config Config, schema *schemaDescription, digest bool) (*clientRuntime, error) {
	endpoint, err := url.Parse(config.Endpoint)
	if err != nil || endpoint.Host == "" || (endpoint.Scheme != "http" && endpoint.Scheme != "https") || endpoint.RawQuery != "" || endpoint.Fragment != "" || endpoint.User != nil {
		return nil, errors.New("an absolute HTTP(S) endpoint without credentials, query, or fragment is required")
	}
	config.Endpoint = strings.TrimRight(config.Endpoint, "/")
	if config.MaxAttempts == 0 {
		config.MaxAttempts = 3
	}
	if config.Timeout == 0 {
		config.Timeout = 30 * time.Second
	}
	if config.BaseDelay == 0 {
		config.BaseDelay = 100 * time.Millisecond
	}
	if config.MaxDelay == 0 {
		config.MaxDelay = 20 * time.Second
	}
	if config.MaxAttempts < 1 || config.Timeout < 0 || config.BaseDelay < 0 || config.MaxDelay < 0 {
		return nil, errors.New("attempts and timeout must be positive; delays must be nonnegative")
	}
	if config.HTTPClient == nil {
		config.HTTPClient = &http.Client{CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	}
	config.Headers = config.Headers.Clone()
	config.Middleware = append([]func(context.Context, *http.Request) error(nil), config.Middleware...)
	return &clientRuntime{config: config, schema: schema, digest: digest}, nil
}

func parseJSON(data []byte) (any, error) {
	var value any
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.UseNumber()
	if err := decoder.Decode(&value); err != nil {
		return nil, err
	}
	if err := decoder.Decode(new(any)); err != io.EOF {
		if err == nil {
			err = errors.New("multiple JSON values")
		}
		return nil, err
	}
	return value, nil
}
func unwrap(value reflect.Value) reflect.Value {
	for value.IsValid() && (value.Kind() == reflect.Pointer || value.Kind() == reflect.Interface) {
		if value.IsNil() {
			return reflect.Value{}
		}
		value = value.Elem()
	}
	return value
}
func encodeModel(schema *schemaDescription, target string, input any) (map[string]any, error) {
	if target == "" {
		return map[string]any{}, nil
	}
	value, err := encodeValue(schema, target, reflect.ValueOf(input), "")
	if err != nil {
		return nil, err
	}
	if value == nil {
		return map[string]any{}, nil
	}
	result, ok := value.(map[string]any)
	if !ok {
		return nil, errors.New("operation input must be a structure")
	}
	return result, nil
}
func encodeValue(schema *schemaDescription, target string, value reflect.Value, format string) (any, error) {
	shape := schema.Shapes[target]
	if shape.Kind == "unit" {
		return map[string]any{}, nil
	}
	value = unwrap(value)
	if !value.IsValid() {
		return nil, nil
	}
	switch shape.Kind {
	case "timestamp":
		if format == "" {
			format = shape.Format
		}
		t, ok := value.Interface().(time.Time)
		if !ok {
			return nil, errors.New("expected time.Time")
		}
		return writeTime(t, format), nil
	case "blob":
		// Raw bytes: a payload sends them as they are, and JSON writes a
		// []byte as standard, padded base64.
		if value.Kind() != reflect.Slice || value.Type().Elem().Kind() != reflect.Uint8 {
			return nil, errors.New("expected []byte")
		}
		data := value.Bytes()
		if data == nil {
			data = []byte{}
		}
		return data, nil
	case "structure", "union":
		result := map[string]any{}
		for wire, member := range shape.Members {
			field := value.FieldByName(member.Attribute)
			if field.IsValid() && field.CanInterface() {
				if _, optional := field.Interface().(optionalField); optional {
					if !field.FieldByName("Set").Bool() {
						continue
					}
					if field.FieldByName("Null").Bool() {
						result[wire] = nil
						continue
					}
					field = field.FieldByName("Value")
				}
				encoded, err := encodeValue(schema, member.Target, field, member.Format)
				if err != nil {
					return nil, fmt.Errorf("%s: %w", wire, err)
				}
				result[wire] = encoded
			}
		}
		if shape.Kind == "union" {
			unknown := value.FieldByName("UnknownVariant")
			if unknown.IsValid() && !unknown.IsNil() {
				arm := unknown.Interface().(*UnknownUnion)
				if _, known := shape.Members[arm.Name]; known {
					return nil, errors.New("unknown union name collides with a declared member")
				}
				result[arm.Name] = arm.Value
			}
			if len(result) != 1 {
				return nil, errors.New("a union must contain exactly one member")
			}
		}
		return result, nil
	case "list":
		result := make([]any, value.Len())
		for i := range result {
			if !unwrap(value.Index(i)).IsValid() && schema.Shapes[shape.Member.Target].Kind != "document" {
				return nil, errors.New("null element in non-sparse list")
			}
			var err error
			result[i], err = encodeValue(schema, shape.Member.Target, value.Index(i), shape.Member.Format)
			if err != nil {
				return nil, err
			}
		}
		return result, nil
	case "map":
		result := map[string]any{}
		iter := value.MapRange()
		for iter.Next() {
			if !unwrap(iter.Value()).IsValid() && schema.Shapes[shape.Value.Target].Kind != "document" {
				return nil, errors.New("null value in non-sparse map")
			}
			item, err := encodeValue(schema, shape.Value.Target, iter.Value(), shape.Value.Format)
			if err != nil {
				return nil, err
			}
			result[iter.Key().String()] = item
		}
		return result, nil
	default:
		return value.Interface(), nil
	}
}
func decodeModel(schema *schemaDescription, target string, value any, output any) error {
	if target == "" {
		return nil
	}
	return decodeValue(schema, target, value, reflect.ValueOf(output).Elem(), "")
}
func decodeValue(schema *schemaDescription, target string, value any, out reflect.Value, format string) error {
	shape := schema.Shapes[target]
	if value == nil {
		out.SetZero()
		return nil
	}
	for out.Kind() == reflect.Pointer {
		if out.IsNil() {
			out.Set(reflect.New(out.Type().Elem()))
		}
		out = out.Elem()
	}
	switch shape.Kind {
	case "unit":
		if object, ok := value.(map[string]any); !ok || len(object) != 0 {
			return errors.New("unit requires an empty object")
		}
		out.SetZero()
	case "timestamp":
		if format == "" {
			format = shape.Format
		}
		t, err := readTime(value, format)
		if err != nil {
			return err
		}
		out.Set(reflect.ValueOf(t))
	case "blob":
		switch v := value.(type) {
		case []byte:
			out.SetBytes(v)
		case string:
			data, err := base64.StdEncoding.DecodeString(v)
			if err != nil {
				return errors.New("a blob must be base64")
			}
			out.SetBytes(data)
		default:
			return errors.New("expected base64 text")
		}
	case "structure", "union":
		object, ok := value.(map[string]any)
		if !ok {
			return fmt.Errorf("%s requires an object", shape.Name)
		}
		if shape.Kind == "union" {
			if len(object) != 1 {
				return errors.New("a union must contain exactly one member")
			}
			for key, v := range object {
				if _, known := shape.Members[key]; !known {
					out.FieldByName("UnknownVariant").Set(reflect.ValueOf(&UnknownUnion{Name: key, Value: v}))
					return nil
				}
			}
		}
		for wire, member := range shape.Members {
			item, present := object[wire]
			if !present && member.Default != nil {
				item = member.Default
				present = true
			}
			field := out.FieldByName(member.Attribute)
			_, optional := field.Interface().(optionalField)
			// Generated Optional fields already encode the IR's client nullability.
			if !optional && (!present || (item == nil && schema.Shapes[member.Target].Kind != "document")) {
				return fmt.Errorf("%s: missing or null non-optional member", wire)
			}
			if !present {
				continue
			}
			if optional {
				field.FieldByName("Set").SetBool(true)
				field.FieldByName("Null").SetBool(item == nil)
				field = field.FieldByName("Value")
			}
			if err := decodeValue(schema, member.Target, item, field, member.Format); err != nil {
				return fmt.Errorf("%s: %w", wire, err)
			}
		}
	case "list":
		items, ok := value.([]any)
		if !ok {
			return errors.New("expected array")
		}
		out.Set(reflect.MakeSlice(out.Type(), len(items), len(items)))
		for i, item := range items {
			if item == nil && schema.Shapes[shape.Member.Target].Kind != "document" {
				return errors.New("null element in non-sparse list")
			}
			if err := decodeValue(schema, shape.Member.Target, item, out.Index(i), shape.Member.Format); err != nil {
				return err
			}
		}
	case "map":
		items, ok := value.(map[string]any)
		if !ok {
			return errors.New("expected object")
		}
		out.Set(reflect.MakeMap(out.Type()))
		for key, item := range items {
			if item == nil && schema.Shapes[shape.Value.Target].Kind != "document" {
				return errors.New("null value in non-sparse map")
			}
			k := reflect.New(out.Type().Key()).Elem()
			k.SetString(key)
			v := reflect.New(out.Type().Elem()).Elem()
			if err := decodeValue(schema, shape.Value.Target, item, v, shape.Value.Format); err != nil {
				return err
			}
			out.SetMapIndex(k, v)
		}
	case "string", "enum":
		text, ok := value.(string)
		if !ok {
			return errors.New("expected string")
		}
		out.SetString(text)
	case "boolean":
		b, ok := value.(bool)
		if !ok {
			return errors.New("expected boolean")
		}
		out.SetBool(b)
	case "byte", "short", "integer", "long":
		number, ok := value.(json.Number)
		if !ok {
			return errors.New("expected integer")
		}
		n, err := number.Int64()
		if err != nil || out.OverflowInt(n) {
			return errors.New("integer outside modeled range")
		}
		out.SetInt(n)
	case "float", "double":
		number, ok := value.(json.Number)
		if !ok {
			return errors.New("expected number")
		}
		n, err := number.Float64()
		if err != nil || math.IsNaN(n) || math.IsInf(n, 0) || out.OverflowFloat(n) {
			return errors.New("number outside modeled range")
		}
		out.SetFloat(n)
	case "document":
		out.Set(reflect.ValueOf(value))
	default:
		return fmt.Errorf("unknown wire kind %q", shape.Kind)
	}
	return nil
}
func writeTime(t time.Time, format string) any {
	t = t.UTC()
	switch format {
	case "epoch-seconds":
		seconds := big.NewInt(t.Unix())
		seconds.Mul(seconds, big.NewInt(1_000_000_000))
		seconds.Add(seconds, big.NewInt(int64(t.Nanosecond())))
		return json.Number(new(big.Rat).SetFrac(seconds, big.NewInt(1_000_000_000)).FloatString(9))
	case "http-date":
		return t.Format(http.TimeFormat)
	default:
		return t.Format(time.RFC3339Nano)
	}
}
func readTime(value any, format string) (time.Time, error) {
	if format == "epoch-seconds" {
		number, ok := value.(json.Number)
		if !ok {
			return time.Time{}, errors.New("epoch timestamp requires a number")
		}
		r, ok := new(big.Rat).SetString(string(number))
		if !ok {
			return time.Time{}, errors.New("invalid epoch timestamp")
		}
		nanos := new(big.Int).Quo(new(big.Int).Mul(r.Num(), big.NewInt(1_000_000_000)), r.Denom())
		sec, ns := new(big.Int), new(big.Int)
		sec.QuoRem(nanos, big.NewInt(1_000_000_000), ns)
		if !sec.IsInt64() {
			return time.Time{}, errors.New("timestamp out of range")
		}
		return time.Unix(sec.Int64(), ns.Int64()).UTC(), nil
	}
	text, ok := value.(string)
	if !ok {
		return time.Time{}, errors.New("timestamp requires a string")
	}
	if format == "http-date" {
		return http.ParseTime(text)
	}
	return time.Parse(time.RFC3339Nano, text)
}
func boundValue(value any) (string, error) {
	if value == nil {
		return "", errors.New("null HTTP binding")
	}
	switch v := value.(type) {
	case string:
		return v, nil
	case json.Number:
		return string(v), nil
	case []any:
		values := make([]string, len(v))
		for i, item := range v {
			text, err := boundValue(item)
			if err != nil {
				return "", err
			}
			if text == "" || strings.ContainsAny(text, ",\"\\") || strings.TrimSpace(text) != text {
				text = strconv.Quote(text)
			}
			values[i] = text
		}
		return strings.Join(values, ", "), nil
	default:
		value := reflect.ValueOf(value)
		if value.Kind() == reflect.String {
			return value.String(), nil
		}
		data, err := json.Marshal(value.Interface())
		return string(data), err
	}
}
func headerValue(schema *schemaDescription, member memberDescription, text string) (any, error) {
	shape := schema.Shapes[member.Target]
	switch shape.Kind {
	case "list":
		if strings.TrimSpace(text) == "" {
			return []any{}, nil
		}
		parts := []string{}
		start := 0
		quoted, escaped := false, false
		for i, c := range text {
			if escaped {
				escaped = false
			} else if c == '\\' && quoted {
				escaped = true
			} else if c == '"' {
				quoted = !quoted
			} else if c == ',' && !quoted {
				parts = append(parts, strings.TrimSpace(text[start:i]))
				start = i + 1
			}
		}
		if quoted {
			return nil, errors.New("unterminated quoted header")
		}
		parts = append(parts, strings.TrimSpace(text[start:]))
		values := make([]any, len(parts))
		for i, p := range parts {
			if strings.HasPrefix(p, "\"") {
				var err error
				p, err = strconv.Unquote(p)
				if err != nil {
					return nil, err
				}
			}
			var err error
			values[i], err = headerValue(schema, shape.Member, p)
			if err != nil {
				return nil, err
			}
		}
		return values, nil
	case "byte", "short", "integer", "long", "float", "double":
		return parseJSON([]byte(text))
	case "boolean":
		if text != "true" && text != "false" {
			return nil, errors.New("invalid boolean header")
		}
		return text == "true", nil
	case "timestamp":
		if member.Format == "epoch-seconds" {
			return parseJSON([]byte(text))
		}
		return text, nil
	default:
		return text, nil
	}
}
func (c *clientRuntime) call(ctx context.Context, name string, input any) (map[string]any, error) {
	op := c.schema.Operations[name]
	values, err := encodeModel(c.schema, op.Input, input)
	if err != nil {
		return nil, err
	}
	return c.callValues(ctx, name, values)
}
func (c *clientRuntime) callValues(ctx context.Context, name string, values map[string]any) (map[string]any, error) {
	ctx, cancel := context.WithTimeout(ctx, c.config.Timeout)
	defer cancel()
	op := c.schema.Operations[name]
	if op.Token != "" && values[op.Token] == nil {
		id := make([]byte, 16)
		if _, err := rand.Read(id); err != nil {
			return nil, err
		}
		id[6] = (id[6] & 15) | 64
		id[8] = (id[8] & 63) | 128
		text := hex.EncodeToString(id)
		values[op.Token] = text[:8] + "-" + text[8:12] + "-" + text[12:16] + "-" + text[16:20] + "-" + text[20:]
	}
	uri := op.URI
	for _, key := range op.Request.Labels {
		text, err := boundValue(values[key])
		if err != nil || text == "" {
			return nil, fmt.Errorf("%s is required", key)
		}
		if text == "." || text == ".." {
			return nil, fmt.Errorf("%s cannot be a dot segment", key)
		}
		uri = strings.ReplaceAll(uri, "{"+key+"}", escape(text))
	}
	query := []string{}
	for _, binding := range op.Request.Query {
		value := values[binding[0]]
		if value == nil {
			continue
		}
		items, ok := value.([]any)
		if !ok {
			items = []any{value}
		}
		for _, item := range items {
			text, err := boundValue(item)
			if err != nil {
				return nil, err
			}
			query = append(query, escape(binding[1])+"="+escape(text))
		}
	}
	if len(query) > 0 {
		if strings.Contains(uri, "?") {
			uri += "&"
		} else {
			uri += "?"
		}
		uri += strings.Join(query, "&")
	}
	headers := c.config.Headers.Clone()
	if headers == nil {
		headers = http.Header{}
	}
	for _, binding := range op.Request.Headers {
		value := values[binding[0]]
		if value == nil {
			continue
		}
		text, err := boundValue(value)
		if err != nil {
			return nil, err
		}
		headers.Set(binding[1], text)
	}
	var body []byte
	if len(op.Request.Body) > 0 {
		object := map[string]any{}
		for _, key := range op.Request.Body {
			if value, present := values[key]; present {
				object[key] = value
			}
		}
		var err error
		body, err = json.Marshal(object)
		if err != nil {
			return nil, err
		}
		headers.Set("Content-Type", "application/json")
	}
	if len(op.Request.Payload) == 2 {
		// The bytes go as they are: a body digest covers exactly these.
		if payload, ok := values[op.Request.Payload[0]].([]byte); ok && len(payload) > 0 {
			body = payload
			headers.Set("Content-Type", op.Request.Payload[1])
		}
	}
	for attempt := 1; attempt <= c.config.MaxAttempts; attempt++ {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		request, err := http.NewRequestWithContext(ctx, op.Method, c.config.Endpoint+uri, bytes.NewReader(body))
		if err != nil {
			return nil, err
		}
		request.Header = headers.Clone()
		if c.config.Token != nil && op.Bearer {
			token, err := c.config.Token(ctx)
			if err != nil {
				return nil, err
			}
			request.Header.Set("Authorization", "Bearer "+token)
		}
		for _, middleware := range c.config.Middleware {
			if err := middleware(ctx, request); err != nil {
				return nil, err
			}
		}
		if request.Header.Get("Authorization") != "" && request.URL.Scheme != "https" {
			host := request.URL.Hostname()
			address := net.ParseIP(host)
			if host != "localhost" && (address == nil || !address.IsLoopback()) {
				return nil, errors.New("refusing to send credentials over plaintext HTTP outside loopback")
			}
		}
		if c.digest && (request.Body != nil || request.Method == "POST" || request.Method == "PUT" || request.Method == "PATCH") {
			var data []byte
			var err error
			if request.Body != nil {
				data, err = io.ReadAll(request.Body)
				request.Body.Close()
			}
			if err != nil {
				return nil, err
			}
			request.ContentLength = int64(len(data))
			if len(data) == 0 {
				request.Body = http.NoBody
				request.GetBody = func() (io.ReadCloser, error) { return http.NoBody, nil }
			} else {
				request.Body = io.NopCloser(bytes.NewReader(data))
				request.GetBody = func() (io.ReadCloser, error) { return io.NopCloser(bytes.NewReader(data)), nil }
			}
			if len(data) > 0 || request.Method == "POST" || request.Method == "PUT" || request.Method == "PATCH" {
				sum := sha256.Sum256(data)
				request.Header.Set("x-amz-content-sha256", hex.EncodeToString(sum[:]))
			}
		}
		response, transportErr := c.config.HTTPClient.Do(request)
		var data []byte
		if response != nil {
			if transportErr == nil && (response.StatusCode < 200 || response.StatusCode >= 300 || len(op.Response.Body) > 0 || len(op.Response.Payload) > 0) {
				data, transportErr = io.ReadAll(response.Body)
			}
			response.Body.Close()
		}
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		var lastErr error
		if transportErr != nil {
			lastErr = &NetworkError{Cause: transportErr}
			if !op.Safe || attempt == c.config.MaxAttempts {
				return nil, &NetworkError{Cause: transportErr}
			}
		} else {
			result, err := c.decode(op, response, data)
			if err == nil {
				return result, nil
			}
			lastErr = err
			var serviceErr *ServiceError
			if !errors.As(err, &serviceErr) || !canRetry(op.Safe, serviceErr) || attempt == c.config.MaxAttempts {
				return nil, err
			}
		}
		delay := retryDelay(attempt, response, c.config.BaseDelay, c.config.MaxDelay)
		if deadline, ok := ctx.Deadline(); ok && delay >= time.Until(deadline) {
			return nil, lastErr
		}
		timer := time.NewTimer(delay)
		select {
		case <-ctx.Done():
			timer.Stop()
			return nil, ctx.Err()
		case <-timer.C:
		}
	}
	panic("unreachable")
}
func escape(text string) string { return strings.ReplaceAll(url.QueryEscape(text), "+", "%20") }
func canRetry(safe bool, e *ServiceError) bool {
	return e.Throttling || safe && (e.Retryable || e.Metadata.Status == 500 || e.Metadata.Status == 502 || e.Metadata.Status == 503 || e.Metadata.Status == 504)
}
func retryDelay(attempt int, response *http.Response, base, max time.Duration) time.Duration {
	if response != nil {
		text := response.Header.Get("Retry-After")
		if seconds, err := strconv.ParseFloat(text, 64); err == nil && !math.IsNaN(seconds) && !math.IsInf(seconds, 0) && seconds >= 0 {
			if seconds >= float64(math.MaxInt64)/float64(time.Second) {
				return time.Duration(math.MaxInt64)
			}
			return time.Duration(seconds * float64(time.Second))
		}
		if date, err := http.ParseTime(text); err == nil {
			if delay := time.Until(date); delay >= 0 {
				return delay
			}
		}
	}
	ceiling := float64(base) * math.Pow(2, float64(min(attempt-1, 30)))
	ceiling = math.Min(ceiling, float64(max))
	return time.Duration(mathrand.Float64() * ceiling)
}
func (c *clientRuntime) decode(op operationDescription, response *http.Response, data []byte) (map[string]any, error) {
	values := map[string]any{}
	if response.StatusCode >= 200 && response.StatusCode < 300 {
		if len(op.Response.Payload) == 2 {
			// A payload is the body as received, whatever it holds.
			if data == nil {
				data = []byte{}
			}
			values[op.Response.Payload[0]] = data
		} else if len(data) > 0 {
			value, err := parseJSON(data)
			if err != nil {
				return nil, err
			}
			var ok bool
			values, ok = value.(map[string]any)
			if !ok {
				return nil, errors.New("operation response requires an object")
			}
		}
		for _, binding := range op.Response.Headers {
			if text, present := response.Header[http.CanonicalHeaderKey(binding[1])]; present {
				member := c.schema.Shapes[op.Output].Members[binding[0]]
				value, err := headerValue(c.schema, member, strings.Join(text, ", "))
				if err != nil {
					return nil, err
				}
				values[binding[0]] = value
			}
		}
		return values, nil
	}
	if value, err := parseJSON(data); err == nil {
		if object, ok := value.(map[string]any); ok {
			values = object
		}
	}
	name, _ := values["__type"].(string)
	target := ""
	if _, named := values["__type"].(string); named {
		for _, id := range op.Errors {
			if c.schema.Shapes[id].Name == name {
				target = id
				break
			}
		}
	} else {
		matches := []string{}
		for _, id := range op.Errors {
			if c.schema.Shapes[id].Error.Status == response.StatusCode {
				matches = append(matches, id)
			}
		}
		if len(matches) == 1 {
			target = matches[0]
			name = c.schema.Shapes[target].Name
		}
	}
	if name == "" {
		name = "UnknownError"
	}
	message, ok := values["message"].(string)
	if !ok {
		message = fmt.Sprintf("HTTP %d", response.StatusCode)
	}
	metadata := &ServiceError{Name: name, Message: message, Metadata: ResponseMetadata{Status: response.StatusCode, RequestID: response.Header.Get("x-request-id"), Headers: response.Header.Clone(), Body: bytes.Clone(data)}}
	if metadata.Metadata.RequestID == "" {
		metadata.Metadata.RequestID, _ = values["requestId"].(string)
	}
	if target != "" {
		shape := c.schema.Shapes[target]
		metadata.Retryable = shape.Error.Retryable
		metadata.Throttling = shape.Error.Throttling
		for _, binding := range shape.Error.Headers {
			if text, present := response.Header[http.CanonicalHeaderKey(binding[1])]; present {
				value, err := headerValue(c.schema, shape.Members[binding[0]], strings.Join(text, ", "))
				if err != nil {
					// Preserve the wire failure and retry traits when a modeled header is malformed.
					return nil, metadata
				}
				values[binding[0]] = value
			}
		}
		return nil, modeledError(target, values, metadata)
	}
	return nil, metadata
}

var errNoMorePages = errors.New("no more pages")
var errRepeatedToken = errors.New("pagination token did not advance")

func pageToken(values map[string]any, path string) (string, error) {
	var value any = values
	for _, part := range strings.Split(path, ".") {
		object, ok := value.(map[string]any)
		if !ok {
			return "", nil
		}
		value = object[part]
	}
	if value == nil {
		return "", nil
	}
	token, ok := value.(string)
	if !ok {
		return "", errors.New("pagination token must be a string")
	}
	return token, nil
}
