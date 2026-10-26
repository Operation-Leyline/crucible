package sdk

import (
	"unicode/utf8"
	"bytes"
	"context"
	"crypto/rand"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"maps"
	"math"
	"math/big"
	mathrand "math/rand/v2"
	"net"
	"net/http"
	"net/url"
	"reflect"
	"regexp"
	"slices"
	"sort"
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
// Some creates a present non-null optional member.
func Some[T any](value T) Optional[T] { return Optional[T]{Value: value, Set: true} }
// Null creates a present JSON-null optional member.
func Null[T any]() Optional[T]        { return Optional[T]{Set: true, Null: true} }

type optionalField interface{ isOptional() }
// Unit represents an operation with no modeled input or output.
type Unit struct{}

// Decimal is an exact JSON decimal. Its zero value denotes zero. ParseDecimal
// validates text; casting directly is also checked before sending a request.
type Decimal string

// String returns a readable representation.
func (d Decimal) String() string { if d == "" { return "0" }; return string(d) }
// MarshalJSON validates and encodes the exact decimal as a JSON number.
func (d Decimal) MarshalJSON() ([]byte, error) {
	text := d.String()
	if strings.TrimSpace(text) != text || !json.Valid([]byte(text)) || !(text[0] == '-' || text[0] >= '0' && text[0] <= '9') {
		return nil, errors.New("invalid JSON decimal")
	}
	return []byte(text), nil
}
// ParseDecimal validates an exact JSON number without converting through binary floating point.
func ParseDecimal(text string) (Decimal, error) {
	if text == "" { return "", errors.New("empty decimal") }
	value := Decimal(text)
	_, err := value.MarshalJSON()
	return value, err
}
// UnmarshalJSON validates and decodes an exact JSON decimal.
func (d *Decimal) UnmarshalJSON(data []byte) error {
	value, err := ParseDecimal(string(data))
	if err == nil { *d = value }
	return err
}

func decimalParts(text string) (string, string) {
	exponent := new(big.Int)
	if index := strings.IndexAny(text, "eE"); index >= 0 {
		exponent.SetString(text[index+1:], 10)
		text = text[:index]
	}
	if index := strings.IndexByte(text, '.'); index >= 0 {
		exponent.Sub(exponent, big.NewInt(int64(len(text)-index-1)))
		text = text[:index] + text[index+1:]
	}
	negative := strings.HasPrefix(text, "-")
	text = strings.TrimLeft(strings.TrimPrefix(text, "-"), "0")
	if text == "" { return "0", "0" }
	coefficient := strings.TrimRight(text, "0")
	exponent.Add(exponent, big.NewInt(int64(len(text)-len(coefficient))))
	if negative { coefficient = "-" + coefficient }
	return coefficient, exponent.String()
}

// Equal compares decimal values without expanding a large exponent.
func (d Decimal) Equal(other Decimal) bool {
	if _, err := d.MarshalJSON(); err != nil { return false }
	if _, err := other.MarshalJSON(); err != nil { return false }
	a, ae := decimalParts(d.String()); b, be := decimalParts(other.String())
	return a == b && ae == be
}

func wireEqual(left, right any) bool {
	switch a := left.(type) {
	case json.Number:
		b, ok := right.(json.Number)
		if !ok { return false }
		return Decimal(a).Equal(Decimal(b))
	case []any:
		b, ok := right.([]any)
		if !ok || len(a) != len(b) { return false }
		for i := range a { if !wireEqual(a[i], b[i]) { return false } }
		return true
	case map[string]any:
		b, ok := right.(map[string]any)
		if !ok || len(a) != len(b) { return false }
		for key, value := range a { other, present := b[key]; if !present || !wireEqual(value, other) { return false } }
		return true
	default:
		return reflect.DeepEqual(left, right)
	}
}
// UnknownUnion preserves an unrecognized union member and its wire value. Debug formatting redacts its payload.
type UnknownUnion struct {
	Name  string
	Value any
}

// ResponseMetadata retains status, request identifier, headers and the buffered response body.
type ResponseMetadata struct {
	Status    int
	RequestID string
	Headers   http.Header
	Body      []byte
// BEGIN ERROR DOCUMENTATION
	// DocURL is the static modeled error/reason documentation page; empty for an unknown identity.
	DocURL string
// END ERROR DOCUMENTATION
}
// ServiceException is the common interface for modeled and unrecognized service responses; use errors.As to catch it.
type ServiceException interface {
    error
    serviceException()
}

// ServiceError retains an error response identity, message, HTTP metadata and retry classification.
type ServiceError struct {
	Name       string
	Message    string
	Metadata   ResponseMetadata
	Retryable  bool
	Throttling bool
}

func (*ServiceError) serviceException() {}

// Error returns diagnostic error text.
func (e *ServiceError) Error() string { return e.Name + ": " + e.Message }

// NetworkError wraps a transport failure; Unwrap exposes its cause.
type NetworkError struct{ Cause error }

// Error returns diagnostic error text.
func (e *NetworkError) Error() string { return "transport failed: " + e.Cause.Error() }
// Unwrap returns the underlying error for errors.Is and errors.As.
func (e *NetworkError) Unwrap() error { return e.Cause }

// HTTPClient sends a request and returns its HTTP response; *http.Client implements it.
type HTTPClient interface {
	Do(*http.Request) (*http.Response, error)
}
// Config configures endpoint selection, authentication, transport, deadlines and retries. Treat maps and callbacks as immutable after construction.
type Config struct {
	// Endpoint Absolute HTTP(S) base URL.
	Endpoint    string
	// HTTPClient Transport; nil creates an http.Client that refuses redirects. Custom transports remain caller-owned.
	HTTPClient  HTTPClient
	// Token Resolves the bearer token for each attempt.
	Token       func(context.Context) (string, error)
	// Headers Headers copied during construction and sent on every request.
	Headers     http.Header
	// MaxAttempts Total attempt limit including first; zero defaults to 3.
	MaxAttempts int
	// Timeout Total call budget; zero defaults to 30 seconds. Context cancellation can shorten it.
	Timeout     time.Duration
	// BaseDelay Initial full-jitter backoff ceiling; zero defaults to 100 milliseconds.
	BaseDelay   time.Duration
	// MaxDelay Maximum backoff ceiling; zero defaults to 20 seconds.
	MaxDelay    time.Duration
	// Middleware Request callbacks run in order on every attempt before transport.
	Middleware  []func(context.Context, *http.Request) error
	// RetryMode is RetryModeLegacy (the zero value), RetryModeStandard or RetryModeAdaptive.
	RetryMode RetryMode
	// Telemetry observes every call; nil disables telemetry.
	Telemetry TelemetryHook
	// Clock replaces the system clock for backoff, rate limiting and
	// telemetry; it is a test seam.
	Clock Clock
}
type clientRuntime struct {
	config Config
	schema *schemaDescription
	// One retry quota and rate limiter for every call of the client.
	retry *retryStrategy
}
type memberDescription struct {
	Target    string
	Attribute string
	Format    string
	Required  bool
	Default   any
	// Sensitive members are redacted from telemetry wherever they are bound.
	Sensitive bool
}
// BEGIN ERROR DOCUMENTATION
type errorDocumentation struct {
    URL string
    ReasonMember *string
    ReasonDefault *string
    ReasonURLs map[string]string
}

func errorDocURL(documentation *errorDocumentation, values map[string]any) string {
    if documentation == nil { return "" }
    // nil values classify a malformed response; its code page remains authoritative.
    if documentation.ReasonMember != nil && values != nil {
        value, present := values[*documentation.ReasonMember]
        if !present && documentation.ReasonDefault != nil { value = *documentation.ReasonDefault }
        if reason, ok := value.(string); ok {
            if url, exists := documentation.ReasonURLs[reason]; exists { return url }
        }
    }
    return documentation.URL
}

// END ERROR DOCUMENTATION
type errorDescription struct {
// BEGIN ERROR DOCUMENTATION
	Documentation *errorDocumentation
// END ERROR DOCUMENTATION
	WireCode string
	HeaderEncodings map[string]string
	Payload []string
	PayloadKind string
	TimestampFormats map[string]string
	Status     int
	Retryable  bool
	Throttling bool
	Headers    [][2]string
}
type shapeDescription struct {
	Sensitive bool
	Sparse bool
	Set bool
	Name    string
	Kind    string
	Format  string
	Member  memberDescription
	Key     memberDescription
	Value   memberDescription
	Members map[string]memberDescription
	Error   *errorDescription
}

// formatModel uses wire metadata rather than fmt.Stringer so nested sensitive
// targets and collection members are redacted before any custom formatter runs.
func formatModel(schema *schemaDescription, target string, value any) string {
	active := map[any]bool{}
	depth := 0
	var render func(string, reflect.Value, bool) string
	render = func(target string, value reflect.Value, sensitive bool) string {
		shape := schema.Shapes[target]
		depth++; defer func() { depth-- }()
		if depth > 64 { return "<recursive>" }
		if sensitive || shape.Sensitive { return "*** Sensitive Data Redacted ***" }
		if !value.IsValid() { return "null" }
		for value.Kind() == reflect.Interface { if value.IsNil() { return "null" }; value = value.Elem() }
		if value.Kind() == reflect.Pointer && shape.Kind != "structure" && shape.Kind != "union" && shape.Kind != "list" && shape.Kind != "map" { if value.IsNil() { return "null" }; return fmt.Sprint(value.Interface()) }
		if value.Kind() == reflect.Pointer {
			if value.IsNil() { return "null" }
			key := value.Interface()
			if active[key] { return "<recursive>" }; active[key] = true; defer delete(active, key)
			value = value.Elem()
		}
		switch shape.Kind {
		case "structure", "union":
			parts := []string{}
			names := make([]string, 0, len(shape.Members)); for name := range shape.Members { names = append(names, name) }; sort.Strings(names)
			for _, name := range names {
				member := shape.Members[name]; item := value.FieldByName(member.Attribute)
				if item.IsValid() && item.CanInterface() {
					if _, ok := item.Interface().(optionalField); ok {
						if !item.FieldByName("Set").Bool() { continue }
						if item.FieldByName("Null").Bool() { if member.Sensitive { parts = append(parts, member.Attribute+"=*** Sensitive Data Redacted ***") } else { parts = append(parts, member.Attribute+"=null") }; continue }
						item = item.FieldByName("Value")
					}
					parts = append(parts, member.Attribute+"="+render(member.Target, item, member.Sensitive))
				}
			}
			if shape.Kind == "union" { if unknown := value.FieldByName("UnknownVariant"); unknown.IsValid() && !unknown.IsNil() { parts = append(parts, "UnknownVariant=*** Sensitive Data Redacted ***") } }
			return value.Type().Name()+"("+strings.Join(parts, ", ")+")"
		case "list":
			parts := []string{}; for i := 0; i < value.Len(); i++ { parts = append(parts, render(shape.Member.Target, value.Index(i), shape.Member.Sensitive)) }; return "["+strings.Join(parts, ", ")+"]"
		case "map":
			parts := []string{}; iterator := value.MapRange(); for iterator.Next() { parts = append(parts, render(shape.Key.Target, iterator.Key(), shape.Key.Sensitive)+": "+render(shape.Value.Target, iterator.Value(), shape.Value.Sensitive)) }; sort.Strings(parts); return "{"+strings.Join(parts, ", ")+"}"
		default:
			// Read primitive values directly: a generated integer enum implements
			// Stringer too, so fmt.Sprint on it would re-enter this formatter.
			switch value.Kind() {
			case reflect.String: return strconv.Quote(value.String())
			case reflect.Bool: return strconv.FormatBool(value.Bool())
			case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64: return strconv.FormatInt(value.Int(), 10)
			case reflect.Float32, reflect.Float64: return strconv.FormatFloat(value.Float(), 'g', -1, value.Type().Bits())
			default: return fmt.Sprint(value.Interface())
			}
		}
	}
	return render(target, reflect.ValueOf(value), false)
}
func (s shapeDescription) errorCode() string {
	if s.Error != nil && s.Error.WireCode != "" { return s.Error.WireCode }; return s.Name
}
type bindings struct {
	HeaderEncodings map[string]string
	PayloadKind string
	TimestampFormats map[string]string
	Labels  []string
	Query   [][2]string
	Headers [][2]string
	Body    []string
	// Payload is the member that is the whole body, raw, and its media type.
	Payload []string
}
type operationDescription struct {
	GreedyLabels []string
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

func (s shapeDescription) knownWire(name string) bool {
	_, known := s.Members[name]
	return known
}
type schemaDescription struct {
	Service string
	APIVersion string `json:"apiVersion"`
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
func newClient(config Config, schema *schemaDescription) (*clientRuntime, error) {
	endpoint, err := url.Parse(config.Endpoint)
	if (err != nil || endpoint.Host == "" || (endpoint.Scheme != "http" && endpoint.Scheme != "https") || endpoint.RawQuery != "" || endpoint.Fragment != "" || endpoint.User != nil) {
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
	// Canonical keys, so a modelled or protocol header replaces a default
	// given as a map literal in another case instead of being sent beside it.
	if config.Headers != nil {
		headers := http.Header{}
		for name, values := range config.Headers { for _, value := range values { headers.Add(name, value) } }
		config.Headers = headers
	}
	config.Middleware = append([]func(context.Context, *http.Request) error(nil), config.Middleware...)
	switch config.RetryMode {
	case "": config.RetryMode = RetryModeLegacy
	case RetryModeLegacy, RetryModeStandard, RetryModeAdaptive:
	default: return nil, errors.New("RetryMode must be legacy, standard or adaptive")
	}
	if config.Clock == nil { config.Clock = systemClock{} }
	runtime := &clientRuntime{config: config, schema: schema}
	runtime.retry = newRetryStrategy(config.RetryMode, config.Clock)
	return runtime, nil
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
	case "enum", "string": return value.String(), nil
	case "float", "double":
		return value.Interface(), nil
	case "bigInteger":
		number, ok := value.Interface().(big.Int)
		if !ok { return nil, errors.New("expected *big.Int") }
		return json.Number(number.String()), nil
	case "bigDecimal":
		number := Decimal(value.String())
		if _, err := number.MarshalJSON(); err != nil { return nil, err }
		return json.Number(number.String()), nil
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
				if shape.knownWire(arm.Name) {
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
			if !shape.Sparse && !unwrap(value.Index(i)).IsValid() {
				return nil, errors.New("null element in non-sparse list")
			}
			var err error
			result[i], err = encodeValue(schema, shape.Member.Target, value.Index(i), shape.Member.Format)
			if err != nil {
				return nil, err
			}
			if shape.Set { for _, earlier := range result[:i] { if wireEqual(earlier, result[i]) { return nil, errors.New("duplicate element in set") } } }
		}
		return result, nil
	case "map":
		result := map[string]any{}
		iter := value.MapRange()
		for iter.Next() {
			if !shape.Sparse && !unwrap(iter.Value()).IsValid() {
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
	case "bigInteger":
		number, ok := value.(json.Number)
		if !ok { return errors.New("expected arbitrary precision integer") }
		integer, ok := new(big.Int).SetString(string(number), 10)
		if !ok { return errors.New("invalid arbitrary precision integer") }
		out.Set(reflect.ValueOf(*integer))
	case "bigDecimal":
		number, ok := value.(json.Number)
		if !ok { return errors.New("expected arbitrary precision decimal") }
		decimal := Decimal(number)
		if _, err := decimal.MarshalJSON(); err != nil { return err }
		out.SetString(string(decimal))
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
				if !shape.knownWire(key) {
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
			if !shape.Sparse && item == nil {
				return errors.New("null element in non-sparse list")
			}
			if err := decodeValue(schema, shape.Member.Target, item, out.Index(i), shape.Member.Format); err != nil {
				return err
			}
			if shape.Set { for _, earlier := range items[:i] { if wireEqual(earlier, item) { return errors.New("duplicate element in set") } } }
		}
	case "map":
		items, ok := value.(map[string]any)
		if !ok {
			return errors.New("expected object")
		}
		out.Set(reflect.MakeMap(out.Type()))
		for key, item := range items {
			if !shape.Sparse && item == nil {
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
	case "byte", "short", "integer", "long", "intEnum":
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
func mediaValue(value any, decode bool) (any, error) {
	if values, ok := value.([]any); ok {
		result := make([]any, len(values))
		for i, item := range values { var err error; result[i], err = mediaValue(item, decode); if err != nil { return nil, err } }
		return result, nil
	}
	text, ok := value.(string)
	if !ok { return nil, errors.New("media header requires a string") }
	if !decode { return base64.StdEncoding.EncodeToString([]byte(text)), nil }
	data, err := base64.StdEncoding.Strict().DecodeString(text)
	if err != nil { return nil, err }
	if !utf8.Valid(data) { return nil, errors.New("media header is not UTF-8") }
	return string(data), nil
}
func payloadValue(kind string, data []byte) (any, error) {
	switch kind {
	case "string":
		if !utf8.Valid(data) { return nil, errors.New("text payload is not UTF-8") }
		return string(data), nil
	case "document":
		if len(data) == 0 { return nil, nil }
		return parseJSON(data)
	default:
		if data == nil { data = []byte{} }
		return data, nil
	}
}
func boundMember(schema *schemaDescription, member memberDescription, value any, format string) (any, error) {
	if format == "" || value == nil { return value, nil }
	shape := schema.Shapes[member.Target]
	if shape.Kind == "timestamp" {
		parsed, err := readTime(value, member.Format)
		if err != nil { return nil, err }
		return writeTime(parsed, format), nil
	}
	if shape.Kind == "list" {
		values := value.([]any); result := make([]any, len(values))
		for i, item := range values { var err error; result[i], err = boundMember(schema, shape.Member, item, format); if err != nil { return nil, err } }
		return result, nil
	}
	return value, nil
}
var httpDateSeparator = regexp.MustCompile(`,\s*(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun),`)
func headerValue(schema *schemaDescription, member memberDescription, text string, formats ...string) (any, error) {
	format := ""
	if len(formats) > 0 { format = formats[0] }
	shape := schema.Shapes[member.Target]
	switch shape.Kind {
	case "list":
		if strings.TrimSpace(text) == "" {
			return []any{}, nil
		}
		parts := []string{}
		elementFormat := format
		if elementFormat == "" { elementFormat = shape.Member.Format }
		if schema.Shapes[shape.Member.Target].Kind == "timestamp" && elementFormat == "http-date" && !strings.HasPrefix(strings.TrimSpace(text), "\"") {
			// Every IMF-fixdate holds a comma and the protocols write these lists unquoted, so only a comma
			// that is followed by the next date's weekday separates two values.
			start := 0
			for _, found := range httpDateSeparator.FindAllStringIndex(text, -1) {
				parts = append(parts, strings.TrimSpace(text[start:found[0]]))
				start = found[0] + 1
			}
			parts = append(parts, strings.TrimSpace(text[start:]))
		} else {
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
		}
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
			values[i], err = headerValue(schema, shape.Member, p, format)
			if err != nil {
				return nil, err
			}
		}
		return values, nil
	case "byte", "short", "integer", "long", "float", "double", "intEnum", "bigInteger", "bigDecimal":
		return parseJSON([]byte(text))
	case "boolean":
		if text != "true" && text != "false" {
			return nil, errors.New("invalid boolean header")
		}
		return text == "true", nil
	case "timestamp":
		if format == "" { format = member.Format }
		var raw any = text
		if format == "epoch-seconds" { var err error; raw, err = parseJSON([]byte(text)); if err != nil { return nil, err } }
		parsed, err := readTime(raw, format)
		if err != nil { return nil, err }
		return writeTime(parsed, member.Format), nil
	default:
		return text, nil
	}
}
func (c *clientRuntime) call(ctx context.Context, name string, input any) (map[string]any, error) {
	trace := c.startTrace(name)
	values, err := encodeModel(c.schema, c.schema.Operations[name].Input, input)
	var result map[string]any
	if err == nil {
		result, err = c.send(ctx, name, values, trace)
	}
	trace.finished(err)
	return result, err
}
func (c *clientRuntime) callValues(ctx context.Context, name string, values map[string]any) (map[string]any, error) {
	trace := c.startTrace(name)
	result, err := c.send(ctx, name, values, trace)
	trace.finished(err)
	return result, err
}

// classify returns a failed attempt's retry reason, or "" when it is not retryable.
func (c *clientRuntime) classify(op operationDescription, transportErr, err error) string {
	if transportErr != nil {
		if !op.Safe { return "" }
		var timeout net.Error
		if errors.As(transportErr, &timeout) && timeout.Timeout() { return "timeout" }
		return "network"
	}
	var service *ServiceError
	if !errors.As(err, &service) { return "" }
	if service.Throttling { return "throttling" }
	if canRetry(op.Safe, service) { return "transient" }
	return ""
}

func throttled(err error) bool {
	var service *ServiceError
	return errors.As(err, &service) && service.Throttling
}

func (c *clientRuntime) send(ctx context.Context, name string, values map[string]any, trace *callTrace) (map[string]any, error) {
	ctx, cancel := context.WithTimeout(ctx, c.config.Timeout)
	defer cancel()
	op := c.schema.Operations[name]
	inputShape := c.schema.Shapes[op.Input]
	// Defaults and generated tokens belong to this call, including its retries.
	values = maps.Clone(values)
	if values == nil { values = map[string]any{} }
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
	boundValues := map[string]any{}
	for key, value := range values { boundValues[key] = value }
	for name, format := range op.Request.TimestampFormats {
		key := name
		var err error
		boundValues[key], err = boundMember(c.schema, inputShape.Members[name], values[key], format)
		if err != nil { return nil, err }
	}
	uri := op.URI
	// Telemetry shows where sensitive members went without their values.
	display := op.URI
	traced := c.config.Telemetry != nil
	sensitiveHeaders, sensitiveQuery := map[string]bool{}, map[string]bool{}
	for _, key := range op.Request.Labels {
		text, err := boundValue(boundValues[key])
		if err != nil || text == "" {
			return nil, fmt.Errorf("%s is required", key)
		}
		greedy := slices.Contains(op.GreedyLabels, key)
		parts := []string{text}
		if greedy { parts = strings.Split(text, "/") }
		for i, part := range parts {
			if part == "." || part == ".." { return nil, fmt.Errorf("%s cannot be a dot segment", key) }
			parts[i] = escape(part)
		}
		placeholder := key
		if greedy { placeholder += "+" }
		label := strings.Join(parts, "/")
		uri = strings.ReplaceAll(uri, "{"+placeholder+"}", label)
		if traced {
			if inputShape.Members[key].Sensitive { label = redacted }
			display = strings.ReplaceAll(display, "{"+placeholder+"}", label)
		}
	}
	display, _, _ = strings.Cut(display, "?")
	query := []string{}
	for _, binding := range op.Request.Query {
		value := boundValues[binding[0]]
		if value == nil {
			continue
		}
		if traced && inputShape.Members[binding[0]].Sensitive { sensitiveQuery[strings.ToLower(binding[1])] = true }
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
	// A Content-Type bound to an input member outranks the payload's media
	// type, which outranks client header defaults.
	modelledContentType := false
	for _, binding := range op.Request.Headers {
		if traced && inputShape.Members[binding[0]].Sensitive { sensitiveHeaders[strings.ToLower(binding[1])] = true }
		value := boundValues[binding[0]]
		if value == nil {
			continue
		}
		if op.Request.HeaderEncodings[binding[0]] == "base64" {
			var err error; value, err = mediaValue(value, false); if err != nil { return nil, err }
		}
		text, err := boundValue(value)
		if err != nil {
			return nil, err
		}
		headers.Set(binding[1], text)
		if strings.EqualFold(binding[1], "Content-Type") { modelledContentType = true }
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
		if payload, present := values[op.Request.Payload[0]]; present && (payload != nil || op.Request.PayloadKind == "document") {
			switch op.Request.PayloadKind {
			case "string": body = []byte(payload.(string))
			case "document": var err error; body, err = json.Marshal(payload); if err != nil { return nil, err }
			default: body = payload.([]byte)
			}
			if !modelledContentType { headers.Set("Content-Type", op.Request.Payload[1]) }
		}
	}
	// The release this client was generated for (docs/api-versions-clients.md).
	if c.schema.APIVersion != "" { headers.Set("Crucible-Version", c.schema.APIVersion) }
	maxAttempts := c.config.MaxAttempts
	endpoint := strings.TrimRight(c.config.Endpoint, "/")
	lastCost := 0
	for attempt := 1; attempt <= maxAttempts; attempt++ {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if wait := c.retry.sendDelay(); wait > 0 {
			if deadline, ok := ctx.Deadline(); ok && wait >= time.Until(deadline) {
				c.retry.cancelSend()
				return nil, fmt.Errorf("client-side rate limit delay exceeds the request deadline: %w", context.DeadlineExceeded)
			}
			if err := c.config.Clock.Sleep(ctx, wait); err != nil {
				c.retry.cancelSend()
				return nil, err
			}
		}
		request, err := http.NewRequestWithContext(ctx, op.Method, endpoint+uri, bytes.NewReader(body))
		if err != nil {
			return nil, err
		}
		request.Header = headers.Clone()
		credentials, err := c.authenticate(ctx, request, op)
		if err != nil { return nil, err }
		for _, middleware := range c.config.Middleware {
			if err := middleware(ctx, request); err != nil {
				return nil, err
			}
		}
		if request.URL == nil || request.URL.Host == "" || request.URL.User != nil || (request.URL.Scheme != "http" && request.URL.Scheme != "https") { return nil,errors.New("middleware must supply an absolute HTTP(S) URL without credentials") }
		if (credentials || request.Header.Get("Authorization") != "") && request.URL.Scheme != "https" {
			host := request.URL.Hostname()
			address := net.ParseIP(host)
			if host != "localhost" && (address == nil || !address.IsLoopback()) {
				return nil, errors.New("refusing to send credentials over plaintext HTTP outside loopback")
			}
		}
		if err := ctx.Err(); err != nil { return nil, err }
		trace.attemptStarted(request, endpoint, display, sensitiveHeaders, sensitiveQuery)
		response, transportErr := c.config.HTTPClient.Do(request)
		// interrupted ends an attempt with a failure that is never retried; a
		// response that arrived still counts toward the measured rate.
		interrupted := func(size int64, err error) (map[string]any, error) {
			if response != nil { c.retry.afterAttempt(false) }
			trace.attemptFinished(response, err, size, "", 0)
			return nil, err
		}
		var data []byte
		size := int64(-1)
		if response != nil {
			if transportErr == nil && (response.StatusCode < 200 || response.StatusCode >= 300 || len(op.Response.Body) > 0 || len(op.Response.Payload) > 0) {
				data, transportErr = io.ReadAll(response.Body)
			}
			response.Body.Close()
			size = int64(len(data))
		}
		if err := ctx.Err(); err != nil {
			return interrupted(size, err)
		}
		var lastErr error
		if transportErr != nil {
			lastErr = &NetworkError{Cause: transportErr}
		} else {
			result, err := c.decode(op, response, data)
			if err == nil {
				c.retry.afterAttempt(false)
				c.retry.succeeded(lastCost)
				trace.attemptFinished(response, nil, size, "", 0)
				return result, nil
			}
			var serviceErr *ServiceError
			if !errors.As(err, &serviceErr) { return interrupted(size, err) }
			lastErr = err
		}
		// Record the failed attempt, then decide whether, and when, to retry.
		c.retry.afterAttempt(throttled(lastErr))
		reason := c.classify(op, transportErr, lastErr)
		if reason == "" || attempt == maxAttempts {
			trace.attemptFinished(response, lastErr, size, "", 0)
			return nil, lastErr
		}
		granted, cost := c.retry.acquireRetry(reason == "timeout")
		if !granted {
			trace.attemptFinished(response, lastErr, size, "", 0)
			return nil, lastErr
		}
		delay := retryDelayWith(attempt, response, c.config.BaseDelay, c.config.MaxDelay, c.retry.jitter)
		trace.attemptFinished(response, lastErr, size, reason, delay)
		lastCost = cost
		if deadline, ok := ctx.Deadline(); ok && delay >= time.Until(deadline) {
			return nil, lastErr
		}
		if err := c.config.Clock.Sleep(ctx, delay); err != nil {
			return nil, err
		}
	}
	panic("unreachable")
}
func escape(text string) string { return strings.ReplaceAll(url.QueryEscape(text), "+", "%20") }
func canRetry(safe bool, e *ServiceError) bool {
	return e.Throttling || safe && (e.Retryable || e.Metadata.Status == 500 || e.Metadata.Status == 502 || e.Metadata.Status == 503 || e.Metadata.Status == 504)
}
func retryDelay(attempt int, response *http.Response, base, max time.Duration) time.Duration {
	return retryDelayWith(attempt, response, base, max, mathrand.Float64)
}

// retryDelayWith is Retry-After when valid, otherwise full-jitter exponential
// backoff after the given (1-based) failed attempt.
func retryDelayWith(attempt int, response *http.Response, base, max time.Duration, jitter func() float64) time.Duration {
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
	return time.Duration(jitter() * ceiling)
}
func (c *clientRuntime) responseBindings(target string, binding bindings, response *http.Response, values map[string]any) error {
	shape := c.schema.Shapes[target]
	for _, entry := range binding.Headers {
		if text, present := response.Header[http.CanonicalHeaderKey(entry[1])]; present {
			value, err := headerValue(c.schema, shape.Members[entry[0]], strings.Join(text, ", "), binding.TimestampFormats[entry[0]])
			if err != nil { return err }
			if binding.HeaderEncodings[entry[0]] == "base64" { value, err = mediaValue(value, true); if err != nil { return err } }
			values[entry[0]] = value
		}
	}
	return nil
}

func (c *clientRuntime) decode(op operationDescription, response *http.Response, data []byte) (map[string]any, error) {
	values := map[string]any{}
	if response.StatusCode >= 200 && response.StatusCode < 300 {
		if len(op.Response.Payload) == 2 {
			value, err := payloadValue(op.Response.PayloadKind, data)
			if err != nil { return nil, err }
			values[op.Response.Payload[0]] = value
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
		if err := c.responseBindings(op.Output, op.Response, response, values); err != nil { return nil, err }
		return values, nil
	}
// BEGIN ERROR DOCUMENTATION
    documentationBodyValid := len(bytes.Trim(data, " \t\r\n")) == 0
// END ERROR DOCUMENTATION
	if value, err := parseJSON(data); err == nil {
		if object, ok := value.(map[string]any); ok {
			values = object
// BEGIN ERROR DOCUMENTATION
            documentationBodyValid = true
// END ERROR DOCUMENTATION
		}
	}
	name, _ := values["__type"].(string)
	if header := response.Header.Get("X-Crucible-Error"); header != "" { name = header }
	target := ""
	if name != "" {
		for _, id := range op.Errors {
			if c.schema.Shapes[id].errorCode() == name {
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
			name = c.schema.Shapes[target].errorCode()
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
// BEGIN ERROR DOCUMENTATION
		metadata.Metadata.DocURL = errorDocURL(shape.Error.Documentation, nil)
        if len(shape.Error.Payload) != 2 && !documentationBodyValid { return nil, metadata }
// END ERROR DOCUMENTATION
		if len(shape.Error.Payload) == 2 {
			value, err := payloadValue(shape.Error.PayloadKind, data)
			if err != nil { return nil, metadata }
			values = map[string]any{shape.Error.Payload[0]: value}
		}
		if err := c.responseBindings(target, bindings{Headers:shape.Error.Headers, TimestampFormats:shape.Error.TimestampFormats, HeaderEncodings:shape.Error.HeaderEncodings}, response, values); err != nil {
			// Preserve wire failure metadata if a modeled binding is malformed.
			return nil, metadata
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
