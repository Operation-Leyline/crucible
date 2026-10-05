using System.Collections;

/// <summary>A CBOR map with text keys, in insertion order. Duplicate keys are rejected.</summary>
public sealed class CborMap : IEnumerable<KeyValuePair<string, object?>>
{
    private readonly List<KeyValuePair<string, object?>> entries = [];
    private readonly Dictionary<string, object?> index = new(StringComparer.Ordinal);
    public int Count => entries.Count;
    public object? this[string key] => index[key];
    public void Add(string key, object? value)
    {
        if (!index.TryAdd(key, value)) throw new FormatException("Duplicate CBOR map key");
        entries.Add(new(key, value));
    }
    public bool ContainsKey(string key) => index.ContainsKey(key);
    public bool TryGetValue(string key, out object? value) => index.TryGetValue(key, out value);
    public IEnumerator<KeyValuePair<string, object?>> GetEnumerator() => entries.GetEnumerator();
    IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
}
