using System.Net;
using Example.Auth;

static class Program
{
    static void Check(bool value, string message) { if (!value) throw new Exception(message); }
    sealed class Handler : HttpMessageHandler
    {
        public Dictionary<string,string> Headers = new();
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken token)
        {
            Headers = request.Headers.ToDictionary(entry => entry.Key.ToLowerInvariant(), entry => string.Join(",", entry.Value));
            return Task.FromResult(new HttpResponseMessage(HttpStatusCode.NoContent));
        }
    }
    static async Task Main()
    {
        using var request = new HttpRequestMessage(HttpMethod.Get, "https://examplebucket.s3.amazonaws.com/test.txt");
        request.Headers.Add("Range", "bytes=0-9");
        var identity = new AwsCredentials("AKIAIOSFODNN7EXAMPLE", "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY");
        await SigV4Signer.SignAsync(request, identity, new("us-east-1", "s3") { Time = new DateTimeOffset(2013,5,24,0,0,0,TimeSpan.Zero) });
        Check(request.Headers.GetValues("Authorization").Single().EndsWith("Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41", StringComparison.Ordinal), "published AWS signing vector");
        const string prefixUrl = "https://examplebucket.s3.amazonaws.com/test.txt?a-=z&a=z&a=aa&a&a=&a-=b&%61=%2F";
        using var prefix = new HttpRequestMessage(HttpMethod.Get, prefixUrl);
        prefix.Headers.Add("Range", "bytes=0-9");
        await SigV4Signer.SignAsync(prefix, identity, new("us-east-1", "s3") { Time = new DateTimeOffset(2013,5,24,0,0,0,TimeSpan.Zero) });
        Check(prefix.Headers.GetValues("Authorization").Single().EndsWith("Signature=a08d64c4c7bf9729f2b51c2a7e248447c77bf5f32413c0271397bd48b353860e", StringComparison.Ordinal),
            "independent SigV4 prefix-key, duplicate-value and escaping vector");
        using var prefixV4a = new HttpRequestMessage(HttpMethod.Get, prefixUrl);
        prefixV4a.Headers.Add("Range", "bytes=0-9");
        var signedV4a = await SigV4aSigner.SignAsync(prefixV4a, identity, new("*", "s3") { Time = new DateTimeOffset(2013,5,24,0,0,0,TimeSpan.Zero) });
        Check(signedV4a.CanonicalRequest.Split('\n')[2] == "a=&a=&a=%2F&a=aa&a=z&a-=b&a-=z", "SigV4a shares key-then-value query ordering");
        Check(!identity.ToString().Contains(identity.SecretAccessKey, StringComparison.Ordinal), "secret formatting");
        var handler = new Handler(); using var http = new HttpClient(handler);
        int calls = 0;
        using (var preset = new AuthClient(new() { Endpoint = "https://example.test", HttpClient = http, Headers = new() { ["Authorization"] = "Basic supplied" }, BasicCredentials = _ => throw new Exception("preset identity invoked provider") }))
        { await preset.ReadAsync(); Check(handler.Headers["authorization"] == "Basic supplied", "preset Basic auth"); }
        using (var preset = new AuthClient(new() { Endpoint = "https://example.test", HttpClient = http, Headers = new() { ["x-api-key"] = "ApiKey supplied" }, ApiKey = _ => throw new Exception("preset key invoked provider") }))
        { await preset.ReadAsync(); Check(handler.Headers["x-api-key"] == "ApiKey supplied", "preset API key"); }
        using (var unrelated = new AuthClient(new() { Endpoint = "https://example.test", HttpClient = http, Headers = new() { ["Authorization"] = "Digest unrelated" } }))
        { try { await unrelated.ReadAsync(); throw new Exception("unrelated auth accepted"); } catch (InvalidOperationException) { } }
        using (var client = new AuthClient(new() { Endpoint = "https://example.test", HttpClient = http, BasicCredentials = _ => { calls++; return ValueTask.FromResult<BasicCredentials?>(new("user", "password")); } }))
        {
            await client.PublicAsync(); Check(calls == 0 && !handler.Headers.ContainsKey("authorization"), "anonymous bypasses provider");
            await client.ReadAsync(); Check(handler.Headers["authorization"] == "Basic dXNlcjpwYXNzd29yZA==", "Basic authentication");
        }
        using (var client = new AuthClient(new() { Endpoint = "https://example.test", HttpClient = http, ApiKey = _ => ValueTask.FromResult<string?>("secret") }))
        { await client.ReadAsync(); Check(handler.Headers["x-api-key"] == "ApiKey secret", "API key scheme"); }
        using (var client = new AuthClient(new() { Endpoint = "https://example.test", HttpClient = http, Region = "us-east-1", Credentials = _ => ValueTask.FromResult<AwsCredentials?>(new("id", "secret")), Middleware = [(request, _) => { request.Headers.Add("X-Hook", "final"); return ValueTask.CompletedTask; }] }))
        { await client.ReadAsync(); Check(handler.Headers["authorization"].Contains(";x-hook", StringComparison.Ordinal), "sign after middleware"); }
        using (var client = new AuthClient(new() { Endpoint = "https://example.test", HttpClient = http }))
        {
            try { await client.ReadAsync(); throw new Exception("missing identity accepted"); } catch (InvalidOperationException) { }
            await client.OptionalAsync(); Check(!handler.Headers.ContainsKey("authorization"), "optional authentication");
        }
        using (var client = new AuthClient(new() { Endpoint = "http://example.test", HttpClient = http, ApiKey = _ => ValueTask.FromResult<string?>("secret") }))
        { try { await client.ReadAsync(); throw new Exception("plaintext credentials accepted"); } catch (ArgumentException) { } }
        int refreshes = 0;
        var cache = new CachedCredentials(async token => { Interlocked.Increment(ref refreshes); await Task.Delay(10,token); return new("id", "secret", ExpiresAt:DateTimeOffset.UtcNow.AddSeconds(20)); });
        await Task.WhenAll(Enumerable.Range(0,30).Select(_ => cache.RetrieveAsync(default).AsTask()));
        Check(refreshes == 1, "single refresh for concurrent short-lived identities");
        var released = new TaskCompletionSource<AwsCredentials?>(TaskCreationOptions.RunContinuationsAsynchronously);
        int sharedRefreshes = 0;
        var shared = new CachedCredentials(_ => { Interlocked.Increment(ref sharedRefreshes); return new(released.Task); });
        using (var abandoned = new CancellationTokenSource()) {
            var first = shared.RetrieveAsync(abandoned.Token).AsTask();
            abandoned.Cancel();
            try { await first; throw new Exception("cancelled cache consumer continued"); } catch (OperationCanceledException) { }
            var second = shared.RetrieveAsync(default).AsTask();
            Check(sharedRefreshes == 1 && !second.IsCompleted, "cancellation started a competing refresh");
            released.SetResult(new("shared", "secret"));
            Check((await second)?.AccessKeyId == "shared", "remaining consumer lost the shared identity");
        }
        var expired = new CachedCredentials(_ => ValueTask.FromResult<AwsCredentials?>(new("id", "secret", ExpiresAt:DateTimeOffset.UtcNow.AddSeconds(-1))));
        try { await expired.RetrieveAsync(default); throw new Exception("expired credentials accepted"); } catch (InvalidOperationException) { }
        var path = Path.GetTempFileName();
        try {
            await File.WriteAllTextAsync(path, "[test]\naws_access_key_id=id\naws_secret_access_key=s%cret\n");
            var provider = CredentialProviders.Chain(_ => ValueTask.FromResult<AwsCredentials?>(null), CredentialProviders.Profile("test",path));
            Check((await provider(default))?.SecretAccessKey == "s%cret", "provider chain and literal profile");
        } finally { File.Delete(path); }
        using var cancelled = new CancellationTokenSource(); cancelled.Cancel();
        try { await cache.RetrieveAsync(cancelled.Token); throw new Exception("cache ignored cancellation"); } catch (OperationCanceledException) { }
        Console.WriteLine("C# authentication passed");
    }
}
