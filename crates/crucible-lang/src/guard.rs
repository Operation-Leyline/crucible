//! The compatibility guard (decision 47d) and declared migrations
//! (decisions 51-54a): a package against the snapshot of its last
//! release. A breaking change compiles only when the migration from that
//! release declares it, and each declared step must match a change that
//! happened, so the migration and the model cannot drift apart.

use std::path::Path;

use std::collections::BTreeSet;

use crucible_policy::{Config, Finding, Severity};
use crucible_smithy::ShapeId;
use crucible_smithy::ir::{ApiVersion, Hook, Operation, Service, Shape, VersionStep};
use serde_json::Value as Json;

use crate::ast::{Decl, Dotted, Ident, Key, Migration, RequireHow, StepKind, Value};
use crate::compile::{Compiled, FileDiagnostic, Package, load_baseline};
use crate::diagnostic::Diagnostic;

/// What the guard found for one package.
#[derive(Debug, Clone)]
pub struct Guard {
    /// The frozen release's version: its directory's name.
    pub version: String,
    /// Breaking changes no migration declares, as the checker reports them;
    /// excepted and `off` findings are left out.
    pub undeclared: Vec<Finding>,
    /// How many findings the migration declared.
    pub declared: usize,
    /// Problems with the migration itself, each at its step.
    pub problems: Vec<FileDiagnostic>,
}

impl Guard {
    /// Whether the guard fails the build.
    pub fn has_errors(&self) -> bool {
        self.undeclared
            .iter()
            .any(|finding| finding.severity == Severity::Error)
            || self.problems.iter().any(|item| item.diagnostic.is_error())
    }

    /// Every undeclared finding and problem, one per line, for a build log.
    pub fn render(&self, compiled: &Compiled) -> String {
        let mut out = String::new();
        for finding in &self.undeclared {
            let severity = if finding.severity == Severity::Error {
                "error"
            } else {
                "warning"
            };
            out.push_str(&format!(
                "{severity}[{}] {}: {}\n",
                finding.rule, finding.shape, finding.message
            ));
        }
        if !self.undeclared.is_empty() {
            out.push_str(&format!(
                "declare these in `migration \"<new version>\" from \"{}\" {{ .. }}`, or freeze a new baseline\n",
                self.version
            ));
        }
        for item in &self.problems {
            match item.file.and_then(|index| compiled.files.get(index)) {
                Some(file) => out.push_str(&item.diagnostic.render(
                    &file.path,
                    &file.text,
                    &crate::span::LineIndex::new(&file.text),
                )),
                // A problem with the package, not with a line of it.
                None => out.push_str(&format!("error: {}\n", item.diagnostic.message)),
            }
        }
        out
    }
}

/// The guard for `compiled`, a package that compiled, when its manifest
/// names a baseline. `Err` explains why the baseline does not load.
pub fn guard(
    package: &Package,
    compiled: &Compiled,
    own: usize,
    config: &Config,
) -> Option<Result<Guard, String>> {
    let service = compiled.service.as_ref()?;
    let old = match load_baseline(package)? {
        Ok(old) => old,
        Err(problem) => return Some(Err(problem)),
    };
    // `releases/<version>.json`: the release's name is its file's stem.
    let version = package
        .baseline
        .as_deref()
        .and_then(Path::file_stem)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let report = crucible_policy::diff(&old, service, config);
    let mut problems = Vec::new();
    // A release is named by its service's version, so migrations can name it.
    if old.version != version {
        problems.push(FileDiagnostic {
            file: None,
            diagnostic: Diagnostic::new(
                crate::span::Span::default(),
                format!(
                    "the baseline is frozen as \"{version}\" but its service declares version \"{}\"; name the release file after the version",
                    old.version
                ),
            ),
        });
    }

    // The migration from the baseline's version declares this release's
    // breaking changes; older ones are history the runtime still serves.
    let mut from_baseline = Vec::new();
    for (file, parsed) in compiled.parsed.iter().enumerate().take(own) {
        for item in &parsed.file.items {
            if let Decl::Migration(migration) = &item.decl
                && migration.from.value == version
            {
                from_baseline.push((file, migration));
            }
        }
    }
    let mut covers = Vec::new();
    if let Some(((file, migration), rest)) = from_baseline.split_first() {
        for (other, extra) in rest {
            problems.push(FileDiagnostic {
                file: Some(*other),
                diagnostic: Diagnostic::new(
                    extra.from.span,
                    format!("a second migration from \"{version}\"; one release has one migration"),
                ),
            });
        }
        let context = Context {
            namespace: &compiled.namespace,
            old: &old,
            new: service,
        };
        for step in &migration.steps {
            match context.checked_step(&step.kind) {
                Ok(mut cover) => covers.append(&mut cover),
                Err(diagnostics) => {
                    problems.extend(diagnostics.into_iter().map(|diagnostic| FileDiagnostic {
                        file: Some(*file),
                        diagnostic,
                    }))
                }
            }
        }
        check_versions(migration, *file, &mut problems);
        if migration.version.value != service.version {
            problems.push(FileDiagnostic {
                file: Some(*file),
                diagnostic: Diagnostic::new(
                    migration.version.span,
                    format!(
                        "the migration from \"{version}\" serves the next release, which is the service's version \"{}\"",
                        service.version
                    ),
                ),
            });
        }
    } else if service.version != old.version {
        // A release with no migration would drop out of the chain of
        // releases the server serves, and its clients, which name it, with
        // it. One that breaks nothing declares an empty migration.
        problems.push(FileDiagnostic {
            file: None,
            diagnostic: Diagnostic::new(
                crate::span::Span::default(),
                format!(
                    "the service's version is \"{}\" but no migration comes from the baseline \"{version}\"; declare `migration \"{}\" from \"{version}\" {{}}`, empty when nothing breaks",
                    service.version, service.version
                ),
            ),
        });
    }

    let mut undeclared = Vec::new();
    let mut declared = 0;
    for finding in report.findings {
        if finding.exception.is_some() || finding.severity == Severity::Off {
            continue;
        }
        let covered = finding
            .shape
            .parse::<ShapeId>()
            .is_ok_and(|id| covers.iter().any(|cover| cover.covers(&id, &finding.rule)));
        if covered {
            declared += 1;
        } else {
            undeclared.push(finding);
        }
    }
    Some(Ok(Guard {
        version,
        undeclared,
        declared,
        problems,
    }))
}

/// A migration's versions must name the release it serves.
fn check_versions(migration: &Migration, file: usize, problems: &mut Vec<FileDiagnostic>) {
    if migration.version.value.trim().is_empty() || migration.version.value == migration.from.value
    {
        problems.push(FileDiagnostic {
            file: Some(file),
            diagnostic: Diagnostic::new(
                migration.version.span,
                "a migration names the new version it serves, different from the one it comes from",
            ),
        });
    }
}

/// What a declared step accounts for among the checker's findings: the
/// kinds of change it declares, at the shapes it names. A `move` accounts
/// for the route it moved, never for another break on the same operation.
#[derive(Debug, Clone)]
struct Cover {
    at: At,
    /// The compatibility rules the step declares; `ANY` for the general
    /// steps (`reshape`, `remove operation`) that may change anything there.
    rules: &'static [&'static str],
}

#[derive(Debug, Clone)]
enum At {
    /// This shape or member, exactly.
    Exact(ShapeId),
    /// Anything in this shape, its members included.
    Within(ShapeId),
}

/// The general steps declare any change within what they name.
const ANY: &[&str] = &[];
const RENAME: &[&str] = &[
    "compatibility.member",
    "compatibility.required",
    "compatibility.nullability",
    "compatibility.sdk",
];
const REQUIRE: &[&str] = &[
    "compatibility.required",
    "compatibility.nullability",
    "compatibility.default",
    "compatibility.sdk",
];
const ENUM_VALUE: &[&str] = &["compatibility.enum", "compatibility.sdk"];
const ERROR: &[&str] = &["compatibility.error", "compatibility.sdk"];
const ROUTE: &[&str] = &["compatibility.http"];
const RETYPE: &[&str] = &[
    "compatibility.type",
    "compatibility.constraints",
    "compatibility.timestamp",
    "compatibility.sdk",
];

impl Cover {
    fn exact(id: ShapeId, rules: &'static [&'static str]) -> Self {
        Self {
            at: At::Exact(id),
            rules,
        }
    }

    fn within(id: ShapeId, rules: &'static [&'static str]) -> Self {
        Self {
            at: At::Within(id),
            rules,
        }
    }

    fn covers(&self, id: &ShapeId, rule: &str) -> bool {
        let place = match &self.at {
            At::Exact(covered) => covered == id,
            At::Within(root) => &id.root() == root,
        };
        place && (self.rules.is_empty() || self.rules.contains(&rule))
    }
}

struct Context<'a> {
    namespace: &'a str,
    old: &'a Service,
    new: &'a Service,
}

/// Where a dotted path points: a shape, or a member of one.
struct Target {
    shape: ShapeId,
    member: Option<String>,
}

fn error(span: crate::span::Span, message: String) -> Diagnostic {
    Diagnostic::new(span, message)
}

fn operation<'s>(service: &'s Service, name: &str) -> Option<&'s Operation> {
    service.operations.iter().find(|op| op.name() == name)
}

impl Context<'_> {
    fn id(&self, name: &str) -> ShapeId {
        ShapeId::new(self.namespace, name)
    }

    /// The shape `name` in `service`: the package's own, else an imported
    /// one, such as `crucible.errors#ValidationException`.
    fn named(&self, service: &Service, name: &str) -> ShapeId {
        let own = self.id(name);
        if service.shapes.contains_key(&own) {
            return own;
        }
        service
            .shapes
            .keys()
            .find(|id| id.name() == name)
            .cloned()
            .unwrap_or(own)
    }

    /// `Shape`, `Shape.member`, `Op.input`, `Op.output.member`, resolved in
    /// `service`: operations' inputs and outputs are their structures.
    fn resolve(&self, service: &Service, path: &Dotted) -> Option<Target> {
        let parts: Vec<&str> = path.parts.iter().map(|p| p.name.as_str()).collect();
        let (shape, rest) = match parts.as_slice() {
            [op, side @ ("input" | "output"), rest @ ..] => {
                let operation = operation(service, op)?;
                let shape = if *side == "input" {
                    operation.input.clone()?
                } else {
                    operation.output.clone()?
                };
                (shape, rest)
            }
            [shape, rest @ ..] => (self.id(shape), rest),
            [] => return None,
        };
        match rest {
            [] => Some(Target {
                shape,
                member: None,
            }),
            [member] => Some(Target {
                shape,
                member: Some((*member).to_owned()),
            }),
            _ => None,
        }
    }

    fn members<'s>(&self, service: &'s Service, shape: &ShapeId) -> Option<Vec<&'s str>> {
        Some(match service.shapes.get(shape)? {
            Shape::Structure(s) | Shape::Union(s) => {
                s.members.iter().map(|m| m.name.as_str()).collect()
            }
            Shape::Enum(e) => e.values.iter().map(|v| v.name.as_str()).collect(),
            Shape::IntEnum(e) => e.values.iter().map(|v| v.name.as_str()).collect(),
            _ => Vec::new(),
        })
    }

    fn has_member(&self, service: &Service, shape: &ShapeId, member: &str) -> bool {
        self.members(service, shape)
            .is_some_and(|members| members.contains(&member))
    }

    /// Validate literals as well as the structural change, collecting every
    /// problem with a default before it reaches the runtime version pipeline.
    fn checked_step(&self, step: &StepKind) -> Result<Vec<Cover>, Vec<Diagnostic>> {
        let covers = self.step(step).map_err(|diagnostic| vec![diagnostic])?;
        if let StepKind::Require {
            path,
            how: RequireHow::Default(value),
        } = step
            && let Some(target) = self.resolve(self.new, path)
            && let Some(Shape::Structure(owner)) = self.new.shapes.get(&target.shape)
            && let Some(member) = target.member.as_ref().and_then(|name| owner.member(name))
        {
            let default = self.require_default(&target, value);
            let problems: Vec<_> = crucible_smithy::validate::value(
                &self.new.shapes,
                &format!("the migration default for `{}`", member.name),
                &member.target,
                &member.constraints,
                &default,
            )
            .into_iter()
            .map(|message| error(value.span(), message))
            .collect();
            if !problems.is_empty() {
                return Err(problems);
            }
        }
        Ok(covers)
    }

    fn require_default(&self, target: &Target, value: &Value) -> Json {
        // An enum literal names its symbol; validation and the runtime
        // conversion must see the same corresponding wire value.
        let default = json(value);
        if let (Json::String(symbol), Some(Shape::Structure(owner))) =
            (&default, self.new.shapes.get(&target.shape))
            && let Some(member) = target.member.as_ref().and_then(|name| owner.member(name))
        {
            match self.new.shapes.get(&member.target) {
                Some(Shape::Enum(enumeration)) => {
                    if let Some(found) = enumeration
                        .values
                        .iter()
                        .find(|value| &value.name == symbol)
                    {
                        return Json::String(found.value.clone());
                    }
                }
                Some(Shape::IntEnum(enumeration)) if matches!(value, Value::Ident(_)) => {
                    if let Some(found) = enumeration
                        .values
                        .iter()
                        .find(|value| &value.name == symbol)
                    {
                        return Json::Number(found.value.into());
                    }
                }
                _ => {}
            }
        }
        default
    }

    /// Checks one step against the two models and returns what it covers.
    fn step(&self, step: &StepKind) -> Result<Vec<Cover>, Diagnostic> {
        match step {
            StepKind::Rename { path, to } => {
                let (old, new) = self.both(path)?;
                let (Some(from), Some(_)) = (&old.member, &new.member) else {
                    return Err(error(
                        path.span,
                        "`rename` names a member: `Shape.member -> newName`".into(),
                    ));
                };
                if !self.has_member(self.old, &old.shape, from) {
                    return Err(error(
                        path.span,
                        format!(
                            "`{from}` is not a member of {} in the baseline",
                            old.shape.name()
                        ),
                    ));
                }
                if self.has_member(self.new, &new.shape, from) {
                    return Err(error(
                        path.span,
                        format!(
                            "`{from}` is still a member of {}; nothing was renamed",
                            new.shape.name()
                        ),
                    ));
                }
                if !self.has_member(self.new, &new.shape, &to.name) {
                    return Err(error(
                        to.span,
                        format!("{} has no member `{}`", new.shape.name(), to.name),
                    ));
                }
                Ok(vec![
                    Cover::exact(old.shape.with_member(from), RENAME),
                    Cover::exact(new.shape.with_member(&to.name), RENAME),
                ])
            }
            StepKind::Require { path, .. } => {
                let (old, new) = self.both(path)?;
                let Some(member) = &new.member else {
                    return Err(error(
                        path.span,
                        "`require` names a member: `Operation.input.member`".into(),
                    ));
                };
                let optional = |service: &Service, shape: &ShapeId| match service.shapes.get(shape)
                {
                    Some(Shape::Structure(s)) => s.member(member).map(|m| m.optional_on_server()),
                    _ => None,
                };
                match (
                    optional(self.old, &old.shape),
                    optional(self.new, &new.shape),
                ) {
                    (Some(true), Some(false)) => Ok(vec![
                        Cover::exact(old.shape.with_member(member), REQUIRE),
                        Cover::exact(new.shape.with_member(member), REQUIRE),
                    ]),
                    (None, _) => Err(error(
                        path.span,
                        format!(
                            "the baseline has no member `{member}` here; a new required member is `add`ed"
                        ),
                    )),
                    (_, None) => Err(error(path.span, format!("`{member}` is not a member here"))),
                    _ => Err(error(
                        path.span,
                        format!("`{member}` did not become required"),
                    )),
                }
            }
            StepKind::Add {
                path,
                name,
                downgrade,
                mapping,
            } => match name {
                // `add Op.errors NewError downgrade OldError { .. }`
                Some(error_name) => {
                    let op_name = path.parts.first().map_or("", |p| p.name.as_str());
                    let (Some(old_op), Some(new_op)) =
                        (operation(self.old, op_name), operation(self.new, op_name))
                    else {
                        return Err(error(
                            path.span,
                            format!("`{op_name}` is not an operation in both releases"),
                        ));
                    };
                    let added = self.id(&error_name.name);
                    let errors =
                        |op: &Operation| op.errors.iter().any(|e| e.name() == added.name());
                    if errors(old_op) || !errors(new_op) {
                        return Err(error(
                            error_name.span,
                            format!("{} was not added to {op_name}'s errors", error_name.name),
                        ));
                    }
                    let Some(older) = old_op.errors.iter().find(|e| e.name() == downgrade.name)
                    else {
                        return Err(error(
                            downgrade.span,
                            format!("{op_name} had no error {} in the baseline", downgrade.name),
                        ));
                    };
                    // Older clients get the downgrade error built from the
                    // new one's members: it needs every member it requires.
                    let newer = new_op.errors.iter().find(|e| e.name() == added.name());
                    if let (Some(Shape::Structure(older)), Some(Shape::Structure(newer))) = (
                        self.old.shapes.get(older),
                        newer.and_then(|id| self.new.shapes.get(id)),
                    ) {
                        for (to, from) in mapping {
                            if let (Some(target), Some(source)) =
                                (older.member(&to.name), newer.member(&from.name))
                                && target.required
                                && target.default.is_none()
                                && !(source.required || source.default.is_some())
                            {
                                return Err(error(
                                    from.span,
                                    format!(
                                        "`{}` is optional on {}, but {} requires `{}`",
                                        from.name, error_name.name, downgrade.name, to.name
                                    ),
                                ));
                            }
                            if older.member(&to.name).is_none() {
                                return Err(error(
                                    to.span,
                                    format!("{} has no member `{}`", downgrade.name, to.name),
                                ));
                            }
                            if newer.member(&from.name).is_none() {
                                return Err(error(
                                    from.span,
                                    format!("{} has no member `{}`", error_name.name, from.name),
                                ));
                            }
                        }
                        let unfilled: Vec<&str> = older
                            .members
                            .iter()
                            .filter(|member| member.required && member.default.is_none())
                            .filter(|member| !mapping.iter().any(|(to, _)| to.name == member.name))
                            .map(|member| member.name.as_str())
                            .collect();
                        if !unfilled.is_empty() {
                            return Err(error(
                                downgrade.span,
                                format!(
                                    "{} requires `{}`, which this step does not fill from {}",
                                    downgrade.name,
                                    unfilled.join("`, `"),
                                    error_name.name
                                ),
                            ));
                        }
                    }
                    Ok(vec![
                        Cover::exact(old_op.id.clone(), ERROR),
                        Cover::exact(new_op.id.clone(), ERROR),
                        Cover::within(added, ERROR),
                    ])
                }
                // `add Enum.VALUE downgrade OLD`
                None => {
                    let (old, new) = self.both(path)?;
                    let Some(value) = &new.member else {
                        return Err(error(path.span, "`add` names an enum value, `Enum.VALUE`, or an error, `Operation.errors Error`".into()));
                    };
                    if self.has_member(self.old, &old.shape, value)
                        || !self.has_member(self.new, &new.shape, value)
                    {
                        return Err(error(
                            path.span,
                            format!("{} did not gain the value {value}", new.shape.name()),
                        ));
                    }
                    if !self.has_member(self.old, &old.shape, &downgrade.name) {
                        return Err(error(
                            downgrade.span,
                            format!(
                                "{} had no value {} in the baseline",
                                old.shape.name(),
                                downgrade.name
                            ),
                        ));
                    }
                    Ok(vec![
                        Cover::exact(old.shape.clone(), ENUM_VALUE),
                        Cover::exact(new.shape.with_member(value), ENUM_VALUE),
                    ])
                }
            },
            StepKind::Move {
                operation: name,
                method,
                uri,
            } => {
                let (Some(old_op), Some(new_op)) = (
                    operation(self.old, &name.name),
                    operation(self.new, &name.name),
                ) else {
                    return Err(error(
                        name.span,
                        format!("`{}` is not an operation in both releases", name.name),
                    ));
                };
                if old_op.method != method.name || old_op.uri.to_string() != uri.value {
                    return Err(error(
                        uri.span,
                        format!(
                            "{} was `{} \"{}\"` in the baseline",
                            name.name, old_op.method, old_op.uri
                        ),
                    ));
                }
                if new_op.method == old_op.method && new_op.uri == old_op.uri {
                    return Err(error(name.span, format!("{} did not move", name.name)));
                }
                Ok(vec![Cover::exact(old_op.id.clone(), ROUTE)])
            }
            StepKind::Reshape {
                path,
                members,
                into,
                ..
            } => {
                let (old, new) = self.both(path)?;
                if old.member.is_some() {
                    return Err(error(
                        path.span,
                        "`reshape` names a structure: `Shape { a, b } -> c`".into(),
                    ));
                }
                for member in members {
                    if !self.has_member(self.old, &old.shape, &member.name) {
                        return Err(error(
                            member.span,
                            format!(
                                "`{}` is not a member of {} in the baseline",
                                member.name,
                                old.shape.name()
                            ),
                        ));
                    }
                }
                if !self.has_member(self.new, &new.shape, &into.name) {
                    return Err(error(
                        into.span,
                        format!("{} has no member `{}`", new.shape.name(), into.name),
                    ));
                }
                Ok(vec![
                    Cover::within(old.shape, ANY),
                    Cover::within(new.shape, ANY),
                ])
            }
            StepKind::RemoveOperation(name) => {
                let Some(old_op) = operation(self.old, &name.name) else {
                    return Err(error(
                        name.span,
                        format!("the baseline has no operation `{}`", name.name),
                    ));
                };
                if operation(self.new, &name.name).is_some() {
                    return Err(error(
                        name.span,
                        format!("`{}` is still an operation", name.name),
                    ));
                }
                let mut covers = vec![Cover::exact(old_op.id.clone(), ANY)];
                covers.extend(
                    old_op
                        .input
                        .iter()
                        .chain(&old_op.output)
                        .cloned()
                        .map(|io| Cover::within(io, ANY)),
                );
                // Errors only the removed operation used go with it.
                covers.extend(
                    old_op
                        .errors
                        .iter()
                        .filter(|error| !self.new.shapes.contains_key(*error))
                        .cloned()
                        .map(|error| Cover::within(error, ANY)),
                );
                Ok(covers)
            }
            StepKind::Retype { path, to, .. } => {
                let (old, new) = self.both(path)?;
                let Some(member) = &new.member else {
                    return Err(error(
                        path.span,
                        "`retype` names a member: `Shape.member -> Type`".into(),
                    ));
                };
                let target = |service: &Service, shape: &ShapeId| match service.shapes.get(shape) {
                    Some(Shape::Structure(s)) | Some(Shape::Union(s)) => {
                        s.member(member).map(|m| m.target.clone())
                    }
                    _ => None,
                };
                // A built-in is written `string` and lowered to `smithy.api#String`.
                let named = |id: &ShapeId| {
                    id.name() == to.name
                        || (id.is_prelude() && id.name().eq_ignore_ascii_case(&to.name))
                };
                match (target(self.old, &old.shape), target(self.new, &new.shape)) {
                    (Some(was), Some(now)) if named(&now) && was != now => Ok(vec![
                        Cover::exact(old.shape.with_member(member), RETYPE),
                        Cover::exact(new.shape.with_member(member), RETYPE),
                        // The checker reports the change on the type it left.
                        Cover::exact(was, RETYPE),
                    ]),
                    (Some(_), Some(now)) if !named(&now) => Err(error(
                        to.span,
                        format!("`{member}` is a {} now, not a {}", now.name(), to.name),
                    )),
                    (Some(_), Some(_)) => Err(error(
                        path.span,
                        format!("`{member}` already had this type"),
                    )),
                    _ => Err(error(
                        path.span,
                        format!("`{member}` is not a member in both releases"),
                    )),
                }
            }
            StepKind::Behavior(path) => {
                let name = path.parts.first().map_or("", |p| p.name.as_str());
                if operation(self.new, name).is_none() {
                    return Err(error(path.span, format!("`{name}` is not an operation")));
                }
                Ok(Vec::new())
            }
            StepKind::Sunset(date) => {
                if !is_date(&date.value) {
                    return Err(error(
                        date.span,
                        "a sunset date is written YYYY-MM-DD".into(),
                    ));
                }
                Ok(Vec::new())
            }
        }
    }

    /// A path resolved in both releases.
    fn both(&self, path: &Dotted) -> Result<(Target, Target), Diagnostic> {
        let old = self
            .resolve(self.old, path)
            .ok_or_else(|| error(path.span, format!("the baseline has no `{}`", dotted(path))))?;
        let new = self.resolve(self.new, path).ok_or_else(|| {
            error(
                path.span,
                format!("the current model has no `{}`", dotted(path)),
            )
        })?;
        Ok((old, new))
    }
}

fn dotted(path: &Dotted) -> String {
    path.parts
        .iter()
        .map(|part: &Ident| part.name.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

fn is_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    let [y, m, d] = parts.as_slice() else {
        return false;
    };
    if y.len() != 4
        || m.len() != 2
        || d.len() != 2
        || !parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit()))
    {
        return false;
    }
    // A date that exists: the `Sunset` header carries it.
    let (year, month, day): (u32, u32, u32) = (
        y.parse().unwrap_or(0),
        m.parse().unwrap_or(0),
        d.parse().unwrap_or(0),
    );
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

/// The older versions `compiled` still serves, oldest first: the chain of
/// migrations back from the service's version, each `from` a release frozen
/// beside the baseline (`releases/<version>.json`). `guard` has checked the
/// newest migration; an older one was checked when it was the newest.
pub fn versions(
    package: &Package,
    compiled: &Compiled,
    own: usize,
) -> Result<Vec<ApiVersion>, String> {
    let (Some(service), Some(baseline)) = (&compiled.service, &package.baseline) else {
        return Ok(Vec::new());
    };
    let releases = baseline.parent().unwrap_or(baseline);
    let migrations: Vec<&Migration> = compiled
        .parsed
        .iter()
        .take(own)
        .flat_map(|parsed| &parsed.file.items)
        .filter_map(|item| match &item.decl {
            Decl::Migration(migration) => Some(migration),
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    let mut newer = service.clone();
    let mut seen = BTreeSet::new();
    while let Some(migration) = migrations
        .iter()
        .find(|migration| migration.version.value == newer.version)
    {
        let name = migration.from.value.clone();
        if !seen.insert(name.clone()) {
            return Err(format!("the migrations from \"{name}\" form a cycle"));
        }
        let file = releases.join(crate::release::file_name(&name)?);
        if !file.is_file() {
            return Err(format!(
                "release \"{name}\", which the migration to \"{}\" comes from, is not frozen at {}; freeze it with `crucible freeze` (`--from <git ref>` for a past release)",
                migration.version.value,
                file.display()
            ));
        }
        let old = crate::release::load(&file)?;
        // The chain follows releases by name: a release frozen under another
        // version's name would serve one version's contract as another's.
        if old.version != name {
            return Err(format!(
                "the frozen release {} declares version \"{}\"; its file must be named for it",
                file.display(),
                old.version
            ));
        }
        let context = Context {
            namespace: &compiled.namespace,
            old: &old,
            new: &newer,
        };
        let mut steps = Vec::new();
        let mut sunset = None;
        for step in &migration.steps {
            if let StepKind::Sunset(date) = &step.kind {
                sunset = Some(date.value.clone());
                continue;
            }
            steps.push(context.lower(&step.kind).map_err(|diagnostic| {
                format!(
                    "the migration to \"{}\": {}",
                    migration.version.value, diagnostic.message
                )
            })?);
        }
        let next = old.clone();
        out.push(ApiVersion {
            name,
            service: Box::new(old),
            steps,
            sunset,
        });
        newer = next;
    }
    out.reverse();
    Ok(out)
}

/// A literal as JSON, for a `require .. default` value.
fn json(value: &Value) -> Json {
    match value {
        Value::Str(text) => Json::String(text.value.clone()),
        Value::Number(number) => serde_json::from_str(&number.text).unwrap_or(Json::Null),
        Value::Ident(ident) => match ident.name.as_str() {
            "true" => Json::Bool(true),
            "false" => Json::Bool(false),
            "null" => Json::Null,
            name => Json::String(name.to_owned()),
        },
        Value::List(items, _) => Json::Array(items.iter().map(json).collect()),
        Value::Object(fields, _) => Json::Object(
            fields
                .iter()
                .map(|field| {
                    let key = match &field.key {
                        Key::Name(name) => name.name.clone(),
                        Key::Text(text) => text.value.clone(),
                    };
                    (key, json(&field.value))
                })
                .collect(),
        ),
    }
}

/// The operations whose input or output reach `shape` in `service`.
fn operations_reaching(service: &Service, shape: &ShapeId) -> Vec<ShapeId> {
    let reaches = |start: &ShapeId| {
        let mut stack = vec![start.clone()];
        let mut seen = BTreeSet::new();
        while let Some(id) = stack.pop() {
            if &id == shape {
                return true;
            }
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Some(found) = service.shapes.get(&id) {
                stack.extend(found.members().into_iter().map(|m| m.target.clone()));
            }
        }
        false
    };
    service
        .operations
        .iter()
        .filter(|op| {
            op.input
                .iter()
                .chain(&op.output)
                .chain(&op.errors)
                .any(&reaches)
        })
        .map(|op| op.id.clone())
        .collect()
}

impl Context<'_> {
    /// A checked step, lowered with its paths resolved.
    fn lower(&self, step: &StepKind) -> Result<VersionStep, Diagnostic> {
        self.checked_step(step).map_err(|diagnostics| {
            error(
                diagnostics[0].span,
                diagnostics
                    .into_iter()
                    .map(|diagnostic| diagnostic.message)
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        })?;
        let first = |path: &Dotted| {
            path.parts
                .first()
                .map_or("", |p| p.name.as_str())
                .to_owned()
        };
        let op_id =
            |name: &str, service: &Service| operation(service, name).map(|op| op.id.clone());
        Ok(match step {
            StepKind::Rename { path, to } => {
                let (old, new) = self.both(path)?;
                VersionStep::Rename {
                    old: old.shape,
                    new: new.shape,
                    from: old.member.unwrap_or_default(),
                    to: to.name.clone(),
                }
            }
            StepKind::Require { path, how } => {
                let (old, new) = self.both(path)?;
                let member = new.member.clone().unwrap_or_default();
                match how {
                    RequireHow::Default(value) => {
                        let default = self.require_default(&new, value);
                        let Some(Shape::Structure(owner)) = self.new.shapes.get(&new.shape) else {
                            unreachable!("a checked requirement targets a structure member")
                        };
                        let target = owner.member(&member).expect("a checked requirement member");
                        // Model literals accept epoch seconds and model member
                        // names; conversions inject protocol document values.
                        let default = self.new.wire_example(target, &default, None);
                        VersionStep::Require {
                            new: new.shape,
                            member,
                            default,
                        }
                    }
                    RequireHow::Hook(hook) => VersionStep::Hooked {
                        kind: "require".into(),
                        // `Op.input.m` or `Shape.m`: every operation it reaches.
                        operations: operations_reaching(self.new, &new.shape),
                        hook: Hook::Require {
                            old: old.shape,
                            new: new.shape,
                            member,
                        },
                        is_async: hook.is_async,
                    },
                }
            }
            StepKind::Add {
                path,
                name: Some(error),
                downgrade,
                mapping,
            } => VersionStep::AddError {
                operation: op_id(&first(path), self.new).unwrap_or_else(|| self.id(&first(path))),
                // The errors the step checked: the operation's own, by name.
                error: operation(self.new, &first(path))
                    .and_then(|op| op.errors.iter().find(|e| e.name() == error.name))
                    .cloned()
                    .unwrap_or_else(|| self.named(self.new, &error.name)),
                downgrade: operation(self.old, &first(path))
                    .and_then(|op| op.errors.iter().find(|e| e.name() == downgrade.name))
                    .cloned()
                    .unwrap_or_else(|| self.named(self.old, &downgrade.name)),
                members: mapping
                    .iter()
                    .map(|(old, new)| (old.name.clone(), new.name.clone()))
                    .collect(),
            },
            StepKind::Add {
                path,
                name: None,
                downgrade,
                ..
            } => {
                let (_, new) = self.both(path)?;
                VersionStep::AddEnumValue {
                    new: new.shape,
                    value: new.member.unwrap_or_default(),
                    downgrade: downgrade.name.clone(),
                }
            }
            StepKind::Move { operation, .. } => VersionStep::Move {
                operation: op_id(&operation.name, self.new)
                    .unwrap_or_else(|| self.id(&operation.name)),
            },
            StepKind::Reshape { path, hook, .. } => {
                let (old, new) = self.both(path)?;
                VersionStep::Hooked {
                    kind: "reshape".into(),
                    operations: operations_reaching(self.new, &new.shape),
                    hook: Hook::Reshape {
                        old: old.shape,
                        new: new.shape,
                    },
                    is_async: hook.is_async,
                }
            }
            StepKind::Retype { path, hook, .. } => {
                let (old, new) = self.both(path)?;
                VersionStep::Hooked {
                    kind: "retype".into(),
                    operations: operations_reaching(self.new, &new.shape),
                    hook: match hook {
                        Some(_) => Hook::Retype {
                            member: new.member.clone().unwrap_or_default(),
                            old: old.shape,
                            new: new.shape,
                        },
                        None => Hook::Conversion,
                    },
                    is_async: hook.is_some_and(|hook| hook.is_async),
                }
            }
            StepKind::RemoveOperation(name) => {
                let operation = op_id(&name.name, self.old).unwrap_or_else(|| self.id(&name.name));
                VersionStep::Hooked {
                    kind: "remove operation".into(),
                    operations: vec![operation.clone()],
                    hook: Hook::RemoveOperation { operation },
                    // A handler may always do I/O.
                    is_async: true,
                }
            }
            StepKind::Behavior(path) => VersionStep::Behavior {
                operation: op_id(&first(path), self.new).unwrap_or_else(|| self.id(&first(path))),
                name: path
                    .parts
                    .get(1)
                    .map_or_else(String::new, |p| p.name.clone()),
            },
            StepKind::Sunset(_) => unreachable!("handled by the caller"),
        })
    }
}
