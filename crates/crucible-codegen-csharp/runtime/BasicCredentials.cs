/// <summary>An HTTP Basic identity. Diagnostic strings never contain the password.</summary>
/// <param name="Username">The user name; it cannot contain a colon.</param>
/// <param name="Password">The password.</param>
public sealed record BasicCredentials(string Username, string Password)
{
    /// <inheritdoc/>
    public override string ToString() => "BasicCredentials { redacted }";
}
