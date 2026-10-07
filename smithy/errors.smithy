$version: "2"

namespace crucible.errors

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
