// Published vectors for SHA-512/256 (FIPS 180-4), HTTP Digest (RFC 7616 section 3.9) and SigV4a (the AWS CRT
// aws-signing-test-suite v4a cases), client behavior through an injected handler, and a loopback HTTP server that
// verifies Digest responses and SigV4a signatures itself.
using System.Collections.Concurrent;
using System.Globalization;
using System.Net;
using System.Net.Sockets;
using System.Numerics;
using System.Reflection;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using Example.Vault;
using Example.Vault.Models;

static class Program
{
    const string AccessKey = "AKIDEXAMPLE";
    const string SecretKey = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
    static void Check(bool ok, string message) { if (!ok) throw new Exception(message); }
    static void Rejects<T>(Func<Task> work, string message) where T : Exception
    {
        try { work().GetAwaiter().GetResult(); }
        catch (T) { return; }
        catch (Exception e) { throw new Exception($"{message}: {e.GetType().Name} {e.Message}", e); }
        throw new Exception($"{message}: expected {typeof(T).Name}");
    }
    static void Rejects<T>(Action work, string message) where T : Exception => Rejects<T>(() => { work(); return Task.CompletedTask; }, message);
    static byte[] Hex(string hex) => Convert.FromHexString(hex);
    static byte[] Sha512_256(byte[] data) =>
        (byte[])typeof(VaultClient).Assembly.GetType("Example.Vault.Sha512_256")!.GetMethod("HashData", BindingFlags.NonPublic | BindingFlags.Static)!.Invoke(null, [data])!;

    static async Task Main()
    {
        DigestVectors();
        await SigV4aVectors();
        await DigestClient();
        await SigV4aClient();
        await Loopback();
        Console.WriteLine("C# SHA-512/256, Digest (RFC 7616), SigV4a (AWS CRT vectors), client and loopback checks passed");
    }

    static void DigestVectors()
    {
        var challenge = new Dictionary<string, string> { ["realm"] = "http-auth@example.org", ["qop"] = "auth, auth-int",
            ["nonce"] = "7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v", ["opaque"] = "FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS" };
        const string cnonce = "f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ";
        Check(DigestAuthentication.Header(new Dictionary<string, string>(challenge) { ["algorithm"] = "MD5" }, "Mufasa", "Circle of Life", "GET", "/dir/index.html", [], "00000001", cnonce)
            == "Digest username=\"Mufasa\", realm=\"http-auth@example.org\", uri=\"/dir/index.html\", algorithm=MD5, nonce=\"7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v\", nc=00000001, cnonce=\"f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ\", qop=auth, response=\"8ca523f5e9506fed4657c9700eebdbec\", opaque=\"FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS\"",
            "RFC 7616 MD5 example");
        Check(DigestAuthentication.Header(new Dictionary<string, string>(challenge) { ["algorithm"] = "SHA-256" }, "Mufasa", "Circle of Life", "GET", "/dir/index.html", [], "00000001", cnonce)
            .Contains("response=\"753927fa0e85d155564e2e272a28d1802ca10daf4496794697cf8db5856cb6c1\"", StringComparison.Ordinal), "RFC 7616 SHA-256 example");
        var header = DigestAuthentication.Header(new Dictionary<string, string> { ["realm"] = "api@example.org", ["qop"] = "auth", ["algorithm"] = "SHA-512-256",
            ["nonce"] = "5TsQWLVdgBdmrQ0XsxbDODV+57QdFR34I9HAbC/RVvkK", ["opaque"] = "HRPCssKJSGjCrkzDg8OhwpzCiGPChXYjwrI2QmXDnsOS", ["charset"] = "UTF-8", ["userhash"] = "true" },
            "Jäsøn Doe", "Secret, or not?", "GET", "/doe.json", [], "00000001", "NTg6RKcb9boFIAS3KrFK9BGeh+iDa/sm6jUMp2wds69v");
        // The printed 3.9.2 values truncate SHA-512 rather than use FIPS 180-4 SHA-512/256; RFC erratum 4897 (reported) gives these.
        Check(header.Contains("username=\"793263caabb707a56211940d90411ea4a575adeccb7e360aeb624ed06ece9b0b\"", StringComparison.Ordinal)
            && header.Contains("userhash=true", StringComparison.Ordinal)
            && header.Contains("response=\"3798d4131c277846293534c3edc11bd8a5e4cdcbff78b05db9d95eeb1cec68a5\"", StringComparison.Ordinal), "RFC 7616 SHA-512-256 userhash example (erratum 4897)");
        Check(DigestAuthentication.Header(new Dictionary<string, string> { ["realm"] = "r", ["nonce"] = "n", ["qop"] = "auth" }, "Jäson", "p", "GET", "/", [], "00000001", "c")
            .Contains("username*=UTF-8''J%C3%A4son", StringComparison.Ordinal), "non-ASCII usernames use the extended parameter");
        var selected = DigestAuthentication.Select("Basic realm=\"x\", Digest realm=\"r\", qop=\"auth\", algorithm=MD5, nonce=\"a\", Digest realm=\"r\", qop=\"auth-int\", algorithm=SHA-256, nonce=\"b\", Digest realm=\"r\", algorithm=UNKNOWN, nonce=\"c\"")!;
        Check(selected["nonce"] == "b" && selected["algorithm"] == "SHA-256", "strongest supported challenge");
        Check(DigestAuthentication.Select("Digest realm=\"r\", nonce=\"n\", qop=\"auth-conf\"") is null && DigestAuthentication.Select("Bearer realm=\"x\"") is null, "unsupported challenges");
        static string Md5(string text) => Convert.ToHexString(MD5.HashData(Encoding.UTF8.GetBytes(text))).ToLowerInvariant();
        Check(DigestAuthentication.Header(new Dictionary<string, string> { ["realm"] = "r", ["nonce"] = "n", ["qop"] = "auth-int", ["algorithm"] = "MD5-sess" }, "u", "p", "POST", "/x", "body"u8.ToArray(), "00000002", "c")
            .Contains("response=\"" + Md5(Md5(Md5("u:r:p") + ":n:c") + ":n:00000002:c:auth-int:" + Md5("POST:/x:" + Md5("body"))) + "\"", StringComparison.Ordinal), "MD5-sess with auth-int");
        Rejects<ArgumentException>(() => DigestAuthentication.Header(new Dictionary<string, string> { ["realm"] = "r", ["nonce"] = "n", ["qop"] = "auth-int" }, "u", "p", "POST", "/", null, "00000001", "c"), "auth-int needs a buffered body");
    }

    // SigV4a: the AWS CRT aws-signing-test-suite/v4a cases (sign_body=false). ECDSA signatures are randomized, so the canonical
    // request and string to sign must match exactly, the derived key must equal the published public key, and both the
    // published and our signatures must verify.
    static async Task SigV4aVectors()
    {
        using var key = SigV4aSigner.DeriveKey(new AwsCredentials(AccessKey, SecretKey));
        var point = key.ExportParameters(false).Q;
        Check(Convert.ToHexString(point.X!) == "B6618F6A65740A99E650B33B6B4B5BD0D43B176D721A3EDFEA7E7D2D56D936B1"
            && Convert.ToHexString(point.Y!) == "865ED22A7EADC9C5CB9D2CBACA1B3699139FEDC5043DC6661864218330C8E518", "SigV4a derived public key");
        var now = new DateTimeOffset(2015, 8, 30, 12, 36, 0, TimeSpan.Zero);
        foreach (var (method, token, signed, hash, published) in new[]
        {
            ("GET", (string?)null, "host;x-amz-date;x-amz-region-set", "cf59db423e841c8b7e3444158185aa261b724a5c27cbe762676f3eed19f4dc02",
                "3045022018b4e277d0281864beb51d3600e23f88510ea5031d68ddfbb68614b82a5eb7d2022100effb9c5f22ed9ef3ae0ab243d21f06bce82365bbb79529a07b6888c343ae5f8c"),
            ("POST", null, "host;x-amz-date;x-amz-region-set", "806a9b01b76472cc6b66fff02630726d55f8b4ada6d2fd9b36eb0d710e215861",
                "3044022051fe398025aafbfc21d054bc78e5edfb96c9acb7fd272795565181d757815e4702202e7b8d2b92324290b1d95f8b0fc5e333bb8b5e333f6160bcab39d7258156d224"),
            ("GET", "6e86291e8372ff2a2260956d9b8aae1d763fbf315fa00fa31553b73ebf194267", "host;x-amz-date;x-amz-region-set;x-amz-security-token", "79893373104239a0547df489af395ec3c1b8873a8601f07f11ffd3f1ac557e7d",
                "3044022035c6bc9a8cab8a434bd8bd1cc3b6f1401842f530f1537722ad10d72a1577f8c80220101352fe0720715efbab80f82edb133175f0b61cad85daf943ad615852f9ae10"),
        })
        {
            using var request = new HttpRequestMessage(new HttpMethod(method), "https://example.amazonaws.com/");
            var result = await SigV4aSigner.SignAsync(request, new AwsCredentials(AccessKey, SecretKey, token), new SigV4aSigningOptions("us-east-1", "service") { Time = now, SignContentSha256Header = false });
            var lines = "host:example.amazonaws.com\nx-amz-date:20150830T123600Z\nx-amz-region-set:us-east-1\n" + (token is null ? "" : $"x-amz-security-token:{token}\n");
            Check(result.CanonicalRequest == $"{method}\n/\n\n{lines}\n{signed}\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", $"SigV4a {method} canonical request");
            Check(result.StringToSign == $"AWS4-ECDSA-P256-SHA256\n20150830T123600Z\n20150830/service/aws4_request\n{hash}", $"SigV4a {method} string to sign");
            bool Verifies(string text, string signature) => key.VerifyData(Encoding.UTF8.GetBytes(text), Hex(signature), HashAlgorithmName.SHA256, DSASignatureFormat.Rfc3279DerSequence);
            Check(Verifies(result.StringToSign, published), $"published SigV4a {method} signature verifies with the derived key");
            var authorization = request.Headers.GetValues("Authorization").Single();
            var match = Regex.Match(authorization, "^AWS4-ECDSA-P256-SHA256 Credential=AKIDEXAMPLE/20150830/service/aws4_request, SignedHeaders=" + Regex.Escape(signed) + ", Signature=([0-9a-f]+)$");
            Check(match.Success, $"SigV4a {method} authorization: {authorization}");
            Check(Verifies(result.StringToSign, match.Groups[1].Value) && !Verifies(result.StringToSign + "x", match.Groups[1].Value), $"SigV4a {method} signature");
            Check(request.Headers.GetValues("X-Amz-Region-Set").Single() == "us-east-1" && !request.Headers.Contains("X-Amz-Content-Sha256"), $"SigV4a {method} headers");
        }
        Rejects<ArgumentException>(() => SigV4aSigner.SignAsync(new HttpRequestMessage(HttpMethod.Get, "https://h/"), new AwsCredentials(AccessKey, SecretKey), new SigV4aSigningOptions("", "service")).AsTask(), "SigV4a needs a region set");
        Rejects<ArgumentException>(() => SigV4aSigner.SignAsync(new HttpRequestMessage(HttpMethod.Get, "https://h/"), new AwsCredentials("a", "b", ExpiresAt: DateTimeOffset.UtcNow.AddMinutes(-1)), new SigV4aSigningOptions("r", "s")).AsTask(), "expired SigV4a credentials");
        Rejects<ArgumentException>(() => new VaultClient(new ClientConfig { Endpoint = "https://vault.example", SigV4aSigningRegionSet = ["us-east-1,us-west-2"] }), "region set entries are single regions");
    }

    sealed class Fake(Func<HttpRequestMessage, byte[], HttpResponseMessage> respond) : HttpMessageHandler
    {
        public readonly List<(HttpRequestMessage Request, byte[] Body)> Requests = [];
        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken token)
        {
            var body = request.Content is null ? [] : await request.Content.ReadAsByteArrayAsync(token);
            lock (Requests) Requests.Add((request, body));
            return respond(request, body);
        }
    }
    static HttpResponseMessage Response(int status, byte[] body, params (string Name, string Value)[] headers)
    {
        var response = new HttpResponseMessage((HttpStatusCode)status) { Content = new ByteArrayContent(body) };
        foreach (var (name, value) in headers) if (!response.Headers.TryAddWithoutValidation(name, value)) response.Content.Headers.TryAddWithoutValidation(name, value);
        return response;
    }
    static readonly (string, string)[] JsonHeaders = [("Content-Type", "application/json")];
    static VaultClient Client(Fake fake, string endpoint = "https://vault.example", Action<ClientConfigBuilder>? configure = null)
    {
        var builder = new ClientConfigBuilder { Endpoint = endpoint };
        configure?.Invoke(builder);
        return new VaultClient(new ClientConfig
        {
            Endpoint = builder.Endpoint, HttpClient = new HttpClient(fake), MaxAttempts = 1, Headers = builder.Headers, Region = builder.Region,
            DigestCredentials = builder.Digest ?? (_ => ValueTask.FromResult<BasicCredentials?>(new("u", "p"))), Credentials = builder.Credentials,
            SigV4aSigningRegionSet = builder.RegionSet,
        });
    }
    sealed class ClientConfigBuilder
    {
        public string Endpoint = "";
        public Dictionary<string, string> Headers = new(StringComparer.OrdinalIgnoreCase);
        public Func<CancellationToken, ValueTask<BasicCredentials?>>? Digest;
        public Func<CancellationToken, ValueTask<AwsCredentials?>>? Credentials;
        public string? Region;
        public IReadOnlyList<string>? RegionSet;
    }

    // Digest through an injected handler: challenge, answer, preemptive reuse, stale nonce, and failure.
    static async Task DigestClient()
    {
        var attempts = new List<string>();
        var server = new Fake((request, body) =>
        {
            var authorization = request.Headers.TryGetValues("Authorization", out var values) ? string.Join(", ", values) : null;
            attempts.Add(authorization is null ? "none" : Regex.Match(authorization, "nc=(\\w+)") is { Success: true } m ? m.Groups[1].Value : "other");
            var challenge = $"Digest realm=\"vault\", qop=\"auth\", algorithm=SHA-256, nonce=\"n{attempts.Count}\", opaque=\"o\"" + (attempts.Count == 4 ? ", stale=true" : "");
            if (authorization is null || attempts.Count == 4) return Response(401, [], [.. JsonHeaders, ("WWW-Authenticate", challenge)]);
            return Response(200, body, JsonHeaders);
        });
        var digest = Client(server, "http://127.0.0.1:1", b => b.Digest = _ => ValueTask.FromResult<BasicCredentials?>(new("Mufasa", "Circle of Life")));
        await digest.PutAsync(new Record { Key = "a" }); Check(attempts.SequenceEqual(["none", "00000001"]), "challenge then answer: " + string.Join(",", attempts));
        await digest.PutAsync(new Record { Key = "b" }); Check(attempts.SequenceEqual(["none", "00000001", "00000002"]), "preemptive answer with the next nonce count");
        await digest.PutAsync(new Record { Key = "c" }); Check(attempts.SequenceEqual(["none", "00000001", "00000002", "00000003", "00000001"]), "stale nonce is answered once with the new challenge");
        // A preemptive answer rejected with a fresh, non-stale challenge is a credential failure, not answered again.
        int rejections = 0;
        var fresh = new Fake((request, body) => request.Headers.Contains("Authorization") && ++rejections > 1
            ? Response(401, [], [.. JsonHeaders, ("WWW-Authenticate", "Digest realm=\"vault\", qop=\"auth\", nonce=\"fresh\"")])
            : request.Headers.Contains("Authorization") ? Response(200, body, JsonHeaders)
            : Response(401, [], [.. JsonHeaders, ("WWW-Authenticate", "Digest realm=\"vault\", qop=\"auth\", nonce=\"first\"")]));
        var rejected = Client(fresh, "http://127.0.0.1:1");
        await rejected.PutAsync(new Record { Key = "a" });
        try { await rejected.PutAsync(new Record { Key = "b" }); throw new Exception("expected 401"); }
        catch (ServiceError error) { Check(error.StatusCode == 401 && fresh.Requests.Count == 3, "a rejected preemptive answer is not retried: " + fresh.Requests.Count); }
        var denied = new Fake((_, _) => Response(401, [], [.. JsonHeaders, ("WWW-Authenticate", "Digest realm=\"vault\", qop=\"auth\", nonce=\"x\"")]));
        try { await Client(denied, "http://127.0.0.1:1", b => b.Digest = _ => ValueTask.FromResult<BasicCredentials?>(new("u", "wrong"))).PutAsync(new Record { Key = "k" }); throw new Exception("expected 401"); }
        catch (ServiceError error) { Check(error.StatusCode == 401 && denied.Requests.Count == 2, "a rejected answer is not retried"); }
        var preset = new Fake((_, _) => Response(401, [], [.. JsonHeaders, ("WWW-Authenticate", "Digest realm=\"vault\", nonce=\"x\"")]));
        try
        {
            await Client(preset, "http://127.0.0.1:1", b => { b.Headers["Authorization"] = "Digest preset"; b.Digest = _ => throw new Exception("provider must not run"); }).PutAsync(new Record { Key = "k" });
            throw new Exception("expected 401");
        }
        catch (ServiceError error) { Check(error.StatusCode == 401, "caller-supplied Authorization fails with the 401"); }
        Check(preset.Requests.Count == 1 && preset.Requests[0].Request.Headers.GetValues("Authorization").Single() == "Digest preset", "caller-supplied Authorization is sent unchanged");
        Rejects<ArgumentException>(() => Client(server, "http://vault.example").PutAsync(new Record { Key = "k" }), "digest credentials require HTTPS off loopback");
    }

    // SigV4a through the client: region set from configuration, then from the region.
    static async Task SigV4aClient()
    {
        var signed = new Fake((_, _) => Response(200, "{}"u8.ToArray(), JsonHeaders));
        Func<CancellationToken, ValueTask<AwsCredentials?>> credentials = _ => ValueTask.FromResult<AwsCredentials?>(new(AccessKey, SecretKey));
        await Client(signed, configure: b => { b.Credentials = credentials; b.Region = "us-west-2"; b.RegionSet = ["us-west-2", "eu-*"]; }).SignAsync(new SignInput { Value = "v" });
        await Client(signed, configure: b => { b.Credentials = credentials; b.Region = "us-west-2"; }).SignAsync(new SignInput { Value = "v" });
        Check(signed.Requests[0].Request.Headers.GetValues("X-Amz-Region-Set").Single() == "us-west-2,eu-*"
            && signed.Requests[1].Request.Headers.GetValues("X-Amz-Region-Set").Single() == "us-west-2", "SigV4a region sets");
        var authorization = signed.Requests[0].Request.Headers.GetValues("Authorization").Single();
        Check(authorization.StartsWith("AWS4-ECDSA-P256-SHA256 Credential=AKIDEXAMPLE/", StringComparison.Ordinal)
            && Regex.Match(authorization, "/vault/aws4_request, SignedHeaders=([a-z0-9;-]+),") is { Success: true } signedHeaders
            && new[] { "content-type", "host", "x-amz-content-sha256", "x-amz-date", "x-amz-region-set" }.All(signedHeaders.Groups[1].Value.Split(';').Contains),
            "SigV4a signs the request: " + authorization);
        Rejects<InvalidOperationException>(() => Client(signed, configure: b => b.Credentials = credentials).SignAsync(new SignInput()), "SigV4a needs a region set");
    }

    /// <summary>A loopback JSON server that verifies Digest responses and SigV4a signatures with its own computation.</summary>
    sealed class VaultServer : IAsyncDisposable
    {
        const string User = "Mufasa", Password = "Circle of Life", Realm = "vault@loopback";
        readonly TcpListener listener = new(IPAddress.Loopback, 0);
        readonly CancellationTokenSource stop = new();
        readonly Task loop;
        readonly ECDsa key = ServerKey();
        readonly Dictionary<string, (long Count, int Uses)> nonces = new();
        public readonly BlockingCollection<string> Log = [];
        public string Endpoint { get; }

        public VaultServer()
        {
            listener.Start();
            Endpoint = $"http://127.0.0.1:{((IPEndPoint)listener.LocalEndpoint).Port}/";
            loop = Task.Run(async () =>
            {
                try { while (!stop.IsCancellationRequested) { using var socket = await listener.AcceptTcpClientAsync(stop.Token); await Serve(socket.GetStream()); } }
                catch (Exception) when (stop.IsCancellationRequested) { }
            });
        }

        /// <summary>The SigV4a public key, derived here independently of the client runtime.</summary>
        static ECDsa ServerKey()
        {
            var limit = Hex("ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc63254f");
            for (int counter = 1; ; counter++)
            {
                byte[] input = [0, 0, 0, 1, .. Encoding.ASCII.GetBytes("AWS4-ECDSA-P256-SHA256"), 0, .. Encoding.ASCII.GetBytes(AccessKey), (byte)counter, 0, 0, 1, 0];
                var k0 = HMACSHA256.HashData(Encoding.ASCII.GetBytes("AWS4A" + SecretKey), input);
                if (k0.AsSpan().SequenceCompareTo(limit) > 0) continue;
                var d = new BigInteger(k0, isUnsigned: true, isBigEndian: true) + 1;
                using var full = ECDsa.Create();
                full.ImportParameters(new ECParameters { Curve = ECCurve.NamedCurves.nistP256, D = d.ToByteArray(isUnsigned: true, isBigEndian: true) });
                var verifier = ECDsa.Create();
                verifier.ImportParameters(full.ExportParameters(false));
                return verifier;
            }
        }

        static async Task<string?> Line(Stream stream)
        {
            var bytes = new List<byte>(); var one = new byte[1];
            while (await stream.ReadAsync(one) == 1) { if (one[0] == '\n') return Encoding.ASCII.GetString([.. bytes]).TrimEnd('\r'); bytes.Add(one[0]); }
            return null;
        }
        static byte[] Json(string text) => Encoding.UTF8.GetBytes(text);
        static async Task Respond(Stream stream, int status, IEnumerable<(string, string)> headers, byte[] body)
        {
            var head = new StringBuilder($"HTTP/1.1 {status} Status\r\nConnection: close\r\nContent-Length: {body.Length}\r\n");
            foreach (var (name, value) in headers) head.Append($"{name}: {value}\r\n");
            await stream.WriteAsync(Encoding.ASCII.GetBytes(head.Append("\r\n").ToString()));
            await stream.WriteAsync(body);
        }
        static Dictionary<string, string> Params(string header) =>
            Regex.Matches(header, "([a-z0-9*-]+)=(?:\"((?:[^\"\\\\]|\\\\.)*)\"|([^,\\s]*))", RegexOptions.IgnoreCase)
                .ToDictionary(m => m.Groups[1].Value.ToLowerInvariant(), m => m.Groups[3].Success && m.Groups[3].Length != 0 ? m.Groups[3].Value : Regex.Replace(m.Groups[2].Value, "\\\\(.)", "$1"));
        static string Hash(string algorithm, byte[] data) => Convert.ToHexString(algorithm switch
        {
            "SHA-256" => SHA256.HashData(data),
            "SHA-512-256" => Sha512_256(data),
            _ => MD5.HashData(data),
        }).ToLowerInvariant();

        async Task Serve(Stream stream)
        {
            var requestLine = (await Line(stream))!.Split(' ');
            var (method, target) = (requestLine[0], requestLine[1]);
            var headers = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            for (var line = await Line(stream); !string.IsNullOrEmpty(line); line = await Line(stream)) { var parts = line.Split(':', 2); headers[parts[0].Trim()] = parts[1].Trim(); }
            var body = new byte[headers.TryGetValue("Content-Length", out var length) ? int.Parse(length, CultureInfo.InvariantCulture) : 0];
            await stream.ReadExactlyAsync(body);
            (string, string)[] json = [("Content-Type", "application/json")];
            var name = target.Split('/')[^1];
            var operation = name.Length == 0 ? name : char.ToUpperInvariant(name[0]) + name[1..];
            if (method != "POST" || !target.StartsWith('/')) { Log.Add($"{operation} 400 request"); await Respond(stream, 400, [], []); return; }
            if (operation == "Ping")
            {
                var ok = body.Length == 0 && !headers.ContainsKey("Content-Type");
                Log.Add("Ping " + (ok ? 200 : 400));
                await Respond(stream, ok ? 200 : 400, [], []);
            }
            else if (operation == "Put")
            {
                var framed = headers.GetValueOrDefault("Content-Type") == "application/json" && length is not null && body.Length != 0;
                var mode = headers.GetValueOrDefault("x-digest") ?? "SHA-256";
                var (algorithm, qop) = mode.Contains('/') ? (mode.Split('/')[0], mode.Split('/')[1]) : (mode, "auth");
                var authorization = headers.GetValueOrDefault("Authorization") ?? "";
                var fields = authorization.StartsWith("Digest ", StringComparison.Ordinal) ? Params(authorization[7..]) : new Dictionary<string, string>();
                var nonce = fields.GetValueOrDefault("nonce") ?? "";
                var verdict = "none";
                if (fields.Count != 0)
                {
                    var used = fields.GetValueOrDefault("algorithm") ?? "MD5";
                    var function = Regex.Replace(used, "-sess$", "", RegexOptions.IgnoreCase);
                    string H(string text) => Hash(function, Encoding.UTF8.GetBytes(text));
                    var a1 = H($"{User}:{Realm}:{Password}");
                    if (used.EndsWith("-sess", StringComparison.OrdinalIgnoreCase)) a1 = H($"{a1}:{nonce}:{fields.GetValueOrDefault("cnonce")}");
                    var a2 = fields.GetValueOrDefault("qop") == "auth-int" ? H($"{method}:{target}:{Hash(function, body)}") : H($"{method}:{target}");
                    var expected = H($"{a1}:{nonce}:{fields.GetValueOrDefault("nc")}:{fields.GetValueOrDefault("cnonce")}:{fields.GetValueOrDefault("qop")}:{a2}");
                    var count = Convert.ToInt64(fields.GetValueOrDefault("nc") ?? "0", 16);
                    if (!nonces.TryGetValue(nonce, out var state)) verdict = "unknown-nonce";
                    else if (fields.GetValueOrDefault("username") != User || fields.GetValueOrDefault("realm") != Realm || fields.GetValueOrDefault("uri") != target
                        || used != algorithm || fields.GetValueOrDefault("qop") != qop || fields.GetValueOrDefault("opaque") != "opaque-value") verdict = "fields";
                    else if (!CryptographicOperations.FixedTimeEquals(Encoding.ASCII.GetBytes(expected), Encoding.ASCII.GetBytes(fields.GetValueOrDefault("response") ?? ""))) verdict = "bad";
                    else if (count <= state.Count) verdict = "replay";
                    else if (state.Uses >= 3) verdict = "stale";
                    else { verdict = "ok"; nonces[nonce] = (count, state.Uses + 1); }
                }
                if (verdict != "ok")
                {
                    var fresh = Convert.ToHexString(RandomNumberGenerator.GetBytes(12)).ToLowerInvariant();
                    nonces[fresh] = (0, 0);
                    var challenge = $"Digest realm=\"{Realm}\", qop=\"{qop}\", algorithm={algorithm}, nonce=\"{fresh}\", opaque=\"opaque-value\"" + (verdict == "stale" ? ", stale=true" : "");
                    // Like RFC 7616 3.9.1, a SHA-256 challenge comes with an MD5 alternative the client should not choose.
                    if (algorithm == "SHA-256") challenge += $", Digest realm=\"{Realm}\", qop=\"{qop}\", algorithm=MD5, nonce=\"{fresh}\", opaque=\"opaque-value\"";
                    Log.Add($"Put 401 {verdict}");
                    await Respond(stream, 401, [.. json, ("WWW-Authenticate", challenge)], Json("{}"));
                }
                else if (!framed) { Log.Add("Put 400 framing"); await Respond(stream, 400, [], []); }
                else if (headers.GetValueOrDefault("x-mode") == "deny")
                {
                    Log.Add($"Put 403 {fields["algorithm"]} {fields["qop"]} {fields["nc"]}");
                    await Respond(stream, 403, json, Json("""{"__type":"Denied","message":"denied by policy"}"""));
                }
                else { Log.Add($"Put 200 {fields["algorithm"]} {fields["qop"]} {fields["nc"]}"); await Respond(stream, 200, json, body); }
            }
            else if (operation == "Sign")
            {
                var verdict = "bad-header";
                var m = Regex.Match(headers.GetValueOrDefault("Authorization") ?? "", "^AWS4-ECDSA-P256-SHA256 Credential=([^/]+)/(\\d{8})/vault/aws4_request, SignedHeaders=([a-z0-9;-]+), Signature=([0-9a-f]+)$");
                if (m.Success)
                {
                    var signed = m.Groups[3].Value.Split(';');
                    var lines = string.Concat(signed.Select(name => name + ":" + Regex.Replace(headers.GetValueOrDefault(name) ?? "", "\\s+", " ") + "\n"));
                    var payload = Convert.ToHexString(SHA256.HashData(body)).ToLowerInvariant();
                    var canonical = $"{method}\n{target}\n\n{lines}\n{m.Groups[3].Value}\n{payload}";
                    var text = $"AWS4-ECDSA-P256-SHA256\n{headers.GetValueOrDefault("X-Amz-Date")}\n{m.Groups[2].Value}/vault/aws4_request\n" + Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(canonical))).ToLowerInvariant();
                    string[] required = ["host", "x-amz-date", "x-amz-region-set", "x-amz-content-sha256", "content-type"];
                    if (m.Groups[1].Value != AccessKey) verdict = "access-key";
                    else if (required.Except(signed).Any()) verdict = "unsigned-headers";
                    else if (headers.GetValueOrDefault("X-Amz-Content-Sha256") != payload) verdict = "payload";
                    else if (!key.VerifyData(Encoding.UTF8.GetBytes(text), Hex(m.Groups[4].Value), HashAlgorithmName.SHA256, DSASignatureFormat.Rfc3279DerSequence)) verdict = "signature";
                    else verdict = "verified";
                }
                Log.Add("Sign " + verdict);
                if (verdict == "verified") await Respond(stream, 200, json, Json($$"""{"regionSet":"{{headers["X-Amz-Region-Set"]}}","value":"verified"}"""));
                else await Respond(stream, 403, json, Json($$"""{"__type":"Denied","message":"{{verdict}}"}"""));
            }
            else await Respond(stream, 404, [], []);
        }

        public string[] Take(int count) => Enumerable.Range(0, count).Select(_ => Log.TryTake(out var line, TimeSpan.FromSeconds(10)) ? line : "timeout").ToArray();
        public async ValueTask DisposeAsync() { stop.Cancel(); listener.Stop(); await loop; stop.Dispose(); key.Dispose(); }
    }

    // HTTP Digest and SigV4a over the real HttpClient transport against VaultServer.
    static async Task Loopback()
    {
        await using var server = new VaultServer();
        VaultClient Make(Action<ClientConfigBuilder>? configure = null)
        {
            var builder = new ClientConfigBuilder { Endpoint = server.Endpoint, Digest = _ => ValueTask.FromResult<BasicCredentials?>(new("Mufasa", "Circle of Life")) };
            configure?.Invoke(builder);
            return new VaultClient(new ClientConfig { Endpoint = builder.Endpoint, MaxAttempts = 1, Headers = builder.Headers, Region = builder.Region,
                DigestCredentials = builder.Digest, Credentials = builder.Credentials, SigV4aSigningRegionSet = builder.RegionSet });
        }
        bool Logged(params string[] lines) { var got = server.Take(lines.Length); return got.SequenceEqual(lines) ? true : throw new Exception("server log: " + string.Join(" | ", got)); }

        using (var client = Make()) Check(await client.PingAsync() is not null && Logged("Ping 200"), "an operation without input over HTTP");

        // Digest: challenge, answer, preemptive nonce counts, and a stale nonce after three uses.
        using (var vault = Make())
        {
            var record = new Record
            {
                Key = "k", DataValue = RandomNumberGenerator.GetBytes(300), When = DateTimeOffset.FromUnixTimeSeconds(1700000000).AddMilliseconds(123),
                Count = BigInteger.Parse("123456789012345678901234567890"), Ratio = new BigDecimal("3.14159265358979323846264338327950288"), Small = long.MinValue,
                Score = -0.5, Level = 0.25f, Tags = new() { ["x"] = "y" }, Items = [[], [0]], Choice = new Choice { Text = "t" },
            };
            var output = await vault.PutAsync(record);
            Check(Logged("Put 401 none", "Put 200 SHA-256 auth 00000001"), "challenge, then the SHA-256 answer");
            Check(output.DataValue!.SequenceEqual(record.DataValue) && output.When == record.When && output.Count == record.Count && output.Ratio == record.Ratio
                && output.Small == long.MinValue && output.Score == -0.5 && output.Level == 0.25f && output.Tags!["x"] == "y" && output.Items!.Count == 2
                && output.Items[1].SequenceEqual(new byte[] { 0 }) && output.Choice!.Text == "t", "record round trip over HTTP");
            await vault.PutAsync(new Record { Key = "b" });
            await vault.PutAsync(new Record { Key = "c" });
            Check(Logged("Put 200 SHA-256 auth 00000002", "Put 200 SHA-256 auth 00000003"), "preemptive answers with increasing nonce counts");
            await vault.PutAsync(new Record { Key = "d" });
            Check(Logged("Put 401 stale", "Put 200 SHA-256 auth 00000001"), "a stale nonce is replaced without new credentials");
        }
        foreach (var (mode, expected) in new[] { ("MD5", "MD5 auth"), ("MD5-sess", "MD5-sess auth"), ("SHA-256-sess", "SHA-256-sess auth"), ("SHA-512-256", "SHA-512-256 auth"), ("SHA-256/auth-int", "SHA-256 auth-int") })
        {
            using var client = Make(b => b.Headers["x-digest"] = mode);
            await client.PutAsync(new Record { Key = "m", DataValue = "body"u8.ToArray() });
            Check(Logged("Put 401 none", $"Put 200 {expected} 00000001"), "digest " + mode);
        }
        using (var client = Make(b => b.Digest = _ => ValueTask.FromResult<BasicCredentials?>(new("Mufasa", "wrong"))))
        {
            try { await client.PutAsync(new Record { Key = "k" }); throw new Exception("expected 401"); }
            catch (ServiceError error) { Check(error.StatusCode == 401 && Logged("Put 401 none", "Put 401 bad"), "wrong password fails after one answer"); }
        }
        using (var client = Make(b => b.Headers["x-mode"] = "deny"))
        {
            try { await client.PutAsync(new Record { Key = "k" }); throw new Exception("expected Denied"); }
            catch (Denied denied) { Check(denied.Message == "denied by policy" && denied.StatusCode == 403 && Logged("Put 401 none", "Put 403 SHA-256 auth 00000001"), "modeled error over HTTP"); }
        }

        // SigV4a: the server rebuilds the canonical request and verifies with its own derived key.
        Func<CancellationToken, ValueTask<AwsCredentials?>> credentials = _ => ValueTask.FromResult<AwsCredentials?>(new(AccessKey, SecretKey));
        using (var client = Make(b => { b.Credentials = credentials; b.Region = "us-east-1"; b.RegionSet = ["us-east-1", "us-west-2"]; }))
        {
            var signed = await client.SignAsync(new SignInput { Value = "payload" });
            Check(signed.Value == "verified" && signed.RegionSet == "us-east-1,us-west-2" && Logged("Sign verified"), "SigV4a verified by the server");
        }
        using (var client = Make(b => { b.Credentials = _ => ValueTask.FromResult<AwsCredentials?>(new(AccessKey, SecretKey, "session-token")); b.Region = "eu-west-1"; }))
        {
            var session = await client.SignAsync(new SignInput());
            Check(session.RegionSet == "eu-west-1" && Logged("Sign verified"), "SigV4a with a session token and the client region");
        }
        using (var client = Make(b => { b.Credentials = _ => ValueTask.FromResult<AwsCredentials?>(new(AccessKey, "another-secret")); b.Region = "us-east-1"; }))
        {
            try { await client.SignAsync(new SignInput { Value = "v" }); throw new Exception("expected Denied"); }
            catch (Denied denied) { Check(denied.Message == "signature" && Logged("Sign signature"), "a signature from another key is rejected"); }
        }
    }
}
