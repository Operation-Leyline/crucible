using System.Globalization;
using System.Security.Cryptography;
using System.Text;

/// <summary>
/// AWS Signature Version 4a: an ECDSA P-256 (SHA-256, DER) signature over a region set, with a private key derived from the
/// secret access key as the AWS SDKs derive it (NIST SP 800-108 HMAC-SHA256 counter KDF). The scope omits the region.
/// </summary>
public static class SigV4aSigner
{
    // n - 2 for P-256: a KDF output above it is rejected and the counter advances.
    private static readonly byte[] Limit = Convert.FromHexString("FFFFFFFF00000000FFFFFFFFFFFFFFFFBCE6FAADA7179E84F3B9CAC2FC63254F");
    private static readonly object Gate = new();
    private static (byte[] Fingerprint, byte[] D)? cache;

    /// <summary>The derived key pair for <paramref name="credentials"/>; the caller disposes it.</summary>
    public static ECDsa DeriveKey(AwsCredentials credentials)
    {
        var key = ECDsa.Create();
        key.ImportParameters(new ECParameters { Curve = ECCurve.NamedCurves.nistP256, D = PrivateKey(credentials) });
        return key;
    }

    private static byte[] PrivateKey(AwsCredentials credentials)
    {
        var fingerprint = SHA256.HashData(Encoding.UTF8.GetBytes(credentials.AccessKeyId + "\0" + credentials.SecretAccessKey));
        lock (Gate) if (cache is { } entry && CryptographicOperations.FixedTimeEquals(entry.Fingerprint, fingerprint)) return entry.D;
        var secret = Encoding.UTF8.GetBytes("AWS4A" + credentials.SecretAccessKey);
        var access = Encoding.UTF8.GetBytes(credentials.AccessKeyId);
        for (int counter = 1; counter <= 254; counter++)
        {
            var input = new List<byte>(48 + access.Length) { 0, 0, 0, 1 };
            input.AddRange("AWS4-ECDSA-P256-SHA256"u8.ToArray());
            input.Add(0);
            input.AddRange(access);
            input.Add((byte)counter);
            input.AddRange([0, 0, 1, 0]);
            var candidate = HMACSHA256.HashData(secret, input.ToArray());
            if (candidate.AsSpan().SequenceCompareTo(Limit) > 0) continue;
            // d = candidate + 1, big-endian; a candidate at most n - 2 cannot overflow.
            int i = candidate.Length - 1;
            while (candidate[i] == 0xff) candidate[i--] = 0;
            candidate[i]++;
            lock (Gate) cache = (fingerprint, candidate);
            return candidate;
        }
        throw new CryptographicException("SigV4a key derivation exhausted its counter");
    }

    public static async ValueTask<SigV4aSigningResult> SignAsync(HttpRequestMessage request, AwsCredentials credentials, SigV4aSigningOptions options, CancellationToken token = default)
    {
        token.ThrowIfCancellationRequested();
        var now = (options.Time ?? DateTimeOffset.UtcNow).ToUniversalTime();
        if (string.IsNullOrEmpty(credentials.AccessKeyId) || string.IsNullOrEmpty(credentials.SecretAccessKey) || credentials.ExpiresAt <= now)
            throw new ArgumentException("AWS credentials are missing or expired");
        if (string.IsNullOrEmpty(options.RegionSet) || string.IsNullOrEmpty(options.Service) || (options.RegionSet + options.Service).IndexOfAny(['/', '\r', '\n']) >= 0)
            throw new ArgumentException("SigV4a region set and service are required scope components");
        var digest = await SigV4Signer.PayloadDigestAsync(request, options.UnsignedPayload, token).ConfigureAwait(false);
        SigV4Signer.SetHeader(request, "Authorization", null);
        SigV4Signer.SetHeader(request, "X-Amz-Content-Sha256", options.SignContentSha256Header ? digest : null);
        var stamp = now.ToString("yyyyMMdd'T'HHmmss'Z'", CultureInfo.InvariantCulture);
        SigV4Signer.SetHeader(request, "X-Amz-Date", stamp);
        SigV4Signer.SetHeader(request, "X-Amz-Region-Set", options.RegionSet);
        SigV4Signer.SetHeader(request, "X-Amz-Security-Token", credentials.SessionToken);
        var (canonical, signed) = SigV4Signer.CanonicalRequest(request, digest, options.Service, options.DisableNormalizePath, options.DisableDoubleEncoding);
        var scope = $"{now.ToString("yyyyMMdd", CultureInfo.InvariantCulture)}/{options.Service}/aws4_request";
        var toSign = "AWS4-ECDSA-P256-SHA256\n" + stamp + "\n" + scope + "\n" + SigV4Signer.Hash(Encoding.UTF8.GetBytes(canonical));
        using var key = DeriveKey(credentials);
        var signature = Convert.ToHexString(key.SignData(Encoding.UTF8.GetBytes(toSign), HashAlgorithmName.SHA256, DSASignatureFormat.Rfc3279DerSequence)).ToLowerInvariant();
        SigV4Signer.SetHeader(request, "Authorization", $"AWS4-ECDSA-P256-SHA256 Credential={credentials.AccessKeyId}/{scope}, SignedHeaders={signed}, Signature={signature}");
        token.ThrowIfCancellationRequested();
        return new(canonical, toSign, signature);
    }
}
