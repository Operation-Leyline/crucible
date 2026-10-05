// Published vectors for the CBOR codec (RFC 8949 Appendix A), SHA-512/256 (FIPS 180-4), HTTP Digest (RFC 7616 section 3.9)
// and SigV4a (the AWS CRT aws-signing-test-suite v4a cases); RPC v2 CBOR client behavior through an injected handler; and
// a loopback HTTP server that checks RPC v2 framing, verifies Digest responses, and verifies SigV4a signatures itself.
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

    static bool Same(object? left, object? right) => (left, right) switch
    {
        (null, null) => true,
        (double a, double b) => double.IsNaN(a) ? double.IsNaN(b) : BitConverter.DoubleToInt64Bits(a) == BitConverter.DoubleToInt64Bits(b),
        (long a, long b) => a == b,
        (BigInteger a, BigInteger b) => a == b,
        (byte[] a, byte[] b) => a.AsSpan().SequenceEqual(b),
        (CborTag a, CborTag b) => a.Tag == b.Tag && Same(a.Value, b.Value),
        (CborMap a, CborMap b) => a.Select(pair => pair.Key).SequenceEqual(b.Select(pair => pair.Key)) && a.All(pair => Same(pair.Value, b[pair.Key])),
        (List<object?> a, List<object?> b) => a.Count == b.Count && a.Zip(b).All(pair => Same(pair.First, pair.Second)),
        (string a, string b) => a == b,
        (bool a, bool b) => a == b,
        _ => false,
    };
    static CborMap Map(params (string Key, object? Value)[] entries) { var map = new CborMap(); foreach (var (key, value) in entries) map.Add(key, value); return map; }
    static List<object?> Seq(params object?[] items) => [.. items];

    static async Task Main()
    {
        CborVectors();
        DigestVectors();
        await SigV4aVectors();
        await RpcClient();
        await DigestClient();
        await SigV4aClient();
        await Loopback();
        Console.WriteLine("C# CBOR (RFC 8949), SHA-512/256, Digest (RFC 7616), SigV4a (AWS CRT vectors), RPC v2 CBOR client and loopback checks passed");
    }

    // RFC 8949 Appendix A: every example decodes; those whose preferred encoding is not a half-precision float also
    // re-encode to the same bytes (Smithy avoids half floats).
    static void CborVectors()
    {
        var vectors = new (string Hex, object? Value, bool Encodes)[]
        {
            ("00", 0L, true), ("01", 1L, true), ("0a", 10L, true), ("17", 23L, true), ("1818", 24L, true), ("1819", 25L, true),
            ("1864", 100L, true), ("1903e8", 1000L, true), ("1a000f4240", 1000000L, true), ("1b000000e8d4a51000", 1000000000000L, true),
            ("1bffffffffffffffff", BigInteger.Parse("18446744073709551615"), true),
            ("c249010000000000000000", new CborTag(2, Hex("010000000000000000")), true),
            ("3bffffffffffffffff", BigInteger.Parse("-18446744073709551616"), true),
            ("c349010000000000000000", new CborTag(3, Hex("010000000000000000")), true),
            ("20", -1L, true), ("29", -10L, true), ("3863", -100L, true), ("3903e7", -1000L, true),
            ("f90000", 0.0, false), ("f98000", -0.0, false), ("f93c00", 1.0, false), ("fb3ff199999999999a", 1.1, true),
            ("f93e00", 1.5, false), ("f97bff", 65504.0, false), ("fa47c35000", 100000.0, true), ("fa7f7fffff", 3.4028234663852886e+38, true),
            ("fb7e37e43c8800759c", 1.0e+300, true), ("f90001", 5.960464477539063e-8, false), ("f90400", 0.00006103515625, false),
            ("f9c400", -4.0, false), ("fbc010666666666666", -4.1, true), ("f97c00", double.PositiveInfinity, false), ("f97e00", double.NaN, false),
            ("f9fc00", double.NegativeInfinity, false), ("fa7f800000", double.PositiveInfinity, true), ("fa7fc00000", double.NaN, true),
            ("faff800000", double.NegativeInfinity, true), ("fb7ff0000000000000", double.PositiveInfinity, false), ("fb7ff8000000000000", double.NaN, false),
            ("fbfff0000000000000", double.NegativeInfinity, false),
            ("f4", false, true), ("f5", true, true), ("f6", null, true), ("f7", null, false),
            ("c11a514b67b0", new CborTag(1, 1363896240L), true), ("c1fb41d452d9ec200000", new CborTag(1, 1363896240.5), true),
            ("d74401020304", new CborTag(23, Hex("01020304")), true),
            ("40", Array.Empty<byte>(), true), ("4401020304", Hex("01020304"), true),
            ("60", "", true), ("6161", "a", true), ("6449455446", "IETF", true), ("62225c", "\"\\", true), ("62c3bc", "ü", true),
            ("63e6b0b4", "水", true), ("64f0908591", "\U00010151", true),
            ("80", Seq(), true), ("83010203", Seq(1L, 2L, 3L), true), ("8301820203820405", Seq(1L, Seq(2L, 3L), Seq(4L, 5L)), true),
            ("98190102030405060708090a0b0c0d0e0f101112131415161718181819", Enumerable.Range(1, 25).Select(i => (object?)(long)i).ToList(), true),
            ("a0", Map(), true), ("a201020304", Map(("1", 2L), ("3", 4L)), false),
            ("a26161016162820203", Map(("a", 1L), ("b", Seq(2L, 3L))), true), ("826161a161626163", Seq("a", Map(("b", "c"))), true),
            ("a56161614161626142616361436164614461656145", Map(("a", "A"), ("b", "B"), ("c", "C"), ("d", "D"), ("e", "E")), true),
            ("5f42010243030405ff", Hex("0102030405"), false), ("7f657374726561646d696e67ff", "streaming", false),
            ("9fff", Seq(), false), ("9f018202039f0405ffff", Seq(1L, Seq(2L, 3L), Seq(4L, 5L)), false),
            ("9f01820203820405ff", Seq(1L, Seq(2L, 3L), Seq(4L, 5L)), false), ("83018202039f0405ff", Seq(1L, Seq(2L, 3L), Seq(4L, 5L)), false),
            ("83019f0203ff820405", Seq(1L, Seq(2L, 3L), Seq(4L, 5L)), false),
            ("9f0102030405060708090a0b0c0d0e0f101112131415161718181819ff", Enumerable.Range(1, 25).Select(i => (object?)(long)i).ToList(), false),
            ("bf61610161629f0203ffff", Map(("a", 1L), ("b", Seq(2L, 3L))), false), ("826161bf61626163ff", Seq("a", Map(("b", "c"))), false),
            ("bf6346756ef563416d7421ff", Map(("Fun", true), ("Amt", -2L)), false),
        };
        foreach (var (hex, value, encodes) in vectors)
        {
            Check(Same(Cbor.Decode(Hex(hex)), value), "CBOR decode " + hex);
            if (encodes) Check(Convert.ToHexString(Cbor.Encode(value)).ToLowerInvariant() == hex, $"CBOR encode {hex}: {Convert.ToHexString(Cbor.Encode(value))}");
        }
        Check(Convert.ToHexString(Cbor.Encode(1.5)) == "FA3FC00000" && Convert.ToHexString(Cbor.Encode(-0.0)) == "FA80000000", "floats use single precision when exact, never half");
        Check(Convert.ToHexString(Cbor.Encode(long.MinValue)) == "3B7FFFFFFFFFFFFFFF" && Same(Cbor.Decode(Hex("3b7fffffffffffffff")), long.MinValue), "minimum 64-bit integer");
        foreach (var (hex, why) in new[] { ("", "empty"), ("18", "truncated argument"), ("1c", "reserved information"), ("5f6161ff", "text chunk in bytes"),
            ("7f4161ff", "byte chunk in text"), ("62c328", "invalid UTF-8"), ("a101", "map value missing"), ("a1f601", "null key"), ("a2616101616102", "duplicate key"),
            ("0001", "trailing bytes"), ("ff", "lone break"), ("9f01", "unterminated array"), ("81ff", "break in definite array"), ("f818", "simple value"),
            ("5b0000000100000000", "oversized length"), ("1f", "indefinite integer"), ("df", "indefinite tag") })
            Rejects<FormatException>(() => Cbor.Decode(Hex(hex)), "malformed CBOR: " + why);
        Rejects<FormatException>(() => Cbor.Decode([.. Enumerable.Repeat((byte)0x81, 300), 0x00]), "CBOR nesting limit");
        Rejects<FormatException>(() => Cbor.Encode("\ud800"), "CBOR text must be valid Unicode");
        // FIPS 180-4 SHA-512/256 examples, which RFC 7616's SHA-512-256 relies on and .NET does not provide.
        Check(Convert.ToHexString(Sha512_256("abc"u8.ToArray())) == "53048E2681941EF99B2E29B76B4C7DABE4C2D0C634FC6D46E0E2F13107E7AF23", "SHA-512/256 one block");
        Check(Convert.ToHexString(Sha512_256(Encoding.ASCII.GetBytes("abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu")))
            == "3928E184FB8690F840DA3988121D31BE65CB9D3EF83EE6146FEAC861E19B563A", "SHA-512/256 two blocks");
    }

    // RFC 7616 section 3.9.1 (MD5 and SHA-256) and 3.9.2 (SHA-512-256 with userhash).
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
    static readonly (string, string)[] CborHeaders = [("Smithy-Protocol", "rpc-v2-cbor"), ("Content-Type", "application/cbor")];
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

    // RPC v2 CBOR through the generated client: wire shape, big numbers, errors, and malformed responses.
    static async Task RpcClient()
    {
        var echo = new Fake((_, body) => Response(200, body, CborHeaders));
        var record = new Record
        {
            Key = "k", DataValue = [0x00, 0xff], When = DateTimeOffset.UnixEpoch.AddSeconds(-2).AddMilliseconds(250), Count = BigInteger.Parse("-18446744073709551617"),
            Ratio = new BigDecimal("-1.0000000000000000000000001"), Small = -9, Score = double.NaN, Level = 1.5f, Tags = new() { ["a"] = "b" },
            Items = [[0x01], []], Choice = new Choice { Bytes = [0x02] },
        };
        var output = await Client(echo, "https://vault.example/prefix").PutAsync(record);
        var (sent, sentBody) = echo.Requests[0];
        Check(sent.RequestUri!.AbsoluteUri == "https://vault.example/prefix/service/Vault/operation/Put" && sent.Method == HttpMethod.Post, "RPC v2 URL keeps the endpoint prefix");
        Check(sent.Headers.GetValues("Smithy-Protocol").Single() == "rpc-v2-cbor" && sent.Content!.Headers.ContentType!.MediaType == "application/cbor"
            && sent.Headers.Accept.Single().MediaType == "application/cbor" && !sent.Headers.Contains("X-Amz-Target"), "RPC v2 request headers");
        var wire = (CborMap)Cbor.Decode(sentBody)!;
        Check(Same(wire["when"], new CborTag(1, -1.75)) && Same(wire["data"], new byte[] { 0x00, 0xff }), "timestamps are tag 1 epochs and blobs byte strings");
        Check(Same(wire["count"], new CborTag(3, Hex("010000000000000000"))), "bigInteger beyond 64 bits is a tag 3 bignum");
        Check(Same(wire["ratio"], new CborTag(4, Seq(-25L, new CborTag(3, BigInteger.Pow(10, 25).ToByteArray(isUnsigned: true, isBigEndian: true))))), "bigDecimal is a tag 4 decimal fraction");
        Check(wire["score"] is double score && double.IsNaN(score) && Same(wire["level"], 1.5) && Same(wire["choice"], Map(("bytes", new byte[] { 0x02 }))), "floats and unions");
        Check(output.When == record.When && output.Count == record.Count && output.Ratio == record.Ratio && double.IsNaN(output.Score!.Value) && output.Level == 1.5f
            && output.DataValue!.SequenceEqual(record.DataValue) && output.Items!.Count == 2 && output.Items[0].SequenceEqual(new byte[] { 1 }) && output.Items[1].Length == 0
            && output.Choice!.Bytes!.SequenceEqual(new byte[] { 2 }) && output.Small == -9 && output.Tags!["a"] == "b", "CBOR round trip");
        var whole = new Fake((_, body) => Response(200, body, CborHeaders));
        await Client(whole).PutAsync(new Record { Key = "k", When = DateTimeOffset.UnixEpoch.AddSeconds(5), Count = 7, Ratio = new BigDecimal("2.50") });
        var integral = (CborMap)Cbor.Decode(whole.Requests[0].Body)!;
        Check(Same(integral["when"], new CborTag(1, 5L)) && Same(integral["count"], 7L) && Same(integral["ratio"], new CborTag(4, Seq(-1L, 25L))), "whole epochs and small numbers stay integers");
        // An indefinite map with half floats, a null member, an unknown variant beside __type, a bignum, and a decimal fraction.
        var handmade = new List<byte> { 0xbf };
        void Add(byte[] bytes) => handmade.AddRange(bytes);
        Add(Cbor.Encode("key")); Add(Cbor.Encode("k"));
        Add(Cbor.Encode("choice")); handmade.Add(0xa2); Add(Cbor.Encode("__type")); Add(Cbor.Encode("x")); Add(Cbor.Encode("otherVariant")); Add([0x41, 0x02]);
        Add(Cbor.Encode("tags")); handmade.Add(0xf6);
        Add(Cbor.Encode("score")); Add([0xf9, 0x7e, 0x00]); Add(Cbor.Encode("level")); Add([0xf9, 0x3e, 0x00]);
        Add(Cbor.Encode("count")); Add([0xc2, 0x49, 0x01, 0, 0, 0, 0, 0, 0, 0, 0]);
        Add(Cbor.Encode("ratio")); Add([0xc4, 0x82, 0x21, 0x19, 0x03, 0xe8]); handmade.Add(0xff);
        var decoded = await Client(new Fake((_, _) => Response(200, [.. handmade], CborHeaders))).PutAsync(new Record { Key = "k" });
        Check(decoded.Choice!.UnknownVariant is { Name: "otherVariant" } variant && variant.Value!.GetValue<string>() == "Ag==", "unknown union variant keeps its value");
        Check(!decoded.TagsPresence.IsSet && double.IsNaN(decoded.Score!.Value) && decoded.Level == 1.5f && decoded.Count == BigInteger.Pow(2, 64)
            && decoded.Ratio == new BigDecimal("10.00"), "half floats, null members, bignums and decimal fractions decode");
        VaultClient Error(int status, byte[] body, params (string, string)[] headers) => Client(new Fake((_, _) => Response(status, body, headers)));
        try { await Error(403, Cbor.Encode(Map(("__type", "example.vault#Denied"), ("message", "no"), ("reason", "policy"))), CborHeaders).PutAsync(new Record { Key = "k" }); throw new Exception("expected Denied"); }
        catch (Denied denied) { Check(denied.ErrorName == "Denied" && denied.Message == "no" && denied.Reason == "policy" && denied.StatusCode == 403, "modeled CBOR error"); }
        try { await Error(403, Cbor.Encode(Map(("code", "Denied"))), [.. CborHeaders, ("X-Amzn-ErrorType", "Denied")]).PutAsync(new Record { Key = "k" }); throw new Exception("expected an unknown error"); }
        catch (ServiceError unknown) { Check(unknown is not Denied && unknown.ErrorName == "UnknownError" && unknown.StatusCode == 403, "X-Amzn-ErrorType and code never identify RPC v2 errors"); }
        try { await Error(403, Cbor.Encode(Map(("__type", "example.vault#Denied"))), ("Content-Type", "application/cbor")).PutAsync(new Record { Key = "k" }); throw new Exception("expected an unknown error"); }
        catch (ServiceError unknown) { Check(unknown is not Denied && unknown.ErrorName == "UnknownError" && unknown.StatusCode == 403, "a response without Smithy-Protocol is malformed"); }
        try { await Error(500, [0xff, 0x00], CborHeaders).PutAsync(new Record { Key = "k" }); throw new Exception("expected an unknown error"); }
        catch (ServiceError unknown) { Check(unknown is not Denied && unknown.StatusCode == 500, "a malformed CBOR error body keeps the status"); }
        Rejects<FormatException>(() => Error(200, [0xa0], ("Content-Type", "application/cbor")).PutAsync(new Record { Key = "k" }), "a success without Smithy-Protocol is malformed");
        Rejects<FormatException>(() => Error(200, Cbor.Encode(Map(("key", "k"), ("when", 5L))), CborHeaders).PutAsync(new Record { Key = "k" }), "timestamps require tag 1");
        Rejects<FormatException>(() => Error(200, Cbor.Encode(Map(("key", "k"), ("data", "text"))), CborHeaders).PutAsync(new Record { Key = "k" }), "blobs require byte strings");
        var ping = new Fake((_, _) => Response(200, [], ("Smithy-Protocol", "rpc-v2-cbor")));
        Check(await Client(ping).PingAsync() is not null, "Unit output");
        Check(ping.Requests[0].Body.Length == 0 && ping.Requests[0].Request.Content?.Headers.ContentType is null
            && ping.Requests[0].Request.Headers.Accept.Single().MediaType == "application/cbor", "Unit input sends no body or content type");
        // RPC v2 JSON documents are any JSON value, but CBOR protocol models cannot use documents.
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
            if (authorization is null || attempts.Count == 4) return Response(401, [], [.. CborHeaders, ("WWW-Authenticate", challenge)]);
            return Response(200, body, CborHeaders);
        });
        var digest = Client(server, "http://127.0.0.1:1", b => b.Digest = _ => ValueTask.FromResult<BasicCredentials?>(new("Mufasa", "Circle of Life")));
        await digest.PutAsync(new Record { Key = "a" }); Check(attempts.SequenceEqual(["none", "00000001"]), "challenge then answer: " + string.Join(",", attempts));
        await digest.PutAsync(new Record { Key = "b" }); Check(attempts.SequenceEqual(["none", "00000001", "00000002"]), "preemptive answer with the next nonce count");
        await digest.PutAsync(new Record { Key = "c" }); Check(attempts.SequenceEqual(["none", "00000001", "00000002", "00000003", "00000001"]), "stale nonce is answered once with the new challenge");
        // A preemptive answer rejected with a fresh, non-stale challenge is a credential failure, not answered again.
        int rejections = 0;
        var fresh = new Fake((request, body) => request.Headers.Contains("Authorization") && ++rejections > 1
            ? Response(401, [], [.. CborHeaders, ("WWW-Authenticate", "Digest realm=\"vault\", qop=\"auth\", nonce=\"fresh\"")])
            : request.Headers.Contains("Authorization") ? Response(200, body, CborHeaders)
            : Response(401, [], [.. CborHeaders, ("WWW-Authenticate", "Digest realm=\"vault\", qop=\"auth\", nonce=\"first\"")]));
        var rejected = Client(fresh, "http://127.0.0.1:1");
        await rejected.PutAsync(new Record { Key = "a" });
        try { await rejected.PutAsync(new Record { Key = "b" }); throw new Exception("expected 401"); }
        catch (ServiceError error) { Check(error.StatusCode == 401 && fresh.Requests.Count == 3, "a rejected preemptive answer is not retried: " + fresh.Requests.Count); }
        var denied = new Fake((_, _) => Response(401, [], [.. CborHeaders, ("WWW-Authenticate", "Digest realm=\"vault\", qop=\"auth\", nonce=\"x\"")]));
        try { await Client(denied, "http://127.0.0.1:1", b => b.Digest = _ => ValueTask.FromResult<BasicCredentials?>(new("u", "wrong"))).PutAsync(new Record { Key = "k" }); throw new Exception("expected 401"); }
        catch (ServiceError error) { Check(error.StatusCode == 401 && denied.Requests.Count == 2, "a rejected answer is not retried"); }
        var preset = new Fake((_, _) => Response(401, [], [.. CborHeaders, ("WWW-Authenticate", "Digest realm=\"vault\", nonce=\"x\"")]));
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
        var signed = new Fake((_, _) => Response(200, [0xa0], CborHeaders));
        Func<CancellationToken, ValueTask<AwsCredentials?>> credentials = _ => ValueTask.FromResult<AwsCredentials?>(new(AccessKey, SecretKey));
        await Client(signed, configure: b => { b.Credentials = credentials; b.Region = "us-west-2"; b.RegionSet = ["us-west-2", "eu-*"]; }).SignAsync(new SignInput { Value = "v" });
        await Client(signed, configure: b => { b.Credentials = credentials; b.Region = "us-west-2"; }).SignAsync(new SignInput { Value = "v" });
        Check(signed.Requests[0].Request.Headers.GetValues("X-Amz-Region-Set").Single() == "us-west-2,eu-*"
            && signed.Requests[1].Request.Headers.GetValues("X-Amz-Region-Set").Single() == "us-west-2", "SigV4a region sets");
        var authorization = signed.Requests[0].Request.Headers.GetValues("Authorization").Single();
        Check(authorization.StartsWith("AWS4-ECDSA-P256-SHA256 Credential=AKIDEXAMPLE/", StringComparison.Ordinal)
            && authorization.Contains("/vault/aws4_request, SignedHeaders=accept;content-type;host;smithy-protocol;x-amz-content-sha256;x-amz-date;x-amz-region-set,", StringComparison.Ordinal),
            "SigV4a signs the RPC v2 request: " + authorization);
        Rejects<InvalidOperationException>(() => Client(signed, configure: b => b.Credentials = credentials).SignAsync(new SignInput()), "SigV4a needs a region set");
    }

    /// <summary>A loopback RPC v2 CBOR server that verifies Digest responses and SigV4a signatures with its own computation.</summary>
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
        static byte[] Text(string value) => Cbor.Encode(value);
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
            (string, string)[] cbor = [("Smithy-Protocol", "rpc-v2-cbor"), ("Content-Type", "application/cbor")];
            var operation = target.Split('/')[^1];
            var protocol = headers.GetValueOrDefault("Smithy-Protocol") == "rpc-v2-cbor" && headers.GetValueOrDefault("Accept") == "application/cbor" && method == "POST"
                && target.StartsWith("/service/Vault/operation/", StringComparison.Ordinal) && !headers.ContainsKey("X-Amz-Target");
            if (!protocol) { Log.Add($"{operation} 400 protocol"); await Respond(stream, 400, [], []); return; }
            if (operation == "Ping")
            {
                var ok = body.Length == 0 && !headers.ContainsKey("Content-Type");
                Log.Add("Ping " + (ok ? 200 : 400));
                await Respond(stream, ok ? 200 : 400, headers.GetValueOrDefault("x-mode") == "malformed" ? [] : [("Smithy-Protocol", "rpc-v2-cbor")], []);
            }
            else if (operation == "Put")
            {
                var framed = headers.GetValueOrDefault("Content-Type") == "application/cbor" && length is not null && body.Length != 0;
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
                    await Respond(stream, 401, [.. cbor, ("WWW-Authenticate", challenge)], [0xa0]);
                }
                else if (!framed) { Log.Add("Put 400 framing"); await Respond(stream, 400, [], []); }
                else if (headers.GetValueOrDefault("x-mode") == "deny")
                {
                    Log.Add($"Put 403 {fields["algorithm"]} {fields["qop"]} {fields["nc"]}");
                    await Respond(stream, 403, cbor, [0xa2, .. Text("__type"), .. Text("example.vault#Denied"), .. Text("message"), .. Text("denied by policy")]);
                }
                else { Log.Add($"Put 200 {fields["algorithm"]} {fields["qop"]} {fields["nc"]}"); await Respond(stream, 200, cbor, body); }
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
                    string[] required = ["host", "x-amz-date", "x-amz-region-set", "x-amz-content-sha256", "content-type", "smithy-protocol"];
                    if (m.Groups[1].Value != AccessKey) verdict = "access-key";
                    else if (required.Except(signed).Any()) verdict = "unsigned-headers";
                    else if (headers.GetValueOrDefault("X-Amz-Content-Sha256") != payload) verdict = "payload";
                    else if (!key.VerifyData(Encoding.UTF8.GetBytes(text), Hex(m.Groups[4].Value), HashAlgorithmName.SHA256, DSASignatureFormat.Rfc3279DerSequence)) verdict = "signature";
                    else verdict = "verified";
                }
                Log.Add("Sign " + verdict);
                if (verdict == "verified") await Respond(stream, 200, cbor, [0xa2, .. Text("regionSet"), .. Text(headers["X-Amz-Region-Set"]), .. Text("value"), .. Text("verified")]);
                else await Respond(stream, 403, cbor, [0xa2, .. Text("__type"), .. Text("example.vault#Denied"), .. Text("message"), .. Text(verdict)]);
            }
            else await Respond(stream, 404, [], []);
        }

        public string[] Take(int count) => Enumerable.Range(0, count).Select(_ => Log.TryTake(out var line, TimeSpan.FromSeconds(10)) ? line : "timeout").ToArray();
        public async ValueTask DisposeAsync() { stop.Cancel(); listener.Stop(); await loop; stop.Dispose(); key.Dispose(); }
    }

    // RPC v2 CBOR, HTTP Digest, and SigV4a over the real HttpClient transport against VaultServer.
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

        using (var client = Make()) Check(await client.PingAsync() is not null && Logged("Ping 200"), "Unit RPC over HTTP");
        using (var client = Make(b => b.Headers["x-mode"] = "malformed"))
            Rejects<FormatException>(() => client.PingAsync(), "a response without Smithy-Protocol is rejected");
        Check(Logged("Ping 200"), "malformed ping reached the server");

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
                && output.Items[1].SequenceEqual(new byte[] { 0 }) && output.Choice!.Text == "t", "CBOR record round trip over HTTP");
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
            catch (Denied denied) { Check(denied.Message == "denied by policy" && denied.StatusCode == 403 && Logged("Put 401 none", "Put 403 SHA-256 auth 00000001"), "modeled CBOR error over HTTP"); }
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
