package jobs

import (
    "context"
    "encoding/json"
    "errors"
    "io"
    "math"
    "net/http"
    "net/http/httptest"
    "strings"
    "sync/atomic"
    "testing"
    "time"
)

func TestWaiterQueriesUseNativeModelNumbers(t *testing.T) {
    type numericCode int32
    cases := []struct { kind string; value,lower any; number,less string }{
        {"byte",int8(4),int8(1),"4","1"},
        {"short",int16(4),int16(1),"4","1"},
        {"integer",int32(4),int32(1),"4","1"},
        {"long",int64(9007199254740993),int64(9007199254740992),"9007199254740993","9007199254740992"},
        {"intEnum",numericCode(4),numericCode(1),"4","1"},
        {"float",float32(1.5),float32(0.25),"1.5","0.25"},
        {"double",float64(2.5),float64(0.25),"2.5","0.25"},
    }
    field := func(name string) map[string]any { return map[string]any{"kind":"field","name":name} }
    literal := func(number string) map[string]any { return map[string]any{"kind":"literal","value":json.Number(number)} }
    compare := func(left map[string]any,comparator string,right map[string]any) map[string]any {
        return map[string]any{"kind":"compare","left":left,"right":right,"comparator":comparator}
    }
    function := func(name string) map[string]any { return map[string]any{"kind":"function","function":name,"arguments":[]any{field("counts")}} }
    for _,item := range cases {
        t.Run(item.kind,func(t *testing.T) {
            schema := &schemaDescription{Shapes:map[string]shapeDescription{
                "number":{Kind:item.kind},
                "numbers":{Kind:"list",Member:memberDescription{Target:"number"}},
                "model":{Kind:"structure",Members:map[string]memberDescription{
                    "count":{Target:"number",Attribute:"Count",JSONName:"wire-count"},
                    "counts":{Target:"numbers",Attribute:"Counts"},
                }},
            }}
            model := struct { Count any; Counts []any }{item.value,[]any{item.value,item.lower}}
            encoded,err := encodeModel(schema,"model",model); if err != nil { t.Fatal(err) }
            runtime := &clientRuntime{schema:schema}
            queried,err := runtime.queryModel("model",encoded,""); if err != nil { t.Fatal(err) }
            checks := map[string]map[string]any{
                "equality":compare(field("count"),"equal",literal(item.number)),
                "ordering":compare(field("count"),"greaterThan",literal(item.less)),
                "minimum":compare(function("min"),"equal",literal(item.less)),
                "maximum":compare(function("max"),"equal",literal(item.number)),
                "sort":compare(map[string]any{"kind":"subexpression","left":function("sort"),"right":map[string]any{"kind":"index","index":json.Number("0")}},"equal",literal(item.less)),
            }
            for name,expression := range checks {
                for _,kind := range []string{"output","inputOutput"} {
                    path := expression
                    if kind == "inputOutput" { path = map[string]any{"kind":"subexpression","left":field("input"),"right":expression} }
                    description := map[string]any{"acceptors":[]any{map[string]any{"state":"success","matcher":map[string]any{
                        "kind":kind,"comparator":"booleanEquals","expected":"true","path":map[string]any{"expression":path},
                    }}}}
                    state,_,err := waiterState(description,queried,queried,nil)
                    if err != nil || state != "success" { t.Errorf("%s %s: state=%s error=%v",kind,name,state,err) }
                }
            }
        })
    }
}

func TestWaiterQueryPreservesWireNumbersAndRejectsNonFiniteNativeNumbers(t *testing.T) {
    runtime := &clientRuntime{schema:&schemaDescription{Shapes:map[string]shapeDescription{
        "double":{Kind:"double"},
        "bigDecimal":{Kind:"bigDecimal"},
    }}}
    for _,value := range []any{json.Number("9007199254740993.0000000001"),"NaN","Infinity","-Infinity"} {
        queried,err := runtime.queryModel("double",value,"")
        if err != nil || queried != value { t.Fatal(queried,err) }
    }
    exact := json.Number("12345678901234567890.0000000001")
    queried,err := runtime.queryModel("bigDecimal",exact,"")
    if err != nil || queried != exact { t.Fatal(queried,err) }
    for _,value := range []float64{math.NaN(),math.Inf(1),math.Inf(-1)} {
        _,err := runtime.queryModel("double",value,"")
        var unsupported *json.UnsupportedValueError
        if !errors.As(err,&unsupported) { t.Fatalf("nonfinite native number: %v",err) }
    }
}

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
