/** HTTP Basic identity; diagnostic strings never contain its password. */
class BasicCredentials(val username: String, val password: String) {
    init { require(':' !in username) { "Basic username cannot contain a colon" } }
    override fun toString(): String = "BasicCredentials[redacted]"
}
