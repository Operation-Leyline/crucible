import kotlin.time.Duration
import kotlin.time.Duration.Companion.minutes

/** Null delays use the modeled waiter bounds. The deadline includes requests and sleeps. */
data class WaiterOptions(val timeout: Duration = 5.minutes, val minDelay: Duration? = null, val maxDelay: Duration? = null)
