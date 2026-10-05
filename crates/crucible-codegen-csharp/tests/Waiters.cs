using System.Net;
using Example.Jobs;
using Example.Jobs.Models;

static class Program
{
    static void Check(bool value, string message) { if (!value) throw new Exception(message); }
    sealed class Handler : HttpMessageHandler
    {
        public int Calls;
        public string Mode = "ready";
        public TaskCompletionSource Entered = new(TaskCreationOptions.RunContinuationsAsynchronously);
        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken token)
        {
            Calls++;
            if (Mode == "blocked") { Entered.TrySetResult(); await Task.Delay(Timeout.Infinite,token); }
            var body = Mode switch {
                "ready" => Calls == 1 ? "{\"__type\":\"Missing\"}" : Calls == 2 ? "{\"wire-status\":\"PENDING\"}" : "{\"wire-status\":\"READY\"}",
                "gone" => "{\"__type\":\"Missing\"}",
                "query" => "{\"items\":[{\"wire-status\":\"READY\"},{\"wire-status\":\"READY\"}],\"updatedAt\":\"2026-01-01T00:00:00Z\"}",
                "failed" => "{\"wire-status\":\"FAILED\",\"reasons\":[\"job rejected\"]}",
                _ => "{\"wire-status\":\"PENDING\"}"
            };
            return new(body.Contains("__type",StringComparison.Ordinal) ? HttpStatusCode.NotFound : HttpStatusCode.OK) { Content = new StringContent(body) };
        }
    }
    static async Task Main()
    {
        var handler = new Handler(); using var http = new HttpClient(handler);
        using var client = new JobsClient(new() { Endpoint = "https://example.test", HttpClient = http, MaxAttempts = 1 });
        var options = new WaiterOptions { MinDelay = TimeSpan.Zero, MaxDelay = TimeSpan.Zero };
        var input = new Input { Names = ["a", "b"] }; var timeout = TimeSpan.FromSeconds(3);
        Check((await client.WaitUntilReadyAsync(input,timeout,options))?.Status == "READY" && handler.Calls == 3,"ordered acceptors and error retry");
        handler.Mode = "gone";
        Check(await client.WaitUntilDeletedAsync(input,timeout,options) is null,"error success returns no response");
        handler.Mode = "query";
        await client.WaitUntilAllReadyAsync(input,timeout,options); await client.WaitUntilAnyReadyAsync(input,timeout,options);
        await client.WaitUntilFilteredReadyAsync(input,timeout,options); await client.WaitUntilCountsMatchAsync(input,timeout,options);
        await client.WaitUntilUpdatedAsync(input,timeout,options); await client.WaitUntilFunctionsAsync(input,timeout,options);
        Check(input.Names!.SequenceEqual(new[]{"a","b"}),"caller input preserved");
        handler.Mode = "failed";
        try { await client.WaitUntilReadyAsync(input,timeout,options); throw new Exception("terminal failure accepted"); }
        catch (WaiterException error) { Check(error.Attempts == 1 && error.Message.Contains("job rejected",StringComparison.Ordinal),"failure message and attempts"); }
        handler.Mode = "pending";
        try { await client.WaitUntilReadyAsync(input,TimeSpan.FromMilliseconds(100)); throw new Exception("deadline ignored"); }
        catch (WaiterTimeoutException error) { Check(error.Attempts >= 1,"timeout retains attempts"); }
        handler.Mode = "blocked";
        using var cancelled = new CancellationTokenSource();
        var pending = client.WaitUntilReadyAsync(input,timeout,options,cancelled.Token);
        await handler.Entered.Task.WaitAsync(timeout); int before = handler.Calls; cancelled.Cancel();
        try { await pending; throw new Exception("caller cancellation ignored"); } catch (OperationCanceledException) { }
        Check(handler.Calls == before,"no polling after cancellation");
        Console.WriteLine("C# waiters passed");
    }
}
