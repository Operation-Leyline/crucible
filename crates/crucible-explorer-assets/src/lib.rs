//! Internal shared renderer assets, embedded without code-generation dependencies.

/// The static reference page shell. The local runtime has its own hosting shell.
#[doc(hidden)]
pub const INDEX_HTML: &str = include_str!("../runtime/index.html");

/// The renderer shared by generated references and the runtime explorer.
#[doc(hidden)]
pub const EXPLORER_JS: &str = include_str!("../runtime/explorer.js");

/// Styles shared by generated references and the runtime explorer.
#[doc(hidden)]
pub const EXPLORER_CSS: &str = include_str!("../runtime/explorer.css");
