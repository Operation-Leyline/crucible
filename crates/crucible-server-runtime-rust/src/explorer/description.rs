/// Build-time describe/1 contract and its script wrapper. Constructed by
/// generated bindings; serving it performs no model parsing or serialization.
#[derive(Debug)]
#[non_exhaustive]
pub struct Description {
    pub(crate) json: &'static str,
    pub(crate) script: &'static str,
}

pub const fn description(json: &'static str, script: &'static str) -> Description {
    Description { json, script }
}
