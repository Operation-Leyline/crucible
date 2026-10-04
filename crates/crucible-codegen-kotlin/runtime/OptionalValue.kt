/** Presence is independent of null: unset omits, of(null) writes explicit null. */
data class OptionalValue<T>(val present: Boolean, val value: T?) {
    companion object {
        @JvmStatic fun <T> unset(): OptionalValue<T> = OptionalValue(false, null)
        @JvmStatic fun <T> of(value: T?): OptionalValue<T> = OptionalValue(true, value)
    }
}
