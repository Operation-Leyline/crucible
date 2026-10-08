/// <summary>
/// A union variant this client does not know, preserved so a newer service response round-trips.
/// The payload is never printed by <see cref="ToString"/>, since it may be sensitive.
/// </summary>
/// <param name="Name">The wire name of the unrecognized variant.</param>
/// <param name="Value">The variant's raw JSON value.</param>
public sealed record UnknownUnion(string Name, global::System.Text.Json.Nodes.JsonNode? Value)
{
    /// <inheritdoc/>
    public override string ToString() => ModelFormat.Format("UnknownUnion", ("Name", Name), ("Value", ModelFormat.Redacted));
}
