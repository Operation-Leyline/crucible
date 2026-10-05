import kotlin.time.Instant
import kotlinx.datetime.UtcOffset
import kotlinx.datetime.format.DateTimeComponents
import kotlinx.datetime.format.format
import kotlinx.datetime.format.char
import kotlinx.datetime.format.DayOfWeekNames
import kotlinx.datetime.format.MonthNames

/** Decimal lexemes stay decimal until the requested integer width or nanosecond scale is checked. */
internal class WireDecimal(text: String) {
    private val negative: Boolean
    private val digits: String
    private val scale: Int
    init {
        val match = Regex("(-?)([0-9]+)(?:\\.([0-9]+))?(?:[eE]([+-]?[0-9]+))?").matchEntire(text)
            ?: throw IllegalArgumentException("Expected decimal number")
        negative = match.groupValues[1].isNotEmpty()
        val fraction = match.groupValues[3]
        val exponent = match.groupValues[4].ifEmpty { "0" }.toIntOrNull()
            ?: throw IllegalArgumentException("Decimal exponent outside range")
        require(exponent in -100000..100000) { "Decimal exponent outside range" }
        digits = (match.groupValues[2] + fraction).trimStart('0').ifEmpty { "0" }
        scale = fraction.length - exponent
    }
    private fun parts(): Pair<String, String> {
        if (digits == "0") return "0" to ""
        if (scale <= 0) {
            return digits + "0".repeat(-scale) to ""
        }
        val whole = if (scale < digits.length) digits.dropLast(scale) else "0"
        val fraction = if (scale < digits.length) digits.takeLast(scale) else "0".repeat(scale - digits.length) + digits
        return whole to fraction.trimEnd('0')
    }
    fun integerText(): String {
        val (whole, fraction) = parts()
        require(fraction.isEmpty()) { "Expected integral number" }
        return (if (negative && whole != "0") "-" else "") + whole
    }
    fun longExact(): Long {
        val (whole, fraction) = parts()
        require(fraction.isEmpty()) { "Expected integral number" }
        return ((if (negative) "-" else "") + whole).toLongOrNull()
            ?: throw IllegalArgumentException("Integer outside signed 64-bit range")
    }
    fun instant(): Instant {
        val (whole, fraction) = parts()
        require(fraction.length <= 9) { "Timestamp exceeds nanosecond precision" }
        val sign = if (negative) -1 else 1
        val seconds = ((if (negative) "-" else "") + whole).toLongOrNull()
            ?: throw IllegalArgumentException("Timestamp outside range")
        val nanos = fraction.padEnd(9, '0').toInt() * sign
        require(nanos >= 0 || seconds > Long.MIN_VALUE) { "Timestamp outside range" }
        val result = Instant.fromEpochSeconds(seconds, nanos.toLong())
        require(result.epochSeconds == seconds - (if (nanos < 0) 1 else 0) &&
            result.nanosecondsOfSecond == (if (nanos < 0) 1_000_000_000 + nanos else nanos)) { "Timestamp outside range" }
        return result
    }
}

internal fun epochText(value: Instant): String {
    val seconds = value.epochSeconds
    val nanos = value.nanosecondsOfSecond
    if (nanos == 0) return seconds.toString()
    val whole = if (seconds < 0) "-" + (-(seconds + 1)).toString() else seconds.toString()
    val fraction = if (seconds < 0) 1_000_000_000 - nanos else nanos
    return whole + "." + fraction.toString().padStart(9, '0').trimEnd('0')
}
internal fun parseHttpDate(value: String): Instant = DateTimeComponents.Formats.RFC_1123.parse(value).toInstantUsingOffset()
private val httpDateFormat = DateTimeComponents.Format {
    dayOfWeek(DayOfWeekNames.ENGLISH_ABBREVIATED); chars(", "); day(); char(' ')
    monthName(MonthNames.ENGLISH_ABBREVIATED); char(' '); year(); char(' ')
    hour(); char(':'); minute(); char(':'); second(); chars(" GMT")
}
internal fun httpDate(value: Instant): String = httpDateFormat.format { setDateTimeOffset(value, UtcOffset.ZERO) }
