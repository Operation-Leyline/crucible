//! The Crucible Git revision the generators were built from, embedded by
//! this crate's build script. It is its own leaf crate so a new commit
//! rebuilds only what records the revision, not every generator.

/// The Git revision (`HEAD`) of the Crucible checkout this was built from.
/// Packaged or vendored builds without source provenance return `None`.
/// A runtime pin uses it even when the checkout has uncommitted changes.
pub fn source_revision() -> Option<&'static str> {
    option_env!("CRUCIBLE_GENERATOR_REVISION")
}

/// Whether tracked files differed from [`source_revision`] when this was built.
pub fn dirty() -> bool {
    option_env!("CRUCIBLE_GENERATOR_DIRTY").is_some()
}

/// The revision that reproduces this build: [`source_revision`] when the
/// checkout was clean, `None` otherwise.
pub fn verified_revision() -> Option<&'static str> {
    source_revision().filter(|_| !dirty())
}

/// The revision as provenance records it: `<rev>`, or `<rev>+dirty` when
/// the checkout had uncommitted changes.
pub fn recorded_revision() -> Option<String> {
    source_revision().map(|revision| {
        if dirty() {
            format!("{revision}+dirty")
        } else {
            revision.to_owned()
        }
    })
}
