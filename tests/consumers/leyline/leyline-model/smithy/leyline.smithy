$version: "2"

namespace example.leyline

use crucible.errors#InternalServerException
use crucible.errors#UnknownOperationException
use crucible.errors#ValidationException
use crucible.protocols#json

/// A graph service in the shape of a real Crucible consumer: bearer
/// authentication with scopes, modelled refusals and validation errors.
@json
@httpBearerAuth
@auth([httpBearerAuth])
service Leyline {
    version: "1"
    operations: [Health, GetGraph, CreateGraph]
    errors: [AccessDeniedException, ValidationException, InternalServerException, UnknownOperationException]
}

/// Liveness probe; public.
@auth([])
@http(method: "GET", uri: "/health", code: 200)
operation Health {
    output: HealthOutput
}

/// Read a graph. Requires the `graphs:read` scope.
@http(method: "GET", uri: "/graphs/{graphId}", code: 200)
operation GetGraph {
    input: GetGraphInput
    output: Graph
    errors: [GraphNotFound]
}

/// Create a graph. Requires the `graphs:write` scope.
@http(method: "POST", uri: "/graphs", code: 201)
operation CreateGraph {
    input: CreateGraphInput
    output: Graph
}

@output
structure HealthOutput {
    @required
    status: String
}

@input
structure GetGraphInput {
    @required
    @httpLabel
    graphId: String
}

@input
structure CreateGraphInput {
    @required
    @length(min: 1, max: 64)
    name: String
}

structure Graph {
    @required
    graphId: String

    @required
    name: String
}

/// The credential is absent, invalid, or does not grant access.
@error("client")
@httpError(401)
structure AccessDeniedException {
    @required
    message: String
}

@error("client")
@httpError(404)
structure GraphNotFound {
    @required
    message: String
}
