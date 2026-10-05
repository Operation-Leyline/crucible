/** Presence is independent of null: unset omits, of(null) writes explicit null. */
internal data class OptionalValue<T>(val present: Boolean, val value: T?) {
    companion object {
        fun <T> unset(): OptionalValue<T> = OptionalValue(false, null)
        fun <T> of(value: T?): OptionalValue<T> = OptionalValue(true, value)
    }
}
