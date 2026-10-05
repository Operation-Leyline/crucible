using System.Net;
using Example.Behavior;
using Example.Behavior.Models;

static class Program
{
    static void Check(bool value, string message) { if (!value) throw new Exception(message); }
    sealed class Handler : HttpMessageHandler
    {
        public int Calls; public string Host = "";
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken token)
        {
            Calls++; Host = request.RequestUri!.Host;
            Check(request.Headers.GetValues("X-Region").Single() == "local","endpoint header");
            Check(request.Headers.GetValues("Authorization").Single().Contains("/local/example/aws4_request",StringComparison.Ordinal),"endpoint signing properties");
            Check(request.Headers.GetValues("X-Amz-Content-Sha256").Single() == "UNSIGNED-PAYLOAD","unsigned payload binding");
            return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent("{\"wire-status\":\"READY\"}") });
        }
    }
    static async Task Main()
    {
        var handler = new Handler(); using var http = new HttpClient(handler);
        using var client = new ServiceClient(new() { HttpClient = http, Region = "local", Credentials = _ => ValueTask.FromResult<AwsCredentials?>(new("key","secret")) });
        Check((await client.ReadAsync(new() { Account = "customer" })).Status == "READY","operation response");
        Check(handler.Host == "customer.local.example.test","host prefix");
        foreach (var label in new[] { "a.b", "-bad", "x/y", "x@evil", "", new string('x',64) })
        {
            try { await client.ReadAsync(new() { Account = label }); throw new Exception("bad host label accepted"); }
            catch (ArgumentException) { }
        }
        Check(handler.Calls == 1,"invalid labels do not dispatch");
        using var disabled = new ServiceClient(new() { HttpClient = http, Region = "local", DisableHostPrefix = true, Credentials = _ => ValueTask.FromResult<AwsCredentials?>(new("key","secret")) });
        await disabled.ReadAsync(new() { Account = "ignored" }); Check(handler.Host == "local.example.test","host prefix disabled");
        using var wrong = new ServiceClient(new() { HttpClient = http, Region = "missing" });
        try { await wrong.ReadAsync(new() { Account = "customer" }); throw new Exception("rule error ignored"); }
        catch (ArgumentException error) { Check(error.Message.Contains("Unknown region missing",StringComparison.Ordinal),"modeled endpoint error"); }
        var type = typeof(ServiceClient).Assembly.GetType("Example.Behavior.Endpoints")!;
        var parse = type.GetMethod("ParseUrl",System.Reflection.BindingFlags.NonPublic|System.Reflection.BindingFlags.Static)!;
        foreach (var url in new[] { "file:///path", "https://user:pass@example.test", "https://example.test?q=x", "https://example.test#x", "https://example.test:99999", "https://evil\n.example" })
            Check(parse.Invoke(null,[url]) is null,"invalid endpoint URL rejected");
        Console.WriteLine("C# endpoint rules passed");
    }
}
