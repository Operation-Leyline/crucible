package sdk

import (
    "bufio"
    "bytes"
    "context"
    "crypto/hmac"
    "crypto/sha256"
    "encoding/hex"
    "errors"
    "fmt"
    "io"
    "net/http"
    "net/url"
    "os"
    "path"
    "path/filepath"
    "sort"
    "strings"
    "sync"
    "time"
)

// BasicCredentials holds HTTP Basic credentials. String and GoString redact both fields.
type BasicCredentials struct { Username, Password string }
// String returns a readable representation.
func (BasicCredentials) String() string { return "BasicCredentials{redacted}" }
// GoString returns the same redacted representation for Go syntax formatting.
func (c BasicCredentials) GoString() string { return c.String() }
// AWSCredentials holds AWS signing credentials and optional expiry. String and GoString redact credentials.
type AWSCredentials struct { AccessKeyID, SecretAccessKey, SessionToken string; ExpiresAt time.Time }
// String returns a readable representation.
func (AWSCredentials) String() string { return "AWSCredentials{redacted}" }
// GoString returns the same redacted representation for Go syntax formatting.
func (c AWSCredentials) GoString() string { return c.String() }
// CredentialsProvider resolves signing credentials for an attempt; nil means unavailable.
type CredentialsProvider func(context.Context) (*AWSCredentials, error)

// CredentialChain stops on the first identity or error. Nil means unavailable.
func CredentialChain(providers ...CredentialsProvider) CredentialsProvider {
    providers = append([]CredentialsProvider(nil), providers...)
    return func(ctx context.Context) (*AWSCredentials, error) {
        for _, provider := range providers {
            if err := ctx.Err(); err != nil { return nil, err }
            if provider == nil { continue }
            value, err := provider(ctx)
            if err != nil || value != nil { return value, err }
        }
        return nil, nil
    }
}
func staticCredentials(access, secret, token string) (*AWSCredentials, error) {
    if access == "" && secret == "" { return nil, nil }
    if access == "" || secret == "" { return nil, errors.New("AWS credentials are incomplete") }
    return &AWSCredentials{AccessKeyID:access, SecretAccessKey:secret, SessionToken:token}, nil
}
// EnvironmentCredentials reads static AWS environment credentials; partial credentials return an error.
func EnvironmentCredentials(ctx context.Context) (*AWSCredentials, error) {
    if err := ctx.Err(); err != nil { return nil, err }
    return staticCredentials(os.Getenv("AWS_ACCESS_KEY_ID"), os.Getenv("AWS_SECRET_ACCESS_KEY"), os.Getenv("AWS_SESSION_TOKEN"))
}
// ProfileCredentials reads static shared-file credentials without executing commands.
func ProfileCredentials(profile, filename string) CredentialsProvider {
    return func(ctx context.Context) (*AWSCredentials, error) {
        if err := ctx.Err(); err != nil { return nil, err }
        name, source := profile, filename
        if name == "" { name = os.Getenv("AWS_PROFILE"); if name == "" { name = "default" } }
        if source == "" { source = os.Getenv("AWS_SHARED_CREDENTIALS_FILE") }
        if source == "" { home, err := os.UserHomeDir(); if err != nil { return nil, err }; source = filepath.Join(home, ".aws", "credentials") }
        file, err := os.Open(source)
        if os.IsNotExist(err) { return nil, nil }; if err != nil { return nil, err }; defer file.Close()
        section := ""; values := map[string]string{}; scanner := bufio.NewScanner(file)
        for scanner.Scan() {
            if err := ctx.Err(); err != nil { return nil, err }
            line := strings.TrimSpace(scanner.Text())
            if line == "" || strings.HasPrefix(line, "#") || strings.HasPrefix(line, ";") { continue }
            if strings.HasPrefix(line, "[") && strings.HasSuffix(line, "]") { section = strings.TrimSpace(line[1:len(line)-1]); continue }
            if section != name { continue }
            key, value, ok := strings.Cut(line, "="); if !ok { return nil, errors.New("invalid credentials profile entry") }
            key = strings.TrimSpace(key)
            if _, exists := values[key]; exists { return nil, errors.New("duplicate credentials profile entry") }
            values[key] = strings.TrimSpace(value)
        }
        if err := scanner.Err(); err != nil { return nil, err }
        return staticCredentials(values["aws_access_key_id"], values["aws_secret_access_key"], values["aws_session_token"])
    }
}
// CachedCredentials coalesces concurrent provider refreshes and caches credentials until the refresh window.
type CachedCredentials struct {
    provider CredentialsProvider
    skew time.Duration
    mu sync.Mutex
    refreshing chan struct{}
    value *AWSCredentials
    refreshAt time.Time
}
// NewCachedCredentials requires a provider and nonnegative refresh window; credentials are resolved lazily.
func NewCachedCredentials(provider CredentialsProvider, refreshBefore time.Duration) (*CachedCredentials, error) {
    if provider == nil || refreshBefore < 0 { return nil, errors.New("a provider and nonnegative refresh interval are required") }
    return &CachedCredentials{provider:provider, skew:refreshBefore}, nil
}
// Retrieve returns cached credentials or coalesces a provider refresh; ctx cancels waiting.
func (c *CachedCredentials) Retrieve(ctx context.Context) (*AWSCredentials, error) {
    for {
        if err := ctx.Err(); err != nil { return nil, err }
        c.mu.Lock()
        if c.value != nil && (c.refreshAt.IsZero() || time.Now().Before(c.refreshAt)) { value := *c.value; c.mu.Unlock(); return &value, nil }
        if wait := c.refreshing; wait != nil {
            c.mu.Unlock(); select { case <-ctx.Done(): return nil, ctx.Err(); case <-wait: continue }
        }
        c.refreshing = make(chan struct{}); c.mu.Unlock()
        value, err := c.provider(ctx)
        now := time.Now()
        if err == nil && value != nil && !value.ExpiresAt.IsZero() && !value.ExpiresAt.After(now) { err = errors.New("credential provider returned expired credentials") }
        c.mu.Lock()
        if err == nil {
            c.value = nil; c.refreshAt = time.Time{}
            if value != nil {
                copied := *value; c.value = &copied
                if !value.ExpiresAt.IsZero() { c.refreshAt = value.ExpiresAt.Add(-min(c.skew, value.ExpiresAt.Sub(now)/2)) }
            }
        }
        close(c.refreshing); c.refreshing = nil; c.mu.Unlock()
        return value, err
    }
}

// SigningOptions selects SigV4 region, service signing name and path/payload signing policy.
type SigningOptions struct {
    Region, Service string
    Time time.Time
    UnsignedPayload bool
    DisableDoubleEncoding, DisableNormalizePath *bool
}
func hashText(data []byte) string { sum := sha256.Sum256(data); return hex.EncodeToString(sum[:]) }
func signHMAC(key []byte, value string) []byte { mac := hmac.New(sha256.New, key); mac.Write([]byte(value)); return mac.Sum(nil) }
// SignV4 signs the final HTTP request and leaves its body readable by the transport.
func SignV4(request *http.Request, credentials AWSCredentials, options SigningOptions) error {
    if err := request.Context().Err(); err != nil { return err }
    now := options.Time.UTC(); if options.Time.IsZero() { now = time.Now().UTC() }
    if credentials.AccessKeyID == "" || credentials.SecretAccessKey == "" || (!credentials.ExpiresAt.IsZero() && !credentials.ExpiresAt.After(now)) { return errors.New("AWS credentials are missing or expired") }
    if options.Service == "" || options.Region == "" || strings.ContainsAny(options.Service+options.Region, "/\r\n") { return errors.New("SigV4 region and service are required scope components") }
    if request.URL == nil { return errors.New("request URL is required") }
    if request.Header == nil { request.Header = http.Header{} }
    request.Header.Del("Authorization")
    request.Header.Set("X-Amz-Date", now.Format("20060102T150405Z"))
    if credentials.SessionToken != "" { request.Header.Set("X-Amz-Security-Token", credentials.SessionToken) } else { request.Header.Del("X-Amz-Security-Token") }
    digest := "UNSIGNED-PAYLOAD"
    if _, isStream := request.Body.(*streamBody); isStream && !options.UnsignedPayload {
        // A stream is never read to sign it: its digest must be declared.
        digest = strings.ToLower(request.Header.Get("X-Amz-Content-Sha256"))
        if digest == "" { return fmt.Errorf("%w: SigV4 needs ByteStream.WithSHA256 or an unsigned-payload operation", ErrStreamDigest) }
    } else if !options.UnsignedPayload {
        var data []byte
        if request.Body != nil {
            var err error; data, err = io.ReadAll(request.Body); request.Body.Close(); if err != nil { return err }
            request.Body = io.NopCloser(bytes.NewReader(data)); request.ContentLength = int64(len(data))
            request.GetBody = func() (io.ReadCloser, error) { return io.NopCloser(bytes.NewReader(data)), nil }
        }
        digest = hashText(data)
    }
    request.Header.Set("X-Amz-Content-Sha256", digest)
    uri := request.URL.EscapedPath(); if uri == "" { uri = "/" }
    normalize, double := options.Service != "s3", options.Service != "s3"
    if options.DisableNormalizePath != nil { normalize = !*options.DisableNormalizePath }
    if options.DisableDoubleEncoding != nil { double = !*options.DisableDoubleEncoding }
    if normalize { trailing := strings.HasSuffix(uri, "/"); uri = "/" + strings.TrimLeft(path.Clean(uri), "/"); if trailing && !strings.HasSuffix(uri, "/") { uri += "/" } }
    if double { parts := strings.Split(uri, "/"); for i, part := range parts { parts[i] = escape(part) }; uri = strings.Join(parts, "/") }
    query := []string{}
    for _, pair := range strings.Split(request.URL.RawQuery, "&") {
        if pair == "" { continue }; key, value, _ := strings.Cut(pair, "=")
        key, err := url.PathUnescape(key); if err != nil { return err }; value, err = url.PathUnescape(value); if err != nil { return err }
        query = append(query, escape(key)+"="+escape(value))
    }; sort.Strings(query)
    excluded := map[string]bool{"authorization":true,"connection":true,"expect":true,"transfer-encoding":true,"user-agent":true,"x-amzn-trace-id":true,"accept-encoding":true,"content-length":true}
    headers := map[string][]string{}
    for key, values := range request.Header {
        name := strings.ToLower(key); if excluded[name] || name == "host" { continue }
        for _, value := range values { headers[name] = append(headers[name], strings.Join(strings.Fields(value), " ")) }
    }
    host := request.Host; if host == "" { host = request.URL.Host }; headers["host"] = []string{host}
    names := make([]string, 0, len(headers)); for key := range headers { names = append(names, key) }; sort.Strings(names)
    var canonicalHeaders strings.Builder
    for _, name := range names { canonicalHeaders.WriteString(name+":"+strings.Join(headers[name], ",")+"\n") }
    signed := strings.Join(names, ";")
    canonical := strings.Join([]string{request.Method, uri, strings.Join(query, "&"), canonicalHeaders.String(), signed, digest}, "\n")
    date := now.Format("20060102"); scope := date+"/"+options.Region+"/"+options.Service+"/aws4_request"
    toSign := "AWS4-HMAC-SHA256\n"+request.Header.Get("X-Amz-Date")+"\n"+scope+"\n"+hashText([]byte(canonical))
    key := []byte("AWS4"+credentials.SecretAccessKey)
    for _, part := range []string{date, options.Region, options.Service, "aws4_request"} { key = signHMAC(key, part) }
    request.Header.Set("Authorization", fmt.Sprintf("AWS4-HMAC-SHA256 Credential=%s/%s, SignedHeaders=%s, Signature=%s", credentials.AccessKeyID, scope, signed, hex.EncodeToString(signHMAC(key, toSign))))
    return request.Context().Err()
}

type authScheme struct { Kind, Name, Location, Scheme, SigningName, SigningRegion string; DisableDoubleEncoding, DisableNormalizePath *bool }
func presetAuth(request *http.Request, scheme authScheme) bool {
    value,prefix := request.Header.Get("Authorization"),""
    switch scheme.Kind {
    case "basic": prefix="Basic"
    case "bearer": prefix="Bearer"
    case "sigV4": prefix="AWS4-HMAC-SHA256"
    case "apiKey":
        if scheme.Location=="query" { return request.URL.Query().Get(scheme.Name)!="" }
        value,prefix=request.Header.Get(scheme.Name),scheme.Scheme
        if prefix=="" { return value!="" }
    default: return false
    }
    return len(value)>len(prefix)+1 && strings.EqualFold(value[:len(prefix)+1],prefix+" ")
}
func (c *clientRuntime) authenticate(ctx context.Context, request *http.Request, op operationDescription) (bool, func() error, error) {
    ids := op.Auth
    if ids == nil && op.Bearer { ids = []string{"smithy.api#httpBearerAuth"} }
    for _, id := range ids {
        scheme := c.schema.AuthSchemes[id]; if id == "smithy.api#httpBearerAuth" { scheme.Kind = "bearer" }
        if op.ResolvedEndpoint != nil {
            if choices,ok := op.ResolvedEndpoint.Properties["authSchemes"].([]any); ok {
                matched := false
                for _,item := range choices {
                    value := item.(map[string]any); name,_ := value["name"].(string); if !strings.EqualFold(name,scheme.Kind) { continue }; matched = true
                    if name,ok := value["signingName"].(string); ok { scheme.SigningName = name }
                    if region,ok := value["signingRegion"].(string); ok { scheme.SigningRegion = region }
                    if option,ok := value["disableDoubleEncoding"].(bool); ok { scheme.DisableDoubleEncoding = &option }
                    if option,ok := value["disableNormalizePath"].(bool); ok { scheme.DisableNormalizePath = &option }
                    break
                }; if !matched { continue }
            }
        }
        if presetAuth(request,scheme) { return true,nil,nil }
        switch scheme.Kind {
        case "bearer":
            if c.config.Token != nil { token, err := c.config.Token(ctx); if err != nil { return false, nil, err }; if token != "" { request.Header.Set("Authorization", "Bearer "+token); return true, nil, nil } }
        case "basic":
            if c.config.BasicCredentials != nil { value, err := c.config.BasicCredentials(ctx); if err != nil { return false, nil, err }; if value != nil { if strings.Contains(value.Username, ":") { return false, nil, errors.New("Basic username cannot contain a colon") }; request.SetBasicAuth(value.Username, value.Password); return true, nil, nil } }
        case "apiKey":
            if c.config.APIKey != nil { value, err := c.config.APIKey(ctx); if err != nil { return false, nil, err }; if value != "" {
                if scheme.Location == "query" { query := request.URL.Query(); query.Set(scheme.Name, value); request.URL.RawQuery = query.Encode() } else { prefix := scheme.Scheme; if prefix != "" { prefix += " " }; request.Header.Set(scheme.Name, prefix+value) }; return true, nil, nil
            } }
        case "sigV4":
            if c.config.Credentials != nil { value, err := c.config.Credentials(ctx); if err != nil { return false, nil, err }; if value != nil {
                identity := *value
                region := scheme.SigningRegion; if region == "" { region = c.config.Region }
                return true, func() error { return SignV4(request, identity, SigningOptions{Region:region, Service:scheme.SigningName, UnsignedPayload:op.UnsignedPayload, DisableDoubleEncoding:scheme.DisableDoubleEncoding, DisableNormalizePath:scheme.DisableNormalizePath}) }, nil
            } }
        }
    }
    if len(ids) > 0 && !(len(ids) == 1 && op.Bearer) && !op.OptionalAuth { return false, nil, errors.New("no identity available for modeled authentication schemes") }
    return false, nil, nil
}
