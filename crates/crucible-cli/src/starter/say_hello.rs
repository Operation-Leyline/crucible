use crate::hello_service::{SayHelloInput, SayHelloOperation, SayHelloOutput};
use crate::operations::Service;
use crucible_server_runtime_rust::Result;

impl SayHelloOperation for Service {
    async fn say_hello(&self, input: SayHelloInput) -> Result<SayHelloOutput> {
        Ok(SayHelloOutput {
            message: format!("Hello, {}!", input.name),
        })
    }
}
