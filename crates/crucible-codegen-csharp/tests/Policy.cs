// Runtime policy: retry quota, adaptive rate limiting and telemetry, against the shared vectors in
// tests/conformance/{retry-quota,adaptive-retry,telemetry}.json and a client generated from
// crates/crucible-ir/tests/fixtures/runtime-policy.json.
using System.Diagnostics;
using System.Diagnostics.Metrics;
using System.Net;
using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Example.Policy;
using Example.Policy.Models;

static class Program
{
    const string Endpoint = "https://records.example";
    const BindingFlags Hidden = BindingFlags.NonPublic | BindingFlags.Instance;
    static void Check(bool ok, string message) { if (!ok) throw new Exception(message); }
    static JsonNode Load(string name) => JsonNode.Parse(File.ReadAllText(name))!;
    static string Str(JsonNode? node) => node!.GetValue<string>();

    /// <summary>A clock that only moves when told to, or when something sleeps on it.</summary>
    sealed class FakeClock : TimeProvider
    {
        long ticks;
        public readonly List<double> Sleeps = [];
        public override long TimestampFrequency => TimeSpan.TicksPerSecond;
        public override long GetTimestamp() => Interlocked.Read(ref ticks);
        public override DateTimeOffset GetUtcNow() => DateTimeOffset.UnixEpoch.AddTicks(GetTimestamp());
        public void Advance(double seconds) => Interlocked.Add(ref ticks, (long)Math.Round(seconds * TimeSpan.TicksPerSecond));
        public override ITimer CreateTimer(TimerCallback callback, object? state, TimeSpan dueTime, TimeSpan period)
        {
            lock (Sleeps) Sleeps.Add(dueTime.TotalSeconds);
            Interlocked.Add(ref ticks, dueTime.Ticks);
            ThreadPool.QueueUserWorkItem(_ => callback(state));
            return new Fired();
        }
        sealed class Fired : ITimer
        {
            public bool Change(TimeSpan dueTime, TimeSpan period) => true;
            public void Dispose() { }
            public ValueTask DisposeAsync() => default;
        }
    }

    sealed class Handler(Func<HttpRequestMessage, CancellationToken, Task<HttpResponseMessage>> handle) : HttpMessageHandler
    {
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken token) => handle(request, token);
    }

    static Task<HttpResponseMessage> Reply(JsonNode spec)
    {
        if (spec["transport"] is JsonNode transport)
        {
            Check(Str(transport) == "timeout", "transport failure kind");
            // What HttpClient raises when its own timeout ends an exchange.
            return Task.FromException<HttpResponseMessage>(new TaskCanceledException("timed out", new TimeoutException()));
        }
        var response = new HttpResponseMessage((HttpStatusCode)spec["status"]!.GetValue<int>())
        { Content = new StringContent((spec["json"] ?? new JsonObject()).ToJsonString(), Encoding.UTF8, "application/json") };
        foreach (var (key, value) in spec["headers"]?.AsObject() ?? new JsonObject()) response.Headers.TryAddWithoutValidation(key, Str(value));
        return Task.FromResult(response);
    }

    static RecordsClient Client(Func<HttpRequestMessage, CancellationToken, Task<HttpResponseMessage>> handle, TimeProvider? clock = null,
        RetryMode mode = RetryMode.Legacy, int maxAttempts = 3, TelemetryHook? telemetry = null, string key = "key",
        TimeSpan? baseDelay = null, TimeSpan? maxDelay = null) =>
        new(new ClientConfig
        {
            Endpoint = Endpoint, HttpClient = new HttpClient(new Handler(handle)), Token = _ => ValueTask.FromResult<string?>(key),
            BaseDelay = baseDelay ?? TimeSpan.Zero, MaxDelay = maxDelay ?? TimeSpan.FromSeconds(20), RetryMode = mode,
            MaxAttempts = maxAttempts, Telemetry = telemetry, TimeProvider = clock ?? TimeProvider.System
        });

    static object Strategy(RecordsClient client)
    {
        var runtime = typeof(RecordsClient).GetField("runtime", Hidden)!.GetValue(client)!;
        return runtime.GetType().GetField("retry", Hidden)!.GetValue(runtime)!;
    }
    static RetryQuota? QuotaOf(RecordsClient client) { var s = Strategy(client); return (RetryQuota?)s.GetType().GetProperty("Quota", Hidden)!.GetValue(s); }
    static AdaptiveRateLimiter? LimiterOf(RecordsClient client) { var s = Strategy(client); return (AdaptiveRateLimiter?)s.GetType().GetProperty("Limiter", Hidden)!.GetValue(s); }

    static Task Call(RecordsClient client, string name, JsonNode? input, CancellationToken token = default)
    {
        switch (name)
        {
            case "ListRecords": return client.ListRecordsAsync(token);
            case "GetRecord": return client.GetRecordAsync(new GetRecordInput { Table = Str(input!["table"]), Key = Str(input["key"]) }, token);
            case "PutRecord":
                // Only members the vector names are set: an assigned null would be an explicit null.
                var put = new PutRecordInput { Table = Str(input!["table"]) };
                if (input["secret"] is JsonNode secret) put.Secret = Str(secret);
                if (input["ticket"] is JsonNode ticket) put.Ticket = Str(ticket);
                if (input["label"] is JsonNode label) put.Label = Str(label);
                if (input["value"] is JsonNode value) put.Value = Str(value);
                return client.PutRecordAsync(put, token);
            default: throw new ArgumentException("unknown call " + name);
        }
    }
    static async Task<string> Outcome(Task call)
    {
        try { await call; return "success"; }
        catch (ServiceError error) { return error.ErrorName; }
    }

    static async Task Main(string[] args)
    {
        var directory = args.Length != 0 ? args[0] : ".";
        var quota = Load(Path.Combine(directory, "retry-quota.json"));
        var adaptive = Load(Path.Combine(directory, "adaptive-retry.json"));
        var telemetry = Load(Path.Combine(directory, "telemetry.json"));
        QuotaSteps(quota, adaptive);
        await QuotaCalls(quota);
        AdaptiveVectors(adaptive);
        await AdaptiveClient();
        await TelemetryScenarios(telemetry);
        await TelemetryHookFailuresAreIgnored();
        await TelemetryUsesTheInjectedClock();
        await CancellationFinishesTheOperation();
        await ActivityAdapter();
        Console.WriteLine("C# runtime policy passed");
    }

    static void QuotaSteps(JsonNode vectors, JsonNode adaptive)
    {
        var constants = vectors["constants"]!;
        Check(RetryQuota.InitialTokens == constants["initialRetryTokens"]!.GetValue<int>() && RetryQuota.RetryCost == constants["retryCost"]!.GetValue<int>()
            && RetryQuota.TimeoutRetryCost == constants["retryTimeoutCost"]!.GetValue<int>() && RetryQuota.NoRetryIncrement == constants["noRetryIncrement"]!.GetValue<int>(), "quota constants");
        var rates = adaptive["constants"]!;
        Check(AdaptiveRateLimiter.MinFillRate == rates["minFillRate"]!.GetValue<double>() && AdaptiveRateLimiter.MinCapacity == rates["minCapacity"]!.GetValue<double>()
            && AdaptiveRateLimiter.Smoothing == rates["smoothing"]!.GetValue<double>() && AdaptiveRateLimiter.TimeBucketRange == rates["timeBucketRange"]!.GetValue<double>()
            && CubicCalculator.Beta == rates["beta"]!.GetValue<double>() && CubicCalculator.ScaleConstant == rates["scaleConstant"]!.GetValue<double>()
            && AdaptiveRateLimiter.MaxRateAdjustScale == rates["maxRateAdjustScale"]!.GetValue<double>(), "adaptive constants");
        foreach (var test in vectors["quota"]!.AsArray())
        {
            var bucket = test!["initial"] is JsonNode initial ? new RetryQuota(initial.GetValue<int>()) : new RetryQuota();
            foreach (var step in test["steps"]!.AsArray())
            {
                if (step!["acquire"] is JsonNode acquire)
                    Check(bucket.TryAcquire(Str(acquire) == "timeout") is not null == step["granted"]!.GetValue<bool>(), test["name"] + " granted");
                else bucket.Release(step["release"]?.GetValue<int>());
                Check(bucket.Available == step["available"]!.GetValue<int>(), test["name"] + " available " + step.ToJsonString());
            }
        }
    }

    static readonly Dictionary<string, (int Status, string Json)> Tokens = new()
    {
        ["ok"] = (200, "{}"), ["unavailable"] = (503, """{"__type":"Unavailable"}"""),
        ["throttled"] = (429, """{"__type":"Throttled"}"""), ["denied"] = (400, """{"__type":"Denied"}"""),
    };

    static async Task QuotaCalls(JsonNode vectors)
    {
        foreach (var test in vectors["calls"]!.AsArray())
        {
            var script = new Queue<string>();
            var mode = Enum.Parse<RetryMode>(Str(test!["mode"]), ignoreCase: true);
            using var client = Client((_, _) => {
                var token = script.Dequeue();
                if (token == "timeout") return Reply(JsonNode.Parse("""{"transport":"timeout"}""")!);
                var (status, json) = Tokens[token];
                return Reply(new JsonObject { ["status"] = status, ["json"] = JsonNode.Parse(json) });
            }, new FakeClock(), mode);
            foreach (var step in test["steps"]!.AsArray())
            {
                var responses = step!["responses"]!.AsArray().Select(Str).ToList();
                for (int i = 0; i < (step["repeat"]?.GetValue<int>() ?? 1); i++)
                {
                    script.Clear(); foreach (var token in responses) script.Enqueue(token);
                    bool success;
                    try { await client.ListRecordsAsync(); success = true; }
                    catch (Exception e) when (e is ServiceError or NetworkError) { success = false; }
                    Check(responses.Count - script.Count == step["attempts"]!.GetValue<int>(), test["name"] + " attempts " + step.ToJsonString());
                    Check(success == step["success"]!.GetValue<bool>(), test["name"] + " success");
                }
                var available = QuotaOf(client)?.Available;
                Check(available == step["available"]?.GetValue<int>(), test["name"] + " quota " + available + " " + step.ToJsonString());
            }
        }
    }

    static void Near(double actual, JsonNode? expected, string message) =>
        Check(Math.Abs(actual - expected!.GetValue<double>()) <= 1e-6, message + ": " + actual + " != " + expected);

    static void AdaptiveVectors(JsonNode vectors)
    {
        var success = vectors["cubicSuccess"]!;
        var cubic = new CubicCalculator(success["lastMaxRate"]!.GetValue<double>(), success["lastThrottleTime"]!.GetValue<double>());
        foreach (var step in success["expect"]!.AsArray()) Near(cubic.Success(step!["time"]!.GetValue<double>()), step["rate"], "cubic success");
        var throttle = vectors["cubicThrottle"]!;
        cubic = new CubicCalculator(throttle["lastMaxRate"]!.GetValue<double>(), throttle["lastThrottleTime"]!.GetValue<double>());
        double last = 0;
        foreach (var step in throttle["steps"]!.AsArray())
        {
            var time = step!["time"]!.GetValue<double>();
            var rate = step["throttled"]!.GetValue<bool>() ? cubic.Throttle(last, time) : cubic.Success(time);
            Near(rate, step["rate"], "cubic throttle at " + time);
            last = rate;
        }
        var limiter = new AdaptiveRateLimiter(0);
        foreach (var step in vectors["rateUpdates"]!.AsArray())
        {
            limiter.AfterAttempt(step!["throttled"]!.GetValue<bool>(), step["time"]!.GetValue<double>());
            Near(limiter.MeasuredRate, step["measuredRate"], "measured rate at " + step["time"]);
            Near(limiter.FillRate, step["fillRate"], "fill rate at " + step["time"]);
            Check(limiter.Enabled == step["enabled"]!.GetValue<bool>(), "enabled at " + step["time"]);
        }
        limiter = new AdaptiveRateLimiter(0);
        foreach (var step in vectors["sendDelays"]!.AsArray())
        {
            var time = step!["time"]!.GetValue<double>();
            if (step["send"]?.GetValue<bool>() == true) Near(limiter.BeforeSend(time), step["delay"], "send delay at " + time);
            else
            {
                limiter.AfterAttempt(step["throttled"]!.GetValue<bool>(), time);
                Near(limiter.FillRate, step["fillRate"], "send fill rate at " + time);
            }
        }
        // A send cancelled while waiting returns its token: the next caller waits one token less.
        limiter = new AdaptiveRateLimiter(0);
        limiter.AfterAttempt(true, 0.5);
        var first = limiter.BeforeSend(0.5); limiter.ReturnToken();
        Check(Math.Abs(limiter.BeforeSend(0.5) - first) < 1e-9, "returned send token");
    }

    static async Task AdaptiveClient()
    {
        var clock = new FakeClock();
        var statuses = new Queue<int>([200, 429, 200]);
        using (var client = Client((_, _) => {
            clock.Advance(0.01); // each response arrives a little later
            var status = statuses.Dequeue();
            return Reply(new JsonObject { ["status"] = status, ["json"] = status == 429 ? new JsonObject { ["__type"] = "Throttled" } : new JsonObject() });
        }, clock, RetryMode.Adaptive))
        {
            await client.ListRecordsAsync();
            Check(clock.Sleeps.Count == 0, "no send delay before the first throttle");
            await client.ListRecordsAsync();
            // The retry after the throttle waited for a send token; the backoff itself is zero.
            Check(clock.Sleeps.Count == 1 && clock.Sleeps[0] > 0, "adaptive send delay: " + string.Join(",", clock.Sleeps));
            Check(LimiterOf(client)!.Enabled, "limiter enabled after throttling");
        }
        foreach (var mode in new[] { RetryMode.Legacy, RetryMode.Standard })
        {
            clock = new FakeClock();
            using var client = Client((_, _) => Reply(JsonNode.Parse("""{"status":429,"json":{"__type":"Throttled"}}""")!), clock, mode);
            Check(await Outcome(client.ListRecordsAsync()) == "Throttled", mode + " throttled");
            Check(LimiterOf(client) is null && clock.Sleeps.Count == 0, mode + " never rate limits");
        }
        try { using var invalid = new RecordsClient(new() { Endpoint = Endpoint, RetryMode = (RetryMode)7 }); throw new Exception("invalid retry mode accepted"); }
        catch (ArgumentException) { }
        // Backoff and Retry-After sleep on the injected clock.
        clock = new FakeClock();
        var replies = new Queue<string>(["""{"status":503,"json":{"__type":"Unavailable"},"headers":{"Retry-After":"2"}}""",
            """{"status":503,"json":{"__type":"Unavailable"}}""", """{"status":200,"json":{}}"""]);
        using (var client = Client((_, _) => Reply(JsonNode.Parse(replies.Dequeue())!), clock, RetryMode.Standard,
            baseDelay: TimeSpan.FromSeconds(0.5), maxDelay: TimeSpan.FromSeconds(0.75)))
        {
            var strategy = Strategy(client);
            strategy.GetType().GetField("Jitter", Hidden)!.SetValue(strategy, (Func<double>)(() => 1.0));
            await client.ListRecordsAsync();
        }
        Check(clock.Sleeps.SequenceEqual([2.0, 0.75]), "backoff sleeps: " + string.Join(",", clock.Sleeps));
        Check(clock.GetTimestamp() == TimeSpan.FromSeconds(2.75).Ticks, "sleeps advanced the clock");
    }

    sealed class Recorder : TelemetryHook
    {
        public readonly List<object> Events = [];
        public override void OnOperationStarted(OperationStarted e) { lock (Events) Events.Add(e); }
        public override void OnAttemptStarted(AttemptStarted e) { lock (Events) Events.Add(e); }
        public override void OnAttemptFinished(AttemptFinished e) { lock (Events) Events.Add(e); }
        public override void OnOperationFinished(OperationFinished e) { lock (Events) Events.Add(e); }
    }

    /// <summary>An event as the vectors spell it: the event name and every field, camelCased.</summary>
    static JsonObject Record(object e)
    {
        var name = e.GetType().Name;
        var record = new JsonObject { ["event"] = char.ToLowerInvariant(name[0]) + name[1..] };
        foreach (var property in e.GetType().GetProperties())
        {
            if (property.Name == "EqualityContract") continue;
            var value = property.GetValue(e);
            record[char.ToLowerInvariant(property.Name[0]) + property.Name[1..]] = value is null ? null : JsonSerializer.SerializeToNode(value, value.GetType());
        }
        return record;
    }

    static bool Matches(JsonNode? expected, JsonNode? actual)
    {
        if (expected is JsonValue star && star.TryGetValue<string>(out var text) && text == "*") return actual is not null;
        if (expected is JsonObject fields)
            return actual is JsonObject values && fields.All(field => values.ContainsKey(field.Key) && Matches(field.Value, values[field.Key]));
        if (expected is null || actual is null) return expected is null && actual is null;
        return JsonNode.DeepEquals(expected, actual);
    }

    static void CheckEvents(JsonNode test, List<object> events)
    {
        var name = Str(test["name"]);
        var records = events.Select(Record).ToList();
        var dump = string.Join("\n", records.Select(record => record.ToJsonString()));
        var expected = test["events"]!.AsArray();
        Check(records.Count == expected.Count, name + ": event count\n" + dump);
        for (int i = 0; i < records.Count; i++) Check(Matches(expected[i], records[i]), name + ": event " + i + " " + expected[i]!.ToJsonString() + "\n" + dump);
        foreach (var secret in test["secrets"]!.AsArray()) Check(!dump.Contains(Str(secret), StringComparison.Ordinal), name + ": leaked " + secret);
        Check(records.Where(record => Str(record["operation"]) == Str(test["call"])).Select(record => Str(record["invocationId"])).Distinct().Count() == 1,
            name + ": one invocation id per call");
        var times = events.Select(e => (DateTimeOffset)e.GetType().GetProperty("Time")!.GetValue(e)!).ToList();
        Check(times.Zip(times.Skip(1)).All(pair => pair.First <= pair.Second), name + ": times do not go backwards");
    }

    static async Task TelemetryScenarios(JsonNode vectors)
    {
        foreach (var test in vectors["scenarios"]!.AsArray())
        {
            var recorder = new Recorder();
            var responses = new Queue<JsonNode>(test!["responses"]!.AsArray().Select(response => response!));
            using (var client = Client((_, _) => Reply(responses.Dequeue()), new FakeClock(), telemetry: recorder, key: "api-key-secret"))
            {
                try { await Call(client, Str(test["call"]), test["input"]); } catch (Exception) { }
            }
            CheckEvents(test, recorder.Events);
        }
    }

    sealed class Broken : TelemetryHook
    {
        public override void OnAttemptStarted(AttemptStarted e) => throw new InvalidOperationException("hook bug");
    }
    sealed class Captured : TraceListener
    {
        public readonly StringBuilder Text = new();
        public override void Write(string? message) { lock (Text) Text.Append(message); }
        public override void WriteLine(string? message) { lock (Text) Text.AppendLine(message); }
    }

    static async Task TelemetryHookFailuresAreIgnored()
    {
        var captured = new Captured();
        Trace.Listeners.Add(captured);
        try
        {
            using var client = Client((_, _) => Reply(JsonNode.Parse("""{"status":200,"json":{"count":2}}""")!), telemetry: new Broken());
            Check((await client.ListRecordsAsync()).Count == 2, "a failing hook never changes the outcome");
            Check(client.ListRecords().Count == 2, "nor a blocking call's outcome");
        }
        finally { Trace.Listeners.Remove(captured); }
        Check(captured.Text.ToString().Contains("telemetry hook failed", StringComparison.Ordinal), "hook failure traced");
    }

    static async Task TelemetryUsesTheInjectedClock()
    {
        var clock = new FakeClock(); var recorder = new Recorder();
        using (var client = Client((_, _) => { clock.Advance(0.25); return Reply(JsonNode.Parse("""{"status":200,"json":{"count":7}}""")!); }, clock, telemetry: recorder))
            await client.ListRecordsAsync();
        var finished = recorder.Events.OfType<AttemptFinished>().Single();
        Check(finished.Latency == TimeSpan.FromSeconds(0.25) && finished.ResponseBytes == """{"count":7}""".Length, "attempt latency and bytes");
        Check(((OperationFinished)recorder.Events[^1]).Latency == TimeSpan.FromSeconds(0.25), "operation latency");
        Check(((OperationStarted)recorder.Events[0]).Time == DateTimeOffset.UnixEpoch, "event times read the injected clock");
    }

    static async Task CancellationFinishesTheOperation()
    {
        var recorder = new Recorder(); var started = new TaskCompletionSource();
        using var client = Client(async (_, token) => { started.SetResult(); await Task.Delay(Timeout.Infinite, token); throw new InvalidOperationException(); }, telemetry: recorder);
        using var cancel = new CancellationTokenSource();
        var call = client.ListRecordsAsync(cancel.Token);
        await started.Task.WaitAsync(TimeSpan.FromSeconds(5));
        cancel.Cancel();
        try { await call; throw new Exception("not cancelled"); } catch (OperationCanceledException) { }
        Check(recorder.Events.Select(e => e.GetType()).SequenceEqual([typeof(OperationStarted), typeof(AttemptStarted), typeof(AttemptFinished), typeof(OperationFinished)]),
            "cancelled call events");
        Check(((OperationFinished)recorder.Events[^1]).Error!.EndsWith("CanceledException", StringComparison.Ordinal), "cancellation error type");
    }

    static async Task ActivityAdapter()
    {
        using var source = new ActivitySource("Test.Policy");
        using var meter = new Meter("Test.Policy");
        var activities = new List<Activity>();
        using var listener = new ActivityListener
        {
            ShouldListenTo = candidate => candidate.Name == "Test.Policy",
            Sample = (ref ActivityCreationOptions<ActivityContext> _) => ActivitySamplingResult.AllDataAndRecorded,
            ActivityStopped = activity => { lock (activities) activities.Add(activity); },
        };
        ActivitySource.AddActivityListener(listener);
        var durations = new List<double>(); long attempts = 0;
        using var meters = new MeterListener();
        meters.InstrumentPublished = (instrument, l) => { if (instrument.Meter == meter) l.EnableMeasurementEvents(instrument); };
        meters.SetMeasurementEventCallback<double>((_, value, _, _) => { lock (durations) durations.Add(value); });
        meters.SetMeasurementEventCallback<long>((_, value, _, _) => Interlocked.Add(ref attempts, value));
        meters.Start();
        var replies = new Queue<string>(["""{"status":503,"json":{"__type":"Unavailable"}}""", """{"status":400,"json":{"__type":"Denied"}}"""]);
        using (var client = Client((_, _) => Reply(JsonNode.Parse(replies.Dequeue())!), new FakeClock(), telemetry: new ActivityTelemetryHook(source, meter), key: "secret"))
            Check(await Outcome(client.ListRecordsAsync()) == "Denied", "adapter call outcome");
        var span = activities.Single();
        Check(span.DisplayName == "Records.ListRecords" && span.Kind == ActivityKind.Client, "span name and kind");
        Check((string?)span.GetTagItem("rpc.system") == "crucible" && (string?)span.GetTagItem("rpc.service") == "example.policy#Records"
            && (string?)span.GetTagItem("rpc.method") == "ListRecords", "rpc tags");
        Check((string?)span.GetTagItem("error.type") == "Denied" && span.Status == ActivityStatusCode.Error, "error status");
        Check(Equals(span.GetTagItem("http.request.resend_count"), 1) && Equals(span.GetTagItem("crucible.attempts"), 2)
            && Equals(span.GetTagItem("http.response.status_code"), 400) && (string?)span.GetTagItem("server.address") == Endpoint, "attempt tags");
        Check(span.Events.Select(e => e.Name).SequenceEqual(["crucible.attempt", "crucible.attempt"]), "attempt events");
        Check(span.Events.Select(e => e.Tags.FirstOrDefault(tag => tag.Key == "crucible.retry_reason").Value as string).SequenceEqual(["transient", null]), "retry reasons");
        var everything = string.Join(",", span.Tags.Select(tag => tag.Value) .Concat(span.Events.SelectMany(e => e.Tags.Select(tag => tag.Value?.ToString()))));
        Check(!everything.Contains("secret", StringComparison.Ordinal), "adapter never sees credentials");
        meters.RecordObservableInstruments();
        Check(durations.Count == 1 && Interlocked.Read(ref attempts) == 2, "duration and attempt metrics");
    }
}
