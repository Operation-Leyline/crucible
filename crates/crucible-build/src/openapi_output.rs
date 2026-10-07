use crucible_codegen::{Generator, OpenApiOptions};

use crate::ModelPackage;

/// OpenAPI document settings for a named output directory.
pub struct OpenApiOutput<'a> {
    model: &'a ModelPackage,
    name: String,
    options: OpenApiOptions,
}

impl<'a> OpenApiOutput<'a> {
    pub(crate) fn new(model: &'a ModelPackage, name: &str) -> Self {
        Self {
            model,
            name: name.into(),
            options: OpenApiOptions::default(),
        }
    }

    /// Replace the native document generator options.
    pub fn options(mut self, options: OpenApiOptions) -> Self {
        self.options = options;
        self
    }

    /// Deployment URL, including any base path, embedded in the document.
    pub fn endpoint(mut self, endpoint: &str) -> Self {
        self.options.endpoint = Some(endpoint.into());
        self
    }

    pub(crate) fn prepare(self) -> Result<(&'a ModelPackage, String, Generator), String> {
        Ok((self.model, self.name, Generator::OpenApi(self.options)))
    }

    crate::output_builder::output_generation_methods!();
}
