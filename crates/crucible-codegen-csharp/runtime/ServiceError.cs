public class ServiceError : Exception
{
    private string message = "Service request failed";
    public override string Message => message;
    public string ErrorName { get; private set; } = "UnknownError";
    public int StatusCode { get; private set; }
    public string? RequestId { get; private set; }
    public IReadOnlyDictionary<string, string> Headers { get; private set; } = new Dictionary<string, string>();
    public bool Retryable { get; private set; }
    public bool Throttling { get; private set; }
    internal void Initialize(string name, string text, int status, string? requestId,
        Dictionary<string, string> headers, bool retryable, bool throttling)
    {
        ErrorName = name; message = text; StatusCode = status; RequestId = requestId;
        Headers = new System.Collections.ObjectModel.ReadOnlyDictionary<string, string>(headers);
        Retryable = retryable; Throttling = throttling;
    }
}
