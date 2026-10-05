/// <summary>A per-client cache of discovered endpoints, refreshed by one shared discovery per key.</summary>
internal sealed class EndpointCache(Func<double> now)
{
    // How long an optional operation keeps using the configured endpoint after discovery failed.
    internal const double FailureTtl = 60;
    private readonly object gate = new();
    private readonly Dictionary<string, DiscoveredAddress> entries = new(StringComparer.Ordinal);
    private readonly Dictionary<string, Task<DiscoveredAddress>> flights = new(StringComparer.Ordinal);
    private readonly Dictionary<string, double> failures = new(StringComparer.Ordinal);

    internal bool FailedRecently(string key)
    {
        lock (gate)
        {
            if (!failures.TryGetValue(key, out var expires)) return false;
            if (now() < expires) return true;
            failures.Remove(key);
            return false;
        }
    }
    internal void RecordFailure(string key) { lock (gate) failures[key] = now() + FailureTtl; }

    /// <summary>Evicts an entry a response rejected, unless a newer one already replaced it.</summary>
    internal void Invalidate(string key, DiscoveredAddress entry)
    {
        lock (gate) if (entries.TryGetValue(key, out var current) && ReferenceEquals(current, entry)) entries.Remove(key);
    }

    /// <summary>
    /// A valid cached entry, or the one discovery in flight for <paramref name="key"/>, started by the first caller.
    /// The discovery runs as its own task: a caller that stops waiting does not cancel it for the others.
    /// </summary>
    internal Task<DiscoveredAddress> ResolveAsync(string key, Func<Task<DiscoveredAddress>> fetch)
    {
        TaskCompletionSource<DiscoveredAddress> flight;
        lock (gate)
        {
            if (entries.TryGetValue(key, out var entry) && now() < entry.Expires) return Task.FromResult(entry);
            if (flights.TryGetValue(key, out var running)) return running;
            flight = new(TaskCreationOptions.RunContinuationsAsynchronously);
            flights[key] = flight.Task;
        }
        // Every waiter may have stopped waiting; the outcome is still observed.
        _ = flight.Task.ContinueWith(static done => _ = done.Exception, CancellationToken.None,
            TaskContinuationOptions.OnlyOnFaulted | TaskContinuationOptions.ExecuteSynchronously, TaskScheduler.Default);
        _ = Task.Run(async () =>
        {
            DiscoveredAddress? result = null; Exception? error = null;
            try
            {
                result = await fetch().ConfigureAwait(false);
            }
            catch (Exception e) { error = e; }
            lock (gate)
            {
                flights.Remove(key);
                // A zero period serves only the calls that waited for this discovery.
                if (result is not null && now() < result.Expires) entries[key] = result;
                else if (result is not null) entries.Remove(key);
            }
            if (result is not null) flight.SetResult(result);
            else flight.SetException(error!);
        });
        return flight.Task;
    }
}
