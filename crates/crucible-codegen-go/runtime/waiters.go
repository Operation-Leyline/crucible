package sdk

import (
    "context"
    "encoding/base64"
    "encoding/json"
    "errors"
    "fmt"
    "math"
    mathrand "math/rand/v2"
    "strings"
    "strconv"
    "time"
)

type WaiterOptions struct { MinDelay, MaxDelay *time.Duration }
type WaiterError struct { Name string; Attempts int; Response any; Cause error; Message any }
func (e *WaiterError) Error() string { return fmt.Sprintf("waiter %s failed after %d attempts: %v",e.Name,e.Attempts,e.Message) }
func (e *WaiterError) Unwrap() error { return e.Cause }
type WaiterTimeoutError struct { *WaiterError }
func (*WaiterTimeoutError) Timeout() bool { return true }
func (e *WaiterTimeoutError) Is(target error) bool { return target == context.DeadlineExceeded || errors.Is(e.Cause,target) }

func (c *clientRuntime) queryModel(target string, value any, format string) (any,error) {
    if target == "" || value == nil { return nil,nil }
    shape := c.schema.Shapes[target]
    switch shape.Kind {
    case "blob": if data,ok := value.([]byte); ok { return base64.StdEncoding.EncodeToString(data),nil }; return value,nil
    case "timestamp":
        if format == "" { format = shape.Format }; parsed,err := readTime(value,format); if err != nil { return nil,err }; return writeTime(parsed,"epoch-seconds"),nil
    case "structure","union":
        object,ok := value.(map[string]any); if !ok { return nil,errors.New("query model requires object") }; result := map[string]any{}
        for name,member := range shape.Members { if item,present := object[shape.wireName(name)]; present { next,err := c.queryModel(member.Target,item,member.Format); if err != nil { return nil,err }; result[name] = next } }; return result,nil
    case "list":
        list,ok := value.([]any); if !ok { return nil,errors.New("query model requires list") }; result := make([]any,len(list))
        for i,item := range list { next,err := c.queryModel(shape.Member.Target,item,shape.Member.Format); if err != nil { return nil,err }; result[i] = next }; return result,nil
    case "map":
        object,ok := value.(map[string]any); if !ok { return nil,errors.New("query model requires map") }; result := map[string]any{}
        for key,item := range object { next,err := c.queryModel(shape.Value.Target,item,shape.Value.Format); if err != nil { return nil,err }; result[key] = next }; return result,nil
    default: return value,nil
    }
}
func waiterState(description map[string]any, input, output any, failure error) (string,any,error) {
    for _,entry := range description["acceptors"].([]any) {
        acceptor := entry.(map[string]any); matcher := acceptor["matcher"].(map[string]any); kind := matcher["kind"].(string); matches := false
        switch kind {
        case "success": matches = (failure == nil) == matcher["expected"].(bool)
        case "errorType":
            var service *ServiceError
            if errors.As(failure,&service) { name := matcher["name"].(string); parts := strings.Split(name,"#"); matches = service.Name == name || service.Name == parts[len(parts)-1] }
        default:
            if failure == nil {
                source := output; if kind == "inputOutput" { source = map[string]any{"input":input,"output":output} }
                result,err := queryEvaluate(matcher["path"].(map[string]any)["expression"].(map[string]any),source); if err != nil { return "",nil,err }
                expected := matcher["expected"].(string)
                switch matcher["comparator"].(string) {
                case "stringEquals": value,ok := result.(string); matches = ok && value == expected
                case "booleanEquals": value,ok := result.(bool); matches = ok && value == (expected == "true")
                case "allStringEquals","anyStringEquals":
                    if list,ok := result.([]any); ok { all,anyMatch := len(list) > 0,false; for _,item := range list { value,ok := item.(string); equal := ok && value == expected; all = all && equal; anyMatch = anyMatch || equal }; if matcher["comparator"] == "allStringEquals" { matches = all } else { matches = anyMatch } }
                }
            }
        }
        if matches {
            var message any
            if query,ok := acceptor["message"].(map[string]any); ok { var err error; message,err = queryEvaluate(query["expression"].(map[string]any),output); if err != nil { return "",nil,err } }
            return acceptor["state"].(string),message,nil
        }
    }
    if failure != nil { return "failure",nil,nil }; return "retry",nil,nil
}
func (c *clientRuntime) wait(ctx context.Context, operation, name string, input any, timeout time.Duration, options WaiterOptions, decode func(map[string]any)(any,error)) (any,error) {
    op := c.schema.Operations[operation]; description := op.Waiters[name]
    duration := func(value any)(time.Duration,error) { number,ok := value.(json.Number); if !ok { return 0,errors.New("invalid waiter delay") }; seconds,err := strconv.ParseInt(string(number),10,64); if err != nil || seconds < 0 || seconds > math.MaxInt64/int64(time.Second) { return 0,errors.New("waiter delay exceeds duration range") }; return time.Duration(seconds)*time.Second,nil }
    minimum,err := duration(description["minDelay"]); if err != nil { return nil,err }; maximum,err := duration(description["maxDelay"]); if err != nil { return nil,err }
    if options.MinDelay != nil { minimum = *options.MinDelay }; if options.MaxDelay != nil { maximum = *options.MaxDelay }
    if timeout <= 0 || minimum < 0 || maximum < minimum { return nil,errors.New("waiter deadline and ordered delays must be valid") }
    values,err := encodeModel(c.schema,op.Input,input); if err != nil { return nil,err }
    queryInput,err := c.queryModel(op.Input,values,""); if err != nil { return nil,err }
    deadline,cancel := context.WithTimeout(ctx,timeout); end,_ := deadline.Deadline()
    // A returned streaming response keeps the waiter's deadline until it is read or closed.
    kept := false; defer func() { if !kept { cancel() } }()
    attempts := 0; var response any; var failure error
    timeoutError := func() error { if ctx.Err() != nil { return ctx.Err() }; return &WaiterTimeoutError{&WaiterError{Name:name,Attempts:attempts,Response:response,Cause:failure,Message:"deadline exceeded"}} }
    for {
        if deadline.Err() != nil { return nil,timeoutError() }; attempts++
        wire,callError := c.callValues(deadline,operation,values); failure = callError; response = nil; var output any
        if failure == nil {
            response,failure = decode(wire)
            if failure == nil {
                encoded,encodeError := encodeModel(c.schema,op.Output,response); failure = encodeError
                if failure == nil { output,failure = c.queryModel(op.Output,encoded,"") }
            }
        }
        if deadline.Err() != nil { closeStreams(wire); return nil,timeoutError() }
        state,message,matchError := waiterState(description,queryInput,output,failure)
        if matchError != nil { closeStreams(wire); return nil,&WaiterError{Name:name,Attempts:attempts,Response:response,Cause:matchError} }
        if state == "success" { kept = keepStreams(wire,cancel); return response,nil }
        closeStreams(wire)
        if state == "failure" { return nil,&WaiterError{Name:name,Attempts:attempts,Response:response,Cause:failure,Message:message} }
        ceiling := math.Min(float64(maximum),float64(minimum)*math.Pow(2,float64(min(attempts-1,62))))
        delay := time.Duration(float64(minimum)+mathrand.Float64()*(ceiling-float64(minimum)))
        remaining := time.Until(end); if remaining-delay <= minimum { delay = remaining }
        timer := time.NewTimer(max(0,delay))
        select { case <-deadline.Done(): timer.Stop(); return nil,timeoutError(); case <-timer.C: }
    }
}
