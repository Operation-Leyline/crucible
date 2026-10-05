package wire

import (
    "context"
    "errors"
    "io"
    "net/http"
    "net/http/httptest"
    "reflect"
    "strings"
    "testing"
)

func TestPayloadsAndMediaHeaders(t *testing.T) {
    fail := false
    invalid := ""
    server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
        if fail { w.Header().Set("X-Crucible-Error", "TextError"); w.WriteHeader(422); io.WriteString(w, "invalid text"); return }
        data, _ := io.ReadAll(r.Body)
        if r.URL.Path == "/document" && string(data) != `{"name":"nested"}` { t.Errorf("payload %s", data) }
        if r.URL.Path == "/text" {
            if string(data) != "héllo 🌍" && len(data) != 0 { t.Errorf("payload %s", data) }
            if len(data) > 0 && r.Header.Get("X-Meta") != `eyJrZXkiOiJow6lsbG8ifQ==, YSxi, ""` { t.Errorf("media header %s", r.Header.Get("X-Meta")) }
        }
        w.Header().Set("X-Meta", r.Header.Get("X-Meta"))
        for name,values := range r.Header { if strings.HasPrefix(strings.ToLower(name),"x-extra-") { w.Header()[name] = values } }
        if invalid != "" { w.Header().Set("X-Meta", invalid) }
        w.Write(data)
    }))
    defer server.Close()
    client, err := NewWireClient(Config{Endpoint:server.URL})
    if err != nil { t.Fatal(err) }
    input := &TextInput{}
    input.SetText("héllo 🌍"); input.SetMeta(MediaList{`{"key":"héllo"}`, "a,b", ""})
    input.SetExtras(MediaMap{"note":"héllo"})
    output, err := client.Text(context.Background(), input)
    if err != nil || output.GetText() != input.GetText() || !reflect.DeepEqual(output.GetMeta(), input.GetMeta()) { t.Fatal(output, err) }
    if len(output.GetExtras()) != 1 { t.Fatal(output.GetExtras()) }; for _,value := range output.GetExtras() { if value != "héllo" { t.Fatal("encoded prefix media value",value) } }
    record := &Record{}; record.SetName("nested")
    document := &DocumentInput{}; document.SetDocument(record)
    result, err := client.Document(context.Background(), document)
    if err != nil || result.GetDocument().GetName() != "nested" { t.Fatal(result, err) }
    input.SetText("")
    output, err = client.Text(context.Background(), input)
    if err != nil || output.GetText() != "" { t.Fatal(output, err) }
    for _, header := range []string{"%%%", "/w=="} {
        invalid = header
        if _, err := client.Text(context.Background(), input); err == nil { t.Fatal("invalid media header accepted", header) }
    }
    invalid = ""; fail = true
    _, err = client.Text(context.Background(), input)
    var modeled *TextError
    if !errors.As(err, &modeled) || modeled.GetDetails() != "invalid text" { t.Fatal(err) }
}
