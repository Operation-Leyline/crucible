$version: "2"
namespace example.hello

use crucible.protocols#json
use crucible.errors#ValidationError
use crucible.errors#InternalError

@json
service HelloService {
    version: "1"
    operations: [SayHello]
    errors: [ValidationError, InternalError]
}

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
