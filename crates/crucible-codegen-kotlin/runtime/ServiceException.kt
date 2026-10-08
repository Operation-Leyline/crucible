/**
 * Base type of every error response a service call raises. Modeled errors extend it, and an error
 * the model does not describe is thrown as this class itself, so catching it handles all of a
 * service's failures; [NetworkError] signals that no response arrived.
 */
open class ServiceException : ServiceError()
