using System.Net;
using System.Text;
using System.Text.Json;
using Example.Dynamic;
using Example.Dynamic.Models;

static class Program
{
    static void Check(bool value, string message) { if (!value) throw new Exception(message); }
    sealed class Handler : HttpMessageHandler
    {
        public int Calls;
        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken token)
        {
            Calls++;
            if (Calls == 2)
            {
                var error = new HttpResponseMessage((HttpStatusCode)422) { Content = new StringContent("{\"__type\":\"Problem\",\"message\":\"bad\"}") };
                error.Headers.Add("X-Meta-Reason", "invalid");
                return error;
            }
            Check(request.RequestUri!.AbsolutePath == "/files/a/b%20c/%25", "greedy path");
            Check(request.RequestUri.Query == "?q%20%2B=x%2Fy&q%20%2B=", "query map");
            Check(request.Headers.GetValues("X-Meta-Color").Single() == "green", "prefix header");
            using var body = JsonDocument.Parse(await request.Content!.ReadAsStringAsync(token));
            Check(body.RootElement.GetProperty("wire-value").GetString() == "request", "jsonName");
            Check(body.RootElement.EnumerateObject().Count() == 1, "bindings leaked into body");
            var response = new HttpResponseMessage(HttpStatusCode.Created) { Content = new StringContent("{\"wire-value\":\"result\"}", Encoding.UTF8, "application/json") };
            response.Headers.Add("X-Meta-Color", "blue");
            return response;
        }
    }
    static async Task Main()
    {
        var handler = new Handler();
        using var http = new HttpClient(handler);
        using var client = new DynamicClient(new ClientConfig { Endpoint = "https://example.test", HttpClient = http });
        var input = new Input { Key = "a/b c/%", Query = new() { ["q +"] = new() { "x/y", "" } },
            Metadata = new() { ["Color"] = "green" }, Value = "request" };
        var output = await client.GetAsync(input);
        Check(output.Status == 201 && output.Value == "result" && output.Metadata!["Color"] == "blue", "response bindings");
        try { await client.GetAsync(input); throw new Exception("modeled error not raised"); }
        catch (Problem error) { Check(error.Status == 422 && error.StatusCode == 422 && error.Details!["Reason"] == "invalid", "error bindings"); }
        foreach (var path in new[] { "a/../b", "./b", "a/.", ".." })
        {
            input.Key = path;
            try { await client.GetAsync(input); throw new Exception("dot path accepted"); }
            catch (ArgumentException) { }
        }
        Check(handler.Calls == 2, "invalid label reached transport");
        Console.WriteLine("C# dynamic bindings passed");
    }
}
