using System.Collections.Concurrent;
using System.Diagnostics;
using System.Diagnostics.Metrics;

/// <summary>
/// Maps telemetry onto .NET's built-in <see cref="ActivitySource"/> and <see cref="Meter"/>, which OpenTelemetry .NET
/// listens to: one client activity per operation named <c>Service.Operation</c> with <c>rpc.*</c> tags, each attempt as a
/// <c>crucible.attempt</c> activity event, and an error status when the call failed. It also records the
/// <c>crucible.client.operation.duration</c> histogram (seconds) and the <c>crucible.client.attempts</c> counter.
/// Nothing is recorded unless a listener is attached. Both default to the source and meter named <see cref="Name"/>.
/// </summary>
public sealed class ActivityTelemetryHook : TelemetryHook
{
    public const string Name = "Crucible.Client";
    private static readonly ActivitySource DefaultSource = new(Name);
    private static readonly Meter DefaultMeter = new(Name);
    private static readonly Histogram<double> DefaultDuration = Duration(DefaultMeter);
    private static readonly Counter<long> DefaultAttempts = Attempts(DefaultMeter);
    private readonly ActivitySource source;
    private readonly Histogram<double> duration;
    private readonly Counter<long> attempts;
    private readonly ConcurrentDictionary<string, Activity> activities = new(StringComparer.Ordinal);

    public ActivityTelemetryHook() : this(DefaultSource, DefaultDuration, DefaultAttempts) { }
    public ActivityTelemetryHook(ActivitySource source, Meter meter) : this(source, Duration(meter), Attempts(meter)) { }
    private ActivityTelemetryHook(ActivitySource source, Histogram<double> duration, Counter<long> attempts)
    { this.source = source; this.duration = duration; this.attempts = attempts; }
    private static Histogram<double> Duration(Meter meter) =>
        meter.CreateHistogram<double>("crucible.client.operation.duration", "s", "Client operation duration, including retries");
    private static Counter<long> Attempts(Meter meter) =>
        meter.CreateCounter<long>("crucible.client.attempts", "{attempt}", "Attempts that reached the transport");

    public override void OnOperationStarted(OperationStarted e)
    {
        var activity = source.StartActivity(e.Service.Split('#')[^1] + "." + e.Operation, ActivityKind.Client, default(ActivityContext),
            [new("rpc.system", "smithy"), new("rpc.service", e.Service), new("rpc.method", e.Operation)]);
        if (activity is not null) activities[e.InvocationId] = activity;
    }

    public override void OnAttemptStarted(AttemptStarted e)
    {
        if (!activities.TryGetValue(e.InvocationId, out var activity)) return;
        activity.SetTag("server.address", e.Endpoint);
        activity.SetTag("http.request.method", e.Method);
        if (e.Attempt > 1) activity.SetTag("http.request.resend_count", e.Attempt - 1);
    }

    public override void OnAttemptFinished(AttemptFinished e)
    {
        attempts.Add(1, new KeyValuePair<string, object?>("rpc.service", e.Service), new KeyValuePair<string, object?>("rpc.method", e.Operation));
        if (!activities.TryGetValue(e.InvocationId, out var activity)) return;
        var tags = new ActivityTagsCollection { ["crucible.attempt"] = e.Attempt, ["crucible.latency"] = e.Latency.TotalSeconds };
        if (e.Status is int status) tags["http.response.status_code"] = status;
        if (e.Error is not null) tags["error.type"] = e.Error;
        if (e.RetryReason is not null) tags["crucible.retry_reason"] = e.RetryReason;
        if (e.RequestId is not null) tags["crucible.request_id"] = e.RequestId;
        activity.AddEvent(new ActivityEvent("crucible.attempt", tags: tags));
    }

    public override void OnOperationFinished(OperationFinished e)
    {
        var tags = new TagList { { "rpc.service", e.Service }, { "rpc.method", e.Operation } };
        if (e.Error is not null) tags.Add("error.type", e.Error);
        duration.Record(e.Latency.TotalSeconds, tags);
        if (!activities.TryRemove(e.InvocationId, out var activity)) return;
        activity.SetTag("crucible.attempts", e.Attempts);
        if (e.Status is int status) activity.SetTag("http.response.status_code", status);
        if (e.Error is not null)
        {
            activity.SetTag("error.type", e.Error);
            activity.SetStatus(ActivityStatusCode.Error);
        }
        activity.Stop();
    }
}
