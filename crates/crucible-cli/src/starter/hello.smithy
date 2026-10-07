$version: "2"
namespace example.hello

use crucible.protocols#json
use crucible.errors#ValidationException
use crucible.errors#InternalServerException
use crucible.errors#UnknownOperationException

@json
service HelloService {
    version: "1"
    operations: [SayHello]
    errors: [ValidationException, InternalServerException, UnknownOperationException]
}

/// Return a greeting for the supplied name.
@http(method: "POST", uri: "/hello", code: 200)
operation SayHello {
    input: SayHelloInput
    output: SayHelloOutput
}

@input
structure SayHelloInput {
    @required
    @length(min: 1)
    name: String
}

@output
structure SayHelloOutput {
    @required
    message: String
}
