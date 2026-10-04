internal interface IOptional
{
    bool IsSet { get; }
    bool IsNull { get; }
    object? Untyped { get; }
}

/// <summary>Distinguishes omitted members from explicit null and a supplied value.</summary>
public readonly struct Optional<T> : IOptional
{
    public bool IsSet { get; }
    public bool IsNull { get; }
    public T Value { get; }
    object? IOptional.Untyped => IsNull ? null : Value;
    private Optional(bool isNull, T value) { IsSet = true; IsNull = isNull; Value = value; }
    public static Optional<T> Null => new(true, default!);
    public static Optional<T> Of(T value) => new(value is null, value);
    public static implicit operator Optional<T>(T value) => Of(value);
    internal static Optional<T> FromWire(object? value) => value is null ? Null : Of((T)value);
}
