/// <summary>Overrides of the modeled waiter delay bounds. The overall deadline is the timeout passed to the waiter call.</summary>
public sealed record WaiterOptions
{
    /// <summary>Overrides the modeled minimum delay between polls; null keeps the model.</summary>
    public TimeSpan? MinDelay { get; init; }
    /// <summary>Overrides the modeled maximum delay between polls; null keeps the model.</summary>
    public TimeSpan? MaxDelay { get; init; }
}
