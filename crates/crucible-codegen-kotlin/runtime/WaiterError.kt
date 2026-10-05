import kotlinx.serialization.json.JsonElement

open class WaiterError(val waiterName: String, val attempts: Int, val response: Any?,
    val lastError: Throwable?, val waiterMessage: JsonElement? = null) :
    Exception("Waiter $waiterName failed after $attempts attempts" + (waiterMessage?.let { ": $it" } ?: ""), lastError)

class WaiterTimeout(waiterName: String, attempts: Int, response: Any?, lastError: Throwable?) :
    WaiterError(waiterName, attempts, response, lastError)
