/// <summary>An error response from the service. Modeled errors derive from the service exception, which derives from this class; the details of the response are available here.</summary>
public class ServiceError : Exception
{
    private string message = "Service request failed";
    /// <summary>The error message from the response.</summary>
    public override string Message => message;
    /// <summary>The error code as it appeared on the wire.</summary>
    public string ErrorName { get; private set; } = "UnknownError";
    /// <summary>The HTTP status code of the response.</summary>
    public int StatusCode { get; private set; }
    /// <summary>The request id the service reported, if any.</summary>
    public string? RequestId { get; private set; }
    /// <summary>The response headers.</summary>
    public IReadOnlyDictionary<string, string> Headers { get; private set; } = new Dictionary<string, string>();
    /// <summary>Whether the client treats the error as retryable.</summary>
    public bool Retryable { get; private set; }
    /// <summary>Whether the client treats the error as throttling.</summary>
    public bool Throttling { get; private set; }
    // BEGIN ERROR DOCUMENTATION
    /// <summary>Static documentation for the modeled error code and its known reason, or null for an unknown identity.</summary>
    public string? DocUrl { get; internal set; }
    // END ERROR DOCUMENTATION
    internal void Initialize(string name, string text, int status, string? requestId,
        Dictionary<string, string> headers, bool retryable, bool throttling)
    {
        ErrorName = name; message = text; StatusCode = status; RequestId = requestId;
        Headers = new System.Collections.ObjectModel.ReadOnlyDictionary<string, string>(headers);
        Retryable = retryable; Throttling = throttling;
    }
}
