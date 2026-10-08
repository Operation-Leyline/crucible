import kotlinx.serialization.json.JsonElement

/**
 * A terminal waiter failure, retaining the last operation result for diagnosis.
 *
 * @property waiterName the waiter's modeled name
 * @property attempts how many polls ran before the waiter gave up
 * @property response the last operation output, or null when the last attempt raised an error
 * @property lastError the last error, or null
 * @property waiterMessage the message the failure acceptor extracted, if any
 */
open class WaiterError(val waiterName: String, val attempts: Int, val response: Any?,
    val lastError: Throwable?, val waiterMessage: JsonElement? = null) :
    Exception("Waiter $waiterName failed after $attempts attempts" + (waiterMessage?.let { ": $it" } ?: ""), lastError)

/** The overall polling deadline expired, including operation and retry time. */
class WaiterTimeout(waiterName: String, attempts: Int, response: Any?, lastError: Throwable?) :
    WaiterError(waiterName, attempts, response, lastError)
