using System.Net;
using System.Text;
using Example.Wire;
using Example.Wire.Models;

static class Program
{
    static void Check(bool value, string message) { if (!value) throw new Exception(message); }
    sealed class Handler : HttpMessageHandler
    {
        public bool Fail;
        public string? Invalid;
        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken token)
        {
            if (Fail) {
                var error = new HttpResponseMessage((HttpStatusCode)422) { Content = new StringContent("invalid text") };
                error.Headers.Add("X-Crucible-Error", "TextError"); return error;
            }
            var bytes = await request.Content!.ReadAsByteArrayAsync(token);
            if (request.RequestUri!.AbsolutePath == "/document") Check(Encoding.UTF8.GetString(bytes) == "{\"name\":\"nested\"}", "whole document payload");
            if (request.RequestUri.AbsolutePath == "/text" && bytes.Length != 0) {
                Check(Encoding.UTF8.GetString(bytes) == "héllo 🌍", "raw UTF-8 payload");
                Check(request.Headers.GetValues("X-Meta").Single() == "eyJrZXkiOiJow6lsbG8ifQ==, YSxi, \"\"", "encoded media list");
            }
            var response = new HttpResponseMessage(HttpStatusCode.OK) { Content = new ByteArrayContent(bytes) };
            if (Invalid is not null) response.Headers.TryAddWithoutValidation("X-Meta", Invalid);
            else if (request.Headers.TryGetValues("X-Meta", out var meta)) response.Headers.TryAddWithoutValidation("X-Meta", meta);
            if (request.Headers.TryGetValues("X-Extra-Note",out var extra)) response.Headers.TryAddWithoutValidation("X-Extra-Note",extra);
            return response;
        }
    }
    static async Task Main()
    {
        var handler = new Handler(); using var http = new HttpClient(handler);
        using var client = new WireClient(new() { Endpoint = "https://example.test", HttpClient = http });
        var output = await client.TextAsync(new() { Text = "héllo 🌍", Meta = ["{\"key\":\"héllo\"}", "a,b", ""], Extras = new() { ["note"] = "héllo" } });
        Check(output.Text == "héllo 🌍" && output.Meta!.SequenceEqual(new[] { "{\"key\":\"héllo\"}", "a,b", "" }), "text and media round trip");
        Check(output.Extras!.Values.Single() == "héllo","encoded prefix media round trip");
        var record = await client.DocumentAsync(new() { Document = new() { Name = "nested" } });
        Check(record.Document!.Name == "nested", "document response");
        Check((await client.TextAsync(new() { Text = "" })).Text == "", "empty text");
        foreach (var header in new[] { "%%%", "/w==" }) {
            handler.Invalid = header;
            try { await client.TextAsync(new() { Text = "" }); throw new Exception("invalid media header accepted"); }
            catch (Exception error) when (error is FormatException or DecoderFallbackException) { }
        }
        handler.Invalid = null; handler.Fail = true;
        try { await client.TextAsync(new() { Text = "" }); throw new Exception("error payload accepted"); }
        catch (TextError error) { Check(error.Details == "invalid text" && error.StatusCode == 422, "modeled error payload"); }
        Console.WriteLine("C# payloads passed");
    }
}
