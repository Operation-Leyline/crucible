use crucible_client::operation::Unit;
use crucible_client::{CallOptions, Config, WaiterError, WaiterOptions, WaiterResult};
use std::time::Duration;

mod sdk {
    include!(concat!(env!("OUT_DIR"), "/keyword_client.rs"));
}

#[test]
fn generated_keyword_methods_and_unit_waiter_are_usable_by_a_consumer() {
    fn unit_waiter(
        _: impl Future<
            Output = Result<WaiterResult<Unit, sdk::TypeError>, WaiterError<Unit, sdk::TypeError>>,
        >,
    ) {
    }
    let client = sdk::KeywordsClient::new(Config::new("http://localhost:1"));
    drop(client.r#type());
    drop(client.type_with_options_2(CallOptions::default()));
    drop(client.type_with_options());
    drop(client.wait_until_match());
    unit_waiter(client.wait_until_match_2(WaiterOptions::new(Duration::from_secs(1))));
}
