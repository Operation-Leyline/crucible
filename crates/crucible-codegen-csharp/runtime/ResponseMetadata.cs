using System.Collections.ObjectModel;

/// <summary>HTTP metadata available as soon as a response stream opens.</summary>
public sealed class ResponseMetadata
{
    /// <summary>The opening HTTP status.</summary>
    public int StatusCode { get; }
    /// <summary>The opening request identifier, when present.</summary>
    public string? RequestId { get; }
    /// <summary>The opening HTTP headers, with case-insensitive lookup.</summary>
    public IReadOnlyDictionary<string, string> Headers { get; }
    internal ResponseMetadata(int status, Dictionary<string, string> headers)
    {
        StatusCode = status; RequestId = headers.GetValueOrDefault("x-request-id");
        Headers = new ReadOnlyDictionary<string, string>(new Dictionary<string, string>(headers, StringComparer.OrdinalIgnoreCase));
    }
}
