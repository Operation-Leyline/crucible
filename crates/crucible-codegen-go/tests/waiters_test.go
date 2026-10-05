package jobs

import (
    "context"
    "errors"
    "io"
    "net/http"
    "net/http/httptest"
    "strings"
    "testing"
    "time"
)

func TestGeneratedWaiters(t *testing.T) {
    calls := 0; mode := "ready"
    server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter,r *http.Request){
        io.ReadAll(r.Body)
        calls++
        body := `{"wire-status":"PENDING"}`
        switch mode {
        case "ready": if calls == 1 { body = `{"__type":"Missing"}` } else if calls >= 3 { body = `{"wire-status":"READY"}` }
        case "gone": body = `{"__type":"Missing"}`
        case "query": body = `{"items":[{"wire-status":"READY"},{"wire-status":"READY"}],"updatedAt":"2026-01-01T00:00:00Z"}`
        case "failed": body = `{"wire-status":"FAILED","reasons":["job rejected"]}`
        case "blocked": <-r.Context().Done(); return
        }
        if strings.Contains(body,"__type") { w.WriteHeader(404) }; io.WriteString(w,body)
    })); defer server.Close()
    client,err := NewJobsClient(Config{Endpoint:server.URL,MaxAttempts:1}); if err != nil { t.Fatal(err) }
    input := &Input{}; input.SetNames(Names{"a","b"}); zero := time.Duration(0); options := WaiterOptions{MinDelay:&zero,MaxDelay:&zero}
    output,err := client.WaitUntilReady(context.Background(),input,time.Second,options)
    if err != nil || output.GetStatus() != "READY" || calls != 3 { t.Fatal(output,err,calls) }
    mode = "gone"; output,err = client.WaitUntilDeleted(context.Background(),input,time.Second,options); if err != nil || output != nil { t.Fatal(output,err) }
    mode = "query"
    for _,wait := range []func(context.Context,*Input,time.Duration,...WaiterOptions)(*Output,error){ client.WaitUntilAllReady,client.WaitUntilAnyReady,client.WaitUntilFilteredReady,client.WaitUntilCountsMatch,client.WaitUntilUpdated,client.WaitUntilFunctions } {
        if _,err := wait(context.Background(),input,time.Second,options); err != nil { t.Fatal(err) }
    }
    mode = "failed"; _,err = client.WaitUntilReady(context.Background(),input,time.Second,options); var failure *WaiterError
    if !errors.As(err,&failure) || failure.Attempts != 1 || !strings.Contains(err.Error(),"job rejected") { t.Fatal(err) }
    mode = "pending"; _,err = client.WaitUntilReady(context.Background(),input,50*time.Millisecond); var timeout *WaiterTimeoutError
    if !errors.As(err,&timeout) || timeout.Attempts < 1 || !errors.Is(err,context.DeadlineExceeded) { t.Fatal(err) }
    cancelled,cancel := context.WithCancel(context.Background()); cancel()
    if _,err := client.WaitUntilReady(cancelled,input,time.Second); !errors.Is(err,context.Canceled) { t.Fatal(err) }
    mode = "blocked"
    if _,err := client.WaitUntilReady(context.Background(),input,50*time.Millisecond); !errors.As(err,&timeout) { t.Fatal(err) }
}
