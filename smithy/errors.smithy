$version: "2"

namespace crucible.errors

/// No operation matches the request's method and path.
@error("client")
@httpError(404)
structure UnknownOperationException {
    @required
    message: String
}

/// A request cannot be parsed or does not satisfy the service's input model.
@error("client")
@httpError(400)
structure ValidationException {
    @required
    message: String

    @required
    reason: ValidationExceptionReason

    fieldList: ValidationExceptionFieldList
}

/// A machine-readable reason; clients never need to parse message text.
enum ValidationExceptionReason {
    UNKNOWN_OPERATION = "unknownOperation"
    CANNOT_PARSE = "cannotParse"
    FIELD_VALIDATION_FAILED = "fieldValidationFailed"
    OTHER = "other"
}

list ValidationExceptionFieldList {
    member: ValidationExceptionField
}

/// A request field that failed validation.
structure ValidationExceptionField {
    @required
    name: String

    @required
    message: String
}

/// An unexpected server failure. Its message never exposes the underlying cause.
@error("server")
@httpError(500)
@retryable
structure InternalServerException {
    @required
    message: String

    @httpHeader("Retry-After")
    retryAfterSeconds: Integer
}

// Retained for existing service contracts. New services use the exception
// shapes above; changing an existing error's name or status breaks clients.

/// A request cannot be parsed or does not satisfy the service's input model.
@error("client")
@httpError(400)
structure ValidationError {
    @required
    message: String
}

/// An unexpected server failure. Its message never exposes the underlying cause.
@error("server")
@httpError(500)
structure InternalError {
    @required
    message: String
}
