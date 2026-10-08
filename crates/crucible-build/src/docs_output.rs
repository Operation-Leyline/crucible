use crucible_codegen::Generator;

use crate::ModelPackage;

/// API reference settings for a named Markdown and description output.
pub struct DocsOutput<'a> {
    model: &'a ModelPackage,
    name: String,
    audience: crucible_codegen::Audience,
}

impl<'a> DocsOutput<'a> {
    pub(crate) fn new(model: &'a ModelPackage, name: &str) -> Self {
        Self {
            model,
            name: name.into(),
            audience: crucible_codegen::Audience::External,
        }
    }

    /// Include internal items with an Internal badge; the default is external.
    pub fn audience(mut self, audience: crucible_codegen::Audience) -> Self {
        self.audience = audience;
        self
    }

    pub(crate) fn prepare(self) -> Result<(&'a ModelPackage, String, Generator), String> {
        Ok((self.model, self.name, Generator::Docs))
    }

    crate::output_builder::output_generation_methods!();
}
