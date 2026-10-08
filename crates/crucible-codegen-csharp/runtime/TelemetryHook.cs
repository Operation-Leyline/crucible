/// <summary>
/// Receives a client's telemetry: for every call <c>OnOperationStarted</c>, one <c>OnAttemptStarted</c>/<c>OnAttemptFinished</c>
/// pair per attempt that reached the transport, then <c>OnOperationFinished</c>. Events never carry bodies, inputs, outputs,
/// error messages, credentials or sensitive member values. Methods run synchronously on the calling thread and may run
/// concurrently for concurrent calls. A method that throws is traced (<see cref="System.Diagnostics.Trace"/>) and never
/// changes the call's result, retries or timing.
/// </summary>
public abstract class TelemetryHook
{
    /// <summary>A call began.</summary>
    /// <param name="e">The event.</param>
    public virtual void OnOperationStarted(OperationStarted e) { }
    /// <summary>An attempt is about to reach the transport.</summary>
    /// <param name="e">The event.</param>
    public virtual void OnAttemptStarted(AttemptStarted e) { }
    /// <summary>An attempt finished, successfully or not.</summary>
    /// <param name="e">The event.</param>
    public virtual void OnAttemptFinished(AttemptFinished e) { }
    /// <summary>A call finished.</summary>
    /// <param name="e">The event.</param>
    public virtual void OnOperationFinished(OperationFinished e) { }
}
