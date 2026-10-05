/// <summary>A tagged CBOR data item (major type 6), such as a tag 1 epoch timestamp.</summary>
public sealed record CborTag(ulong Tag, object? Value);
