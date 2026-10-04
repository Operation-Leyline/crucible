package weather

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"reflect"
	"strings"
	"testing"
	"time"
)

func clientFor(t *testing.T, handler http.HandlerFunc) (*Client, *httptest.Server) {
	t.Helper()
	server := httptest.NewServer(handler)
	t.Cleanup(server.Close)
	client, err := NewClient(Config{Endpoint: server.URL, BaseDelay: time.Nanosecond, MaxDelay: time.Nanosecond})
	if err != nil {
		t.Fatal(err)
	}
	return client, server
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
		Wire   string
		Values []any
	}
	if err := json.Unmarshal(data, &cases); err != nil {
		t.Fatal(err)
	}
	for _, test := range cases {
		value, err := headerValue(wireSchema, memberDescription{Target: "example.weather#Tags"}, test.Wire)
		if err != nil || !reflect.DeepEqual(value, test.Values) {
			t.Fatalf("%q => %#v, %v", test.Wire, value, err)
		}
	}
}
func TestExplicitNullDoesNotUseDefault(t *testing.T) {
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
	client, err := NewClient(Config{Endpoint: "http://localhost", BaseDelay: time.Nanosecond, MaxDelay: time.Nanosecond, HTTPClient: transportFunc(func(r *http.Request) (*http.Response, error) {
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
	client, err := NewClient(Config{Endpoint: "http://example.invalid", Token: func(context.Context) (string, error) { return "secret", nil }, HTTPClient: transportFunc(func(*http.Request) (*http.Response, error) { t.Fatal("credentials leaked"); return nil, nil })})
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
	client, err := NewClient(Config{Endpoint: endpoint})
	if err != nil {
		t.Fatal(err)
	}
	city, err := client.CreateCity(context.Background(), &CreateCityInput{Name: Some[CityName]("go-http"), Location: Some(&Location{Unknown: Some(Unit{})})})
	if err != nil {
		t.Fatal(err)
	}
	got, err := client.GetCity(context.Background(), &GetCityInput{CityId: Some(city.City.Id)})
	if err != nil || got.City.Name != "go-http" {
		t.Fatalf("%v %v", got, err)
	}
	_, err = client.CreateCity(context.Background(), &CreateCityInput{Name: Some[CityName]("go-http")})
	var conflict *ConflictError
	if !errors.As(err, &conflict) {
		t.Fatal(err)
	}
	pages, err := client.ListCitiesPages(&ListCitiesInput{MaxResults: Some[int32](1)})
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
