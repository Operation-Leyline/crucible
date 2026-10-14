package sdk

import (
	"unicode/utf8"
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
	// RequestChecksumCalculation Selects WHEN_SUPPORTED (default) or WHEN_REQUIRED request checksums.
	RequestChecksumCalculation ChecksumPolicy
	// ResponseChecksumValidation Selects WHEN_SUPPORTED (default) or WHEN_REQUIRED response validation.
	ResponseChecksumValidation ChecksumPolicy
	// ChecksumValidated Receives operation and algorithm after a response checksum is verified.
	ChecksumValidated func(operation, algorithm string)
	// EndpointParameters Overrides modeled endpoint rule parameters; keep immutable after construction.
	EndpointParameters map[string]any
	// EndpointBuiltIns Overrides endpoint built-in values; keep immutable after construction.
	EndpointBuiltIns map[string]any
	// DisableHostPrefix Disables the modeled host prefix when true.
	DisableHostPrefix bool
	// BasicCredentials Resolves HTTP Basic credentials for each attempt.
	BasicCredentials func(context.Context) (*BasicCredentials, error)
	// APIKey Resolves the modeled API key for each attempt.
	APIKey func(context.Context) (string, error)
	// Credentials Resolves SigV4 signing credentials for each attempt.
	Credentials CredentialsProvider
	// Region AWS region for endpoint rules and signing.
	Region string
	// Endpoint Absolute HTTP(S) base URL; optional when endpoint rules resolve it.
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
	// EndpointDiscovery is EndpointDiscoveryAuto (the zero value),
	// EndpointDiscoveryEnabled or EndpointDiscoveryDisabled.
	EndpointDiscovery EndpointDiscoveryMode
	// Telemetry observes every call; nil disables telemetry.
	Telemetry TelemetryHook
	// Clock replaces the system clock for backoff, rate limiting, discovery
	// expiry and telemetry; it is a test seam.
	Clock Clock
}
type clientRuntime struct {
	config Config
	schema *schemaDescription
	digest bool
	// One retry quota, rate limiter and discovery cache for every call of the client.
	retry     *retryStrategy
	discovery *endpointCache
	// Header and query names that carry an API key, lowercased, for telemetry redaction.
	credentialHeaders map[string]bool
	credentialQuery   map[string]bool
}
type memberDescription struct {
	JSONName  string
	Target    string
	Attribute string
	Format    string
	Required  bool
	Default   any
	// Sensitive members are redacted from telemetry wherever they are bound.
	Sensitive bool
	// Length holds a streaming member's modeled byte bounds.
	Length    *struct{ Min, Max *int64 }
}
type errorDescription struct {
	WireCode string
	HeaderEncodings map[string]string
	Payload []string
	PayloadKind string
	TimestampFormats map[string]string
	PrefixHeaders [][2]string
	ResponseCode string
	Status     int
	Retryable  bool
	Throttling bool
	Headers    [][2]string
}
type shapeDescription struct {
	Sensitive bool
	Streaming bool
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
		if shape.Streaming { if value.Kind() == reflect.Pointer && value.IsNil() { return "null" }; return "<stream>" }
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
	// Streaming marks a payload that is sent or returned as a stream.
	Streaming bool
	RequiresLength bool
	HeaderEncodings map[string]string
	PayloadKind string
	TimestampFormats map[string]string
	QueryParams string
	PrefixHeaders [][2]string
	ResponseCode string
	Labels  []string
	Query   [][2]string
	Headers [][2]string
	Body    []string
	// Payload is the member that is the whole body, raw, and its media type.
	Payload []string
}
type operationDescription struct {
	Checksum *checksumDescription
	Endpoint map[string]any
	EndpointParameters map[string][]map[string]any
	ResolvedEndpoint *resolvedEndpoint `json:"-"`
	Waiters map[string]map[string]any
	Auth []string
	OptionalAuth bool
	UnsignedPayload bool
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
	DiscoveredEndpoint *discoveredEndpointDescription
}

func (s shapeDescription) wireName(name string) string {
	if value := s.Members[name].JSONName; value != "" { return value }
	return name
}

func (s shapeDescription) knownWire(name string) bool {
	for key := range s.Members { if s.wireName(key) == name { return true } }
	return false
}
type schemaDescription struct {
	Service string
	Protocol string
	APIVersion string `json:"apiVersion"`
	ProtocolSettings struct {
		OmitNullStructureMembers bool
		IgnoreUnknownUnionType bool
		NonFiniteFloatStrings bool
	}
	EndpointRules map[string]any
	EndpointDiscovery *endpointDiscoveryDescription
	AuthSchemes map[string]authScheme
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
	for _,policy := range []ChecksumPolicy{config.RequestChecksumCalculation,config.ResponseChecksumValidation} {
		if policy != "" && policy != ChecksumWhenSupported && policy != ChecksumWhenRequired { return nil,errors.New("checksum policy must be WHEN_SUPPORTED or WHEN_REQUIRED") }
	}
	endpoint, err := url.Parse(config.Endpoint)
	if !(config.Endpoint == "" && schema.EndpointRules != nil) && (err != nil || endpoint.Host == "" || (endpoint.Scheme != "http" && endpoint.Scheme != "https") || endpoint.RawQuery != "" || endpoint.Fragment != "" || endpoint.User != nil) {
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
	config.EndpointParameters, err = copyEndpointParameters(config.EndpointParameters); if err != nil { return nil,err }
	config.EndpointBuiltIns, err = copyEndpointParameters(config.EndpointBuiltIns); if err != nil { return nil,err }
	switch config.RetryMode {
	case "": config.RetryMode = RetryModeLegacy
	case RetryModeLegacy, RetryModeStandard, RetryModeAdaptive:
	default: return nil, errors.New("RetryMode must be legacy, standard or adaptive")
	}
	switch config.EndpointDiscovery {
	case "": config.EndpointDiscovery = EndpointDiscoveryAuto
	case EndpointDiscoveryAuto, EndpointDiscoveryEnabled, EndpointDiscoveryDisabled:
	default: return nil, errors.New("EndpointDiscovery must be auto, enabled or disabled")
	}
	if config.Clock == nil { config.Clock = systemClock{} }
	runtime := &clientRuntime{config: config, schema: schema, digest: digest, credentialHeaders: map[string]bool{}, credentialQuery: map[string]bool{}}
	runtime.retry = newRetryStrategy(config.RetryMode, config.Clock)
	if schema.EndpointDiscovery != nil { runtime.discovery = newEndpointCache(runtime.retry.now) }
	for _, scheme := range schema.AuthSchemes {
		if scheme.Kind != "apiKey" { continue }
		if scheme.Location == "query" { runtime.credentialQuery[strings.ToLower(scheme.Name)] = true } else { runtime.credentialHeaders[strings.ToLower(scheme.Name)] = true }
	}
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
	if shape.Streaming && shape.Kind == "blob" {
		// A stream travels as the caller's reader; the call resolves it.
		if !value.IsValid() || (value.Kind() == reflect.Interface || value.Kind() == reflect.Pointer) && value.IsNil() { return nil, nil }
		return value.Interface(), nil
	}
	value = unwrap(value)
	if !value.IsValid() {
		return nil, nil
	}
	switch shape.Kind {
	case "enum", "string": return value.String(), nil
	case "float", "double":
		if schema.ProtocolSettings.NonFiniteFloatStrings {
			n := value.Float()
			if math.IsNaN(n) { return "NaN", nil }
			if math.IsInf(n, 1) { return "Infinity", nil }
			if math.IsInf(n, -1) { return "-Infinity", nil }
		}
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
		for key, member := range shape.Members {
			wire := shape.wireName(key)
			field := value.FieldByName(member.Attribute)
			if field.IsValid() && field.CanInterface() {
				if _, optional := field.Interface().(optionalField); optional {
					if !field.FieldByName("Set").Bool() {
						continue
					}
					if field.FieldByName("Null").Bool() {
						if shape.Kind == "structure" && schema.ProtocolSettings.OmitNullStructureMembers && schema.Shapes[member.Target].Kind != "document" { continue }
						result[wire] = nil
						continue
					}
					field = field.FieldByName("Value")
				}
				encoded, err := encodeValue(schema, member.Target, field, member.Format)
				if err != nil {
					return nil, fmt.Errorf("%s: %w", wire, err)
				}
				if !(encoded == nil && shape.Kind == "structure" && schema.ProtocolSettings.OmitNullStructureMembers && schema.Shapes[member.Target].Kind != "document") { result[wire] = encoded }
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
	if shape.Streaming && shape.Kind == "blob" {
		stream := reflect.ValueOf(value)
		if !stream.Type().AssignableTo(out.Type()) { return errors.New("streaming blob field cannot hold a stream") }
		out.Set(stream)
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
			if schema.ProtocolSettings.IgnoreUnknownUnionType && !shape.knownWire("__type") {
				clean := make(map[string]any, len(object))
				for key, value := range object { if key != "__type" { clean[key] = value } }
				object = clean
			}
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
		for key, member := range shape.Members {
			wire := shape.wireName(key)
			item, present := object[wire]
			if shape.Kind == "structure" && item == nil && schema.ProtocolSettings.OmitNullStructureMembers && schema.Shapes[member.Target].Kind != "document" { present = false }
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
		if schema.ProtocolSettings.NonFiniteFloatStrings {
			if text, ok := value.(string); ok {
				switch text {
				case "NaN": out.SetFloat(math.NaN()); return nil
				case "Infinity": out.SetFloat(math.Inf(1)); return nil
				case "-Infinity": out.SetFloat(math.Inf(-1)); return nil
				default: return errors.New("invalid floating point string")
				}
			}
		}
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
		// A non-finite float travels as the protocol's string, which decodeModel converts as it does for a body.
		if schema.ProtocolSettings.NonFiniteFloatStrings && (shape.Kind == "float" || shape.Kind == "double") {
			switch trimmed := strings.TrimSpace(text); trimmed {
			case "NaN", "Infinity", "-Infinity": return trimmed, nil
			}
		}
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
func (c *clientRuntime) classify(op operationDescription, transportErr, err error, discovered *discoveredAddress) string {
	if transportErr != nil {
		if !op.Safe { return "" }
		var timeout net.Error
		if errors.As(transportErr, &timeout) && timeout.Timeout() { return "timeout" }
		return "network"
	}
	var service *ServiceError
	if !errors.As(err, &service) { return "" }
	// A misdirected request was not processed, so even a write may be resent.
	if discovered != nil && c.invalidates(err) { return "endpoint" }
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
	// A streamed response keeps the deadline, released when its body ends or closes.
	streamed := false
	defer func() { if !streamed { cancel() } }()
	op := c.schema.Operations[name]
	inputShape := c.schema.Shapes[op.Input]
	// Defaults and generated tokens belong to this call, including its retries.
	values = maps.Clone(values)
	if values == nil { values = map[string]any{} }
	var source *streamSource
	var streamMember memberDescription
	if op.Request.Streaming {
		streamMember = inputShape.Members[op.Request.Payload[0]]
		var err error
		if source, err = newStreamSource(values[inputShape.wireName(op.Request.Payload[0])]); err != nil { return nil, err }
		if source != nil {
			defer source.release()
			if err := source.check(streamMember, op.Request.RequiresLength); err != nil { return nil, err }
		} else if streamMember.Length != nil && streamMember.Length.Min != nil && *streamMember.Length.Min > 0 {
			return nil, fmt.Errorf("%w: an absent stream is below the modeled minimum", ErrStreamLength)
		}
	}
	checksum,err := prepareChecksum(op.Checksum,inputShape,values,c.config.RequestChecksumCalculation,c.config.ResponseChecksumValidation,source != nil)
	if err != nil { return nil,err }
	endpoint := c.config.Endpoint
	if c.schema.EndpointRules != nil || op.Endpoint != nil {
		input,err := c.queryModel(op.Input,values,""); if err != nil { return nil,err }
		if c.schema.EndpointRules != nil {
			builtins,_ := copyEndpointParameters(c.config.EndpointBuiltIns)
			if _,present := builtins["AWS::Region"]; !present && c.config.Region != "" { builtins["AWS::Region"] = c.config.Region }
			if _,present := builtins["SDK::Endpoint"]; !present && endpoint != "" { builtins["SDK::Endpoint"] = endpoint }
			resolved,err := resolveEndpoint(c.schema.EndpointRules,op.EndpointParameters,input,c.config.EndpointParameters,builtins); if err != nil { return nil,err }
			op.ResolvedEndpoint = resolved; endpoint = resolved.URL
		}
		if op.Endpoint != nil && !c.config.DisableHostPrefix { endpoint,err = prefixEndpoint(endpoint,op.Endpoint,input); if err != nil { return nil,err } }
	}
	if op.Token != "" && values[inputShape.wireName(op.Token)] == nil {
		id := make([]byte, 16)
		if _, err := rand.Read(id); err != nil {
			return nil, err
		}
		id[6] = (id[6] & 15) | 64
		id[8] = (id[8] & 63) | 128
		text := hex.EncodeToString(id)
		values[inputShape.wireName(op.Token)] = text[:8] + "-" + text[8:12] + "-" + text[12:16] + "-" + text[16:20] + "-" + text[20:]
	}
	boundValues := map[string]any{}
	for key, value := range values { boundValues[key] = value }
	for name, format := range op.Request.TimestampFormats {
		key := inputShape.wireName(name)
		var err error
		boundValues[key], err = boundMember(c.schema, inputShape.Members[name], values[key], format)
		if err != nil { return nil, err }
	}
	uri := op.URI
	// Telemetry shows where sensitive members went without their values.
	display := op.URI
	traced := c.config.Telemetry != nil
	sensitiveHeaders, sensitiveQuery := c.credentialHeaders, c.credentialQuery
	if traced {
		sensitiveHeaders, sensitiveQuery = maps.Clone(c.credentialHeaders), maps.Clone(c.credentialQuery)
	}
	for _, key := range op.Request.Labels {
		text, err := boundValue(boundValues[inputShape.wireName(key)])
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
		value := boundValues[inputShape.wireName(binding[0])]
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
	if key := op.Request.QueryParams; key != "" {
		if entries, ok := values[inputShape.wireName(key)].(map[string]any); ok {
			reserved := map[string]bool{}
			for _, binding := range op.Request.Query { reserved[binding[1]] = true }
			parsed, _ := url.Parse(uri)
			for name := range parsed.Query() { reserved[name] = true }
			keys := make([]string, 0, len(entries))
			for name := range entries { keys = append(keys, name) }
			sort.Strings(keys)
			for _, name := range keys {
				if reserved[name] { continue }
				if traced && inputShape.Members[key].Sensitive { sensitiveQuery[strings.ToLower(name)] = true }
				items, ok := entries[name].([]any)
				if !ok { items = []any{entries[name]} }
				for _, item := range items {
					text, err := boundValue(item)
					if err != nil { return nil, err }
					query = append(query, escape(name)+"="+escape(text))
				}
			}
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
	if op.ResolvedEndpoint != nil { for name,values := range op.ResolvedEndpoint.Headers { headers.Del(name); for _,value := range values { headers.Add(name,value) } } }
	reservedHeaders := map[string]bool{}
	for _, binding := range op.Request.Headers { reservedHeaders[strings.ToLower(binding[1])] = true }
	for _, binding := range op.Request.PrefixHeaders {
		if entries, ok := values[inputShape.wireName(binding[0])].(map[string]any); ok {
			for suffix, item := range entries {
				name := binding[1] + suffix
				if traced && inputShape.Members[binding[0]].Sensitive { sensitiveHeaders[strings.ToLower(name)] = true }
				if reservedHeaders[strings.ToLower(name)] { continue }
				if op.Request.HeaderEncodings[binding[0]] == "base64" { var err error; item,err = mediaValue(item,false); if err != nil { return nil,err } }
				text, err := boundValue(item)
				if err != nil { return nil, err }
				headers.Set(name, text)
			}
		}
	}
	// A Content-Type bound to an input member outranks the payload's media
	// type, which outranks endpoint and client header defaults.
	modelledContentType := false
	for _, binding := range op.Request.Headers {
		if traced && inputShape.Members[binding[0]].Sensitive { sensitiveHeaders[strings.ToLower(binding[1])] = true }
		value := boundValues[inputShape.wireName(binding[0])]
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
			key = inputShape.wireName(key)
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
	if source != nil {
		if !modelledContentType { headers.Set("Content-Type", op.Request.Payload[1]) }
		if source.sha256 != "" { headers.Set("X-Amz-Content-Sha256", source.sha256) }
	} else if len(op.Request.Payload) == 2 && !op.Request.Streaming {
		// The codec has already applied the protocol's null-member policy.
		if payload, present := values[inputShape.wireName(op.Request.Payload[0])]; present && (payload != nil || op.Request.PayloadKind == "document") {
			switch op.Request.PayloadKind {
			case "string": body = []byte(payload.(string))
			case "document": var err error; body, err = json.Marshal(payload); if err != nil { return nil, err }
			default: body = payload.([]byte)
			}
			if !modelledContentType { headers.Set("Content-Type", op.Request.Payload[1]) }
		}
	}
	if c.schema.Protocol == "awsJson1_0" || c.schema.Protocol == "awsJson1_1" {
		if body == nil { body = []byte("{}") }
		version := "1.1"; if c.schema.Protocol == "awsJson1_0" { version = "1.0" }
		headers.Set("Content-Type", "application/x-amz-json-" + version)
		service := strings.Split(c.schema.Service, "#")
		headers.Set("X-Amz-Target", service[len(service)-1] + "." + name)
	}
	// The release this client was generated for (docs/api-versions-clients.md).
	if c.schema.APIVersion != "" { headers.Set("Crucible-Version", c.schema.APIVersion) }
	discovery, err := c.discoveryRequest(name, op, inputShape, values)
	if err != nil { return nil, err }
	endpointSource := "configured"
	if c.schema.EndpointRules != nil { endpointSource = "rules" }
	maxAttempts := c.config.MaxAttempts
	// A one-shot stream is opened once, so its operation is never retried.
	if source != nil && !source.replayable { maxAttempts = 1 }
	lastCost := 0
	for attempt := 1; attempt <= maxAttempts; attempt++ {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		discovered, err := c.discover(ctx, discovery)
		if err != nil { return nil, err }
		attemptEndpoint, attemptSource := strings.TrimRight(endpoint, "/"), endpointSource
		// A discovered address replaces the endpoint, host prefix included.
		if discovered != nil { attemptEndpoint, attemptSource = discovered.url, "discovered" }
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
		request, err := http.NewRequestWithContext(ctx, op.Method, attemptEndpoint+uri, bytes.NewReader(body))
		if err != nil {
			return nil, err
		}
		var stream *streamBody
		if source != nil {
			stream = newStreamBody(source, streamMember)
			stream.install(request)
		}
		request.Header = headers.Clone()
		credentials, sign, err := c.authenticate(ctx, request, op)
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
		if _, isStream := request.Body.(*streamBody); isStream && c.digest {
			// Streams are never buffered to hash them: the digest is declared.
			if request.Header.Get("X-Amz-Content-Sha256") == "" { return nil, fmt.Errorf("%w: declare it with ByteStream.WithSHA256 or set x-amz-content-sha256", ErrStreamDigest) }
		} else if c.digest && (request.Body != nil || request.Method == "POST" || request.Method == "PUT" || request.Method == "PATCH") {
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
		if err := checksum.request(request); err != nil { return nil,err }
		if sign != nil { if err := sign(); err != nil { return nil, err } }
		if err := ctx.Err(); err != nil { return nil, err }
		trace.attemptStarted(request, attemptEndpoint, attemptSource, display, sensitiveHeaders, sensitiveQuery)
		response, transportErr := c.config.HTTPClient.Do(request)
		// interrupted ends an attempt with a failure that is never retried; a
		// response that arrived still counts toward the measured rate.
		interrupted := func(size int64, err error) (map[string]any, error) {
			if response != nil { c.retry.afterAttempt(false) }
			trace.attemptFinished(response, err, size, "", 0)
			return nil, err
		}
		if stream != nil {
			if err := ctx.Err(); err != nil {
				if response != nil { response.Body.Close() }
				return interrupted(-1, err)
			}
			if err := stream.err(); err != nil {
				// The caller's stream failed or broke its length: never a retried NetworkError.
				if response != nil { response.Body.Close() }
				return interrupted(-1, err)
			}
		}
		if transportErr == nil && op.Response.Streaming && response.StatusCode >= 200 && response.StatusCode < 300 {
			result, err := c.streamResponse(ctx, cancel, name, op, response, checksum)
			if err != nil { response.Body.Close(); return interrupted(-1, err) }
			streamed = true
			c.retry.afterAttempt(false)
			c.retry.succeeded(lastCost)
			trace.attemptFinished(response, nil, response.ContentLength, "", 0)
			return result, nil
		}
		var data []byte
		size := int64(-1)
		if response != nil {
			if transportErr == nil && (response.StatusCode < 200 || response.StatusCode >= 300 || len(op.Response.Body) > 0 || len(op.Response.Payload) > 0 || len(checksum.Response)>0) {
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
			if response.StatusCode >= 200 && response.StatusCode < 300 {
				algorithm,err := checksum.response(response.Header,data); if err != nil { return interrupted(size, err) }
				if algorithm != "" && c.config.ChecksumValidated != nil { c.config.ChecksumValidated(name,algorithm) }
			}
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
		if discovered != nil && c.invalidates(lastErr) { c.discovery.invalidate(discovery.key, discovered) }
		reason := c.classify(op, transportErr, lastErr, discovered)
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
		// Fence the attempt's body before a retry reopens or rewinds its source.
		if stream != nil { stream.Close() }
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
	for _, prefix := range binding.PrefixHeaders {
		entries := map[string]any{}
		for key, value := range response.Header {
			if strings.HasPrefix(strings.ToLower(key), strings.ToLower(prefix[1])) {
				var item any = strings.Join(value, ", ")
				if binding.HeaderEncodings[prefix[0]] == "base64" { var err error; item,err = mediaValue(item,true); if err != nil { return err } }
				entries[key[len(prefix[1]):]] = item
			}
		}
		values[shape.wireName(prefix[0])] = entries
	}
	if binding.ResponseCode != "" { values[shape.wireName(binding.ResponseCode)] = json.Number(strconv.Itoa(response.StatusCode)) }
	for _, entry := range binding.Headers {
		if text, present := response.Header[http.CanonicalHeaderKey(entry[1])]; present {
			value, err := headerValue(c.schema, shape.Members[entry[0]], strings.Join(text, ", "), binding.TimestampFormats[entry[0]])
			if err != nil { return err }
			if binding.HeaderEncodings[entry[0]] == "base64" { value, err = mediaValue(value, true); if err != nil { return err } }
			values[shape.wireName(entry[0])] = value
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
			values[c.schema.Shapes[op.Output].wireName(op.Response.Payload[0])] = value
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
	if value, err := parseJSON(data); err == nil {
		if object, ok := value.(map[string]any); ok {
			values = object
		}
	}
	name, _ := values["__type"].(string)
	if header := response.Header.Get("X-Crucible-Error"); header != "" { name = header }
	awsJSON := c.schema.Protocol == "awsRestJson1" || c.schema.Protocol == "awsJson1_0" || c.schema.Protocol == "awsJson1_1"
	if awsJSON {
		if code, ok := values["code"].(string); ok { name = code }
		if header := response.Header.Get("X-Amzn-Errortype"); header != "" { name = header }
		name = strings.SplitN(name, ":", 2)[0]
		if index := strings.LastIndex(name, "#"); index >= 0 { name = name[index+1:] }
	}
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
	if !ok && awsJSON { message, ok = values["Message"].(string) }
	if !ok {
		message = fmt.Sprintf("HTTP %d", response.StatusCode)
	}
	metadata := &ServiceError{Name: name, Message: message, Metadata: ResponseMetadata{Status: response.StatusCode, RequestID: response.Header.Get("x-request-id"), Headers: response.Header.Clone(), Body: bytes.Clone(data)}}
	if awsJSON {
		if id := response.Header.Get("X-Amzn-Requestid"); id != "" { metadata.Metadata.RequestID = id } else if id := response.Header.Get("X-Amz-Request-Id"); id != "" { metadata.Metadata.RequestID = id }
	}
	if metadata.Metadata.RequestID == "" {
		metadata.Metadata.RequestID, _ = values["requestId"].(string)
	}
	if target != "" {
		shape := c.schema.Shapes[target]
		metadata.Retryable = shape.Error.Retryable
		metadata.Throttling = shape.Error.Throttling
		if len(shape.Error.Payload) == 2 {
			value, err := payloadValue(shape.Error.PayloadKind, data)
			if err != nil { return nil, metadata }
			values = map[string]any{shape.wireName(shape.Error.Payload[0]): value}
		}
		if err := c.responseBindings(target, bindings{Headers:shape.Error.Headers, PrefixHeaders:shape.Error.PrefixHeaders, ResponseCode:shape.Error.ResponseCode, TimestampFormats:shape.Error.TimestampFormats, HeaderEncodings:shape.Error.HeaderEncodings}, response, values); err != nil {
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
