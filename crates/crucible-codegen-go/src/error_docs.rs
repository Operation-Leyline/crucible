//! Packages without `errorDocs` retain the existing runtime surface.
use crucible_ir::ir::Service;

pub(crate) fn runtime(service: &Service) -> String {
    let enabled = service.error_docs_base.is_some();
    let mut source = String::new();
    let mut documentation = false;
    for line in include_str!("../runtime/runtime.go").lines() {
        match line.trim() {
            "// BEGIN ERROR DOCUMENTATION" => documentation = true,
            "// END ERROR DOCUMENTATION" => documentation = false,
            _ if !documentation || enabled => {
                source.push_str(line);
                source.push('\n');
            }
            _ => {}
        }
    }
    source
}
