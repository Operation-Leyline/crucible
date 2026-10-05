using System.Globalization;
using System.Security.Cryptography;
using System.Text;

public static class SigV4Signer
{
    private static string Hash(byte[] bytes) => Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant();
    private static byte[] Hmac(byte[] key, string value) => HMACSHA256.HashData(key, Encoding.UTF8.GetBytes(value));
    public static async ValueTask SignAsync(HttpRequestMessage request, AwsCredentials credentials, SigningOptions options, CancellationToken token = default)
    {
        token.ThrowIfCancellationRequested();
        var now = (options.Time ?? DateTimeOffset.UtcNow).ToUniversalTime();
        if (string.IsNullOrEmpty(credentials.AccessKeyId) || string.IsNullOrEmpty(credentials.SecretAccessKey) || credentials.ExpiresAt <= now)
            throw new ArgumentException("AWS credentials are missing or expired");
        if (string.IsNullOrEmpty(options.Region) || string.IsNullOrEmpty(options.Service) || (options.Region + options.Service).IndexOfAny(['/', '\r', '\n']) >= 0)
            throw new ArgumentException("SigV4 region and service are required scope components");
        var url = request.RequestUri ?? throw new ArgumentException("Request URL required");
        void Header(string key, string? value) { request.Headers.Remove(key); if (value is not null) request.Headers.TryAddWithoutValidation(key, value); }
        Header("Authorization", null);
        Header("X-Amz-Date", now.ToString("yyyyMMdd'T'HHmmss'Z'", CultureInfo.InvariantCulture));
        Header("X-Amz-Security-Token", credentials.SessionToken);
        var bytes = options.UnsignedPayload || request.Content is null ? [] : await request.Content.ReadAsByteArrayAsync(token).ConfigureAwait(false);
        var digest = options.UnsignedPayload ? "UNSIGNED-PAYLOAD" : Hash(bytes);
        Header("X-Amz-Content-Sha256", digest);
        var path = url.GetComponents(UriComponents.Path, UriFormat.UriEscaped);
        path = "/" + path;
        if (!(options.DisableNormalizePath ?? options.Service == "s3"))
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
        if (!(options.DisableDoubleEncoding ?? options.Service == "s3")) path = string.Join("/", path.Split('/').Select(Uri.EscapeDataString));
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
        var canonical = string.Join("\n", request.Method.Method, path, string.Join("&", query), canonicalHeaders, signed, digest);
        var date = now.ToString("yyyyMMdd", CultureInfo.InvariantCulture);
        var scope = $"{date}/{options.Region}/{options.Service}/aws4_request";
        var toSign = "AWS4-HMAC-SHA256\n" + request.Headers.GetValues("X-Amz-Date").Single() + "\n" + scope + "\n" + Hash(Encoding.UTF8.GetBytes(canonical));
        var keyBytes = Encoding.UTF8.GetBytes("AWS4" + credentials.SecretAccessKey);
        foreach (var component in new[] { date, options.Region, options.Service, "aws4_request" }) keyBytes = Hmac(keyBytes, component);
        Header("Authorization", $"AWS4-HMAC-SHA256 Credential={credentials.AccessKeyId}/{scope}, SignedHeaders={signed}, Signature={Convert.ToHexString(Hmac(keyBytes, toSign)).ToLowerInvariant()}");
        token.ThrowIfCancellationRequested();
    }
}
