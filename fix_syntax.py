import sys

with open('crates/crucible-lsp/src/server.rs', 'r') as f:
    lines = f.readlines()

new_lines = []
i = 0
while i < len(lines):
    if 'use lsp_types::{' in lines[i] and 'assert!(caps.semantic_tokens_provider.is_some());' in lines[i-1]:
        # Close test_capabilities
        new_lines.append('    }\n\n')
        # We also need to move the imports up
    new_lines.append(lines[i])
    i += 1

with open('crates/crucible-lsp/src/server.rs', 'w') as f:
    f.writelines(new_lines)
