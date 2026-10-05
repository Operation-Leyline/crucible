package behavior

import (
    "context"
    "encoding/json"
    "io"
    "net/http"
    "strings"
    "testing"
)
type endpointTransport func(*http.Request)(*http.Response,error)
func (f endpointTransport) Do(request *http.Request)(*http.Response,error) { return f(request) }

func TestGeneratedEndpointRules(t *testing.T) {
    calls := 0; host := ""
    transport := endpointTransport(func(request *http.Request)(*http.Response,error){
        calls++; host = request.URL.Hostname()
        if request.Header.Get("X-Region") != "local" { t.Fatal("endpoint header",request.Header) }
        if !strings.Contains(request.Header.Get("Authorization"),"/local/example/aws4_request") || request.Header.Get("X-Amz-Content-Sha256") != "UNSIGNED-PAYLOAD" { t.Fatal("endpoint signing properties",request.Header) }
        return &http.Response{StatusCode:200,Header:http.Header{},Body:io.NopCloser(strings.NewReader(`{"wire-status":"READY"}`))},nil
    })
    config := Config{HTTPClient:transport,Region:"local",Credentials:func(context.Context)(*AWSCredentials,error){ return &AWSCredentials{AccessKeyID:"key",SecretAccessKey:"secret"},nil }}
    client,err := NewServiceClient(config); if err != nil { t.Fatal(err) }
    input := &Input{}; input.SetAccount("customer")
    output,err := client.Read(context.Background(),input)
    if err != nil || output.GetStatus() != "READY" || host != "customer.local.example.test" { t.Fatal(output,host,err) }
    for _,label := range []string{"a.b","-bad","x/y","x@evil","",strings.Repeat("x",64)} {
        input.SetAccount(label); if _,err := client.Read(context.Background(),input); err == nil { t.Fatal("invalid host label",label) }
    }
    if calls != 1 { t.Fatal("invalid host dispatched") }
    config.DisableHostPrefix = true; client,err = NewServiceClient(config); if err != nil { t.Fatal(err) }; input.SetAccount("ignored")
    if _,err := client.Read(context.Background(),input); err != nil || host != "local.example.test" { t.Fatal(host,err) }
    config.Region = "missing"; client,err = NewServiceClient(config); if err != nil { t.Fatal(err) }
    if _,err := client.Read(context.Background(),input); err == nil || !strings.Contains(err.Error(),"Unknown region missing") { t.Fatal(err) }
}
func TestEndpointStandardFunctions(t *testing.T) {
    for _,value := range []string{"file:///path","https://user:pass@example.test","https://example.test?q=x","https://example.test#x","https://example.test:99999","https://evil\n.example"} { if parseEndpoint(value) != nil { t.Fatal("invalid endpoint accepted",value) } }
    if parseEndpoint("https://example.test:443/a")["authority"] != "example.test" || parseEndpoint("https://[::1]:8443")["authority"] != "[::1]:8443" { t.Fatal("endpoint authority normalization") }
    if endpointFunction("substring",[]any{"abcdef",json.Number("1"),json.Number("3"),true}) != "de" { t.Fatal("reverse substring") }
    if endpointFunction("substring",[]any{"é",json.Number("0"),json.Number("1"),false}) != nil { t.Fatal("non-ASCII substring") }
    if endpointFunction("isSet",[]any{false}) != true { t.Fatal("false is present") }
    if endpointFunction("uriEncode",[]any{"a/b +é"}) != "a%2Fb%20%2B%C3%A9" { t.Fatal("URI encoding") }
}
