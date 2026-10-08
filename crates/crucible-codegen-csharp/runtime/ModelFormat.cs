/// <summary>
/// Renders generated model types as <c>TypeName(Member=value, ...)</c>, the way the AWS SDKs do.
/// Unset optional members are omitted, an explicit null prints as <c>null</c>, and collections render
/// their elements recursively. Members marked sensitive are never printed: generated code routes
/// them through <see cref="Redact"/>.
/// </summary>
public static class ModelFormat
{
    /// <summary>Printed in place of the value of a sensitive member.</summary>
    public const string Redacted = "*** Sensitive Data Redacted ***";

    private sealed class Omit { public static readonly Omit Instance = new(); }

    /// <summary>Marks a member to be left out of the output.</summary>
    /// <param name="value">A value that may be null.</param>
    /// <returns>The value, or a marker that omits it when it is null.</returns>
    public static object? Optional(object? value) => value ?? Omit.Instance;

    /// <summary>Hides a sensitive member's value while preserving whether it is set.</summary>
    /// <param name="value">The member's value or its presence wrapper.</param>
    /// <returns>An unset presence unchanged, otherwise the redaction marker.</returns>
    public static object? Redact(object? value) => value is IOptional { IsSet: false } ? value : Redacted;

    /// <summary>Formats a model from its members, in declaration order.</summary>
    /// <param name="type">The model's type name.</param>
    /// <param name="members">Member name and value pairs; an unset presence is skipped.</param>
    /// <returns><c>type(Name=value, ...)</c>.</returns>
    public static string Format(string type, params (string Name, object? Value)[] members)
    {
        var text = new System.Text.StringBuilder(type).Append('(');
        var first = true;
        foreach (var (name, raw) in members)
        {
            var value = raw;
            if (value is IOptional optional)
            {
                if (!optional.IsSet) continue;
                value = optional.Untyped;
            }
            if (ReferenceEquals(value, Omit.Instance)) continue;
            if (!first) text.Append(", ");
            first = false;
            text.Append(name).Append('=');
            Append(text, value);
        }
        return text.Append(')').ToString();
    }

    private static void Append(System.Text.StringBuilder text, object? value)
    {
        switch (value)
        {
            case null: text.Append("null"); break;
            case string s: text.Append(s); break;
            case byte[] bytes: text.Append("byte[").Append(bytes.Length).Append(']'); break;
            case System.Collections.IDictionary map:
                text.Append('{');
                var firstEntry = true;
                foreach (System.Collections.DictionaryEntry entry in map)
                {
                    if (!firstEntry) text.Append(", ");
                    firstEntry = false;
                    Append(text, entry.Key);
                    text.Append('=');
                    Append(text, entry.Value);
                }
                text.Append('}');
                break;
            case System.Collections.IEnumerable items:
                text.Append('[');
                var firstItem = true;
                foreach (var item in items)
                {
                    if (!firstItem) text.Append(", ");
                    firstItem = false;
                    Append(text, item);
                }
                text.Append(']');
                break;
            case IFormattable formattable: text.Append(formattable.ToString(null, System.Globalization.CultureInfo.InvariantCulture)); break;
            default: text.Append(value); break;
        }
    }
}
