using System.Globalization;
using System.Numerics;
using System.Text.RegularExpressions;

/// <summary>An instant to the nanosecond: whole seconds since the Unix epoch and a nonnegative fraction of a second.</summary>
/// <remarks>
/// <see cref="DateTimeOffset"/> counts 100-nanosecond ticks, so a wire timestamp with eight or nine
/// fractional digits does not fit it. This type keeps every digit. Converting from a
/// <see cref="DateTimeOffset"/> is exact (implicit); converting to one is explicit and lossy: it rounds
/// toward the past to a whole tick and fails outside years 1 through 9999.
/// </remarks>
public readonly struct Timestamp : IEquatable<Timestamp>, IComparable<Timestamp>, IComparable
{
    private const long NanosPerSecond = 1_000_000_000;
    private const long MinTextSeconds = -62_167_219_200; // 0000-01-01T00:00:00Z
    private const long MaxTextSeconds = 253_402_300_799; // 9999-12-31T23:59:59Z
    private static readonly Regex DateTimeSyntax = new(
        @"\A([0-9]{4})-([0-9]{2})-([0-9]{2})[Tt ]([0-9]{2}):([0-9]{2}):([0-9]{2})(?:\.([0-9]+))?(?:([Zz])|([+-])([0-9]{2}):([0-9]{2}))?\z",
        RegexOptions.CultureInvariant | RegexOptions.NonBacktracking);

    /// <summary>Creates a timestamp from exact parts.</summary>
    /// <param name="seconds">Whole seconds since 1970-01-01T00:00:00Z; negative before it.</param>
    /// <param name="nanoseconds">The fraction of a second, from 0 to 999999999.</param>
    /// <exception cref="ArgumentOutOfRangeException"><paramref name="nanoseconds"/> is outside its range.</exception>
    public Timestamp(long seconds, int nanoseconds = 0)
    {
        if (nanoseconds < 0 || nanoseconds >= NanosPerSecond) throw new ArgumentOutOfRangeException(nameof(nanoseconds), "Nanoseconds must be from 0 to 999999999");
        Seconds = seconds;
        Nanoseconds = nanoseconds;
    }

    /// <summary>Whole seconds since the Unix epoch, rounded toward the past.</summary>
    public long Seconds { get; }
    /// <summary>The fraction of a second in nanoseconds, from 0 to 999999999.</summary>
    public int Nanoseconds { get; }

    /// <summary>The same instant as <paramref name="value"/>, exactly.</summary>
    /// <param name="value">The date and time; its offset is not kept.</param>
    /// <returns>The timestamp.</returns>
    public static Timestamp FromDateTimeOffset(DateTimeOffset value)
    {
        var ticks = value.UtcTicks - DateTimeOffset.UnixEpoch.UtcTicks;
        var seconds = Math.DivRem(ticks, TimeSpan.TicksPerSecond, out var remainder);
        if (remainder < 0) { seconds--; remainder += TimeSpan.TicksPerSecond; }
        return new Timestamp(seconds, checked((int)(remainder * 100)));
    }

    /// <summary>The same instant as a <see cref="DateTimeOffset"/> in UTC, with digits finer than 100 nanoseconds dropped (toward the past).</summary>
    /// <returns>The date and time.</returns>
    /// <exception cref="OverflowException">The instant is outside years 1 through 9999.</exception>
    public DateTimeOffset ToDateTimeOffset()
    {
        var ticks = checked(Seconds * TimeSpan.TicksPerSecond + Nanoseconds / 100);
        try { return new DateTimeOffset(checked(DateTimeOffset.UnixEpoch.UtcTicks + ticks), TimeSpan.Zero); }
        catch (ArgumentOutOfRangeException) { throw new OverflowException("Timestamp is outside the range of DateTimeOffset"); }
    }

    /// <summary>Converts exactly.</summary>
    /// <param name="value">The date and time.</param>
    public static implicit operator Timestamp(DateTimeOffset value) => FromDateTimeOffset(value);
    /// <summary>Converts with the loss <see cref="ToDateTimeOffset"/> describes.</summary>
    /// <param name="value">The timestamp.</param>
    public static explicit operator DateTimeOffset(Timestamp value) => value.ToDateTimeOffset();

    /// <summary>Reads an RFC 3339 <c>date-time</c> with up to nine fractional digits. Text without an offset is UTC.</summary>
    /// <param name="text">The text, such as <c>2026-03-04T05:06:07.123456789Z</c>.</param>
    /// <returns>The timestamp.</returns>
    /// <exception cref="FormatException">The text is not a valid date-time, or its fraction has a nonzero digit past the ninth.</exception>
    public static Timestamp Parse(string text)
    {
        ArgumentNullException.ThrowIfNull(text);
        var match = DateTimeSyntax.Match(text);
        if (!match.Success) throw new FormatException("Invalid date-time");
        int Part(int group) => int.Parse(match.Groups[group].Value, CultureInfo.InvariantCulture);
        int year = Part(1), month = Part(2), day = Part(3), hour = Part(4), minute = Part(5), second = Part(6);
        if (month is < 1 or > 12 || day < 1 || day > DaysInMonth(year, month) || hour > 23 || minute > 59 || second > 59)
            throw new FormatException("Invalid date-time components");
        var fraction = match.Groups[7].Value;
        if (fraction.Length > 9 && fraction[9..].Trim('0').Length != 0) throw new FormatException("Timestamp exceeds nanosecond precision");
        var nanoseconds = int.Parse(fraction.PadRight(9, '0')[..9], CultureInfo.InvariantCulture);
        long offset = 0;
        if (match.Groups[9].Success)
        {
            int offsetHours = Part(10), offsetMinutes = Part(11);
            if (offsetHours > 23 || offsetMinutes > 59) throw new FormatException("Invalid date-time offset");
            offset = (offsetHours * 3600L + offsetMinutes * 60L) * (match.Groups[9].Value == "-" ? -1 : 1);
        }
        return new Timestamp(DaysFromCivil(year, month, day) * 86400 + hour * 3600 + minute * 60 + second - offset, nanoseconds);
    }

    /// <summary>Reads epoch seconds as an exact decimal, never through a binary float.</summary>
    /// <param name="value">The number, for example <c>1772600767.123456789</c>.</param>
    /// <returns>The timestamp.</returns>
    /// <exception cref="FormatException">The number is finer than a nanosecond.</exception>
    /// <exception cref="OverflowException">The number is outside the range of a 64-bit second count.</exception>
    public static Timestamp FromEpoch(BigDecimal value)
    {
        var (coefficient, exponent) = value.Normalize();
        if (coefficient.IsZero) return new Timestamp(0);
        if (exponent < -9) throw new FormatException("Timestamp exceeds nanosecond precision");
        if (exponent > 40) throw new OverflowException("Timestamp is out of range");
        var total = coefficient * BigInteger.Pow(10, (int)exponent + 9);
        var seconds = BigInteger.DivRem(total, NanosPerSecond, out var remainder);
        if (remainder.Sign < 0) { seconds -= 1; remainder += NanosPerSecond; }
        if (seconds < long.MinValue || seconds > long.MaxValue) throw new OverflowException("Timestamp is out of range");
        return new Timestamp((long)seconds, (int)remainder);
    }

    /// <summary>Epoch seconds as an exact decimal: <c>1772600767.123456789</c>, or <c>1772600767</c> when whole.</summary>
    /// <returns>The number.</returns>
    public BigDecimal ToEpoch() => new(EpochText());

    internal string EpochText()
    {
        var total = (BigInteger)Seconds * NanosPerSecond + Nanoseconds;
        var magnitude = BigInteger.Abs(total);
        var whole = BigInteger.DivRem(magnitude, NanosPerSecond, out var fraction);
        var text = (total.Sign < 0 ? "-" : "") + whole.ToString(CultureInfo.InvariantCulture);
        return fraction.IsZero ? text : text + "." + ((int)fraction).ToString("D9", CultureInfo.InvariantCulture).TrimEnd('0');
    }

    /// <summary>RFC 3339 in UTC with the fraction trimmed of trailing zeros (up to nine digits), for the wire's four-digit years.</summary>
    /// <returns>The text.</returns>
    /// <exception cref="FormatException">The instant is outside years 0000 through 9999.</exception>
    public string ToDateTimeString()
    {
        if (Seconds < MinTextSeconds || Seconds > MaxTextSeconds) throw new FormatException("Text timestamps require years 0000 through 9999");
        var days = Math.DivRem(Seconds, 86400, out var rest);
        if (rest < 0) { days--; rest += 86400; }
        var (year, month, day) = CivilFromDays(days);
        var text = string.Create(CultureInfo.InvariantCulture, $"{year:D4}-{month:D2}-{day:D2}T{rest / 3600:D2}:{rest / 60 % 60:D2}:{rest % 60:D2}");
        return Nanoseconds == 0 ? text + "Z" : text + "." + Nanoseconds.ToString("D9", CultureInfo.InvariantCulture).TrimEnd('0') + "Z";
    }

    /// <summary>The <c>date-time</c> text, or <c>seconds.nanoseconds</c> when the year is outside 0000 through 9999.</summary>
    /// <returns>The text.</returns>
    public override string ToString() =>
        Seconds < MinTextSeconds || Seconds > MaxTextSeconds ? EpochText() : ToDateTimeString();

    /// <inheritdoc/>
    public bool Equals(Timestamp other) => Seconds == other.Seconds && Nanoseconds == other.Nanoseconds;
    /// <inheritdoc/>
    public override bool Equals(object? value) => value is Timestamp other && Equals(other);
    /// <inheritdoc/>
    public override int GetHashCode() => HashCode.Combine(Seconds, Nanoseconds);
    /// <inheritdoc/>
    public int CompareTo(Timestamp other) => Seconds != other.Seconds ? Seconds.CompareTo(other.Seconds) : Nanoseconds.CompareTo(other.Nanoseconds);
    /// <inheritdoc/>
    public int CompareTo(object? value) => value is null ? 1 : value is Timestamp other ? CompareTo(other) : throw new ArgumentException("Object must be a Timestamp", nameof(value));
    /// <summary>Equality of instants.</summary>
    /// <param name="left">The left value.</param>
    /// <param name="right">The right value.</param>
    /// <returns>True when equal.</returns>
    public static bool operator ==(Timestamp left, Timestamp right) => left.Equals(right);
    /// <summary>Inequality of instants.</summary>
    /// <param name="left">The left value.</param>
    /// <param name="right">The right value.</param>
    /// <returns>True when not equal.</returns>
    public static bool operator !=(Timestamp left, Timestamp right) => !left.Equals(right);
    /// <summary>Whether the left instant is earlier.</summary>
    /// <param name="left">The left value.</param>
    /// <param name="right">The right value.</param>
    /// <returns>True when earlier.</returns>
    public static bool operator <(Timestamp left, Timestamp right) => left.CompareTo(right) < 0;
    /// <summary>Whether the left instant is later.</summary>
    /// <param name="left">The left value.</param>
    /// <param name="right">The right value.</param>
    /// <returns>True when later.</returns>
    public static bool operator >(Timestamp left, Timestamp right) => left.CompareTo(right) > 0;
    /// <summary>Whether the left instant is not later.</summary>
    /// <param name="left">The left value.</param>
    /// <param name="right">The right value.</param>
    /// <returns>True when not later.</returns>
    public static bool operator <=(Timestamp left, Timestamp right) => left.CompareTo(right) <= 0;
    /// <summary>Whether the left instant is not earlier.</summary>
    /// <param name="left">The left value.</param>
    /// <param name="right">The right value.</param>
    /// <returns>True when not earlier.</returns>
    public static bool operator >=(Timestamp left, Timestamp right) => left.CompareTo(right) >= 0;

    private static int DaysInMonth(int year, int month) =>
        month == 2 ? (year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) ? 29 : 28) : month is 4 or 6 or 9 or 11 ? 30 : 31;

    // Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm), valid for year 0 as well.
    private static long DaysFromCivil(long year, int month, int day)
    {
        if (month <= 2) year--;
        var era = (year >= 0 ? year : year - 399) / 400;
        var yearOfEra = year - era * 400;
        var dayOfYear = (153 * (month + (month > 2 ? -3 : 9)) + 2) / 5 + day - 1;
        var dayOfEra = yearOfEra * 365 + yearOfEra / 4 - yearOfEra / 100 + dayOfYear;
        return era * 146097 + dayOfEra - 719468;
    }

    private static (long Year, int Month, int Day) CivilFromDays(long days)
    {
        days += 719468;
        var era = (days >= 0 ? days : days - 146096) / 146097;
        var dayOfEra = days - era * 146097;
        var yearOfEra = (dayOfEra - dayOfEra / 1460 + dayOfEra / 36524 - dayOfEra / 146096) / 365;
        var dayOfYear = dayOfEra - (365 * yearOfEra + yearOfEra / 4 - yearOfEra / 100);
        var shifted = (5 * dayOfYear + 2) / 153;
        var day = (int)(dayOfYear - (153 * shifted + 2) / 5 + 1);
        var month = (int)(shifted < 10 ? shifted + 3 : shifted - 9);
        return (yearOfEra + era * 400 + (month <= 2 ? 1 : 0), month, day);
    }
}
