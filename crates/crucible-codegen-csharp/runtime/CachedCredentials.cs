public sealed class CachedCredentials
{
    private readonly Func<CancellationToken, ValueTask<AwsCredentials?>> provider;
    private readonly TimeSpan refreshBefore;
    private readonly object gate = new();
    private Task<AwsCredentials?>? pending;
    private AwsCredentials? value;
    private DateTimeOffset? refreshAt;
    public CachedCredentials(Func<CancellationToken, ValueTask<AwsCredentials?>> provider, TimeSpan? refreshBefore = null)
    {
        ArgumentNullException.ThrowIfNull(provider);
        this.provider = provider; this.refreshBefore = refreshBefore ?? TimeSpan.FromMinutes(1);
        if (this.refreshBefore < TimeSpan.Zero) throw new ArgumentOutOfRangeException(nameof(refreshBefore));
    }
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
