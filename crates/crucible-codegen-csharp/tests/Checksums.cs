using System.Net;
using System.Text;
using System.Text.Json.Nodes;
using Example.Checksum;
using Example.Checksum.Models;

var vectors = JsonNode.Parse(File.ReadAllText("checksums.json"))!.AsObject();
var payload = Encoding.UTF8.GetBytes(vectors["inputUtf8"]!.GetValue<string>());
using var handler = new ChecksumHandler(vectors);
using var http = new HttpClient(handler);
var validated = new List<string>();
using var client = new ChecksumsClient(new ClientConfig { Endpoint = "https://example.test", HttpClient = http, ChecksumValidated = (op, algorithm) => validated.Add(op + ":" + algorithm) });
await client.LegacyAsync(new Input { DataValue = payload });
await client.RequiredAsync(new Input { DataValue = payload });
await client.LegacyAsync(new Input { DataValue = [] });
var input = new Input { DataValue = payload };
await client.FlexibleAsync(input);
Check(!input.AlgorithmPresence.IsSet && !input.ValidationPresence.IsSet && handler.Headers["x-validation"] == "ENABLED", "private defaults");
foreach (var (algorithm, _) in vectors["base64"]!.AsObject()) {
    await client.FlexibleAsync(new Input { DataValue = payload, Algorithm = new Algorithm(algorithm) });
    Check(validated[^1] == "Flexible:" + algorithm, "validation callback");
}
handler.Mode = "bad";
try { await client.FlexibleAsync(input); throw new Exception("corruption accepted"); } catch (ChecksumException failure) { Check(failure.Algorithm == "CRC32", "checksum algorithm"); }
handler.Mode = "one"; await client.FlexibleAsync(input);
handler.Mode = "unmodeled"; var count = validated.Count; await client.FlexibleAsync(input); Check(count == validated.Count, "unmodeled checksum validated");
handler.Mode = "override"; await client.FlexibleAsync(new Input { DataValue = payload, Checksum = "supplied" }); Check(handler.Headers["x-amz-checksum-crc32"] == "supplied", "explicit checksum lost");
using var optOut = new ChecksumsClient(new ClientConfig { Endpoint = "https://example.test", HttpClient = http, RequestChecksumCalculation = ChecksumPolicy.WhenRequired, ResponseChecksumValidation = ChecksumPolicy.WhenRequired });
handler.Mode = "skip"; await optOut.FlexibleAsync(input); Check(!handler.Headers.ContainsKey("x-algorithm") && !handler.Headers.ContainsKey("x-validation"), "opt out");
handler.Mode = "normal"; await optOut.FlexibleAsync(new Input { DataValue = payload, Algorithm = new Algorithm("SHA256"), Validation = new Validation("ENABLED") });
try { await client.FlexibleAsync(new Input { Algorithm = new Algorithm("XXHASH64") }); throw new Exception("unsupported algorithm accepted"); } catch (ArgumentException) { }
using var hook = new ChecksumsClient(new ClientConfig { Endpoint = "https://example.test", HttpClient = http, Middleware = [(request, _) => { request.Content?.Dispose(); request.Content = new ByteArrayContent(payload); return ValueTask.CompletedTask; }] });
await hook.LegacyAsync(new Input { DataValue = Encoding.UTF8.GetBytes("changed") });
Console.WriteLine("C# checksums passed");
static void Check(bool value, string message) { if (!value) throw new Exception(message); }

sealed class ChecksumHandler(JsonObject vectors) : HttpMessageHandler
{
    public string Mode = "normal";
    public Dictionary<string,string> Headers = new(StringComparer.OrdinalIgnoreCase);
    protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken token)
    {
        Headers = request.Headers.Concat(request.Content?.Headers ?? Enumerable.Empty<KeyValuePair<string,IEnumerable<string>>>()).ToDictionary(entry => entry.Key,entry => string.Join(",",entry.Value),StringComparer.OrdinalIgnoreCase);
        var data = request.Content is null ? [] : await request.Content.ReadAsByteArrayAsync(token);
        var algorithm = Headers.GetValueOrDefault("x-algorithm");
        if (Mode is not ("override" or "skip")) {
            var expected = data.Length == 0 ? vectors["emptyMd5"]!.GetValue<string>() : vectors["base64"]![algorithm ?? "MD5"]!.GetValue<string>();
            var header = algorithm is null ? "content-md5" : "x-amz-checksum-" + algorithm.ToLowerInvariant();
            if (Headers.GetValueOrDefault(header) != expected) throw new Exception("checksum mismatch: " + header);
        }
        var response = new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent(vectors["inputUtf8"]!.GetValue<string>()) };
        if (Mode != "unmodeled") response.Headers.Add("x-amz-checksum-" + (algorithm ?? "CRC32").ToLowerInvariant(),Mode == "bad" ? "AAAA" : vectors["base64"]![algorithm ?? "CRC32"]!.GetValue<string>());
        response.Headers.Add("x-amz-checksum-xxhash3","unmodeled");
        if (Mode == "one") response.Headers.Add("x-amz-checksum-sha256","AAAA");
        return response;
    }
}
