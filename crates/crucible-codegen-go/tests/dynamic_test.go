package dynamic

import (
    "context"
    "encoding/json"
    "errors"
    "io"
    "net/http"
    "net/http/httptest"
    "testing"
)

func TestDynamicBindingsOverHTTP(t *testing.T) {
    calls := 0
    server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
        calls++
        if calls == 2 {
            w.Header().Set("X-Meta-Reason", "invalid")
            w.WriteHeader(400)
            io.WriteString(w, `{"__type":"Problem","message":"bad"}`)
            return
        }
        if r.URL.EscapedPath() != "/files/a/b%20c/%25" { t.Errorf("path %s", r.URL.EscapedPath()) }
        if values := r.URL.Query()["q +"]; len(values) != 2 || values[0] != "x/y" || values[1] != "" { t.Errorf("query %v", r.URL.Query()) }
        if r.Header.Get("X-Meta-Color") != "green" { t.Error("prefix header missing") }
        var body map[string]any
        if err := json.NewDecoder(r.Body).Decode(&body); err != nil { t.Error(err) }
        if body["wire-value"] != "request" || len(body) != 1 { t.Errorf("body %v", body) }
        w.Header().Set("X-Meta-Color", "blue")
        w.WriteHeader(201)
        io.WriteString(w, `{"wire-value":"result"}`)
    }))
    defer server.Close()
    client, err := NewDynamicClient(Config{Endpoint:server.URL})
    if err != nil { t.Fatal(err) }
    input := &Input{}
    input.SetKey("a/b c/%")
    input.SetQuery(StringLists{"q +":StringList{"x/y", ""}})
    input.SetMetadata(Strings{"Color":"green"})
    input.SetValue("request")
    output, err := client.Get(context.Background(), input)
    if err != nil { t.Fatal(err) }
    if output.GetStatus() != 201 || output.GetValue() != "result" || output.GetMetadata()["Color"] != "blue" { t.Fatalf("output %#v", output) }
    _, err = client.Get(context.Background(), input)
    var problem *Problem
    if !errors.As(err, &problem) { t.Fatalf("error %v", err) }
    if problem.GetStatus() != 400 || problem.GetDetails()["Reason"] != "invalid" || problem.Metadata.Metadata.Status != 400 { t.Fatalf("error %#v", problem) }
    for _, key := range []string{"a/../b", "./b", "a/.", ".."} {
        input.SetKey(key)
        if _, err := client.Get(context.Background(), input); err == nil { t.Fatal("unsafe greedy path accepted") }
    }
    if calls != 2 { t.Fatalf("unexpected transport calls %d", calls) }
}
