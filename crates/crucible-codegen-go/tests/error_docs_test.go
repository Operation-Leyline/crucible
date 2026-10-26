package error_docs_test

import (
	"context"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"

	sdk "example.com/error-docs"
)

func TestErrorDocumentationOverHTTP(t *testing.T) {
	const base = "https://docs.example.test/errors/"
	tests := []struct {
		name, code  string
		status      int
		body        string
		headers     map[string]string
		page        string
		reason      string
		checkReason bool
	}{
		{name: "default-known", code: "DefaultReason", status: 419, body: `{"message":"problem"}`, page: "DefaultReason/reason-access%2Fneeded", reason: "access/needed", checkReason: true},
		{name: "default-empty", code: "DefaultEmptyReason", status: 420, body: `{"message":"problem"}`, page: "DefaultEmptyReason/reason-", checkReason: true},
		{name: "default-explicit-null", code: "DefaultReason", status: 419, body: `{"message":"problem","detail":null}`, page: "DefaultReason"},
		{name: "default-future", code: "DefaultReason", status: 419, body: `{"message":"problem","detail":"future"}`, page: "DefaultReason", reason: "future", checkReason: true},

		{name: "DefaultReason-malformed", code: "DefaultReason", status: 419, body: `{invalid`, page: "DefaultReason"},
		{name: "DefaultReason-scalar", code: "DefaultReason", status: 419, body: `42`, page: "DefaultReason"},
		{name: "DefaultReason-array", code: "DefaultReason", status: 419, body: `[]`, page: "DefaultReason"},
		{name: "DefaultReason-json-null", code: "DefaultReason", status: 419, body: `null`, page: "DefaultReason"},
		{name: "DefaultReason-empty-object", code: "DefaultReason", status: 419, body: `{}`, page: "DefaultReason/reason-access%2Fneeded"},
		{name: "DefaultEmptyReason-malformed", code: "DefaultEmptyReason", status: 420, body: `{invalid`, page: "DefaultEmptyReason"},
		{name: "DefaultEmptyReason-scalar", code: "DefaultEmptyReason", status: 420, body: `42`, page: "DefaultEmptyReason"},
		{name: "DefaultEmptyReason-array", code: "DefaultEmptyReason", status: 420, body: `[]`, page: "DefaultEmptyReason"},
		{name: "DefaultEmptyReason-json-null", code: "DefaultEmptyReason", status: 420, body: `null`, page: "DefaultEmptyReason"},
		{name: "DefaultEmptyReason-empty-object", code: "DefaultEmptyReason", status: 420, body: `{}`, page: "DefaultEmptyReason/reason-"},
		{name: "DefaultReason-empty-body", code: "DefaultReason", status: 419, body: ``, page: "DefaultReason/reason-access%2Fneeded"},
		{name: "DefaultEmptyReason-empty-body", code: "DefaultEmptyReason", status: 420, body: ``, page: "DefaultEmptyReason/reason-"},
		{name: "blob-payload", code: "BlobPayloadError", status: 421, body: `not-json /blob`, headers: map[string]string{"X-Error-Reason": "access/needed"}, page: "BlobPayloadError/reason-access%2Fneeded"},
		{name: "empty-blob-payload", code: "BlobPayloadError", status: 421, body: ``, headers: map[string]string{"X-Error-Reason": ""}, page: "BlobPayloadError/reason-"},
		{name: "access", code: "Denied", status: 403, body: `{"detail":"access/needed","message":"problem"}`, page: "Denied/reason-access%2Fneeded", reason: "access/needed", checkReason: true},
		{name: "empty", code: "Denied", status: 403, body: `{"detail":"","message":"problem"}`, page: "Denied/reason-", checkReason: true},
		{name: "dots", code: "Denied", status: 403, body: `{"detail":"..","message":"problem"}`, page: "Denied/reason-%2E%2E", reason: "..", checkReason: true},
		{name: "unicode", code: "Denied", status: 403, body: `{"detail":"caf\u00e9","message":"problem"}`, page: "Denied/reason-caf%C3%A9", reason: "caf\u00e9", checkReason: true},
		{name: "future", code: "Denied", status: 403, body: `{"detail":"new-future","message":"problem"}`, page: "Denied", reason: "new-future", checkReason: true},
		{name: "internal", code: "Denied", status: 403, body: `{"detail":"secret","message":"problem"}`, page: "Denied", reason: "secret", checkReason: true},
		{name: "null", code: "Denied", status: 403, body: `{"detail":null,"message":"problem"}`, page: "Denied"},
		{name: "absent", code: "Denied", status: 403, body: `{"message":"problem"}`, page: "Denied"},
		{name: "wire-url-untrusted", code: "Denied", status: 403, body: `{"detail":"future","docUrl":"https://evil.test","doc_url":"https://evil.test","message":"problem"}`, page: "Denied"},
		{name: "body-identity", status: 403, body: `{"__type":"Denied","detail":"access/needed","message":"problem"}`, page: "Denied/reason-access%2Fneeded", reason: "access/needed", checkReason: true},
		{name: "status-identity", status: 418, body: `{"failureReason":"..","message":"problem"}`, page: "MappedReasonError/reason-%2E%2E", reason: "..", checkReason: true},
		{name: "header-reason", code: "HeaderError", status: 409, body: `{"reason":"future","message":"problem"}`, headers: map[string]string{"X-Error-Reason": "access/needed"}, page: "HeaderError/reason-access%2Fneeded", reason: "access/needed", checkReason: true},
		{name: "header-empty", code: "HeaderError", status: 409, body: `{"message":"problem"}`, headers: map[string]string{"X-Error-Reason": ""}, page: "HeaderError/reason-", checkReason: true},
		{name: "header-future", code: "HeaderError", status: 409, body: `{"message":"problem"}`, headers: map[string]string{"X-Error-Reason": "future"}, page: "HeaderError", reason: "future", checkReason: true},
		{name: "header-identity-precedence", code: "HeaderError", status: 409, body: `{"__type":"Denied","detail":"access/needed","message":"problem"}`, headers: map[string]string{"X-Error-Reason": ".."}, page: "HeaderError/reason-%2E%2E", reason: "..", checkReason: true},
		{name: "payload-reason", code: "PayloadError", status: 422, body: `access/needed`, page: "PayloadError/reason-access%2Fneeded", reason: "access/needed", checkReason: true},
		{name: "payload-empty", code: "PayloadError", status: 422, body: ``, page: "PayloadError/reason-", checkReason: true},
		{name: "payload-null-literal", code: "PayloadError", status: 422, body: `null`, page: "PayloadError", reason: "null", checkReason: true},
		{name: "payload-future", code: "PayloadError", status: 422, body: `future`, page: "PayloadError", reason: "future", checkReason: true},
		{name: "payload-malformed", code: "PayloadError", status: 422, body: "\xff", page: "PayloadError"},
		{name: "unrelated-reason", code: "PlainError", status: 400, body: `{"reason":"access/needed","message":"problem"}`, page: "PlainError", reason: "access/needed", checkReason: true},
		{name: "malformed-member", code: "RequiredReason", status: 402, body: `{"reason":123,"message":"problem"}`, page: "RequiredReason"},
		{name: "malformed-json", code: "RequiredReason", status: 402, body: `{invalid`, page: "RequiredReason"},
		{name: "unknown-identity", code: "FutureError", status: 403, body: `{"detail":"access/needed","docUrl":"https://evil.test","message":"problem"}`},
		{name: "unknown-malformed", code: "FutureError", status: 418, body: `{invalid`},
		{name: "invalid-missing-message", code: "Denied", status: 403, body: `{"detail":"access/needed"}`, page: "Denied"},
		{name: "invalid-null-message", code: "Denied", status: 403, body: `{"message":null,"detail":"access/needed"}`, page: "Denied"},
		{name: "invalid-wrong-message", code: "Denied", status: 403, body: `{"message":123,"detail":"access/needed"}`, page: "Denied"},
		{name: "invalid-missing-reason", code: "RequiredReason", status: 402, body: `{}`, page: "RequiredReason"},
		{name: "invalid-null-reason", code: "RequiredReason", status: 402, body: `{"reason":null}`, page: "RequiredReason"},
		{name: "mapped-member", code: "MappedReasonError", status: 418, body: `{"failureReason":"access/needed","message":"problem"}`, page: "MappedReasonError/reason-access%2Fneeded", reason: "access/needed", checkReason: true},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			var requests atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				requests.Add(1)
				if r.Method != "GET" || r.URL.Path != "/inspect" {
					t.Errorf("unexpected request: %s %s", r.Method, r.URL)
				}
				if test.code != "" {
					w.Header().Set("X-Crucible-Error", test.code)
				}
				w.Header().Set("X-Request-Id", "error-docs-request")
				w.Header().Set("Content-Type", "application/json")
				for name, value := range test.headers {
					w.Header().Set(name, value)
				}
				w.WriteHeader(test.status)
				io.WriteString(w, test.body)
			}))
			defer server.Close()
			client, err := sdk.NewTroubleshootingClient(sdk.Config{Endpoint: server.URL, MaxAttempts: 1})
			if err != nil {
				t.Fatal(err)
			}
			_, err = client.Inspect(context.Background(), sdk.Unit{})
			var service *sdk.ServiceError
			if !errors.As(err, &service) {
				t.Fatalf("not a service error: %T %v", err, err)
			}
			expected := ""
			if test.page != "" {
				expected = base + test.page
			}
			if service.Metadata.DocURL != expected {
				t.Errorf("DocURL = %q, want %q", service.Metadata.DocURL, expected)
			}
			if service.Metadata.Status != test.status || service.Metadata.RequestID != "error-docs-request" || string(service.Metadata.Body) != test.body {
				t.Errorf("lost HTTP metadata: %#v", service.Metadata)
			}
			if requests.Load() != 1 {
				t.Errorf("requests = %d", requests.Load())
			}
			if test.checkReason {
				var reason string
				switch e := err.(type) {
				case *sdk.DefaultReason:
					reason = string(e.GetDetail())
				case *sdk.DefaultEmptyReason:
					reason = string(e.GetDetail())
				case *sdk.Denied:
					reason = string(e.GetDetail())
				case *sdk.HeaderError:
					reason = string(e.GetReason())
				case *sdk.PayloadError:
					reason = string(e.GetReason())
				case *sdk.PlainError:
					reason = string(e.GetReason())
				case *sdk.MappedReasonError:
					reason = string(e.GetFailureReason())
				default:
					t.Fatalf("lost modeled error: %T", err)
				}
				if reason != test.reason {
					t.Errorf("reason = %q, want %q", reason, test.reason)
				}
			}
		})
	}
}
