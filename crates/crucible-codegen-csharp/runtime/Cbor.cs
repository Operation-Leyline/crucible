using System.Buffers.Binary;
using System.Collections;
using System.Numerics;
using System.Text;

/// <summary>
/// RFC 8949 CBOR for the Smithy RPC v2 CBOR protocol, without external packages.
/// Items are <c>long</c> (or <see cref="BigInteger"/> beyond it), <c>double</c>, <c>string</c> (text), <c>byte[]</c>
/// (byte strings), <see cref="List{T}"/> arrays, <see cref="CborMap"/> maps with text keys, <see cref="CborTag"/>,
/// <c>bool</c>, and null (undefined decodes to null). Malformed input throws <see cref="FormatException"/>.
/// </summary>
public static class Cbor
{
    private const int MaxDepth = 256;
    private static readonly UTF8Encoding StrictUtf8 = new(false, true);
    private static readonly object Break = new();

    public static byte[] Encode(object? item)
    {
        using var output = new MemoryStream();
        Write(output, item, 0);
        return output.ToArray();
    }

    private static void Write(MemoryStream output, object? item, int depth)
    {
        if (depth >= MaxDepth) throw new FormatException("CBOR nesting limit");
        switch (item)
        {
            case null: output.WriteByte(0xf6); return;
            case bool flag: output.WriteByte(flag ? (byte)0xf5 : (byte)0xf4); return;
            case sbyte or short or int or long:
                var signed = Convert.ToInt64(item, System.Globalization.CultureInfo.InvariantCulture);
                if (signed >= 0) Head(output, 0, (ulong)signed); else Head(output, 1, (ulong)(-1 - signed));
                return;
            case byte or ushort or uint or ulong: Head(output, 0, Convert.ToUInt64(item, System.Globalization.CultureInfo.InvariantCulture)); return;
            case BigInteger big:
                if (big.Sign >= 0 && big <= ulong.MaxValue) Head(output, 0, (ulong)big);
                else if (big.Sign < 0 && -1 - big <= ulong.MaxValue) Head(output, 1, (ulong)(-1 - big));
                else throw new FormatException("An integer beyond 64 bits needs a bignum tag");
                return;
            case float single: WriteFloat(output, single); return;
            case double number: WriteFloat(output, number); return;
            case string text:
                byte[] utf8;
                try { utf8 = StrictUtf8.GetBytes(text); }
                catch (EncoderFallbackException e) { throw new FormatException("CBOR text must be valid Unicode", e); }
                Head(output, 3, (ulong)utf8.Length); output.Write(utf8);
                return;
            case byte[] bytes: Head(output, 2, (ulong)bytes.Length); output.Write(bytes); return;
            case CborTag tag: Head(output, 6, tag.Tag); Write(output, tag.Value, depth + 1); return;
            case CborMap map:
                Head(output, 5, (ulong)map.Count);
                foreach (var (key, value) in map) { Write(output, key, depth + 1); Write(output, value, depth + 1); }
                return;
            case IList list:
                Head(output, 4, (ulong)list.Count);
                foreach (var value in list) Write(output, value, depth + 1);
                return;
            default: throw new FormatException("Not a CBOR value: " + item.GetType().Name);
        }
    }

    /// <summary>The smallest of single or double precision that is exact; Smithy asks for no half-precision floats.</summary>
    private static void WriteFloat(MemoryStream output, double value)
    {
        Span<byte> bytes = stackalloc byte[9];
        if (double.IsNaN(value)) { output.Write([0xfa, 0x7f, 0xc0, 0x00, 0x00]); return; }
        var single = (float)value;
        if (single == value || double.IsInfinity(value))
        {
            bytes[0] = 0xfa; BinaryPrimitives.WriteSingleBigEndian(bytes[1..], single);
            output.Write(bytes[..5]);
            return;
        }
        bytes[0] = 0xfb; BinaryPrimitives.WriteDoubleBigEndian(bytes[1..], value);
        output.Write(bytes);
    }

    private static void Head(MemoryStream output, int major, ulong argument)
    {
        var type = (byte)(major << 5);
        Span<byte> bytes = stackalloc byte[9];
        if (argument < 24) { output.WriteByte((byte)(type | (byte)argument)); return; }
        if (argument <= byte.MaxValue) { output.WriteByte((byte)(type | 24)); output.WriteByte((byte)argument); return; }
        if (argument <= ushort.MaxValue) { bytes[0] = (byte)(type | 25); BinaryPrimitives.WriteUInt16BigEndian(bytes[1..], (ushort)argument); output.Write(bytes[..3]); return; }
        if (argument <= uint.MaxValue) { bytes[0] = (byte)(type | 26); BinaryPrimitives.WriteUInt32BigEndian(bytes[1..], (uint)argument); output.Write(bytes[..5]); return; }
        bytes[0] = (byte)(type | 27); BinaryPrimitives.WriteUInt64BigEndian(bytes[1..], argument); output.Write(bytes);
    }

    /// <summary>Decodes exactly one item; trailing bytes, truncation, invalid UTF-8 and duplicate keys are errors.</summary>
    public static object? Decode(ReadOnlySpan<byte> source)
    {
        var reader = new Reader(source.ToArray());
        var item = reader.Item(0);
        if (ReferenceEquals(item, Break)) throw new FormatException("Unexpected CBOR break");
        if (reader.Position != source.Length) throw new FormatException("Trailing CBOR input");
        return item;
    }

    private sealed class Reader(byte[] source)
    {
        internal int Position;
        private long Remaining => source.Length - Position;

        private byte Next()
        {
            if (Position >= source.Length) throw new FormatException("Truncated CBOR input");
            return source[Position++];
        }

        private ReadOnlySpan<byte> Take(ulong length)
        {
            // A definite length can never exceed the remaining input, so nothing large is allocated.
            if (length > (ulong)Remaining) throw new FormatException("Truncated CBOR input");
            var span = source.AsSpan(Position, (int)length);
            Position += (int)length;
            return span;
        }

        /// <summary>The argument of an item head; null for an indefinite length.</summary>
        private ulong? Argument(int info) => info switch
        {
            < 24 => (ulong)info,
            24 => Next(),
            25 => BinaryPrimitives.ReadUInt16BigEndian(Take(2)),
            26 => BinaryPrimitives.ReadUInt32BigEndian(Take(4)),
            27 => BinaryPrimitives.ReadUInt64BigEndian(Take(8)),
            31 => null,
            _ => throw new FormatException("Reserved CBOR additional information"),
        };

        private int Length(ulong argument)
        {
            if (argument > (ulong)Remaining) throw new FormatException("Truncated CBOR input");
            return (int)argument;
        }

        internal object? Item(int depth)
        {
            if (depth >= MaxDepth) throw new FormatException("CBOR nesting limit");
            var initial = Next();
            int major = initial >> 5, info = initial & 31;
            if (info == 31 && major is 0 or 1 or 6) throw new FormatException("Invalid indefinite CBOR item");
            var argument = Argument(info);
            switch (major)
            {
                case 0: return argument!.Value <= long.MaxValue ? (object)(long)argument.Value : (BigInteger)argument.Value;
                case 1: return argument!.Value <= long.MaxValue ? (object)(-1 - (long)argument.Value) : -BigInteger.One - argument.Value;
                case 2:
                case 3:
                    byte[] bytes;
                    if (argument is null)
                    {
                        using var chunks = new MemoryStream();
                        while (true)
                        {
                            var chunk = Next();
                            if (chunk == 0xff) break;
                            if (chunk >> 5 != major || (chunk & 31) == 31) throw new FormatException("Invalid CBOR string chunk");
                            chunks.Write(Take(Argument(chunk & 31)!.Value));
                        }
                        bytes = chunks.ToArray();
                    }
                    else bytes = Take(argument.Value).ToArray();
                    if (major == 2) return bytes;
                    try { return StrictUtf8.GetString(bytes); }
                    catch (DecoderFallbackException e) { throw new FormatException("CBOR text is not UTF-8", e); }
                case 4:
                    var items = new List<object?>();
                    if (argument is null)
                    {
                        while (true)
                        {
                            var item = Item(depth + 1);
                            if (ReferenceEquals(item, Break)) return items;
                            items.Add(item);
                        }
                    }
                    for (int i = 0, n = Length(argument.Value); i < n; i++) items.Add(Value(depth));
                    return items;
                case 5:
                    var map = new CborMap();
                    for (long i = 0, n = argument is null ? long.MaxValue : Length(argument.Value); i < n; i++)
                    {
                        var key = Item(depth + 1);
                        if (ReferenceEquals(key, Break) && argument is null) break;
                        var name = key switch
                        {
                            string text => text,
                            long or BigInteger => Convert.ToString(key, System.Globalization.CultureInfo.InvariantCulture)!,
                            _ => throw new FormatException("CBOR map keys must be text"),
                        };
                        map.Add(name, Value(depth));
                    }
                    return map;
                case 6: return new CborTag(argument!.Value, Value(depth));
                default:
                    return info switch
                    {
                        20 => false,
                        21 => true,
                        22 or 23 => null,
                        25 => (double)BitConverter.UInt16BitsToHalf((ushort)argument!.Value),
                        26 => (double)BitConverter.UInt32BitsToSingle((uint)argument!.Value),
                        27 => BitConverter.UInt64BitsToDouble(argument!.Value),
                        31 => Break,
                        _ => throw new FormatException("Unsupported CBOR simple value"),
                    };
            }
        }

        private object? Value(int depth)
        {
            var item = Item(depth + 1);
            if (ReferenceEquals(item, Break)) throw new FormatException("Unexpected CBOR break");
            return item;
        }
    }
}
