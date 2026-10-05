using System.Globalization;
using System.Numerics;
using System.Text.RegularExpressions;

/// <summary>An exact JSON decimal. The original decimal text is preserved.</summary>
public readonly struct BigDecimal : IEquatable<BigDecimal>, IComparable<BigDecimal>
{
    private readonly string? text;
    private static readonly Regex Syntax = new(@"\A-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?\z", RegexOptions.CultureInvariant | RegexOptions.NonBacktracking);

    public BigDecimal(string value)
    {
        ArgumentNullException.ThrowIfNull(value);
        if (!Syntax.IsMatch(value)) throw new FormatException("Invalid JSON decimal");
        text = value;
    }

    public override string ToString() => text ?? "0";

    internal (BigInteger Coefficient, BigInteger Exponent) Normalize()
    {
        var value = ToString();
        int marker = value.IndexOfAny(['e', 'E']);
        var exponent = marker < 0 ? BigInteger.Zero : BigInteger.Parse(value[(marker + 1)..], CultureInfo.InvariantCulture);
        var digits = marker < 0 ? value : value[..marker];
        int dot = digits.IndexOf('.');
        if (dot >= 0) { exponent -= digits.Length - dot - 1; digits = digits.Remove(dot, 1); }
        var coefficient = BigInteger.Parse(digits, CultureInfo.InvariantCulture);
        if (coefficient.IsZero) return (BigInteger.Zero, BigInteger.Zero);
        while (coefficient % 10 == 0) { coefficient /= 10; exponent += 1; }
        return (coefficient, exponent);
    }

    public bool Equals(BigDecimal other) => Normalize() == other.Normalize();
    public int CompareTo(BigDecimal other)
    {
        var (a, ae) = Normalize(); var (b, be) = other.Normalize();
        if (a.Sign != b.Sign) return a.Sign.CompareTo(b.Sign);
        if (a.IsZero) return 0;
        var left = BigInteger.Abs(a).ToString(CultureInfo.InvariantCulture);
        var right = BigInteger.Abs(b).ToString(CultureInfo.InvariantCulture);
        int magnitude = (ae + left.Length).CompareTo(be + right.Length);
        if (magnitude != 0) return a.Sign * magnitude;
        return a.Sign * string.CompareOrdinal(left.PadRight(Math.Max(left.Length, right.Length), '0'), right.PadRight(Math.Max(left.Length, right.Length), '0'));
    }
    public override bool Equals(object? value) => value is BigDecimal other && Equals(other);
    public override int GetHashCode() => Normalize().GetHashCode();
    public static bool operator ==(BigDecimal left, BigDecimal right) => left.Equals(right);
    public static bool operator !=(BigDecimal left, BigDecimal right) => !left.Equals(right);
}
