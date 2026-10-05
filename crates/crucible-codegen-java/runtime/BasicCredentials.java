/** An HTTP Basic identity. Passwords are excluded from diagnostic strings. */
public record BasicCredentials(String username, String password) {
  public BasicCredentials {
    java.util.Objects.requireNonNull(username); java.util.Objects.requireNonNull(password);
    if (username.indexOf(':') >= 0) throw new IllegalArgumentException("Basic username cannot contain a colon");
  }
  @Override public String toString() { return "BasicCredentials[redacted]"; }
}
