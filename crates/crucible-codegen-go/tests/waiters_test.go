package jobs

import (
    "context"
    "errors"
    "io"
    "net/http"
    "net/http/httptest"
    "strings"
    "sync/atomic"
    "testing"
    "time"
)

func TestWaiterLeavesTimeForFinalHTTPPoll(t *testing.T) {
    var calls atomic.Int32
    server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
        io.ReadAll(r.Body)
        if calls.Add(1) == 1 {
            io.WriteString(w, `{"wire-status":"PENDING"}`)
            return
        }
        // The last attempt needs usable request time, not just an immediate response at the deadline.
        timer := time.NewTimer(100 * time.Millisecond)
        defer timer.Stop()
        select {
        case <-timer.C:
            io.WriteString(w, `{"wire-status":"READY"}`)
        case <-r.Context().Done():
        }
    }))
    defer server.Close()
    client, err := NewJobsClient(Config{Endpoint: server.URL, MaxAttempts: 1})
    if err != nil { t.Fatal(err) }
    minimum := time.Second
    output, err := client.WaitUntilReady(context.Background(), &Input{}, 1500 * time.Millisecond,
        WaiterOptions{MinDelay: &minimum, MaxDelay: &minimum})
    if err != nil { t.Fatalf("final poll failed after %d HTTP attempts: %v", calls.Load(), err) }
    if output == nil || output.GetStatus() != "READY" || calls.Load() != 2 {
        t.Fatal(output, calls.Load())
    }
}

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
