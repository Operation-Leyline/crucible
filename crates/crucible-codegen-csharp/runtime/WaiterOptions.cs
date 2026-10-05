public sealed record WaiterOptions
{
    public TimeSpan? MinDelay { get; init; }
    public TimeSpan? MaxDelay { get; init; }
}
