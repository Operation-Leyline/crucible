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
        byte[] bytes;
        if (algorithm is "CRC32" or "CRC32C" or "CRC64NVME") {
            var wide = algorithm == "CRC64NVME";
            var mask = wide ? ulong.MaxValue : uint.MaxValue;
            var table = wide ? Nvme : algorithm == "CRC32C" ? Castagnoli : Ieee;
            ulong value = mask;
            foreach (var item in data) value = table[(value ^ item) & 255] ^ (value >> 8);
            bytes = new byte[wide ? 8 : 4];
            if (wide) BinaryPrimitives.WriteUInt64BigEndian(bytes, value ^ mask);
            else BinaryPrimitives.WriteUInt32BigEndian(bytes, (uint)(value ^ mask));
        } else bytes = algorithm switch {
            "MD5" => MD5.HashData(data), "SHA1" => SHA1.HashData(data), "SHA256" => SHA256.HashData(data), "SHA512" => SHA512.HashData(data),
            _ => throw new ArgumentException("Unsupported checksum algorithm: " + algorithm)
        };
        return Convert.ToBase64String(bytes);
    }
    internal static ChecksumSettings Prepare(JsonNode? config, JsonObject values, Func<string,string> wire, ChecksumPolicy requestPolicy, ChecksumPolicy responsePolicy)
    {
        var response = new List<string>();
        if (config is null) return new(null,null,response);
        if (config["kind"]!.GetValue<string>() == "md5") return new("MD5","Content-MD5",response);
        string? algorithm = null, header = null;
        var required = config["requestRequired"]?.GetValue<bool>() == true;
        if (config["requestAlgorithmMember"]?.GetValue<string>() is string member) {
            var key = wire(member); algorithm = values[key]?.GetValue<string>();
            if (algorithm is null && (required || requestPolicy == ChecksumPolicy.WhenSupported)) { algorithm = "CRC32"; values[key] = algorithm; }
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
        var data = request.Content is null ? [] : await request.Content.ReadAsByteArrayAsync(token).ConfigureAwait(false);
        var value = Digest(settings.Algorithm!,data);
        if (settings.Header.Equals("Content-MD5",StringComparison.OrdinalIgnoreCase)) {
            request.Content ??= new ByteArrayContent([]); request.Content.Headers.ContentMD5 = Convert.FromBase64String(value);
        } else request.Headers.TryAddWithoutValidation(settings.Header,value);
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
