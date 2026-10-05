using System.Security.Cryptography;
using System.Text;
using System.Text.RegularExpressions;

/// <summary>
/// RFC 7616 HTTP Digest authentication. A client keeps the last accepted challenge and answers later requests
/// preemptively with an increasing nonce count; a 401 carrying a new (or stale) challenge replaces it. SHA-512-256,
/// SHA-256 and MD5 are supported, each also as <c>-sess</c>, with qop <c>auth</c> (preferred) or <c>auth-int</c>, and userhash.
/// </summary>
public sealed class DigestAuthentication
{
    // Strongest first.
    private static readonly string[] Algorithms = ["SHA-512-256", "SHA-256", "MD5"];
    private static readonly Regex Parameter = new(@"\G[\s,]*([!#$%&'*+.^_`|~0-9A-Za-z-]+)(?:[ \t]*=[ \t]*(""(?:[^""\\]|\\.)*""|[^,\s]*))?",
        RegexOptions.CultureInvariant);
    private static readonly Regex Escape = new(@"\\(.)", RegexOptions.CultureInvariant | RegexOptions.Singleline);
    private readonly object gate = new();
    private IReadOnlyDictionary<string, string>? challenge;
    private int count;

    /// <summary>Every challenge in a WWW-Authenticate value: its lowercase scheme and its parameters (lowercase names).</summary>
    public static IReadOnlyList<(string Scheme, IReadOnlyDictionary<string, string> Parameters)> Challenges(string header)
    {
        var result = new List<(string Scheme, IReadOnlyDictionary<string, string> Parameters)>();
        Dictionary<string, string>? current = null;
        for (var match = Parameter.Match(header); match.Success && match.Length != 0; match = Parameter.Match(header, match.Index + match.Length))
        {
            if (!match.Groups[2].Success)
            {
                current = new Dictionary<string, string>(StringComparer.Ordinal);
                result.Add((match.Groups[1].Value.ToLowerInvariant(), current));
                continue;
            }
            if (current is null) continue;
            var value = match.Groups[2].Value;
            if (value.StartsWith('"')) value = Escape.Replace(value[1..^1], "$1");
            current[match.Groups[1].Value.ToLowerInvariant()] = value;
        }
        return result;
    }

    /// <summary>The strongest supported Digest challenge in a WWW-Authenticate value, or null.</summary>
    public static IReadOnlyDictionary<string, string>? Select(string header)
    {
        IReadOnlyDictionary<string, string>? best = null; int rank = int.MaxValue;
        foreach (var (scheme, parameters) in Challenges(header))
        {
            if (scheme != "digest" || !parameters.ContainsKey("realm") || !parameters.ContainsKey("nonce")) continue;
            var position = Array.IndexOf(Algorithms, Base(parameters.GetValueOrDefault("algorithm", "MD5").ToUpperInvariant()));
            if (position < 0) continue;
            if (parameters.ContainsKey("qop") && !Qops(parameters).Any(qop => qop is "auth" or "auth-int")) continue;
            if (position < rank) { best = parameters; rank = position; }
        }
        return best;
    }

    private static string Base(string algorithm) => algorithm.EndsWith("-SESS", StringComparison.Ordinal) ? algorithm[..^5] : algorithm;
    private static List<string> Qops(IReadOnlyDictionary<string, string> parameters) =>
        parameters.GetValueOrDefault("qop", "").Split(',').Select(qop => qop.Trim().ToLowerInvariant()).Where(qop => qop.Length != 0).ToList();

    internal void Accept(IReadOnlyDictionary<string, string> accepted) { lock (gate) { challenge = accepted; count = 0; } }
    internal bool Ready { get { lock (gate) return challenge is not null; } }

    /// <summary>Answers the accepted challenge; a null body (a stream) cannot use auth-int.</summary>
    internal string Authorization(BasicCredentials credentials, string method, Uri url, byte[]? body)
    {
        IReadOnlyDictionary<string, string> accepted; int nc;
        lock (gate)
        {
            accepted = challenge ?? throw new InvalidOperationException("No digest challenge");
            nc = ++count;
        }
        return Header(accepted, credentials.Username, credentials.Password, method, url.PathAndQuery, body,
            nc.ToString("x8", System.Globalization.CultureInfo.InvariantCulture), Convert.ToHexString(RandomNumberGenerator.GetBytes(16)).ToLowerInvariant());
    }

    /// <summary>The Authorization value for one challenge, nonce count and client nonce.</summary>
    public static string Header(IReadOnlyDictionary<string, string> challenge, string username, string password, string method, string uri,
        byte[]? body, string nc, string cnonce)
    {
        var algorithm = challenge.GetValueOrDefault("algorithm", "MD5").ToUpperInvariant();
        var session = algorithm.EndsWith("-SESS", StringComparison.Ordinal);
        Func<byte[], byte[]> function = Base(algorithm) switch
        {
            "SHA-512-256" => Sha512_256.HashData,
            "SHA-256" => SHA256.HashData,
            "MD5" => MD5.HashData,
            _ => throw new ArgumentException("Unsupported digest algorithm"),
        };
        string Hash(byte[] data) => Convert.ToHexString(function(data)).ToLowerInvariant();
        string H(string text) => Hash(Encoding.UTF8.GetBytes(text));
        var realm = challenge["realm"]; var nonce = challenge["nonce"];
        var qops = Qops(challenge);
        string? qop = qops.Contains("auth") ? "auth" : qops.Contains("auth-int") ? "auth-int" : null;
        if (qop is null && qops.Count != 0) throw new ArgumentException("Unsupported digest qop");
        if (qop == "auth-int" && body is null) throw new ArgumentException("Digest auth-int cannot cover a streamed body");
        var a1 = H($"{username}:{realm}:{password}");
        if (session) a1 = H($"{a1}:{nonce}:{cnonce}");
        var a2 = H(qop == "auth-int" ? $"{method}:{uri}:{Hash(body!)}" : $"{method}:{uri}");
        var response = qop is null ? H($"{a1}:{nonce}:{a2}") : H($"{a1}:{nonce}:{nc}:{cnonce}:{qop}:{a2}");
        var userhash = string.Equals(challenge.GetValueOrDefault("userhash"), "true", StringComparison.OrdinalIgnoreCase);
        static string Quote(string value) => "\"" + value.Replace("\\", "\\\\", StringComparison.Ordinal).Replace("\"", "\\\"", StringComparison.Ordinal) + "\"";
        string user;
        if (userhash) user = "username=" + Quote(H($"{username}:{realm}"));
        // A name that cannot be a quoted ASCII string travels as an RFC 8187 extended value.
        else if (username.Any(c => c < 0x20 || c > 0x7e || c is '"' or '\\')) user = "username*=UTF-8''" + Uri.EscapeDataString(username);
        else user = "username=" + Quote(username);
        var fields = new List<string> { user, "realm=" + Quote(realm), "uri=" + Quote(uri), "algorithm=" + challenge.GetValueOrDefault("algorithm", "MD5"), "nonce=" + Quote(nonce) };
        if (qop is not null) fields.AddRange([$"nc={nc}", "cnonce=" + Quote(cnonce), $"qop={qop}"]);
        fields.Add("response=" + Quote(response));
        if (challenge.TryGetValue("opaque", out var opaque)) fields.Add("opaque=" + Quote(opaque));
        if (userhash) fields.Add("userhash=true");
        return "Digest " + string.Join(", ", fields);
    }
}
