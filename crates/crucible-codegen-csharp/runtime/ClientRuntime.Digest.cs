using System.Text.Json.Nodes;

// HTTP Digest authentication: the challenge a 401 offers and the session that answers it.
internal sealed partial class ClientRuntime
{
    /// <summary>The last HTTP Digest challenge per origin, answered preemptively with increasing nonce counts.</summary>
    private readonly System.Collections.Concurrent.ConcurrentDictionary<string, DigestAuthentication> digestSessions = new();

    /// <summary>The Digest session for the origin of <paramref name="uri"/>: a challenge is scoped to its server.</summary>
    private DigestAuthentication DigestSession(Uri uri) => digestSessions.GetOrAdd(uri.GetLeftPart(UriPartial.Authority), _ => new());

    /// <summary>The strongest Digest challenge a 401 offers, when this request may answer it.</summary>
    private IReadOnlyDictionary<string, string>? DigestChallenge(Prepared prepared, HttpRequestMessage request, Dictionary<string, string> headers)
    {
        if (config.DigestCredentials is null || prepared.Headers.ContainsKey("Authorization") || prepared.Stream is { Replayable: false }) return null;
        if (prepared.Operation["auth"]?.AsArray().Any(id => Text(id) == "smithy.api#httpDigestAuth") != true) return null;
        if (DigestAuthentication.Select(headers.GetValueOrDefault("WWW-Authenticate") ?? "") is not { } challenge) return null;
        // An answer that was already sent is retried only for a stale nonce.
        var answered = request.Headers.TryGetValues("Authorization", out var sent) && sent.Any(value => value.StartsWith("Digest ", StringComparison.OrdinalIgnoreCase));
        return !answered || string.Equals(challenge.GetValueOrDefault("stale"), "true", StringComparison.OrdinalIgnoreCase) ? challenge : null;
    }
}
