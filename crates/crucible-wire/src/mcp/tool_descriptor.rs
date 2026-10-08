/// Generated once from the External audience of the lowered Smithy IR.
#[derive(Debug)]
pub struct ToolDescriptor {
    pub name: &'static str,
    pub operation: &'static str,
    pub description: &'static str,
    pub input_schema: &'static str,
    pub output_schema: Option<&'static str>,
    pub read_only: bool,
    pub idempotent: bool,
    pub destructive: bool,
    /// Only errors surviving External audience projection may be disclosed.
    pub errors: &'static [super::ToolError],
}
