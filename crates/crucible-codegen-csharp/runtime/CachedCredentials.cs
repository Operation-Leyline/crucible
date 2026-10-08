/// <summary>Caches the result of a credential provider and refreshes it shortly before it expires. Concurrent callers share one refresh, and a cancelled caller never starts a second one.</summary>
public sealed class CachedCredentials
{
    private readonly Func<CancellationToken, ValueTask<AwsCredentials?>> provider;
    private readonly TimeSpan refreshBefore;
    private readonly object gate = new();
    private Task<AwsCredentials?>? pending;
    private AwsCredentials? value;
    private DateTimeOffset? refreshAt;
    /// <summary>Wraps a provider.</summary>
    /// <param name="provider">The underlying source.</param>
    /// <param name="refreshBefore">How long before expiry to fetch again; one minute when null.</param>
    public CachedCredentials(Func<CancellationToken, ValueTask<AwsCredentials?>> provider, TimeSpan? refreshBefore = null)
    {
        ArgumentNullException.ThrowIfNull(provider);
        this.provider = provider; this.refreshBefore = refreshBefore ?? TimeSpan.FromMinutes(1);
        if (this.refreshBefore < TimeSpan.Zero) throw new ArgumentOutOfRangeException(nameof(refreshBefore));
    }
    /// <summary>Returns cached credentials, refreshing them first when close to expiry.</summary>
    /// <param name="token">Cancels the wait.</param>
    /// <returns>The credentials, or null when the provider has none.</returns>
    public async ValueTask<AwsCredentials?> RetrieveAsync(CancellationToken token)
    {
        token.ThrowIfCancellationRequested();
        Task<AwsCredentials?> refresh;
        lock (gate)
        {
            if (value is not null && (refreshAt is null || refreshAt > DateTimeOffset.UtcNow)) return value;
            if (pending?.IsCompleted == true) pending = null;
            refresh = pending ??= RefreshAsync();
        }
        try { return await refresh.WaitAsync(token).ConfigureAwait(false); }
        finally { lock (gate) { if (ReferenceEquals(pending,refresh) && refresh.IsCompleted) pending = null; } }
    }
    private async Task<AwsCredentials?> RefreshAsync()
    {
        // One cancelled consumer must not start a second refresh alongside this one.
        var next = await provider(CancellationToken.None).ConfigureAwait(false);
        var now = DateTimeOffset.UtcNow;
        if (next?.ExpiresAt <= now) throw new InvalidOperationException("Credential provider returned expired credentials");
        lock (gate)
        {
            refreshAt = null;
            if (next?.ExpiresAt is DateTimeOffset expires)
                refreshAt = expires - TimeSpan.FromTicks(Math.Min(refreshBefore.Ticks, (expires - now).Ticks / 2));
            value = next;
            return value;
        }
    }
}
