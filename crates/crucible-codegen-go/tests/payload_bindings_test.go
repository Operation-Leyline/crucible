package payloadbindings

import (
    "context"
    "io"
    "net/http"
    "net/http/httptest"
    "testing"
)

func TestDocumentPayloadPresence(t *testing.T) {
    for _, state := range []string{"omitted", "null", "value"} {
        t.Run(state, func(t *testing.T) {
            expected := ""
            input := &DocumentInput{}
            switch state {
            case "null": input.SetDocument(nil); expected = "null"
            case "value": input.SetDocument(map[string]any{"answer": 42}); expected = `{"answer":42}`
            }
            server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
                body, err := io.ReadAll(r.Body)
                if err != nil { t.Error(err) }
                if string(body) != expected { t.Errorf("body = %q, want %q", body, expected) }
                if expected != "" && r.Header.Get("Content-Type") != "application/json" { t.Errorf("Content-Type = %q", r.Header.Get("Content-Type")) }
            }))
            defer server.Close()
            client, err := NewPayloadBindingsClient(Config{Endpoint: server.URL})
            if err != nil { t.Fatal(err) }
            if _, err := client.PutDocument(context.Background(), input); err != nil { t.Fatal(err) }
        })
    }
}

func TestRecordNullIsSentAsNull(t *testing.T) {
    for _, state := range []string{"omitted", "null", "value"} {
        t.Run(state, func(t *testing.T) {
            expected := ""
            input := &RecordInput{}
            switch state {
            case "null":
                input.SetRecord(nil)
                expected = "null"
            case "value":
                record := &Record{}; record.SetName("record")
                input.SetRecord(record); expected = `{"name":"record"}`
            }
            server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
                body, err := io.ReadAll(r.Body)
                if err != nil { t.Error(err) }
                if string(body) != expected { t.Errorf("body = %q, want %q", body, expected) }
            }))
            defer server.Close()
            client, err := NewPayloadBindingsClient(Config{Endpoint: server.URL})
            if err != nil { t.Fatal(err) }
            if _, err := client.PutRecord(context.Background(), input); err != nil { t.Fatal(err) }
        })
    }
}

// A modelled Content-Type member, then the payload's media type, then a
// client-wide default (given in a non-canonical case).
func TestPayloadContentType(t *testing.T) {
    for _, mediaType := range []string{"", "image/png"} {
        for _, configured := range []string{"", "image/jpeg"} {
            t.Run(mediaType+"/"+configured, func(t *testing.T) {
                expected := mediaType; if expected == "" { expected = "application/octet-stream" }
                server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
                    body, err := io.ReadAll(r.Body)
                    if err != nil { t.Error(err) }
                    if string(body) != "PNG" { t.Errorf("body = %q", body) }
                    if got := r.Header.Values("Content-Type"); len(got) != 1 || got[0] != expected { t.Errorf("Content-Type = %q, want %q", got, expected) }
                }))
                defer server.Close()
                config := Config{Endpoint: server.URL}
                if configured != "" { config.Headers = http.Header{"content-type": {configured}} }
                client, err := NewPayloadBindingsClient(config)
                if err != nil { t.Fatal(err) }
                input := &UploadInput{}
                input.SetData([]byte("PNG"))
                if mediaType != "" { input.SetContentType(mediaType) }
                if _, err = client.Upload(context.Background(), input); err != nil { t.Fatal(err) }
            })
        }
    }
}
