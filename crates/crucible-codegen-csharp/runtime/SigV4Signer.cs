using System.Globalization;
using System.Security.Cryptography;
using System.Text;

public static class SigV4Signer
{
    internal static string Hash(byte[] bytes) => Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant();
    private static byte[] Hmac(byte[] key, string value) => HMACSHA256.HashData(key, Encoding.UTF8.GetBytes(value));
    public static async ValueTask SignAsync(HttpRequestMessage request, AwsCredentials credentials, SigningOptions options, CancellationToken token = default)
    {
        token.ThrowIfCancellationRequested();
        var now = (options.Time ?? DateTimeOffset.UtcNow).ToUniversalTime();
        if (string.IsNullOrEmpty(credentials.AccessKeyId) || string.IsNullOrEmpty(credentials.SecretAccessKey) || credentials.ExpiresAt <= now)
            throw new ArgumentException("AWS credentials are missing or expired");
        if (string.IsNullOrEmpty(options.Region) || string.IsNullOrEmpty(options.Service) || (options.Region + options.Service).IndexOfAny(['/', '\r', '\n']) >= 0)
            throw new ArgumentException("SigV4 region and service are required scope components");
        SetHeader(request, "Authorization", null);
        SetHeader(request, "X-Amz-Date", now.ToString("yyyyMMdd'T'HHmmss'Z'", CultureInfo.InvariantCulture));
        SetHeader(request, "X-Amz-Security-Token", credentials.SessionToken);
        var digest = await PayloadDigestAsync(request, options.UnsignedPayload, token).ConfigureAwait(false);
        SetHeader(request, "X-Amz-Content-Sha256", digest);
        var (canonical, signed) = CanonicalRequest(request, digest, options.Service, options.DisableNormalizePath, options.DisableDoubleEncoding);
        var date = now.ToString("yyyyMMdd", CultureInfo.InvariantCulture);
        var scope = $"{date}/{options.Region}/{options.Service}/aws4_request";
        var toSign = "AWS4-HMAC-SHA256\n" + request.Headers.GetValues("X-Amz-Date").Single() + "\n" + scope + "\n" + Hash(Encoding.UTF8.GetBytes(canonical));
        var keyBytes = Encoding.UTF8.GetBytes("AWS4" + credentials.SecretAccessKey);
        foreach (var component in new[] { date, options.Region, options.Service, "aws4_request" }) keyBytes = Hmac(keyBytes, component);
        SetHeader(request, "Authorization", $"AWS4-HMAC-SHA256 Credential={credentials.AccessKeyId}/{scope}, SignedHeaders={signed}, Signature={Convert.ToHexString(Hmac(keyBytes, toSign)).ToLowerInvariant()}");
        token.ThrowIfCancellationRequested();
    }

    internal static void SetHeader(HttpRequestMessage request, string key, string? value)
    {
        request.Headers.Remove(key);
        if (value is not null) request.Headers.TryAddWithoutValidation(key, value);
    }

    /// <summary>The payload hash a signature covers. A stream is never buffered to hash it: it signs its declared or precomputed digest, else an unsigned payload.</summary>
    internal static async ValueTask<string> PayloadDigestAsync(HttpRequestMessage request, bool unsignedPayload, CancellationToken token)
    {
        if (unsignedPayload) return "UNSIGNED-PAYLOAD";
        if (request.Content is ByteStreamContent streaming)
            return streaming.Body.Sha256 ?? (request.Headers.TryGetValues("X-Amz-Content-Sha256", out var supplied) ? supplied.Single() : "UNSIGNED-PAYLOAD");
        return Hash(request.Content is null ? [] : await request.Content.ReadAsByteArrayAsync(token).ConfigureAwait(false));
    }

    /// <summary>The SigV4 canonical request (shared by SigV4a) and its signed header names.</summary>
    internal static (string Canonical, string Signed) CanonicalRequest(HttpRequestMessage request, string digest, string service, bool? disableNormalizePath, bool? disableDoubleEncoding)
    {
        var url = request.RequestUri ?? throw new ArgumentException("Request URL required");
        var path = "/" + url.GetComponents(UriComponents.Path, UriFormat.UriEscaped);
        if (!(disableNormalizePath ?? service == "s3"))
        {
            var parts = new List<string>();
            foreach (var part in path.Split('/'))
            {
                if (part is "" or ".") continue;
                if (part == "..") { if (parts.Count != 0) parts.RemoveAt(parts.Count - 1); }
                else parts.Add(part);
            }
            var trailing = path.EndsWith('/'); path = "/" + string.Join("/", parts);
            if (trailing && !path.EndsWith('/')) path += "/";
        }
        if (!(disableDoubleEncoding ?? service == "s3")) path = string.Join("/", path.Split('/').Select(Uri.EscapeDataString));
        var query = new List<string>();
        foreach (var pair in url.Query.TrimStart('?').Split('&', StringSplitOptions.RemoveEmptyEntries))
        {
            var parts = pair.Split('=', 2);
            query.Add(Uri.EscapeDataString(Uri.UnescapeDataString(parts[0])) + "=" + Uri.EscapeDataString(Uri.UnescapeDataString(parts.Length == 2 ? parts[1] : "")));
        }
        query.Sort(StringComparer.Ordinal);
        var excluded = new HashSet<string>(["authorization", "connection", "expect", "transfer-encoding", "user-agent", "x-amzn-trace-id", "accept-encoding", "content-length"], StringComparer.OrdinalIgnoreCase);
        var headers = new SortedDictionary<string, List<string>>(StringComparer.Ordinal);
        var all = request.Headers.AsEnumerable();
        if (request.Content is not null) all = all.Concat(request.Content.Headers);
        foreach (var header in all)
        {
            var key = header.Key.ToLowerInvariant(); if (excluded.Contains(key) || key == "host") continue;
            if (!headers.TryGetValue(key, out var values)) headers[key] = values = [];
            values.AddRange(header.Value.Select(value => string.Join(" ", value.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries))));
        }
        headers["host"] = [request.Headers.Host ?? url.Authority];
        var signed = string.Join(";", headers.Keys);
        var canonicalHeaders = string.Concat(headers.Select(header => header.Key + ":" + string.Join(",", header.Value) + "\n"));
        return (string.Join("\n", request.Method.Method, path, string.Join("&", query), canonicalHeaders, signed, digest), signed);
    }
}
