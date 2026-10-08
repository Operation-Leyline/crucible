import kotlin.time.Duration
import kotlin.time.Duration.Companion.minutes

/**
 * Bounds one waiter call. The deadline includes requests and sleeps.
 *
 * @property timeout overall polling deadline
 * @property minDelay overrides the modeled minimum delay between polls; null keeps the model's
 * @property maxDelay overrides the modeled maximum delay between polls; null keeps the model's
 */
data class WaiterOptions(val timeout: Duration = 5.minutes, val minDelay: Duration? = null, val maxDelay: Duration? = null)
