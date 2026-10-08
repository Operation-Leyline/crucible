/**
 * HTTP Basic identity; diagnostic strings never contain its password.
 *
 * @property username the user name; it cannot contain a colon
 * @property password the password
 */
class BasicCredentials(val username: String, val password: String) {
    init { require(':' !in username) { "Basic username cannot contain a colon" } }
    override fun toString(): String = "BasicCredentials[redacted]"
}
