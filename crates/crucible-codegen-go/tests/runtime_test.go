package weather

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"reflect"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func TestSendOwnsChecksumDefaultsAndIdempotencyTokens(t *testing.T) {
	for _, scenario := range []struct {
		name string
		calls int
		retry bool
		supplied string
		nilInput bool
	}{
		{name: "separate calls generate separate tokens", calls: 2},
		{name: "retry reuses generated token", calls: 1, retry: true},
		{name: "supplied token survives calls and retry", calls: 2, retry: true, supplied: "caller-token"},
		{name: "nil input accepts private defaults", calls: 1, nilInput: true},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			type requestValues struct { token, algorithm, validation, checksum string }
			observed := make(chan requestValues, 4)
			var attempts atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				var body map[string]any
				if err := json.NewDecoder(r.Body).Decode(&body); err != nil { t.Error(err); w.WriteHeader(http.StatusBadRequest); return }
				token, _ := body["wire-token"].(string)
				observed <- requestValues{token, r.Header.Get("x-algorithm"), r.Header.Get("x-validation"), r.Header.Get("x-amz-checksum-crc32")}
				if attempts.Add(1) == 1 && scenario.retry { w.WriteHeader(http.StatusInternalServerError); return }
				w.WriteHeader(http.StatusNoContent)
			}))
			defer server.Close()
			schema := &schemaDescription{
				Shapes: map[string]shapeDescription{
					"text": {Kind: "string"},
					"input": {Kind: "structure", Members: map[string]memberDescription{
						"token": {Target: "text", JSONName: "wire-token"},
						"algorithm": {Target: "text", JSONName: "wire-algorithm"},
						"validation": {Target: "text", JSONName: "wire-validation"},
						"payload": {Target: "text"},
					}},
				},
				Operations: map[string]operationDescription{"Write": {
					Method: "POST", URI: "/write", Input: "input", Safe: true, Token: "token",
					Request: bindings{Body: []string{"token", "payload"}, Headers: [][2]string{{"algorithm", "x-algorithm"}, {"validation", "x-validation"}}},
					Checksum: &checksumDescription{RequestAlgorithmMember: "algorithm", ValidationModeMember: "validation"},
				}},
			}
			client, err := newClient(Config{Endpoint: server.URL, MaxAttempts: 2, BaseDelay: time.Nanosecond, MaxDelay: time.Nanosecond}, schema, false)
			if err != nil { t.Fatal(err) }
			values := map[string]any{"payload": "unchanged"}
			if scenario.supplied != "" { values["wire-token"] = scenario.supplied }
			if scenario.nilInput { values = nil }
			var original map[string]any
			if values != nil { original = map[string]any{}; for key, value := range values { original[key] = value } }
			for call := 0; call < scenario.calls; call++ {
				if _, err := client.callValues(context.Background(), "Write", values); err != nil { t.Fatal(err) }
				if !reflect.DeepEqual(values, original) { t.Fatalf("caller values changed: got %#v, want %#v", values, original) }
			}
			count := scenario.calls
			if scenario.retry { count++ }
			if int(attempts.Load()) != count { t.Fatalf("attempts: got %d, want %d", attempts.Load(), count) }
			requests := make([]requestValues, count)
			for index := range requests {
				requests[index] = <-observed
				request := requests[index]
				if request.token == "" || request.algorithm != "CRC32" || request.validation != "ENABLED" || request.checksum == "" { t.Fatalf("request preparation missing: %#v", request) }
				if scenario.supplied != "" && request.token != scenario.supplied { t.Fatalf("supplied token changed: %q", request.token) }
			}
			if scenario.retry && requests[0].token != requests[1].token { t.Fatal("retry generated another token") }
			if scenario.calls == 2 && scenario.supplied == "" && requests[0].token == requests[1].token { t.Fatal("separate calls reused a generated token") }
		})
	}
}

func clientFor(t *testing.T, handler http.HandlerFunc) (*WeatherClient, *httptest.Server) {
	t.Helper()
	server := httptest.NewServer(handler)
	t.Cleanup(server.Close)
	client, err := NewWeatherClient(Config{Endpoint: server.URL, BaseDelay: time.Nanosecond, MaxDelay: time.Nanosecond})
	if err != nil {
		t.Fatal(err)
	}
	return client, server
}

func TestNativeAccessorsKeepPresenceAndUnionSelection(t *testing.T) {
	input := &ListCitiesInput{}
	if input.HasTags() || input.IsNullTags() || input.GetTags() != nil {
		t.Fatal("initial presence")
	}
	input.SetTags([]string{})
	input.SetMaxResults(0)
	if !input.HasTags() || input.GetTags() == nil || input.IsNullTags() || !input.HasMaxResults() {
		t.Fatal("empty and zero must remain present")
	}
	input.Tags = Null[Tags]()
	if !input.HasTags() || !input.IsNullTags() || input.GetTags() != nil {
		t.Fatal("explicit null")
	}
	input.SetTags([]string{"a", "b"})
	if input.IsNullTags() || len(input.GetTags()) != 2 {
		t.Fatal("setter must replace null")
	}
	encoded, err := encodeModel(wireSchema, "example.weather#ListCitiesInput", input)
	if err != nil || encoded["maxResults"] != int32(0) {
		t.Fatalf("%#v %v", encoded, err)
	}
	location := &Location{}
	location.SetUnknown(Unit{})
	location.SetCoordinates(&Coordinates{Latitude: 1, Longitude: 2})
	if location.HasUnknown() || !location.HasCoordinates() {
		t.Fatal("setting a union arm must replace the previous arm")
	}
	if _, err := encodeModel(wireSchema, "example.weather#Location", location); err != nil {
		t.Fatal(err)
	}
	var absent *ListCitiesInput
	if absent.GetTags() != nil || absent.HasTags() {
		t.Fatal("nil optional getter")
	}
}
func TestSharedRetryCases(t *testing.T) {
	data, err := os.ReadFile("retry.json")
	if err != nil {
		t.Fatal(err)
	}
	var cases []struct {
		Name        string
		Safe        bool
		Status      int
		Error       *string
		Failures    int
		MaxAttempts int
		Attempts    int
		Success     bool
	}
	if err := json.Unmarshal(data, &cases); err != nil {
		t.Fatal(err)
	}
	for _, test := range cases {
		t.Run(test.Name, func(t *testing.T) {
			attempts := 0
			client, _ := clientFor(t, func(w http.ResponseWriter, r *http.Request) {
				attempts++
				if attempts <= test.Failures {
					w.WriteHeader(test.Status)
				}
				if test.Error != nil {
					json.NewEncoder(w).Encode(map[string]any{"__type": *test.Error})
				} else {
					io.WriteString(w, "{}")
				}
			})
			client.config.MaxAttempts = test.MaxAttempts
			shapes := map[string]shapeDescription{}
			for _, name := range []string{"Ordinary", "Transient", "Throttled"} {
				status := 400
				if name == "Throttled" {
					status = 429
				}
				shapes[name] = shapeDescription{Name: name, Error: &errorDescription{Status: status, Retryable: name != "Ordinary", Throttling: name == "Throttled"}}
			}
			client.schema = &schemaDescription{Shapes: shapes, Operations: map[string]operationDescription{"Retry": {Method: "POST", URI: "/", Safe: test.Safe, Errors: []string{"Ordinary", "Transient", "Throttled"}}}}
			_, err := client.call(context.Background(), "Retry", Unit{})
			if attempts != test.Attempts || (err == nil) != test.Success {
				t.Fatalf("attempts=%d success=%v error=%v", attempts, err == nil, err)
			}
		})
	}
}
func TestHeaderLists(t *testing.T) {
	data, err := os.ReadFile("header-lists.json")
	if err != nil {
		t.Fatal(err)
	}
	var cases []struct {
		Wire    string
		Encoded string
		Values  []any
	}
	if err := json.Unmarshal(data, &cases); err != nil {
		t.Fatal(err)
	}
	for _, test := range cases {
		value, err := headerValue(wireSchema, memberDescription{Target: "example.weather#Tags"}, test.Wire)
		if err != nil || !reflect.DeepEqual(value, test.Values) {
			t.Fatalf("%q => %#v, %v", test.Wire, value, err)
		}
		encoded, err := boundValue(test.Values)
		if err != nil || encoded != test.Encoded {
			t.Fatalf("encode %#v => %q, %v", test.Values, encoded, err)
		}
		roundTrip, err := headerValue(wireSchema, memberDescription{Target: "example.weather#Tags"}, encoded)
		if err != nil || !reflect.DeepEqual(roundTrip, test.Values) {
			t.Fatalf("roundtrip %#v => %#v, %v", test.Values, roundTrip, err)
		}
	}
}
func TestUnquotedHTTPDateHeaderLists(t *testing.T) {
	schema := &schemaDescription{Shapes: map[string]shapeDescription{
		"example#Time":  {Kind: "timestamp"},
		"example#Times": {Kind: "list", Member: memberDescription{Target: "example#Time", Format: "http-date"}},
	}}
	member := memberDescription{Target: "example#Times"}
	first, second := "Mon, 16 Dec 2019 23:48:18 GMT", "Tue, 17 Dec 2019 23:48:18 GMT"
	for wire, want := range map[string][]any{
		first + ", " + second: {first, second},
		first:                 {first},
		`"` + first + `"`:     {first},
	} {
		value, err := headerValue(schema, member, wire)
		if err != nil || !reflect.DeepEqual(value, want) {
			t.Fatalf("%q => %#v, %v", wire, value, err)
		}
	}
}
func TestNonFiniteFloatHeaders(t *testing.T) {
	schema := &schemaDescription{Shapes: map[string]shapeDescription{"example#Ratio": {Kind: "float"}}}
	schema.ProtocolSettings.NonFiniteFloatStrings = true
	member := memberDescription{Target: "example#Ratio"}
	for _, text := range []string{"NaN", "Infinity", "-Infinity"} {
		if value, err := headerValue(schema, member, text); err != nil || value != text {
			t.Fatalf("%q => %#v, %v", text, value, err)
		}
	}
	if value, err := headerValue(schema, member, "1.5"); err != nil || value != json.Number("1.5") {
		t.Fatalf("finite => %#v, %v", value, err)
	}
	schema.ProtocolSettings.NonFiniteFloatStrings = false
	if _, err := headerValue(schema, member, "NaN"); err == nil {
		t.Fatal("NaN accepted by a protocol without non-finite floats")
	}
}
func TestMalformedErrorMetadataAndRetry(t *testing.T) {
	data, err := os.ReadFile("malformed-errors.json")
	if err != nil {
		t.Fatal(err)
	}
	var cases []struct {
		Name, Body, Error, Message, RequestID string
		Status                                int
		Headers                               map[string]string
		Retryable, Throttling                 bool
	}
	if err := json.Unmarshal(data, &cases); err != nil {
		t.Fatal(err)
	}
	for _, test := range cases {
		for _, limit := range []int{1, 2} {
			t.Run(fmt.Sprintf("%s/%d", test.Name, limit), func(t *testing.T) {
				attempts := 0
				client, _ := clientFor(t, func(w http.ResponseWriter, r *http.Request) {
					attempts++
					if attempts > 1 {
						w.WriteHeader(200)
						return
					}
					for name, value := range test.Headers {
						w.Header().Set(name, value)
					}
					w.WriteHeader(test.Status)
					io.WriteString(w, test.Body)
				})
				client.config.MaxAttempts = limit
				_, err := client.Ping(context.Background(), Unit{})
				if attempts != limit {
					t.Fatalf("attempts=%d want %d", attempts, limit)
				}
				if limit == 2 {
					if err != nil {
						t.Fatal(err)
					}
					return
				}
				var base WeatherException
				if !errors.As(err, &base) { t.Fatalf("expected service base: %v", err) }
				var service *ServiceError
				if !errors.As(err, &service) {
					t.Fatalf("expected service error, got %T: %v", err, err)
				}
				if service.Name != test.Error || service.Message != test.Message || service.Metadata.Status != test.Status || service.Metadata.RequestID != test.RequestID || string(service.Metadata.Body) != test.Body || service.Retryable != test.Retryable || service.Throttling != test.Throttling {
					t.Fatalf("lost wire metadata: %#v %#v", service, service.Metadata)
				}
				for name, value := range test.Headers {
					if service.Metadata.Headers.Get(name) != value {
						t.Fatalf("lost header %s", name)
					}
				}
			})
		}
	}
}
func TestExplicitNullDoesNotUseDefault(t *testing.T) {
	var input *ListCitiesInput
	if err := decodeModel(wireSchema, "example.weather#ListCitiesInput", map[string]any{}, &input); err != nil || input.HasMaxResults() {
		t.Fatalf("input defaults must remain absent: %v", err)
	}
	type record struct{ Count Optional[int32] }
	schema := &schemaDescription{Shapes: map[string]shapeDescription{
		"record":  {Kind: "structure", Members: map[string]memberDescription{"count": {Attribute: "Count", Target: "integer", Default: json.Number("7")}}},
		"integer": {Kind: "integer"},
	}}
	var absent, explicit record
	if err := decodeModel(schema, "record", map[string]any{}, &absent); err != nil {
		t.Fatal(err)
	}
	if err := decodeModel(schema, "record", map[string]any{"count": nil}, &explicit); err != nil {
		t.Fatal(err)
	}
	if absent.Count.Value != 7 || !explicit.Count.Null {
		t.Fatal("absence and null conflated")
	}
}
func TestBindingsTokensDigestAndAuth(t *testing.T) {
	sent := [][]byte{}
	auth := []string{}
	credential := 0
	client, _ := clientFor(t, func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		sent = append(sent, body)
		auth = append(auth, r.Header.Get("Authorization"))
		sum := sha256.Sum256(body)
		if r.Header.Get("x-amz-content-sha256") != hex.EncodeToString(sum[:]) {
			t.Error("wrong body digest")
		}
		if r.Header.Get("x-expected-revision") != "7" {
			t.Errorf("missing bound header: %v", r.Header)
		}
		if bytes.Contains(body, []byte("expectedRevision")) {
			t.Error("header also in body")
		}
		if len(sent) == 1 {
			w.WriteHeader(503)
			io.WriteString(w, `{"__type":"Unknown"}`)
			return
		}
		io.WriteString(w, `{"city":{"id":"c1","name":"é"}}`)
	})
	client.config.Token = func(context.Context) (string, error) { credential++; return string(rune('0' + credential)), nil }
	input := &CreateCityInput{Name: Some[CityName]("é"), ExpectedRevision: Some[int64](7), Location: Some(&Location{Unknown: Some(Unit{})})}
	output, err := client.CreateCity(context.Background(), input)
	if err != nil {
		t.Fatal(err)
	}
	if output.City.Name != "é" || output.City.Population != 0 {
		t.Fatal(output)
	}
	if input.ClientToken.Set || len(sent) != 2 || !bytes.Equal(sent[0], sent[1]) || auth[0] != "Bearer 1" || auth[1] != "Bearer 2" {
		t.Fatalf("retry ownership/auth failed: %v", auth)
	}
	if !bytes.Contains(sent[0], []byte(`"unknown":{}`)) || !bytes.Contains(sent[0], []byte(`"clientToken":`)) {
		t.Fatalf("invalid union or token: %s", sent[0])
	}
}
func TestPathQueryAndResponseBindings(t *testing.T) {
	client, _ := clientFor(t, func(w http.ResponseWriter, r *http.Request) {
		if !strings.Contains(r.RequestURI, "a%2Fb%20c~%C3%A9") {
			t.Errorf("label encoding: %s", r.RequestURI)
		}
		if r.URL.Query().Get("at") != "2026-10-01T00:00:00.000000123Z" {
			t.Errorf("timestamp query: %s", r.RequestURI)
		}
		w.Header().Set("x-request-id", "req-1")
		io.WriteString(w, `{"city":{"id":"c1","name":"city","population":9223372036854775807,"createdAt":-0.000000001},"forecast":{"new-sky":{"n":9223372036854775807}}}`)
	})
	output, err := client.GetCity(context.Background(), &GetCityInput{CityId: Some[CityId]("a/b c~é"), At: Some(time.Date(2026, 10, 1, 0, 0, 0, 123, time.UTC))})
	if err != nil {
		t.Fatal(err)
	}
	if output.City.Population != 9223372036854775807 || output.City.CreatedAt.Value.UnixNano() != -1 || output.RequestId.Value != "req-1" {
		t.Fatal(output)
	}
	if output.Forecast.Value[Sky("new-sky")].(map[string]any)["n"] != json.Number("9223372036854775807") {
		t.Fatal("document precision lost")
	}
}
func TestUnknownUnionAndAbsence(t *testing.T) {
	var value *Location
	raw, err := parseJSON([]byte(`{"newVariant":{"n":9223372036854775807}}`))
	if err != nil {
		t.Fatal(err)
	}
	if err := decodeModel(wireSchema, "example.weather#Location", raw, &value); err != nil {
		t.Fatal(err)
	}
	encoded, err := encodeModel(wireSchema, "example.weather#Location", value)
	if err != nil {
		t.Fatal(err)
	}
	data, err := json.Marshal(encoded)
	if err != nil || string(data) != `{"newVariant":{"n":9223372036854775807}}` {
		t.Fatalf("%s %v", data, err)
	}
	input := &CreateCityInput{Name: Some[CityName]("x"), Location: Null[*Location]()}
	encoded, err = encodeModel(wireSchema, "example.weather#CreateCityInput", input)
	if err != nil {
		t.Fatal(err)
	}
	if _, present := encoded["clientToken"]; present {
		t.Fatal("unset member emitted")
	}
	if value, present := encoded["location"]; !present || value != nil {
		t.Fatal("explicit null lost")
	}
	if _, err := encodeModel(wireSchema, "example.weather#Location", &Location{}); err == nil {
		t.Fatal("empty union accepted")
	}
}
func TestModeledErrorsAndDecodeFailures(t *testing.T) {
	attempts := 0
	client, _ := clientFor(t, func(w http.ResponseWriter, r *http.Request) {
		attempts++
		w.Header().Set("x-request-id", "request")
		w.WriteHeader(400)
		io.WriteString(w, `{"__type":"ValidationError","message":"bad","reason":"fieldValidationFailed"}`)
	})
	_, err := client.Ping(context.Background(), Unit{})
	var modeled *ValidationError
	var serviceException WeatherException
	if !errors.As(err, &serviceException) { t.Fatalf("expected service exception: %v", err) }
	var generic *ServiceError
	if !errors.As(err, &modeled) || !errors.As(err, &generic) || modeled.Reason != ValidationReasonFIELDVALIDATIONFAILED || generic.Metadata.RequestID != "request" || attempts != 1 {
		t.Fatalf("wrong error: %#v", err)
	}
	client2, _ := clientFor(t, func(w http.ResponseWriter, r *http.Request) { io.WriteString(w, "invalid") })
	_, err = client2.ListCities(context.Background(), nil)
	var network *NetworkError
	if err == nil || errors.As(err, &network) {
		t.Fatalf("wrong decode failure: %v", err)
	}
}
func TestPayloadsTravelRawAndBlobsInJSONAreBase64(t *testing.T) {
	// Not UTF-8, and JSON-looking in places: neither may be reinterpreted.
	photo := []byte("\xff\xd8{\"a\":1}\x00\x80")
	calls := 0
	client, _ := clientFor(t, func(w http.ResponseWriter, r *http.Request) {
		calls++
		switch calls {
		case 1:
			body, _ := io.ReadAll(r.Body)
			sum := sha256.Sum256(photo)
			if !bytes.Equal(body, photo) || r.Header.Get("Content-Type") != "image/jpeg" || r.Header.Get("x-photo-caption") != "harbour" || r.Header.Get("x-amz-content-sha256") != hex.EncodeToString(sum[:]) {
				t.Errorf("payload not sent raw: %q %v", body, r.Header)
			}
			io.WriteString(w, `{"photo":{"size":11,"thumbnail":"/9h7ImEiOjF9","caption":"harbour"}}`)
		case 2:
			w.Header().Set("Content-Type", "image/jpeg")
			w.Header().Set("x-photo-caption", "harbour")
			w.Write(photo)
		default:
			w.WriteHeader(200)
		}
	})
	stored, err := client.PutCityPhoto(context.Background(), &PutCityPhotoInput{CityId: Some[CityId]("c-00000001"), Caption: Some("harbour"), Photo: Some[Photo](photo)})
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(stored.Photo.Thumbnail, photo[:9]) {
		t.Fatalf("base64 thumbnail: %q", stored.Photo.Thumbnail)
	}
	read, err := client.GetCityPhoto(context.Background(), &GetCityPhotoInput{CityId: Some[CityId]("c-00000001")})
	if err != nil || !bytes.Equal(read.Photo.Value, photo) || read.Caption.Value != "harbour" {
		t.Fatalf("%v %v", read, err)
	}
	empty, err := client.GetCityPhoto(context.Background(), &GetCityPhotoInput{CityId: Some[CityId]("c-00000001")})
	if err != nil || !empty.Photo.Set || len(empty.Photo.Value) != 0 {
		t.Fatalf("empty payload: %v %v", empty, err)
	}
	encoded, err := encodeModel(wireSchema, "example.weather#PhotoSummary", &PhotoSummary{Size: 1, Thumbnail: Thumbnail(photo[:9])})
	if err != nil {
		t.Fatal(err)
	}
	data, err := json.Marshal(encoded)
	if err != nil || !bytes.Contains(data, []byte(`"thumbnail":"/9h7ImEiOjF9"`)) {
		t.Fatalf("%s %v", data, err)
	}
	var summary *PhotoSummary
	if err := decodeModel(wireSchema, "example.weather#PhotoSummary", map[string]any{"size": json.Number("1"), "thumbnail": "not base64!"}, &summary); err == nil {
		t.Fatal("invalid base64 accepted")
	}
}
func TestPaginationCopiesInputAndRejectsCycles(t *testing.T) {
	calls := 0
	client, _ := clientFor(t, func(w http.ResponseWriter, r *http.Request) {
		calls++
		if calls == 2 && r.URL.Query().Get("nextToken") != "same" {
			t.Error("token not forwarded")
		}
		if strings.Join(r.URL.Query()["tag"], ",") != "a,b" {
			t.Error("repeated query lost")
		}
		io.WriteString(w, `{"cities":[],"nextToken":"same"}`)
	})
	input := &ListCitiesInput{Tags: Some(Tags{"a", "b"})}
	pages, err := client.ListCitiesPages(input)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := pages.NextPage(context.Background()); err != nil {
		t.Fatal(err)
	}
	if _, err := pages.NextPage(context.Background()); !errors.Is(err, errRepeatedToken) {
		t.Fatal(err)
	}
	if input.NextToken.Set || pages.HasMorePages() {
		t.Fatal("caller mutation or cycle did not stop")
	}
}

type transportFunc func(*http.Request) (*http.Response, error)

func (f transportFunc) Do(r *http.Request) (*http.Response, error) { return f(r) }

type brokenBody struct{}

func (brokenBody) Read([]byte) (int, error) { return 0, io.ErrUnexpectedEOF }
func (brokenBody) Close() error             { return nil }
func TestTransportFailureAndCancellation(t *testing.T) {
	attempts := 0
	client, err := NewWeatherClient(Config{Endpoint: "http://localhost", BaseDelay: time.Nanosecond, MaxDelay: time.Nanosecond, HTTPClient: transportFunc(func(r *http.Request) (*http.Response, error) {
		attempts++
		return &http.Response{StatusCode: 200, Header: http.Header{}, Body: brokenBody{}}, nil
	})})
	if err != nil {
		t.Fatal(err)
	}
	_, err = client.ListCities(context.Background(), nil)
	var network *NetworkError
	if !errors.As(err, &network) || attempts != 3 {
		t.Fatalf("body retries: %d %v", attempts, err)
	}
	attempts = 0
	if _, err = client.Ping(context.Background(), Unit{}); err != nil || attempts != 1 {
		t.Fatal("unit body was read")
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	attempts = 0
	_, err = client.ListCities(ctx, nil)
	if !errors.Is(err, context.Canceled) || attempts != 0 {
		t.Fatal("cancelled call sent")
	}
	client.config.HTTPClient = transportFunc(func(r *http.Request) (*http.Response, error) {
		attempts++
		<-r.Context().Done()
		return nil, r.Context().Err()
	})
	ctx, cancel = context.WithTimeout(context.Background(), 10*time.Millisecond)
	defer cancel()
	_, err = client.ListCities(ctx, nil)
	if !errors.Is(err, context.DeadlineExceeded) || attempts != 1 {
		t.Fatal("deadline retried")
	}
}

func TestCleartextCredentialsAndRetryAfter(t *testing.T) {
	client, err := NewWeatherClient(Config{Endpoint: "http://example.invalid", Token: func(context.Context) (string, error) { return "secret", nil }, HTTPClient: transportFunc(func(*http.Request) (*http.Response, error) { t.Fatal("credentials leaked"); return nil, nil })})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := client.Ping(context.Background(), Unit{}); err == nil || !strings.Contains(err.Error(), "plaintext") {
		t.Fatalf("%v", err)
	}
	response := &http.Response{Header: http.Header{"Retry-After": []string{"1.5"}}}
	if delay := retryDelay(1, response, time.Second, time.Second); delay != 1500*time.Millisecond {
		t.Fatal(delay)
	}
	response.Header.Set("Retry-After", time.Now().Add(5*time.Second).UTC().Format(http.TimeFormat))
	if delay := retryDelay(1, response, time.Second, time.Second); delay < 3*time.Second || delay > 5*time.Second {
		t.Fatal(delay)
	}
}

func TestDigestPreservesAnEmptyRequestBody(t *testing.T) {
	client, _ := clientFor(t, func(w http.ResponseWriter, r *http.Request) {
		if len(r.TransferEncoding) != 0 || r.ContentLength != 0 {
			t.Errorf("empty body changed framing: %v %d", r.TransferEncoding, r.ContentLength)
		}
		w.WriteHeader(200)
	})
	op := client.schema.Operations["Ping"]
	op.Method = "POST"
	client.schema = &schemaDescription{Shapes: client.schema.Shapes, Operations: map[string]operationDescription{"Ping": op}}
	if _, err := client.Ping(context.Background(), Unit{}); err != nil {
		t.Fatal(err)
	}
}

func TestRetryAfterBeyondDeadlinePreservesError(t *testing.T) {
	calls := 0
	client, _ := clientFor(t, func(w http.ResponseWriter, r *http.Request) {
		calls++
		w.Header().Set("Retry-After", "120")
		w.WriteHeader(429)
		io.WriteString(w, `{"__type":"ThrottlingError","message":"slow down"}`)
	})
	client.config.Timeout = time.Second
	start := time.Now()
	_, err := client.Ping(context.Background(), Unit{})
	var throttle *ThrottlingError
	if !errors.As(err, &throttle) || calls != 1 || time.Since(start) > 500*time.Millisecond {
		t.Fatalf("%v after %s", err, time.Since(start))
	}
}
func TestHTTPWeatherService(t *testing.T) {
	endpoint := os.Getenv("WEATHER_URL")
	if endpoint == "" {
		t.Skip("weather service URL is supplied by tools/test-go.py")
	}
	client, err := NewWeatherClient(Config{Endpoint: endpoint})
	if err != nil {
		t.Fatal(err)
	}
	input := &CreateCityInput{}
	input.SetName("go-http")
	location := &Location{}
	location.SetUnknown(Unit{})
	input.SetLocation(location)
	city, err := client.CreateCity(context.Background(), input)
	if err != nil {
		t.Fatal(err)
	}
	get := &GetCityInput{}
	get.SetCityId(city.City.Id)
	got, err := client.GetCity(context.Background(), get)
	if err != nil || got.City.Name != "go-http" {
		t.Fatalf("%v %v", got, err)
	}
	_, err = client.CreateCity(context.Background(), &CreateCityInput{Name: Some[CityName]("go-http")})
	var conflict *ConflictError
	if !errors.As(err, &conflict) {
		t.Fatal(err)
	}
	list := &ListCitiesInput{}
	list.SetMaxResults(1)
	pages, err := client.ListCitiesPages(list)
	if err != nil {
		t.Fatal(err)
	}
	found := false
	for pages.HasMorePages() {
		page, err := pages.NextPage(context.Background())
		if err != nil {
			t.Fatal(err)
		}
		for _, city := range page.Cities {
			found = found || city.Name == "go-http"
		}
	}
	if !found {
		t.Fatal("created city missing")
	}
	photo := []byte("\xff\xd8{\"looks\": \"like json\"}\x00\x80")
	stored, err := client.PutCityPhoto(context.Background(), &PutCityPhotoInput{CityId: Some(city.City.Id), Caption: Some("aurora"), Photo: Some[Photo](photo)})
	if err != nil || stored.Photo.Size != int32(len(photo)) || !bytes.Equal(stored.Photo.Thumbnail, photo[:8]) {
		t.Fatalf("%v %v", stored, err)
	}
	read, err := client.GetCityPhoto(context.Background(), &GetCityPhotoInput{CityId: Some(city.City.Id)})
	if err != nil || !bytes.Equal(read.Photo.Value, photo) || read.Caption.Value != "aurora" {
		t.Fatalf("%v %v", read, err)
	}
	_, err = client.PutCityPhoto(context.Background(), &PutCityPhotoInput{CityId: Some(city.City.Id), Photo: Some[Photo](make([]byte, 4097))})
	var invalid *ValidationError
	if !errors.As(err, &invalid) {
		t.Fatal(err)
	}
}

func TestHardeningAuthEmptyDigestDotLabelsAndDenseCollections(t *testing.T) {
	calls := 0
	client, _ := clientFor(t, func(w http.ResponseWriter, r *http.Request) {
		calls++
		if r.Header.Get("Authorization") != "" {
			t.Error("anonymous credentials")
		}
		body, err := io.ReadAll(r.Body)
		if err != nil || len(body) != 0 {
			t.Error("expected no body")
		}
		hash := sha256.Sum256(nil)
		expected := hex.EncodeToString(hash[:])
		if r.Method == "GET" || r.Method == "DELETE" {
			expected = ""
		}
		if r.Header.Get("x-amz-content-sha256") != expected {
			t.Error("bodiless digest")
		}
		if expected == "" && len(r.Header.Values("x-amz-content-sha256")) != 0 {
			t.Error("unexpected digest header")
		}
		w.WriteHeader(200)
	})
	client.config.Token = func(context.Context) (string, error) {
		t.Fatal("anonymous credential provider invoked")
		return "", nil
	}
	operations := make(map[string]operationDescription)
	for name, op := range client.schema.Operations {
		operations[name] = op
	}
	client.schema = &schemaDescription{Shapes: client.schema.Shapes, Operations: operations, Bearer: true}
	op := operations["Ping"]
	op.Bearer = false
	for _, method := range []string{"POST", "PUT", "PATCH", "GET", "DELETE"} {
		op.Method = method
		operations["Ping"] = op
		if _, err := client.Ping(context.Background(), Unit{}); err != nil {
			t.Fatal(err)
		}
	}
	for _, label := range []string{".", ".."} {
		input := &GetCityInput{}
		input.SetCityId(label)
		if _, err := client.GetCity(context.Background(), input); err == nil || !strings.Contains(err.Error(), "dot segment") {
			t.Fatalf("dot label: %v", err)
		}
	}
	if calls != 5 {
		t.Fatal("invalid request dispatched")
	}
	schema := &schemaDescription{Shapes: map[string]shapeDescription{
		"text": {Kind: "string"}, "doc": {Kind: "document"},
		"list": {Kind: "list", Member: memberDescription{Target: "text"}},
		"map":  {Kind: "map", Key: memberDescription{Target: "text"}, Value: memberDescription{Target: "text"}},
		"docs": {Kind: "list", Member: memberDescription{Target: "doc"}},
	}}
	var list []string
	if err := decodeValue(schema, "list", []any{nil}, reflect.ValueOf(&list).Elem(), ""); err == nil {
		t.Fatal("dense list null accepted")
	}
	var values map[string]string
	if err := decodeValue(schema, "map", map[string]any{"key": nil}, reflect.ValueOf(&values).Elem(), ""); err == nil {
		t.Fatal("dense map null accepted")
	}
	if _, err := encodeValue(schema, "list", reflect.ValueOf([]*string{nil}), ""); err == nil {
		t.Fatal("dense list null encoded")
	}
	if _, err := encodeValue(schema, "map", reflect.ValueOf(map[string]*string{"key": nil}), ""); err == nil {
		t.Fatal("dense map null encoded")
	}
	var documents []any
	if err := decodeValue(schema, "docs", []any{nil}, reflect.ValueOf(&documents).Elem(), ""); err == nil {
		t.Fatal("dense document null accepted")
	}
	if err := decodeValue(schema, "docs", []any{map[string]any{"nested":nil}}, reflect.ValueOf(&documents).Elem(), ""); err != nil {
		t.Fatalf("nested document null: %v", err)
	}
	if _, err := encodeValue(schema, "docs", reflect.ValueOf(documents), ""); err != nil {
		t.Fatal(err)
	}
}
