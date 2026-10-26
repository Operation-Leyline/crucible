//! API policy checks consume the lowered model shared by builds and the CLI.

mod check;
mod checker;
mod compatibility;
mod config;
mod diff;
mod direction;
mod documentation;
mod documentation_target;
mod examples;
mod exception;
mod finding;
mod intent;
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
pub use diff::{Renames, diff, diff_renamed};
pub use documentation::Documentation;
pub use documentation_target::DocumentationTarget;
pub use examples::Examples;
pub use exception::Exception;
pub use finding::Finding;
pub use intent::Intent;
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
    "naming.enum_wire_values",
    "paths.prefix",
    "paths.segments",
    "paths.parameters",
    "documentation.required",
    "intent.operations",
    "examples.operations",
    "examples.errors",
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
