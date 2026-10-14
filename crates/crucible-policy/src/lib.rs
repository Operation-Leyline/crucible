//! API policy checks consume the lowered model shared by builds and the CLI.

mod check;
mod checker;
mod compatibility;
mod config;
mod diff;
mod direction;
mod documentation;
mod documentation_target;
mod exception;
mod finding;
mod lint;
mod name_case;
mod naming;
mod paths;
mod permissions;
mod report;
mod rules;
mod severity;

pub use check::Check;
pub use compatibility::Compatibility;
pub use config::{Config, ConfigError, POLICY_KEYS};
pub use diff::diff;
pub use documentation::Documentation;
pub use documentation_target::DocumentationTarget;
pub use exception::Exception;
pub use finding::Finding;
pub use lint::check;
pub use name_case::NameCase;
pub use naming::Naming;
pub use paths::Paths;
pub use report::Report;
pub use rules::Rules;
pub use severity::Severity;

pub const RULE_IDS: &[&str] = &[
    "naming.shapes",
    "naming.operations",
    "naming.members",
    "naming.enums",
    "paths.prefix",
    "paths.segments",
    "paths.parameters",
    "documentation.required",
    "compatibility.service",
    "compatibility.protocol",
    "compatibility.operation",
    "compatibility.http",
    "compatibility.auth",
    "compatibility.bindings",
    "compatibility.type",
    "compatibility.member",
    "compatibility.required",
    "compatibility.nullability",
    "compatibility.constraints",
    "compatibility.enum",
    "compatibility.default",
    "compatibility.error",
    "compatibility.pagination",
    "compatibility.timestamp",
    "compatibility.sdk",
    "compatibility.unchecked",
    "compatibility.permissions",
    "compatibility.authorization",
];
