import kotlinx.serialization.json.JsonObject

/** Adds the bearer token to an operation that requires one; true when it was applied. */
internal suspend fun applyAuth(config: ClientConfig, op: JsonObject, request: Request): Boolean {
    if (!op.flag("bearer")) return false
    val token = config.token?.invoke() ?: return false
    request.headers.keys.filter { it.equals("authorization", ignoreCase = true) }.forEach { request.headers.remove(it) }
    request.headers["authorization"] = "Bearer $token"
    return true
}
