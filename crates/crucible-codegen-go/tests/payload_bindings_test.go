package payloadbindings

import (
    "context"
    "io"
    "net/http"
    "net/http/httptest"
    "strings"
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

func TestRecordNullStillOmitsPayload(t *testing.T) {
    server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
        body, err := io.ReadAll(r.Body)
        if err != nil { t.Error(err) }
        if len(body) != 0 { t.Errorf("body = %q, want empty", body) }
    }))
    defer server.Close()
    client, err := NewPayloadBindingsClient(Config{Endpoint: server.URL})
    if err != nil { t.Fatal(err) }
    input := &RecordInput{}
    input.SetRecord(nil)
    if _, err := client.PutRecord(context.Background(), input); err != nil { t.Fatal(err) }
}

func TestPayloadContentType(t *testing.T) {
    for _, streaming := range []bool{false, true} {
        for _, mediaType := range []string{"", "image/png"} {
            name := "buffered"; if streaming { name = "streaming" }
            t.Run(name+"/"+mediaType, func(t *testing.T) {
                expected := mediaType; if expected == "" { expected = "application/octet-stream" }
                server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
                    body, err := io.ReadAll(r.Body)
                    if err != nil { t.Error(err) }
                    if string(body) != "PNG" { t.Errorf("body = %q", body) }
                    if r.Header.Get("Content-Type") != expected { t.Errorf("Content-Type = %q, want %q", r.Header.Get("Content-Type"), expected) }
                }))
                defer server.Close()
                client, err := NewPayloadBindingsClient(Config{Endpoint: server.URL})
                if err != nil { t.Fatal(err) }
                if streaming {
                    input := &StreamInput{}
                    input.SetData(NewByteStream(func() (io.ReadCloser, error) { return io.NopCloser(strings.NewReader("PNG")), nil }, 3))
                    if mediaType != "" { input.SetContentType(mediaType) }
                    _, err = client.UploadStream(context.Background(), input)
                } else {
                    input := &UploadInput{}
                    input.SetData([]byte("PNG"))
                    if mediaType != "" { input.SetContentType(mediaType) }
                    _, err = client.Upload(context.Background(), input)
                }
                if err != nil { t.Fatal(err) }
            })
        }
    }
}
