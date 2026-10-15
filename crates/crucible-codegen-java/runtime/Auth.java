import com.google.gson.JsonObject;

/** Bearer authentication, the one scheme the wire contract carries. */
final class Auth {
  private Auth() {}

  /** Adds the bearer token to {@code request}; returns whether credentials were attached. */
  static boolean apply(ClientConfig config, JsonObject op, ClientConfig.Request request) {
    if (!op.has("bearer") || !op.get("bearer").getAsBoolean() || config.token == null) return false;
    String token = config.token.get();
    if (token == null) return false;
    request.headers.keySet().removeIf(key -> key.equalsIgnoreCase("authorization"));
    request.headers.put("authorization", "Bearer " + token);
    return true;
  }
}
