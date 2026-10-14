using System.Net;
using System.Text;
using Example.PayloadBindings;
using Example.PayloadBindings.Models;

// A payload's Content-Type is the modelled Content-Type member, then the
// payload's media type, then a client-wide default header.
static class Program
{
    static void Check(bool value, string message) { if (!value) throw new Exception(message); }
    sealed class Handler : HttpMessageHandler
    {
        public string Expected = "";
        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken token)
        {
            var body = Encoding.UTF8.GetString(await request.Content!.ReadAsByteArrayAsync(token));
            Check(body == "PNG", "payload body: " + body);
            var values = new List<string>();
            if (request.Headers.TryGetValues("Content-Type", out var requestValues)) values.AddRange(requestValues);
            if (request.Content.Headers.TryGetValues("Content-Type", out var contentValues)) values.AddRange(contentValues);
            Check(values.Count == 1 && values[0] == Expected, $"Content-Type = [{string.Join(", ", values)}], want {Expected}");
            return new HttpResponseMessage(HttpStatusCode.OK) { Content = new ByteArrayContent([]) };
        }
    }
    static async Task Main()
    {
        foreach (var streaming in new[] { false, true })
        // An empty or malformed modelled value is sent as given, as the
        // other clients send it, never refused by .NET's header parser.
        foreach (var modelled in new string?[] { null, "image/png", "", "not a media type" })
        foreach (var configured in new string?[] { null, "image/jpeg" })
        {
            var handler = new Handler { Expected = modelled ?? "application/octet-stream" };
            using var http = new HttpClient(handler);
            var headers = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            if (configured is not null) headers["Content-Type"] = configured;
            using var client = new PayloadBindingsClient(new() { Endpoint = "https://example.test", HttpClient = http, Headers = headers });
            if (streaming)
            {
                var input = new StreamInput { DataValue = ByteStream.FromBytes(Encoding.UTF8.GetBytes("PNG")) };
                if (modelled is not null) input.ContentType = modelled;
                await client.UploadStreamAsync(input);
            }
            else
            {
                var input = new UploadInput { DataValue = Encoding.UTF8.GetBytes("PNG") };
                if (modelled is not null) input.ContentType = modelled;
                await client.UploadAsync(input);
            }
        }
        Console.WriteLine("C# payload Content-Type precedence passed");
    }
}
