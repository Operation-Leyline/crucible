using System.Buffers.Binary;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json.Nodes;

internal sealed record ChecksumSettings(string? Algorithm, string? Header, List<string> Response);

internal static class Checksums
{
    private static readonly ulong[] Ieee = Table(0xedb88320);
    private static readonly ulong[] Castagnoli = Table(0x82f63b78);
    private static readonly ulong[] Nvme = Table(0x9a6c9329ac4bc9b5);
    private static ulong[] Table(ulong polynomial)
    {
        var result = new ulong[256];
        for (int index = 0; index < result.Length; index++) {
            ulong value = (ulong)index;
            for (int bit = 0; bit < 8; bit++) value = (value >> 1) ^ ((value & 1) != 0 ? polynomial : 0);
            result[index] = value;
        }
        return result;
    }
    private static bool Supported(string algorithm) => algorithm is "CRC32" or "CRC32C" or "CRC64NVME" or "MD5" or "SHA1" or "SHA256" or "SHA512";
    internal static string Digest(string algorithm, byte[] data)
    {
        var hasher = new ChecksumHasher(algorithm);
        hasher.Append(data);
        return hasher.Finish();
    }
    internal static ulong[] CrcTable(string algorithm) => algorithm switch { "CRC64NVME" => Nvme, "CRC32C" => Castagnoli, _ => Ieee };
    internal static ChecksumSettings Prepare(JsonNode? config, JsonObject values, Func<string,string> wire, ChecksumPolicy requestPolicy, ChecksumPolicy responsePolicy, bool streaming = false)
    {
        var response = new List<string>();
        if (config is null) return new(null,null,response);
        if (config["kind"]!.GetValue<string>() == "md5") return new("MD5","Content-MD5",response);
        string? algorithm = null, header = null;
        var required = config["requestRequired"]?.GetValue<bool>() == true;
        if (config["requestAlgorithmMember"]?.GetValue<string>() is string member) {
            var key = wire(member); algorithm = values[key]?.GetValue<string>();
            // A stream is never buffered to compute a header, so only a required or explicit algorithm applies to it.
            if (algorithm is null && (required || !streaming && requestPolicy == ChecksumPolicy.WhenSupported)) { algorithm = "CRC32"; values[key] = algorithm; }
            if (algorithm is not null) {
                if (!Supported(algorithm)) throw new ArgumentException("Unsupported checksum algorithm: " + algorithm);
                header = "x-amz-checksum-" + algorithm.ToLowerInvariant();
            }
        } else if (required) { algorithm = "MD5"; header = "Content-MD5"; }
        if (config["validationModeMember"]?.GetValue<string>() is string mode) {
            var key = wire(mode); var choice = values[key]?.GetValue<string>();
            if (choice is null && responsePolicy == ChecksumPolicy.WhenSupported) { choice = "ENABLED"; values[key] = choice; }
            if (choice == "ENABLED") foreach (var item in config["responseAlgorithms"]!.AsArray()) { var name = item!.GetValue<string>(); if (Supported(name)) response.Add(name); }
        }
        return new(algorithm,header,response);
    }
    internal static async Task RequestAsync(ChecksumSettings settings, HttpRequestMessage request, CancellationToken token)
    {
        if (settings.Header is null) return;
        var contentHeader = settings.Header.Equals("Content-MD5",StringComparison.OrdinalIgnoreCase);
        if (contentHeader ? request.Content?.Headers.Contains(settings.Header) == true : request.Headers.Contains(settings.Header)) return;
        if (request.Content is ByteStreamContent) throw new ArgumentException("Streaming request checksum " + settings.Header + " must be supplied precomputed");
        var data = request.Content is null ? [] : await request.Content.ReadAsByteArrayAsync(token).ConfigureAwait(false);
        var value = Digest(settings.Algorithm!,data);
        if (settings.Header.Equals("Content-MD5",StringComparison.OrdinalIgnoreCase)) {
            request.Content ??= new ByteArrayContent([]); request.Content.Headers.ContentMD5 = Convert.FromBase64String(value);
        } else request.Headers.TryAddWithoutValidation(settings.Header,value);
    }
    /// <summary>Verifies a streaming response as it is read; null when nothing is to be verified.</summary>
    internal static RunningChecksum? Running(ChecksumSettings settings, Dictionary<string,string> headers, string operation, Action<string,string>? validated)
    {
        foreach (var algorithm in settings.Response) {
            if (!headers.TryGetValue("x-amz-checksum-" + algorithm.ToLowerInvariant(),out var expected)) continue;
            return new RunningChecksum(operation, algorithm, expected, validated);
        }
        return null;
    }
    internal static string? Response(ChecksumSettings settings, Dictionary<string,string> headers, byte[] data)
    {
        foreach (var algorithm in settings.Response) {
            if (!headers.TryGetValue("x-amz-checksum-" + algorithm.ToLowerInvariant(),out var expected)) continue;
            if (!CryptographicOperations.FixedTimeEquals(Encoding.UTF8.GetBytes(expected),Encoding.ASCII.GetBytes(Digest(algorithm,data)))) throw new ChecksumException(algorithm);
            return algorithm;
        }
        return null;
    }
}

/// <summary>An incremental checksum, so streamed bytes are never buffered to verify them.</summary>
internal sealed class ChecksumHasher
{
    private readonly ulong[]? table;
    private readonly ulong mask;
    private readonly IncrementalHash? hash;
    private ulong value;
    internal ChecksumHasher(string algorithm)
    {
        if (algorithm is "CRC32" or "CRC32C" or "CRC64NVME") {
            table = Checksums.CrcTable(algorithm); mask = algorithm == "CRC64NVME" ? ulong.MaxValue : uint.MaxValue; value = mask;
        } else hash = IncrementalHash.CreateHash(algorithm switch {
            "MD5" => HashAlgorithmName.MD5, "SHA1" => HashAlgorithmName.SHA1, "SHA256" => HashAlgorithmName.SHA256, "SHA512" => HashAlgorithmName.SHA512,
            _ => throw new ArgumentException("Unsupported checksum algorithm: " + algorithm)
        });
    }
    internal void Append(ReadOnlySpan<byte> data)
    {
        if (hash is not null) { hash.AppendData(data); return; }
        var crc = value;
        foreach (var item in data) crc = table![(crc ^ item) & 255] ^ (crc >> 8);
        value = crc;
    }
    internal string Finish()
    {
        if (hash is not null) return Convert.ToBase64String(hash.GetHashAndReset());
        var bytes = new byte[mask == ulong.MaxValue ? 8 : 4];
        if (bytes.Length == 8) BinaryPrimitives.WriteUInt64BigEndian(bytes, value ^ mask);
        else BinaryPrimitives.WriteUInt32BigEndian(bytes, (uint)(value ^ mask));
        return Convert.ToBase64String(bytes);
    }
}

/// <summary>A response checksum checked when its stream ends; the callback runs only after success.</summary>
internal sealed class RunningChecksum(string operation, string algorithm, string expected, Action<string,string>? validated)
{
    private readonly ChecksumHasher hasher = new(algorithm);
    internal void Append(ReadOnlySpan<byte> data) => hasher.Append(data);
    internal void Verify()
    {
        if (!CryptographicOperations.FixedTimeEquals(Encoding.UTF8.GetBytes(expected), Encoding.ASCII.GetBytes(hasher.Finish()))) throw new ChecksumException(algorithm);
        validated?.Invoke(operation, algorithm);
    }
}
