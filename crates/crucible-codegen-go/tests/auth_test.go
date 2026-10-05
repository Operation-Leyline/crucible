package auth

import (
    "context"
    "errors"
    "fmt"
    "net/http"
    "net/http/httptest"
    "os"
    "path/filepath"
    "strings"
    "sync"
    "sync/atomic"
    "testing"
    "time"
)

func TestPublishedSigningVector(t *testing.T) {
    request, _ := http.NewRequest("GET", "https://examplebucket.s3.amazonaws.com/test.txt", nil)
    request.Header.Set("Range", "bytes=0-9")
    identity := AWSCredentials{AccessKeyID:"AKIAIOSFODNN7EXAMPLE", SecretAccessKey:"wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"}
    err := SignV4(request, identity, SigningOptions{Service:"s3", Region:"us-east-1", Time:time.Date(2013,5,24,0,0,0,0,time.UTC)})
    if err != nil { t.Fatal(err) }
    if !strings.HasSuffix(request.Header.Get("Authorization"), "Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41") { t.Fatal(request.Header.Get("Authorization")) }
    if strings.Contains(fmt.Sprintf("%v %#v", identity, identity), identity.SecretAccessKey) { t.Fatal("secret in formatting") }
}

func TestPreconfiguredCredentials(t *testing.T) {
    server:=httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter,r *http.Request){w.WriteHeader(204)}));defer server.Close()
    provider:=func(context.Context)(*BasicCredentials,error){t.Error("preconfigured identity invoked provider");return nil,errors.New("provider invoked")}
    client,err:=NewAuthClient(Config{Endpoint:server.URL,Headers:http.Header{"Authorization":[]string{"Basic supplied"}},BasicCredentials:provider});if err!=nil{t.Fatal(err)}
    if _,err:=client.Read(context.Background(),Unit{});err!=nil{t.Fatal(err)}
    client,err=NewAuthClient(Config{Endpoint:server.URL,Headers:http.Header{"X-Api-Key":[]string{"ApiKey supplied"}},APIKey:func(context.Context)(string,error){t.Error("preconfigured key invoked provider");return "",nil}});if err!=nil{t.Fatal(err)}
    if _,err:=client.Read(context.Background(),Unit{});err!=nil{t.Fatal(err)}
    client,err=NewAuthClient(Config{Endpoint:server.URL,Headers:http.Header{"Authorization":[]string{"Digest unrelated"}}});if err!=nil{t.Fatal(err)}
    if _,err:=client.Read(context.Background(),Unit{});err==nil{t.Fatal("unrelated auth accepted")}
}

func TestModeledAuthentication(t *testing.T) {
    var received http.Header
    server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { received = r.Header.Clone(); w.WriteHeader(204) }))
    defer server.Close()
    calls := 0
    config := Config{Endpoint:server.URL, BasicCredentials:func(context.Context) (*BasicCredentials,error) { calls++; return &BasicCredentials{"user","password"},nil }}
    client, err := NewAuthClient(config); if err != nil { t.Fatal(err) }
    if _, err := client.Public(context.Background(), Unit{}); err != nil || calls != 0 || received.Get("Authorization") != "" { t.Fatal(calls, err) }
    if _, err := client.Read(context.Background(), Unit{}); err != nil || received.Get("Authorization") != "Basic dXNlcjpwYXNzd29yZA==" { t.Fatal(err, received) }
    config.BasicCredentials = nil
    config.APIKey = func(context.Context)(string,error) { return "secret",nil }
    client, _ = NewAuthClient(config)
    if _, err := client.Read(context.Background(), Unit{}); err != nil || received.Get("X-Api-Key") != "ApiKey secret" { t.Fatal(err, received) }
    config.APIKey = nil; config.Region = "us-east-1"
    config.Credentials = func(context.Context)(*AWSCredentials,error) { return &AWSCredentials{AccessKeyID:"id",SecretAccessKey:"secret"},nil }
    config.Middleware = []func(context.Context,*http.Request)error{func(_ context.Context,r *http.Request)error { r.Header.Set("X-Hook","final"); return nil }}
    client, _ = NewAuthClient(config)
    if _, err := client.Read(context.Background(), Unit{}); err != nil || !strings.Contains(received.Get("Authorization"), ";x-hook") { t.Fatal(err, received) }
    config.Credentials = nil
    client, _ = NewAuthClient(config)
    if _, err := client.Read(context.Background(), Unit{}); err == nil { t.Fatal("required identity missing") }
    if _, err := client.Optional(context.Background(), Unit{}); err != nil || received.Get("Authorization") != "" { t.Fatal(err) }
    config.Endpoint = "http://example.test"; config.APIKey = func(context.Context)(string,error) { return "secret",nil }; client, _ = NewAuthClient(config)
    if _, err := client.Read(context.Background(), Unit{}); err == nil || !strings.Contains(err.Error(), "plaintext") { t.Fatal(err) }
}

func TestCredentialProvidersAndRefresh(t *testing.T) {
    source := filepath.Join(t.TempDir(), "credentials")
    if err := os.WriteFile(source, []byte("[test]\naws_access_key_id = id\naws_secret_access_key = s%cret\naws_session_token = token\n"),0600); err != nil { t.Fatal(err) }
    provider := CredentialChain(func(context.Context)(*AWSCredentials,error){ return nil,nil }, ProfileCredentials("test",source))
    value, err := provider(context.Background()); if err != nil || value.SecretAccessKey != "s%cret" { t.Fatal(value,err) }
    var calls atomic.Int32
    cached, err := NewCachedCredentials(func(context.Context)(*AWSCredentials,error) { calls.Add(1); return &AWSCredentials{AccessKeyID:"id", SecretAccessKey:"secret", ExpiresAt:time.Now().Add(20*time.Second)},nil },time.Minute)
    if err != nil { t.Fatal(err) }
    var wait sync.WaitGroup
    for range 30 { wait.Add(1); go func(){ defer wait.Done(); if _, err := cached.Retrieve(context.Background()); err != nil { t.Error(err) } }() }; wait.Wait()
    if calls.Load() != 1 { t.Fatal(calls.Load()) }
    value,_ = cached.Retrieve(context.Background()); value.SecretAccessKey = "changed"
    value,_ = cached.Retrieve(context.Background()); if value.SecretAccessKey != "secret" { t.Fatal("cache shares mutable identity") }
    started, release := make(chan struct{}), make(chan struct{})
    blocked,_ := NewCachedCredentials(func(context.Context)(*AWSCredentials,error){ close(started); <-release; return nil,nil },time.Minute)
    done := make(chan struct{}); go func(){ defer close(done); blocked.Retrieve(context.Background()) }(); <-started
    ctx,cancel := context.WithCancel(context.Background()); cancel()
    if _, err := blocked.Retrieve(ctx); !errors.Is(err,context.Canceled) { t.Fatal(err) }; close(release); <-done
    expired,_ := NewCachedCredentials(func(context.Context)(*AWSCredentials,error){ return &AWSCredentials{ExpiresAt:time.Now().Add(-time.Second)},nil },0)
    if _, err := expired.Retrieve(context.Background()); err == nil { t.Fatal("expired identity accepted") }
}
