/// A parsed request ready to pass to the generated SDK. A service wrapper
/// can customize `config` with credential providers or interceptors.
pub struct Invocation {
    pub operation: String,
    pub input: serde_json::Value,
    pub config: crucible_client::Config,
    /// Whether to retrieve every page of a paginated operation.
    pub paginate: bool,
    pub output: crate::Output,
}
