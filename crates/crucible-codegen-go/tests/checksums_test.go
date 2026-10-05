package checksum

import (
    "context"
    "encoding/json"
    "errors"
    "io"
    "net/http"
    "net/http/httptest"
    "os"
    "strings"
    "testing"
)

func TestChecksumHTTP(t *testing.T) {
    data,err:=os.ReadFile("checksums.json");if err!=nil{t.Fatal(err)}
    var vectors struct{ InputUtf8 string; Base64 map[string]string; EmptyMd5 string }
    if err:=json.Unmarshal(data,&vectors);err!=nil{t.Fatal(err)}
    payload:=[]byte(vectors.InputUtf8);mode:="normal";var observed http.Header;validated:=[]string{}
    server:=httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter,r *http.Request){
        observed=r.Header.Clone();body,_:=io.ReadAll(r.Body);algorithm:=r.Header.Get("x-algorithm")
        if mode!="override" && mode!="skip"{
            name:="content-md5";selected:="MD5";if algorithm!=""{selected=algorithm;name="x-amz-checksum-"+strings.ToLower(algorithm)}
            expected:=vectors.Base64[selected];if len(body)==0{expected=vectors.EmptyMd5}
            if r.Header.Get(name)!=expected{t.Errorf("request checksum %s: %s != %s",name,r.Header.Get(name),expected)}
        }
        selected:=algorithm;if selected==""{selected="CRC32"}
        if mode!="unmodeled"{value:=vectors.Base64[selected];if mode=="bad"{value="AAAA"};w.Header().Set("x-amz-checksum-"+strings.ToLower(selected),value)}
        w.Header().Set("x-amz-checksum-xxhash3","unmodeled")
        if mode=="one"{w.Header().Set("x-amz-checksum-sha256","AAAA")}
        w.Write(payload)
    }));defer server.Close()
    client,err:=NewChecksumsClient(Config{Endpoint:server.URL,ChecksumValidated:func(operation,algorithm string){validated=append(validated,operation+":"+algorithm)}});if err!=nil{t.Fatal(err)}
    ctx:=context.Background();input:=&Input{};input.SetData(payload)
    if _,err:=client.Legacy(ctx,input);err!=nil{t.Fatal(err)}
    if _,err:=client.Required(ctx,input);err!=nil{t.Fatal(err)}
    if _,err:=client.Legacy(ctx,&Input{});err!=nil{t.Fatal(err)}
    if _,err:=client.Flexible(ctx,input);err!=nil{t.Fatal(err)}
    if input.Algorithm.Set || input.Validation.Set || observed.Get("x-validation")!="ENABLED"{t.Fatal("private defaults",input,observed)}
    for algorithm:=range vectors.Base64{
        selected:=&Input{};selected.SetData(payload);selected.SetAlgorithm(Algorithm(algorithm))
        if _,err:=client.Flexible(ctx,selected);err!=nil{t.Fatal(algorithm,err)}
        if validated[len(validated)-1]!="Flexible:"+algorithm{t.Fatal("validation callback")}
    }
    mode="bad";_,err=client.Flexible(ctx,input);var mismatch *ChecksumError;if !errors.As(err,&mismatch)||mismatch.Algorithm!="CRC32"{t.Fatal("corruption accepted",err)}
    mode="one";if _,err:=client.Flexible(ctx,input);err!=nil{t.Fatal(err)}
    mode="unmodeled";count:=len(validated);if _,err:=client.Flexible(ctx,input);err!=nil{t.Fatal(err)};if len(validated)!=count{t.Fatal("unmodeled checksum validated")}
    mode="override";explicit:=&Input{};explicit.SetData(payload);explicit.SetChecksum("supplied");if _,err:=client.Flexible(ctx,explicit);err!=nil{t.Fatal(err)};if observed.Get("x-amz-checksum-crc32")!="supplied"{t.Fatal("override lost")}
    mode="skip";optout,err:=NewChecksumsClient(Config{Endpoint:server.URL,RequestChecksumCalculation:ChecksumWhenRequired,ResponseChecksumValidation:ChecksumWhenRequired});if err!=nil{t.Fatal(err)}
    if _,err:=optout.Flexible(ctx,input);err!=nil{t.Fatal(err)};if observed.Get("x-algorithm")!=""||observed.Get("x-validation")!=""{t.Fatal("optout failed")}
    unsupported:=&Input{};unsupported.SetAlgorithm(Algorithm("XXHASH64"));if _,err:=client.Flexible(ctx,unsupported);err==nil{t.Fatal("unsupported algorithm accepted")}
    mode="normal";hook,err:=NewChecksumsClient(Config{Endpoint:server.URL,Middleware:[]func(context.Context,*http.Request)error{func(_ context.Context,r *http.Request)error{r.Body=io.NopCloser(strings.NewReader(vectors.InputUtf8));r.ContentLength=int64(len(payload));return nil}}});if err!=nil{t.Fatal(err)}
    if _,err:=hook.Legacy(ctx,&Input{});err!=nil{t.Fatal(err)}
}
