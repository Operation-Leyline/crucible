/// <summary>Explicit credential sources for <see cref="ClientConfig.Credentials"/>: no ambient identity lookup happens unless you configure one.</summary>
public static class CredentialProviders
{
    /// <summary>Tries each provider in order and returns the first non-null result.</summary>
    /// <param name="providers">The sources, in priority order.</param>
    /// <returns>A provider yielding null when none has credentials.</returns>
    public static Func<CancellationToken, ValueTask<AwsCredentials?>> Chain(params Func<CancellationToken, ValueTask<AwsCredentials?>>[] providers)
    {
        var snapshot = providers.ToArray();
        return async token => {
            foreach (var provider in snapshot)
            {
                token.ThrowIfCancellationRequested();
                var value = await provider(token).AsTask().WaitAsync(token).ConfigureAwait(false);
                if (value is not null) return value;
            }
            return null;
        };
    }
    private static AwsCredentials? Create(string? access, string? secret, string? session)
    {
        if (string.IsNullOrEmpty(access) && string.IsNullOrEmpty(secret)) return null;
        if (string.IsNullOrEmpty(access) || string.IsNullOrEmpty(secret)) throw new InvalidOperationException("AWS credentials are incomplete");
        return new(access, secret, session);
    }
    /// <summary>Reads <c>AWS_ACCESS_KEY_ID</c>, <c>AWS_SECRET_ACCESS_KEY</c> and <c>AWS_SESSION_TOKEN</c>.</summary>
    /// <param name="token">Cancels the read.</param>
    /// <returns>The credentials, or null when neither key nor secret is set.</returns>
    /// <exception cref="InvalidOperationException">Only one of key and secret is set.</exception>
    public static ValueTask<AwsCredentials?> Environment(CancellationToken token)
    {
        token.ThrowIfCancellationRequested();
        return ValueTask.FromResult(Create(System.Environment.GetEnvironmentVariable("AWS_ACCESS_KEY_ID"),
            System.Environment.GetEnvironmentVariable("AWS_SECRET_ACCESS_KEY"), System.Environment.GetEnvironmentVariable("AWS_SESSION_TOKEN")));
    }
    /// <summary>Reads a profile of an AWS credentials file on each call.</summary>
    /// <param name="profile">The section name; <c>AWS_PROFILE</c> or <c>default</c> when null.</param>
    /// <param name="path">The credentials file; <c>AWS_SHARED_CREDENTIALS_FILE</c> or <c>~/.aws/credentials</c> when null.</param>
    /// <returns>A provider yielding null when the file or profile is missing.</returns>
    public static Func<CancellationToken, ValueTask<AwsCredentials?>> Profile(string? profile = null, string? path = null) => async token =>
    {
        var name = profile ?? System.Environment.GetEnvironmentVariable("AWS_PROFILE") ?? "default";
        var source = path ?? System.Environment.GetEnvironmentVariable("AWS_SHARED_CREDENTIALS_FILE") ??
            Path.Combine(System.Environment.GetFolderPath(System.Environment.SpecialFolder.UserProfile), ".aws", "credentials");
        token.ThrowIfCancellationRequested();
        string[] lines;
        try { lines = await File.ReadAllLinesAsync(source, token).ConfigureAwait(false); }
        catch (FileNotFoundException) { return null; }
        catch (DirectoryNotFoundException) { return null; }
        var section = ""; var values = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var raw in lines)
        {
            var line = raw.Trim();
            if (line.Length == 0 || line.StartsWith('#') || line.StartsWith(';')) continue;
            if (line.StartsWith('[') && line.EndsWith(']')) { section = line[1..^1].Trim(); continue; }
            if (section != name) continue;
            var parts = line.Split('=', 2);
            if (parts.Length != 2 || !values.TryAdd(parts[0].Trim(), parts[1].Trim())) throw new FormatException("Invalid or duplicate credentials profile entry");
        }
        return Create(values.GetValueOrDefault("aws_access_key_id"), values.GetValueOrDefault("aws_secret_access_key"), values.GetValueOrDefault("aws_session_token"));
    };
}
