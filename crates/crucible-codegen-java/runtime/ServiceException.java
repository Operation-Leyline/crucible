/**
 * Base type of every error response a service call raises. Modeled errors extend it, and an error
 * the model does not describe is thrown as this class itself, so catching it handles all of a
 * service failures; {@link NetworkError} signals that no response arrived.
 */
public class ServiceException extends ServiceError {
    private static final long serialVersionUID = 1L;

    /** Creates an uninitialized exception; the client fills it in before throwing. */
    public ServiceException() {}
}
