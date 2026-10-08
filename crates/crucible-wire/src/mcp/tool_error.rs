/// A modeled error visible in the External audience.
#[derive(Debug)]
pub struct ToolError {
    pub name: &'static str,
    /// Wire spelling of a public `message` member; absent messages stay private.
    pub message_member: Option<&'static str>,
}
