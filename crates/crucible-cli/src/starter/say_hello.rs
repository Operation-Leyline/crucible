use crate::contract::{
    SayHelloOperation,
    model::{SayHelloInput, SayHelloOutput},
};
use crate::operations::Service;

impl SayHelloOperation for Service {
    async fn say_hello(&self, input: SayHelloInput) -> crucible::Result<SayHelloOutput> {
        Ok(SayHelloOutput {
            message: format!("Hello, {}!", input.name),
        })
    }
}
