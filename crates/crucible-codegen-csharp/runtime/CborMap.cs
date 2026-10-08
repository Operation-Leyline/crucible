using System.Collections;

/// <summary>A CBOR map with text keys, in insertion order. Duplicate keys are rejected.</summary>
public sealed class CborMap : IEnumerable<KeyValuePair<string, object?>>
{
    private readonly List<KeyValuePair<string, object?>> entries = [];
    private readonly Dictionary<string, object?> index = new(StringComparer.Ordinal);
    /// <summary>The number of entries.</summary>
    public int Count => entries.Count;
    /// <summary>The value for a key.</summary>
    /// <param name="key">The map key.</param>
    public object? this[string key] => index[key];
    /// <summary>Adds an entry, preserving insertion order.</summary>
    /// <param name="key">The map key; it must be new.</param>
    /// <param name="value">The value.</param>
    public void Add(string key, object? value)
    {
        if (!index.TryAdd(key, value)) throw new FormatException("Duplicate CBOR map key");
        entries.Add(new(key, value));
    }
    /// <summary>Whether a key is present.</summary>
    /// <param name="key">The map key.</param>
    /// <returns>True when present.</returns>
    public bool ContainsKey(string key) => index.ContainsKey(key);
    /// <summary>Looks a key up.</summary>
    /// <param name="key">The map key.</param>
    /// <param name="value">The value when found.</param>
    /// <returns>True when found.</returns>
    public bool TryGetValue(string key, out object? value) => index.TryGetValue(key, out value);
    /// <inheritdoc/>
    public IEnumerator<KeyValuePair<string, object?>> GetEnumerator() => entries.GetEnumerator();
    IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
}
