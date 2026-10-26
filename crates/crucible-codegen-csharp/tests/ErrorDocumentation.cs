using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Text.Json.Nodes;
using Example.ErrorDocs;

// A separate NuGet consumer using actual HttpClient and loopback TCP responses.
static class Program
{
    static async Task Main(string[] args)
    {
        var cases = JsonNode.Parse(File.ReadAllText(args[0]))!["cases"]!.AsArray();
        using var stop = new CancellationTokenSource();
        var listener = new TcpListener(IPAddress.Loopback, 0);
        listener.Start();
        var serving = Task.Run(async () =>
        {
            try
            {
                foreach (var item in cases)
                {
                    var test = item!;
                    using var socket = await listener.AcceptTcpClientAsync(stop.Token);
                    await using var stream = socket.GetStream();
                    var request = new StringBuilder();
                    using var reader = new StreamReader(stream, Encoding.ASCII, false, 1024, leaveOpen: true);
                    string? line;
                    while (!string.IsNullOrEmpty(line = await reader.ReadLineAsync(stop.Token)))
                    {
                        request.AppendLine(line);
                        if (request.Length > 65536) throw new Exception("request header bound");
                    }
                    if (!request.ToString().StartsWith("GET /inspect HTTP/1.1")) throw new Exception("native GET request");
                    var body = Encoding.UTF8.GetBytes(test["body"]!.GetValue<string>());
                    var headers = $"HTTP/1.1 {test["status"]!.GetValue<int>()} Fixture\r\nConnection: close\r\nContent-Length: {body.Length}\r\nx-request-id: documentation-consumer\r\n";
                    if (test["identity"] is JsonNode identity) headers += $"x-crucible-error: {identity.GetValue<string>()}\r\n";
                    if (test["header"] is JsonNode reason) headers += $"X-Error-Reason: {reason.GetValue<string>()}\r\n";
                    await stream.WriteAsync(Encoding.ASCII.GetBytes(headers + "\r\n"), stop.Token);
                    await stream.WriteAsync(body, stop.Token);
                }
            }
            catch (OperationCanceledException) when (stop.IsCancellationRequested) { }
        });
        try
        {
            using var client = new TroubleshootingClient(new ClientConfig
            {
                Endpoint = "http://127.0.0.1:" + ((IPEndPoint)listener.LocalEndpoint).Port,
                MaxAttempts = 1,
                Timeout = TimeSpan.FromSeconds(5),
            });
            foreach (var test in cases.Select(item => item!))
            {
                ServiceError actual;
                try { await client.InspectAsync(); throw new Exception("expected modeled response failure"); }
                catch (ServiceError error) { actual = error; }
                var expected = test["docUrl"]?.GetValue<string>();
                var typed = actual.GetType() != typeof(TroubleshootingException);
                if (actual.DocUrl != expected || typed != test["typed"]!.GetValue<bool>()
                    || actual.StatusCode != test["status"]!.GetValue<int>() || actual.RequestId != "documentation-consumer")
                    throw new Exception($"{test["name"]}: URL={actual.DocUrl}, type={actual.GetType()}, status={actual.StatusCode}");
            }
            await serving;
        }
        finally
        {
            stop.Cancel();
            listener.Stop();
            await serving;
        }
        Console.WriteLine($"C# error documentation: {cases.Count} native HTTP cases passed");
    }
}
