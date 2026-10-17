use std::path::{Path, PathBuf};

use crucible_codegen::Generator;

use crate::ModelPackage;

/// API reference settings for a named Markdown and description output.
pub struct DocsOutput<'a> {
    model: &'a ModelPackage,
    name: String,
    audience: crucible_codegen::Audience,
    sdks: Option<PathBuf>,
    schemas: crucible_codegen::Schemas,
    all_releases: bool,
}

impl<'a> DocsOutput<'a> {
    pub(crate) fn new(model: &'a ModelPackage, name: &str) -> Self {
        Self {
            model,
            name: name.into(),
            audience: crucible_codegen::Audience::External,
            sdks: None,
            schemas: crucible_codegen::Schemas::default(),
            all_releases: false,
        }
    }

    /// Include internal items with an Internal badge; the default is external.
    pub fn audience(mut self, audience: crucible_codegen::Audience) -> Self {
        self.audience = audience;
        self
    }

    /// Describe the SDKs a `crucible-sdk.toml` declares in `sdks.json`
    /// (`crucible.sdks/1`) beside `describe.json`: those this reference's
    /// audience may see, named by their generators. A relative path is
    /// relative to the producer's manifest directory; the file's `model`
    /// must be this builder's model. Enable the crate feature of each SDK's
    /// language.
    pub fn sdks(mut self, file: impl AsRef<Path>) -> Self {
        self.sdks = Some(crate::absolute(file.as_ref()));
        self
    }

    /// Where `describe.json` carries each operation's `inputSchema` and
    /// `outputSchema`: `Inline` (the default) gives each its own `$defs`,
    /// `Shared` puts every definition once in a root `schemas` object and
    /// makes each a `$ref` into it, and `Omit` leaves them out. Each mode
    /// is `crucible.describe/1`; a large API's description shrinks by its
    /// repeated definitions.
    pub fn schemas(mut self, schemas: crucible_codegen::Schemas) -> Self {
        self.schemas = schemas;
        self
    }

    /// Document every supported release, not only the model's own: each
    /// release's reference, as `Model::api_version` would select it, in a
    /// directory named for the release
    /// (`crucible_codegen::release_directory`), with an `index.html` and an
    /// `llms.txt` at the root that list them, so a documentation site can
    /// serve them side by side and switch between them. Each reference is
    /// complete on its own (with its own `sdks.json` when [`Self::sdks`]
    /// is set), and an older release's links the current one beside it,
    /// at `../<current>/index.html`, the current release's explorer. A
    /// release whose directory would be a root file (`index.html`,
    /// `llms.txt`) or `dist` or `target` fails the output.
    /// See docs/docs.md, "Every release's reference".
    pub fn all_releases(mut self) -> Self {
        self.all_releases = true;
        self
    }

    /// Generate and publish the named output under OUT_DIR.
    pub fn generate(self) -> crate::Artifact {
        self.try_generate()
            .unwrap_or_else(|message| panic!("crucible: {message}"))
    }

    /// Generate without panicking on invalid settings or unsupported semantics.
    pub fn try_generate(self) -> Result<crate::Artifact, String> {
        self.output().try_generate()
    }

    /// Generate under OUT_DIR and export relative to the producer's manifest.
    pub fn generate_into(self, output: impl AsRef<Path>) -> crate::Artifact {
        self.try_generate_into(output)
            .unwrap_or_else(|message| panic!("crucible: {message}"))
    }

    /// Fallible equivalent of named output generation and export.
    pub fn try_generate_into(self, output: impl AsRef<Path>) -> Result<crate::Artifact, String> {
        self.output().try_generate_into(output)
    }

    fn output(self) -> crate::Output<'a> {
        let mut output = self
            .model
            .output(&self.name, Generator::Docs)
            .audience(self.audience);
        output.sdks = self.sdks;
        output.docs.schemas = self.schemas;
        output.all_releases = self.all_releases;
        output
    }
}

/// `sdks.json` for a reference of `service`, `model`'s model or a line of
/// it, at `audience`, from the SDK file at `path`, which must declare
/// `model`'s SDKs.
pub(crate) fn sdks_file(
    path: &Path,
    model: &ModelPackage,
    service: &crucible_ir::ir::Service,
    audience: crucible_codegen::Audience,
) -> Result<crucible_codegen::File, String> {
    println!("cargo::rerun-if-changed={}", path.display());
    // The environment is never read: an endpoint's variable is named.
    let file = crucible_codegen::sdk::load(path).map_err(|errors| errors.to_string())?;
    let declared = file.join(&file.model);
    let same = match (declared.canonicalize(), model.source.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if !same {
        return Err(format!(
            "{} declares SDKs for the model {}, not {}",
            path.display(),
            declared.display(),
            model.source.display()
        ));
    }
    crucible_codegen::sdks::file(&file, service, audience)
}
