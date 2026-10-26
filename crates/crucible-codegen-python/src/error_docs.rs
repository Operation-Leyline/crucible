//! Packages without `errorDocs` retain the existing runtime surface.
use crucible_ir::ir::Service;

pub(crate) fn runtime(service: &Service) -> String {
    let enabled = service.error_docs_base.is_some();
    let mut source = String::new();
    let mut include = true;
    for line in include_str!("../runtime/runtime.py").lines() {
        match line.trim() {
            "# BEGIN ERROR DOCUMENTATION" => include = enabled,
            "# BEGIN LEGACY ERROR MEMBERS" => include = !enabled,
            "# END ERROR DOCUMENTATION" | "# END LEGACY ERROR MEMBERS" => include = true,
            _ if include => {
                source.push_str(line);
                source.push('\n');
            }
            _ => {}
        }
    }
    source
}
