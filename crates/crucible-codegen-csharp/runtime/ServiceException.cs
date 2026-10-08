/// <summary>
/// Base type of every error response a service call raises. Modeled errors derive from it, and an error
/// the model does not describe is thrown as this class itself, so catching it handles all of a service's
/// failures; <see cref="NetworkError"/> signals that no response arrived.
/// </summary>
public class ServiceException : ServiceError {}
