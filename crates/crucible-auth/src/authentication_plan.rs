use crucible_server_runtime_rust::operation::AuthScheme;

pub(crate) struct AuthenticationPlan {
    pub attempts: Vec<(usize, &'static AuthScheme)>,
    pub optional: bool,
    pub bearer: bool,
}
