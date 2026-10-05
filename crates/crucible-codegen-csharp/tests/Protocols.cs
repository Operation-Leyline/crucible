using Example.Protocol;
using Example.Protocol.Models;
using System.Net;
using System.Text;
using System.Text.Json.Nodes;

using var handler = new ProtocolHandler();
using var http = new HttpClient(handler);
using var client = new JsonServiceClient(new ClientConfig { Endpoint = "https://example.test", HttpClient = http });
var instant = DateTimeOffset.UnixEpoch.AddMilliseconds(-250);
var output = await client.EchoAsync(new ValuesInput {
    Renamed = "hello", When = instant, Iso = instant, Date = instant, Dates = [instant], Query = instant,
    Nullable = null, Document = null, FloatValue = float.NaN, DoubleValue = double.PositiveInfinity
});
Check(!output.NullablePresence.IsSet && output.DocumentPresence.IsNull, "null handling");
Check(float.IsNaN(output.FloatValue!.Value) && double.IsPositiveInfinity(output.DoubleValue!.Value), "nonfinite values");
Check(output.Choice!.Text == "ok", "union __type");
handler.Mode = "error";
try { await client.EchoAsync(new ValuesInput()); throw new Exception("error not thrown"); }
catch (ProblemFault failure) { Check(failure.ErrorName == "Problem" && failure.RequestId == "request-42" && failure.Message == "bad", "error metadata"); }
handler.Mode = "empty";
await client.EmptyAsync();
Console.WriteLine("C# AWS JSON protocol passed");
static void Check(bool condition, string message) { if (!condition) throw new Exception(message); }

sealed class ProtocolHandler : HttpMessageHandler
{
    public string Mode = "echo";
    protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken token)
    {
        static void Check(bool condition, string message) { if (!condition) throw new Exception(message); }
        var rpc = request.Headers.TryGetValues("X-Amz-Target", out var targets);
        var target = targets?.Single();
        Check(request.Method == HttpMethod.Post, "method");
        var text = request.Content is null ? "" : await request.Content.ReadAsStringAsync(token);
        if (Mode == "empty") {
            Check(!rpc || text == "{}" && target == "JsonService.Empty", "empty RPC");
            return new(HttpStatusCode.OK) { Content = new StringContent("") };
        }
        if (Mode == "error") {
            var response = new HttpResponseMessage(HttpStatusCode.Conflict) { Content = new StringContent("{\"__type\":\"Other\",\"Message\":\"bad\"}") };
            response.Headers.Add("X-Amzn-Errortype", "example.json#Problem:detail");
            response.Headers.Add("X-Amzn-Requestid", "request-42"); return response;
        }
        var body = JsonNode.Parse(text)!.AsObject();
        Check(body["when"]!.GetValue<decimal>() == -0.25m && body["iso"]!.GetValue<string>() == "1969-12-31T23:59:59.75Z", "timestamps");
        Check(!body.ContainsKey("nullable") && body.ContainsKey("document") && body["document"] is null, "wire nulls");
        Check(body["floatValue"]!.GetValue<string>() == "NaN" && body["doubleValue"]!.GetValue<string>() == "Infinity", "wire floats");
        if (rpc) {
            Check(request.RequestUri!.AbsolutePath == "/" && target == "JsonService.Echo" && body["renamed"]!.GetValue<string>() == "hello" && body["date"]!.GetValue<decimal>() == -0.25m, "RPC request");
            Check(request.Content!.Headers.ContentType!.MediaType is "application/x-amz-json-1.0" or "application/x-amz-json-1.1", "content type");
        } else {
            Check(request.RequestUri!.AbsolutePath == "/echo" && body["wire-name"]!.GetValue<string>() == "hello", "REST request");
            Check(request.Headers.GetValues("X-Date").Single() == "Wed, 31 Dec 1969 23:59:59 GMT", "REST timestamp header");
        }
        body["choice"] = new JsonObject { ["text"] = "ok", ["__type"] = "ignored" }; body["nullable"] = null;
        return new(HttpStatusCode.OK) { Content = new StringContent(body.ToJsonString(), Encoding.UTF8, "application/json") };
    }
}
