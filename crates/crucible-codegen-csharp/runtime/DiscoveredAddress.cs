/// <summary>A discovered endpoint URL and the clock reading (seconds) at which it expires. Compared by identity.</summary>
internal sealed class DiscoveredAddress(string url, double expires)
{
    internal string Url { get; } = url;
    internal double Expires { get; } = expires;
}
