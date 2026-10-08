/**
 * Renders generated model types as `TypeName(member=value, ...)`, the way the AWS SDKs do.
 *
 * Unset optional members are omitted, an explicit `null` prints as `null`, and collections render
 * their elements recursively. Members that are `@sensitive` are never printed: generated code
 * routes them through [redact].
 */
internal object ModelFormat {
    /** Printed in place of the value of a `@sensitive` member. */
    const val REDACTED: String = "*** Sensitive Data Redacted ***"

    /**
     * Formats a model from its members, in declaration order.
     *
     * @param type the model's type name
     * @param members member name to value pairs; an unset [OptionalValue] is skipped
     * @return `type(name=value, ...)`
     */
    fun format(type: String, vararg members: Pair<String, Any?>): String {
        val out = StringBuilder(type).append('(')
        var first = true
        for ((name, raw) in members) {
            var value = raw
            if (value is OptionalValue<*>) {
                if (!value.present) continue
                value = value.value
            }
            if (!first) out.append(", ")
            first = false
            out.append(name).append('=').append(render(value))
        }
        return out.append(')').toString()
    }

    /**
     * Hides a sensitive member's value while preserving whether it is set.
     *
     * @param value the member's stored value
     * @return an unset [OptionalValue] as is, otherwise the redaction marker
     */
    fun redact(value: Any?): Any? = if (value is OptionalValue<*> && !value.present) value else REDACTED

    private fun render(value: Any?): String = when (value) {
        is ByteArray -> "ByteArray(${value.size})"
        is Map<*, *> -> value.entries.joinToString(", ", "{", "}") { render(it.key) + "=" + render(it.value) }
        is Iterable<*> -> value.joinToString(", ", "[", "]") { render(it) }
        else -> value.toString()
    }
}
