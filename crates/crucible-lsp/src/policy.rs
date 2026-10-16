//! API policy in the editor: the package's policy runs on every compile, and
//! each finding is shown at the name it is about, as the build would report
//! it, with a rename where a naming case says what the name should be.

use std::path::Path;

use crucible_ir::ShapeId;
use crucible_lang::{Compiled, TokenKind};
use crucible_policy::{Check, Config, NameCase, Severity};

/// One policy finding in a file of the package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub file: usize,
    pub start: u32,
    pub end: u32,
    pub error: bool,
    pub rule: String,
    pub message: String,
    /// The name a naming case asks for, offered as a fix.
    pub rename: Option<String>,
}

/// The policy that applies to the package in `dir`: its own manifest's, or
/// a `crucible.toml` above it that carries policy, as a workspace sharing
/// one file selects with `.policy(..)`. `manifest` is the package manifest's
/// unsaved text, when the editor has it open.
pub fn source(dir: &Path, manifest: Option<&str>) -> Option<String> {
    let own = manifest
        .map(str::to_owned)
        .or_else(|| std::fs::read_to_string(dir.join("crucible.toml")).ok());
    if let Some(text) = own.filter(|text| Config::declares_policy(text)) {
        return Some(text);
    }
    dir.ancestors().skip(1).find_map(|parent| {
        std::fs::read_to_string(parent.join("crucible.toml"))
            .ok()
            .filter(|text| Config::declares_policy(text))
    })
}

/// Every unexcepted finding of `config` on the compiled package, at the
/// name of the declaration, member or enum value it concerns. Findings on
/// imports, or on shapes with no source position, are dropped. A package
/// with errors is checked as far as it lowered, so a typo in one place does
/// not hide every naming finding elsewhere.
pub fn findings(compiled: &Compiled, own: usize, config: &Config) -> Vec<Finding> {
    let Some(service) = compiled.service.as_ref().or(compiled.best_effort.as_ref()) else {
        return Vec::new();
    };
    let report = crucible_policy::check(service, config);
    let mut out = Vec::new();
    for finding in report.findings {
        if finding.exception.is_some() || finding.severity == Severity::Off {
            continue;
        }
        let Ok(id) = finding.shape.parse::<ShapeId>() else {
            continue;
        };
        let Some((file, start, end)) = name_of(compiled, own, &id) else {
            continue;
        };
        let name = id.member().unwrap_or(id.name());
        let rename = case_for(config, &finding.rule)
            .filter(|case| !case.value.matches(name))
            .map(|case| case.value.apply(name))
            .filter(|renamed| renamed != name);
        out.push(Finding {
            file,
            start,
            end,
            error: finding.severity == Severity::Error,
            rule: finding.rule,
            message: finding.message,
            rename,
        });
    }
    out
}

/// The compatibility guard in the editor: each undeclared breaking change at
/// what it concerns, and each migration problem at its step. Something
/// removed is shown at what held it (a member at its structure, an
/// operation at the service); what has no place in the sources is returned
/// apart, for the manifest.
pub fn compatibility(
    compiled: &Compiled,
    own: usize,
    guard: crucible_lang::Guard,
) -> (Vec<Finding>, Vec<(bool, String)>) {
    let Some(service) = &compiled.service else {
        return (Vec::new(), Vec::new());
    };
    let mut placed = Vec::new();
    let mut unplaced = Vec::new();
    for finding in guard.undeclared {
        let error = finding.severity == Severity::Error;
        let message = format!(
            "breaking change since {}: {}; declare it in the migration from \"{}\"",
            guard.version, finding.message, guard.version
        );
        let location = finding.shape.parse::<ShapeId>().ok().and_then(|id| {
            name_of(compiled, own, &id)
                .or_else(|| name_of(compiled, own, &id.root()))
                .or_else(|| name_of(compiled, own, &service.id))
        });
        match location {
            Some((file, start, end)) => placed.push(Finding {
                file,
                start,
                end,
                error,
                rule: finding.rule,
                message,
                rename: None,
            }),
            None => unplaced.push((
                error,
                format!("[{}] {}: {message}", finding.rule, finding.shape),
            )),
        }
    }
    for item in guard.problems {
        let Some(file) = item.file else {
            unplaced.push((item.diagnostic.is_error(), item.diagnostic.message));
            continue;
        };
        placed.push(Finding {
            file,
            start: item.diagnostic.span.start,
            end: item.diagnostic.span.end,
            error: item.diagnostic.is_error(),
            rule: "migration".into(),
            message: item.diagnostic.message,
            rename: None,
        });
    }
    (placed, unplaced)
}

/// What building and checking the older releases found, at its place:
/// a step, a reused name, an example a release leaves out. A problem with
/// no place in a model file shows on the manifest.
pub fn versions(versions: crucible_lang::Versions) -> (Vec<Finding>, Vec<(bool, String)>) {
    let mut placed = Vec::new();
    let mut unplaced = Vec::new();
    for item in versions.diagnostics {
        let Some(file) = item.file else {
            unplaced.push((item.diagnostic.is_error(), item.diagnostic.message));
            continue;
        };
        placed.push(Finding {
            file,
            start: item.diagnostic.span.start,
            end: item.diagnostic.span.end,
            error: item.diagnostic.is_error(),
            rule: "versions".into(),
            message: item.diagnostic.message,
            rename: None,
        });
    }
    (placed, unplaced)
}

/// Where the name of `id`'s declaration is written in the package's own
/// files: the declaration's span covers its body; this is its name.
fn name_of(compiled: &Compiled, own: usize, id: &ShapeId) -> Option<(usize, u32, u32)> {
    let location = compiled.source_map.get(id)?;
    if location.file >= own {
        return None;
    }
    let name = id.member().unwrap_or(id.name());
    let file = &compiled.files[location.file];
    let (start, end) = compiled.parsed[location.file]
        .tokens
        .iter()
        .find(|token| {
            token.kind == TokenKind::Ident
                && token.span.start >= location.span.start
                && token.span.end <= location.span.end
                && token.span.text(&file.text) == name
        })
        .map_or((location.span.start, location.span.start), |token| {
            (token.span.start, token.span.end)
        });
    Some((location.file, start, end))
}

/// The naming case a naming rule checks, if the config sets one.
fn case_for<'c>(config: &'c Config, rule: &str) -> Option<&'c Check<NameCase>> {
    let naming = config.rules.naming.as_ref()?;
    match rule {
        "naming.operations" => naming.operations.as_ref(),
        "naming.shapes" => naming.shapes.as_ref(),
        "naming.members" => naming.members.as_ref(),
        "naming.enums" => naming.enum_values.as_ref(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = r#"service Shop {
    version "1"
    protocol json
    operations [listItems]
}

/// Lists items.
readonly operation listItems {
    http GET "/items"
    input { @query Page_size?: integer }
    output { names: Names }
}

list Names: string
"#;

    #[test]
    fn findings_point_at_names_with_their_severity_and_fix() {
        let package = crucible_lang::Package::new(
            "example.shop",
            vec![crucible_lang::SourceFile {
                path: "shop.cru".into(),
                text: SOURCE.into(),
            }],
        );
        let compiled = crucible_lang::compile(&package, None);
        assert!(compiled.service.is_some(), "{}", compiled.render());
        let config = Config::parse(
            "[rules.naming]\noperations = \"PascalCase\"\nmembers = { case = \"camelCase\", severity = \"warning\" }\n",
        )
        .unwrap();
        let findings = findings(&compiled, 1, &config);
        let operation = findings
            .iter()
            .find(|f| f.rule == "naming.operations")
            .unwrap();
        assert!(operation.error);
        assert_eq!(
            &SOURCE[operation.start as usize..operation.end as usize],
            "listItems"
        );
        // The declaration, not the reference in the service.
        assert!(operation.start as usize > SOURCE.find("readonly").unwrap());
        assert_eq!(operation.rename.as_deref(), Some("ListItems"));
        let member = findings
            .iter()
            .find(|f| f.rule == "naming.members")
            .unwrap();
        assert!(!member.error);
        assert_eq!(
            &SOURCE[member.start as usize..member.end as usize],
            "Page_size"
        );
        assert_eq!(member.rename.as_deref(), Some("pageSize"));
    }

    #[test]
    fn an_error_elsewhere_does_not_hide_findings() {
        let broken = SOURCE.replace("names: Names", "names: Nmaes");
        let package = crucible_lang::Package::new(
            "example.shop",
            vec![crucible_lang::SourceFile {
                path: "shop.cru".into(),
                text: broken,
            }],
        );
        let compiled = crucible_lang::compile(&package, None);
        assert!(compiled.service.is_none());
        let config = Config::parse(
            "[rules.naming]
operations = \"PascalCase\"
",
        )
        .unwrap();
        assert!(
            findings(&compiled, 1, &config)
                .iter()
                .any(|f| f.rule == "naming.operations")
        );
    }
}
