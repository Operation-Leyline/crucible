/// <summary>
/// Receives a client's telemetry: for every call <c>OnOperationStarted</c>, one <c>OnAttemptStarted</c>/<c>OnAttemptFinished</c>
/// pair per attempt that reached the transport, then <c>OnOperationFinished</c>. Events never carry bodies, inputs, outputs,
/// error messages, credentials or sensitive member values. Methods run synchronously on the calling thread and may run
/// concurrently for concurrent calls. A method that throws is traced (<see cref="System.Diagnostics.Trace"/>) and never
/// changes the call's result, retries or timing.
/// </summary>
public abstract class TelemetryHook
{
    public virtual void OnOperationStarted(OperationStarted e) { }
    public virtual void OnAttemptStarted(AttemptStarted e) { }
    public virtual void OnAttemptFinished(AttemptFinished e) { }
    public virtual void OnOperationFinished(OperationFinished e) { }
}
