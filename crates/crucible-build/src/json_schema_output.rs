use crucible_codegen::{Generator, JsonSchemaOptions};

use crate::ModelPackage;

/// JSON Schema document settings for a named output directory.
pub struct JsonSchemaOutput<'a> {
    model: &'a ModelPackage,
    name: String,
    audience: crucible_codegen::Audience,
    options: JsonSchemaOptions,
}

impl<'a> JsonSchemaOutput<'a> {
    /// Select internal API explicitly; the default is external.
    pub fn audience(mut self, audience: crucible_codegen::Audience) -> Self {
        self.audience = audience;
        self
    }

    pub(crate) fn new(model: &'a ModelPackage, name: &str) -> Self {
        Self {
            model,
            name: name.into(),
            audience: crucible_codegen::Audience::External,
            options: JsonSchemaOptions::default(),
        }
    }

    /// Replace the native document generator options.
    pub fn options(mut self, options: JsonSchemaOptions) -> Self {
        self.options = options;
        self
    }

    /// Select a fully qualified shape id or an unambiguous shape name.
    pub fn shape(mut self, shape: &str) -> Self {
        self.options.shape = shape.into();
        self
    }

    pub(crate) fn prepare(self) -> Result<(&'a ModelPackage, String, Generator), String> {
        Ok((self.model, self.name, Generator::JsonSchema(self.options)))
    }

    crate::output_builder::output_generation_methods!();
}
