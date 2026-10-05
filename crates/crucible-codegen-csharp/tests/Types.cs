using System.Net;
using System.Numerics;
using System.Text.Json;
using Example.Types;
using Example.Types.Models;

static class Program
{
    static void Check(bool value, string message) { if (!value) throw new Exception(message); }
    sealed class Handler : HttpMessageHandler
    {
        public string Body = """{"mode":42,"names":["a","b"],"sparseNames":["a",null],"sparseMap":{"known":-1,"future":42,"absent":null}}""";
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken token) =>
            Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent(Body) });
    }
    static async Task Main()
    {
        var handler = new Handler();
        using var http = new HttpClient(handler);
        using var client = new TypesClient(new() { Endpoint = "https://example.test", HttpClient = http });
        var output = await client.ReadAsync();
        Check(output.Integer.ToString() == "12345678901234567890123456789012345678901234567890", "integer default precision");
        Check(output.Decimal.ToString() == "12345678901234567890.12345678901234567890123456789", "decimal default precision");
        Check(output.Mode == new Mode(42), "open integer enum");
        Check(output.SparseNames![0] == "a" && output.SparseNames[1] is null, "sparse list");
        Check(output.SparseMap!["known"] == Mode.OFF && output.SparseMap["future"] == new Mode(42) && output.SparseMap["absent"] is null, "sparse map");
        Check(new BigDecimal("1.000e2") == new BigDecimal("100"), "decimal numeric equality");
        Check(new BigDecimal("-0.0") == default(BigDecimal), "decimal zero equality");
        Check(new BigDecimal("10e99999999999999999999999999") == new BigDecimal("1e100000000000000000000000000"), "unbounded decimal exponent");
        foreach (var body in new[] { """{"mode":2147483648}""", """{"names":["a","a"]}""", """{"integer":1.5}""", """{"decimal":"1.5"}""", """{"integers":[123456789012345678901234567890,123456789012345678901234567890]}""", """{"decimals":[1,1.0]}""" })
        {
            handler.Body = body;
            try { await client.ReadAsync(); throw new Exception("invalid model accepted: " + body); }
            catch (Exception error) when (error is JsonException or FormatException or OverflowException) { }
        }
        for (int i = 1; i < 120; i++) {
            var text = new string('9', i) + "." + new string('0', i) + "1";
            handler.Body = "{\"decimal\":" + text + "}";
            Check((await client.ReadAsync()).Decimal.ToString() == text, "decimal round trip");
        }
        foreach (var text in new[] { "", "NaN", "+1", "01", "1/2", "null", "[1]" }) {
            try { _ = new BigDecimal(text); throw new Exception("invalid decimal accepted"); }
            catch (FormatException) { }
        }
        Console.WriteLine("C# extended types passed");
    }
}
