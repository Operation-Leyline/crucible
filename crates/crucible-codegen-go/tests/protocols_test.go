package protocol

import (
    "context"
    "encoding/json"
    "errors"
    "io"
    "math"
    "net/http"
    "net/http/httptest"
    "testing"
    "time"
)

func TestAWSJSONProtocol(t *testing.T) {
    mode := "echo"
    var rpc bool
    server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
        rpc = r.Header.Get("X-Amz-Target") != ""
        if r.Method != "POST" { t.Error("method", r.Method) }
        data, err := io.ReadAll(r.Body); if err != nil { t.Fatal(err) }
        if mode == "empty" {
            if rpc && (string(data) != "{}" || r.Header.Get("X-Amz-Target") != "JsonService.Empty") { t.Error("empty RPC", string(data), r.Header) }
            w.WriteHeader(200); return
        }
        if mode == "error" {
            w.Header().Set("X-Amzn-Errortype", "example.json#Problem:detail")
            w.Header().Set("X-Amzn-Requestid", "request-42")
            w.WriteHeader(409); io.WriteString(w, `{"__type":"Other","Message":"bad"}`); return
        }
        body := map[string]any{}
        if err := json.Unmarshal(data, &body); err != nil { t.Fatal(err) }
        if body["when"] != -0.25 || body["iso"] != "1969-12-31T23:59:59.75Z" { t.Error("timestamp", body) }
        if _, found := body["nullable"]; found { t.Error("structure null was sent") }
        if doc, found := body["document"]; !found || doc != nil { t.Error("document null lost") }
        if body["floatValue"] != "NaN" || body["doubleValue"] != "Infinity" { t.Error("nonfinite", body) }
        if rpc {
            if r.URL.Path != "/" || r.Header.Get("X-Amz-Target") != "JsonService.Echo" || body["renamed"] != "hello" || body["date"] != -0.25 { t.Error("RPC", r.URL, r.Header, body) }
            if r.Header.Get("Content-Type") != "application/x-amz-json-1.0" && r.Header.Get("Content-Type") != "application/x-amz-json-1.1" { t.Error("RPC content type") }
        } else {
            if r.URL.Path != "/echo" || r.Header.Get("X-Date") != "Wed, 31 Dec 1969 23:59:59 GMT" || r.URL.Query().Get("time") != "1969-12-31T23:59:59.75Z" || body["wire-name"] != "hello" { t.Error("REST", r.URL, r.Header, body) }
        }
        body["choice"] = map[string]any{"text":"ok", "__type":"ignored"}
        body["nullable"] = nil
        encoded, err := json.Marshal(body); if err != nil { t.Fatal(err) }; w.Write(encoded)
    }))
    defer server.Close()
    client, err := NewJsonServiceClient(Config{Endpoint:server.URL}); if err != nil { t.Fatal(err) }
    instant := time.Unix(-1,750000000).UTC()
    input := &ValuesInput{}
    input.SetRenamed("hello"); input.SetWhen(instant); input.SetIso(instant)
    input.SetDate(instant); input.SetDates([]time.Time{instant}); input.SetQuery(instant)
    input.SetFloatValue(float32(math.NaN())); input.SetDoubleValue(math.Inf(1))
    input.Nullable = Null[string](); input.Document = Null[any]()
    output, err := client.Echo(context.Background(), input); if err != nil { t.Fatal(err) }
    if output.Nullable.Set || !output.Document.Null || !math.IsNaN(float64(output.GetFloatValue())) || output.GetDoubleValue() != math.Inf(1) || output.GetChoice().GetText() != "ok" { t.Fatal("decoded protocol values", output) }
    mode = "error"
    _, err = client.Echo(context.Background(), &ValuesInput{})
    var modeled *ProblemFault
    if !errors.As(err, &modeled) || modeled.Metadata.Name != "Problem" || modeled.Metadata.Metadata.RequestID != "request-42" || modeled.Metadata.Message != "bad" { t.Fatal("modeled AWS failure", err) }
    mode = "empty"
    _, err = client.Empty(context.Background(), Unit{}); if err != nil { t.Fatal(err) }
}
