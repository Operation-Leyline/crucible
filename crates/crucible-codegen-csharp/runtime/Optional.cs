internal interface IOptional
{
    bool IsSet { get; }
    bool IsNull { get; }
    object? Untyped { get; }
}

/// <summary>Distinguishes omitted members from explicit null and a supplied value.</summary>
public readonly struct Optional<T> : IOptional
{
    /// <summary>Whether the member was set, to a value or to an explicit null.</summary>
    public bool IsSet { get; }
    /// <summary>Whether the member was set to an explicit null.</summary>
    public bool IsNull { get; }
    /// <summary>The value; meaningful only when <see cref="IsSet"/> and not <see cref="IsNull"/>.</summary>
    public T Value { get; }
    object? IOptional.Untyped => IsNull ? null : Value;
    private Optional(bool isNull, T value) { IsSet = true; IsNull = isNull; Value = value; }
    /// <summary>An explicit null, which is sent as a JSON null.</summary>
    public static Optional<T> Null => new(true, default!);
    /// <summary>A present value.</summary>
    /// <param name="value">The value; null becomes an explicit null.</param>
    /// <returns>A set optional.</returns>
    public static Optional<T> Of(T value) => new(value is null, value);
    /// <summary>Wraps a value as a set optional.</summary>
    /// <param name="value">The value.</param>
    /// <returns>A set optional.</returns>
    public static implicit operator Optional<T>(T value) => Of(value);
    internal static Optional<T> FromWire(object? value) => value is null ? Null : Of((T)value);
}
