//! The compatibility guard (decision 47d) and declared migrations
//! (decisions 51-54a): a package against the snapshot of its last
//! release. A breaking change compiles only when the migration from that
//! release declares it, and each declared step must match a change that
//! happened, so the migration and the model cannot drift apart.

use std::path::Path;

use std::collections::{BTreeMap, BTreeSet};

use crucible_ir::ShapeId;
use crucible_ir::ir::{
    ApiVersion, Hook, Operation, Paginated, RetypeBy, Service, Shape, VersionStep,
};
use crucible_policy::{Config, Finding, Renames, Severity};
use serde_json::Value as Json;

use crate::ast::{Decl, Doc, Dotted, Ident, Key, Migration, RequireHow, Step, StepKind, Value};
use crate::compile::{Compiled, FileDiagnostic, Package};
use crate::diagnostic::Diagnostic;
use crate::literal::{duplicate_keys, resolve_enum_symbols};

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
    guard_in(package, compiled, own, config, &crate::Releases::default())
}

/// [`guard`], reading frozen releases through `releases`, which a build
/// shares with [`versions_in`] and an editor keeps across compiles.
pub fn guard_in(
    package: &Package,
    compiled: &Compiled,
    own: usize,
    config: &Config,
    releases: &crate::Releases,
) -> Option<Result<Guard, String>> {
    let service = compiled.service.as_ref()?;
    let old = match releases.load(package.baseline.as_deref()?) {
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
    // Each enum value change no step declares, by enum, for its finding.
    let mut unexplained = BTreeMap::new();
    // An operation a migration drops without `remove operation` is refused
    // at the migration, or not served at older releases when an exception
    // accepts the drop, in every migration of the chain: the diff below
    // sees only the newest. A chain that does not load is `versions`' to
    // report.
    if let Ok(links) = chain(package, compiled, own, releases) {
        for link in &links {
            for drop in dropped(link, service, config) {
                // The refusal at the migration says what the undeclared
                // finding would.
                if !drop.excepted && link.migration.from.value == version {
                    covers.push(Cover::exact(drop.operation, &["compatibility.operation"]));
                }
                problems.push(FileDiagnostic {
                    file: Some(link.file),
                    diagnostic: drop.diagnostic,
                });
            }
        }
    }
    // A member the migration renames is compared with its new self, so the
    // rename declares its name and nothing else about it.
    let mut renames = Renames::new();
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
        let context = Context::new(&compiled.namespace, &old, service, migration);
        renames = context.renames.clone();
        unexplained = context.unexplained.clone();
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
        covers.extend(context.pagination_covers());
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

    let report = crucible_policy::diff_renamed(&old, service, config, &renames);
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
            let mut finding = finding;
            if finding.rule == "compatibility.enum"
                && let Some(values) = finding
                    .shape
                    .parse::<ShapeId>()
                    .ok()
                    .and_then(|id| unexplained.get(&id))
            {
                finding.message = format!(
                    "{}; no step declares: {}",
                    finding.message,
                    values.join(", ")
                );
            }
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
/// A rename changes a member's name, which only generated code sees: the
/// checker compares the renamed member with its new self, so any other
/// change to it needs its own step.
const RENAME: &[&str] = &["compatibility.sdk"];
const REQUIRE: &[&str] = &[
    "compatibility.required",
    "compatibility.nullability",
    "compatibility.default",
    "compatibility.sdk",
];
const ENUM_VALUE: &[&str] = &["compatibility.enum", "compatibility.sdk"];
const PAGINATION: &[&str] = &["compatibility.pagination"];
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
    /// The migration's renames that name a member of both releases.
    renames: Renames,
    /// The members the migration retypes, as the current model names them.
    retyped: BTreeSet<(ShapeId, String)>,
    /// What the migration's renames and additions of enum values leave
    /// ambiguous, at the step that does it ([`Context::enum_changes`]).
    enum_problems: Vec<(crate::span::Span, String)>,
    /// For each enum those steps change, the values whose removal or
    /// addition no step declares: its `compatibility.enum` finding stands,
    /// naming them. An enum missing here is wholly explained.
    unexplained: BTreeMap<ShapeId, Vec<String>>,
}

/// One enum's value changes a migration declares: its renamed values
/// (older name, newer name) and its added ones (value, downgrade), each at
/// its step.
#[derive(Default)]
struct EnumChanges {
    old: Option<ShapeId>,
    renames: Vec<(String, String, crate::span::Span)>,
    adds: Vec<(String, String, crate::span::Span)>,
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

impl<'a> Context<'a> {
    fn new(namespace: &'a str, old: &'a Service, new: &'a Service, migration: &Migration) -> Self {
        let mut context = Self {
            namespace,
            old,
            new,
            renames: Renames::new(),
            retyped: BTreeSet::new(),
            enum_problems: Vec::new(),
            unexplained: BTreeMap::new(),
        };
        for step in &migration.steps {
            if let StepKind::Retype { path, .. } = &step.kind
                && let Some(Target {
                    shape,
                    member: Some(member),
                }) = context.resolve(context.new, path)
            {
                context.retyped.insert((shape, member));
            }
            if let StepKind::Rename { path, to } = &step.kind
                && let Ok((old, new)) = context.both(path)
                && let Some(from) = old.member
                && context.has_member(context.old, &old.shape, &from)
                && context.has_member(context.new, &new.shape, &to.name)
            {
                context
                    .renames
                    .insert((old.shape, from), (new.shape, to.name.clone()));
            }
        }
        context.enum_changes(migration);
        context
    }

    /// The wire value of each value of enum `shape` in `service`, by name.
    fn wires(service: &Service, shape: &ShapeId) -> Option<BTreeMap<String, String>> {
        Some(match service.shapes.get(shape)? {
            Shape::Enum(e) => e
                .values
                .iter()
                .map(|value| (value.name.clone(), value.value.clone()))
                .collect(),
            Shape::IntEnum(e) => e
                .values
                .iter()
                .map(|value| (value.name.clone(), value.value.to_string()))
                .collect(),
            _ => return None,
        })
    }

    /// Composes, for each enum, the value renames and additions the
    /// migration declares, as the server applies them: one lookup per value
    /// each way. Refuses, at a step taking part, a composition that sends
    /// one newer value to two older ones, or two older values to one newer
    /// one (an older client's two values would read the same). Records the
    /// values whose removal or addition no step declares, so the enum's
    /// `compatibility.enum` finding is covered only when the steps explain
    /// the whole change.
    fn enum_changes(&mut self, migration: &Migration) {
        let mut changes: BTreeMap<ShapeId, EnumChanges> = BTreeMap::new();
        for step in &migration.steps {
            match &step.kind {
                StepKind::Rename { path, to } => {
                    if let Ok((old, new)) = self.both(path)
                        && let Some(from) = old.member
                        && Self::wires(self.old, &old.shape).is_some()
                        && Self::wires(self.new, &new.shape).is_some()
                    {
                        let entry = changes.entry(new.shape).or_default();
                        entry.old = Some(old.shape);
                        entry.renames.push((from, to.name.clone(), path.span));
                    }
                }
                StepKind::Add {
                    path,
                    name: None,
                    downgrade,
                    ..
                } => {
                    if let Ok((old, new)) = self.both(path)
                        && let Some(value) = new.member
                        && Self::wires(self.old, &old.shape).is_some()
                        && Self::wires(self.new, &new.shape).is_some()
                    {
                        let entry = changes.entry(new.shape).or_default();
                        entry.old = Some(old.shape);
                        entry.adds.push((value, downgrade.name.clone(), path.span));
                    }
                }
                _ => {}
            }
        }
        for (shape, change) in changes {
            let (Some(older), Some(newer)) = (
                change
                    .old
                    .as_ref()
                    .and_then(|old| Self::wires(self.old, old)),
                Self::wires(self.new, &shape),
            ) else {
                continue;
            };
            let enum_name = shape.name();
            // Upgrade: what each older value becomes. Two reaching one
            // newer value could not be told apart again.
            let mut reached: BTreeMap<&str, Vec<(&str, Option<crate::span::Span>)>> =
                BTreeMap::new();
            for (name, wire) in &older {
                let target = match change.renames.iter().find(|(from, ..)| from == name) {
                    Some((_, to, span)) => newer.get(to).map(|wire| (wire.as_str(), Some(*span))),
                    None => newer
                        .values()
                        .find(|value| *value == wire)
                        .map(|wire| (wire.as_str(), None)),
                };
                if let Some((target, span)) = target {
                    reached.entry(target).or_default().push((name, span));
                }
            }
            for (target, sources) in &reached {
                if let [first, second, ..] = sources.as_slice()
                    && let Some(span) = sources.iter().find_map(|(_, span)| *span)
                {
                    self.enum_problems.push((
                        span,
                        format!(
                            "older clients' {enum_name}.{} and {enum_name}.{} would both be read as `{target}`; rename to a value the release did not have",
                            first.0, second.0
                        ),
                    ));
                }
            }
            // Downgrade: what older clients see of each newer value. Two
            // steps sending it two ways is ambiguous.
            for (name, wire) in &newer {
                let mut seen: Vec<(&str, Option<crate::span::Span>)> = change
                    .renames
                    .iter()
                    .filter(|(_, to, _)| to == name)
                    .map(|(from, _, span)| (from.as_str(), Some(*span)))
                    .chain(
                        change
                            .adds
                            .iter()
                            .filter(|(value, ..)| value == name)
                            .map(|(_, downgrade, span)| (downgrade.as_str(), Some(*span))),
                    )
                    .collect();
                if seen.is_empty() {
                    continue;
                }
                // A value the release already had stays itself, unless a
                // rename took it away.
                if let Some((kept, _)) = older.iter().find(|(old, value)| {
                    *value == wire && !change.renames.iter().any(|(from, ..)| from == *old)
                }) {
                    seen.push((kept, None));
                }
                let names: BTreeSet<&str> = seen.iter().map(|(name, _)| *name).collect();
                let names: Vec<&str> = names.into_iter().collect();
                if let [first, second, ..] = names.as_slice()
                    && let Some(span) = seen.iter().find_map(|(_, span)| *span)
                {
                    self.enum_problems.push((
                        span,
                        format!(
                            "{enum_name}.{name} would reach older clients as both {enum_name}.{first} and {enum_name}.{second}; declare one step for it"
                        ),
                    ));
                }
            }
            // What changed on the wire that no step declares.
            let explained_old: BTreeSet<&str> = change
                .renames
                .iter()
                .filter_map(|(from, ..)| older.get(from).map(String::as_str))
                .collect();
            let explained_new: BTreeSet<&str> = change
                .renames
                .iter()
                .map(|(_, to, _)| to)
                .chain(change.adds.iter().map(|(value, ..)| value))
                .filter_map(|name| newer.get(name).map(String::as_str))
                .collect();
            let older_wires: BTreeSet<&str> = older.values().map(String::as_str).collect();
            let newer_wires: BTreeSet<&str> = newer.values().map(String::as_str).collect();
            let mut missing: Vec<String> = older
                .iter()
                .filter(|(_, wire)| {
                    !newer_wires.contains(wire.as_str()) && !explained_old.contains(wire.as_str())
                })
                .map(|(name, _)| format!("{name} removed"))
                .collect();
            missing.extend(
                newer
                    .iter()
                    .filter(|(_, wire)| {
                        !older_wires.contains(wire.as_str())
                            && !explained_new.contains(wire.as_str())
                    })
                    .map(|(name, _)| format!("{name} added")),
            );
            if !missing.is_empty() {
                self.unexplained.insert(shape, missing);
            }
        }
    }

    /// What `enum_changes` found wrong with the step at `span`.
    fn enum_problem(&self, span: crate::span::Span) -> Option<Diagnostic> {
        self.enum_problems
            .iter()
            .find(|(at, _)| *at == span)
            .map(|(at, message)| error(*at, message.clone()))
    }

    /// The baseline's name for `member` of `shape`: the one a rename in
    /// this migration gave it, else its own.
    fn old_name<'m>(&'m self, shape: &ShapeId, member: &'m str) -> &'m str {
        self.renames
            .iter()
            .find(|(_, (new, to))| new == shape && to == member)
            .map_or(member, |((_, from), _)| from.as_str())
    }

    /// What a rename in this migration calls the baseline's `member`.
    fn renamed(&self, shape: &ShapeId, member: &str) -> Option<&str> {
        self.renames
            .get(&(shape.clone(), member.to_owned()))
            .map(|(_, to)| to.as_str())
    }

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
            let (default, collisions) = self.require_default(&target, value);
            if !collisions.is_empty() {
                return Err(collisions
                    .into_iter()
                    .map(|problem| error(value.span(), problem))
                    .collect());
            }
            // `require` makes the member required, and a required member is
            // never null: not even a document, whose null is a value only
            // where the member is optional (as nested defaults are checked).
            if default.is_null() {
                return Err(vec![error(
                    value.span(),
                    format!(
                        "the migration default for `{}` must not be null: `require` makes it required",
                        member.name
                    ),
                )]);
            }
            let problems: Vec<_> = crucible_ir::validate::value(
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

    fn require_default(&self, target: &Target, value: &Value) -> (Json, Vec<String>) {
        // Enum symbols, at any depth, become their wire values, as in a
        // member's default: validation and the runtime conversion must see
        // the same value.
        let mut default = json(value);
        // The plain conversion keeps a key's last value, so a key given
        // twice (`{ RED: "x", "RED": "y" }`) would drop one unseen; a
        // member's default refuses it while converting.
        let mut problems = duplicate_keys(value);
        if let Some(Shape::Structure(owner)) = self.new.shapes.get(&target.shape)
            && let Some(member) = target.member.as_ref().and_then(|name| owner.member(name))
        {
            problems.extend(resolve_enum_symbols(
                &self.new.shapes,
                &member.target,
                value,
                &mut default,
            ));
        }
        (default, problems)
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
                let mut covers = vec![
                    Cover::exact(old.shape.with_member(from), RENAME),
                    Cover::exact(new.shape.with_member(&to.name), RENAME),
                ];
                // A string value and an integer one differ in their JSON
                // type, whatever their text: that is a retype, which a
                // rename does not convert.
                let kind = |service: &Service, shape: &ShapeId| match service.shapes.get(shape) {
                    Some(Shape::Enum(_)) => Some("an enum"),
                    Some(Shape::IntEnum(_)) => Some("an integer enum"),
                    _ => None,
                };
                if let (Some(before), Some(after)) =
                    (kind(self.old, &old.shape), kind(self.new, &new.shape))
                    && before != after
                {
                    return Err(error(
                        path.span,
                        format!(
                            "{} is {before} in the baseline and {after} now, so its values change JSON type; a `rename` cannot convert that",
                            new.shape.name()
                        ),
                    ));
                }
                // An enum value renamed: the checker sees the enum lose one
                // value and gain another, which the rename declares. The
                // server maps a string enum's wire value both ways; an
                // intEnum's number is its wire value, which is not mapped.
                let number = |service: &Service, shape: &ShapeId, name: &str| match service
                    .shapes
                    .get(shape)
                {
                    Some(Shape::IntEnum(e)) => e
                        .values
                        .iter()
                        .find(|value| value.name == name)
                        .map(|value| value.value),
                    _ => None,
                };
                if let (Some(before), Some(after)) = (
                    number(self.old, &old.shape, from),
                    number(self.new, &new.shape, &to.name),
                ) && before != after
                {
                    return Err(error(
                        path.span,
                        format!(
                            "renaming the intEnum value `{from}` to `{}` changes its number from {before} to {after}, which older clients send; keep it {before}",
                            to.name
                        ),
                    ));
                }
                let is_enum = |service: &Service, shape: &ShapeId| {
                    matches!(
                        service.shapes.get(shape),
                        Some(Shape::Enum(_) | Shape::IntEnum(_))
                    )
                };
                if is_enum(self.old, &old.shape) && is_enum(self.new, &new.shape) {
                    if let Some(problem) = self.enum_problem(path.span) {
                        return Err(problem);
                    }
                    // The enum's own finding is this migration's when its
                    // steps explain every value that changed on the wire.
                    if !self.unexplained.contains_key(&new.shape) {
                        covers.push(Cover::exact(old.shape.clone(), ENUM_VALUE));
                        covers.push(Cover::exact(new.shape.clone(), ENUM_VALUE));
                    }
                }
                Ok(covers)
            }
            StepKind::Require { path, how } => {
                let (old, new) = self.both(path)?;
                let Some(member) = &new.member else {
                    return Err(error(
                        path.span,
                        "`require` names a member: `Operation.input.member`".into(),
                    ));
                };
                if let Some(to) = self.renamed(&old.shape, member) {
                    return Err(error(
                        path.span,
                        format!(
                            "`{member}` is renamed to `{to}` in this migration; `require` names the member as it is now, `{to}`"
                        ),
                    ));
                }
                // A member renamed in this migration had its old name in the
                // baseline: `rename S.a -> b` with `require S.b default v`.
                let was = self.old_name(&new.shape, member);
                if was != member && matches!(how, RequireHow::Hook(_)) {
                    return Err(error(
                        path.span,
                        format!(
                            "`{member}` is renamed from `{was}` in this migration, which a hooked requirement cannot follow yet; declare `require .. default`"
                        ),
                    ));
                }
                // The hook fills the member with the next version's value
                // before the other conversions run, so a retype would
                // convert a value that is already converted.
                if matches!(how, RequireHow::Hook(_))
                    && self.retyped.contains(&(new.shape.clone(), member.clone()))
                {
                    return Err(error(
                        path.span,
                        format!(
                            "`{}.{member}` is both required by hook and retyped in this migration; declare one step for it",
                            new.shape.name()
                        ),
                    ));
                }
                let optional = |service: &Service, shape: &ShapeId, name: &str| match service
                    .shapes
                    .get(shape)
                {
                    Some(Shape::Structure(s)) => s.member(name).map(|m| m.optional_on_server()),
                    _ => None,
                };
                match (
                    optional(self.old, &old.shape, was),
                    optional(self.new, &new.shape, member),
                ) {
                    (Some(true), Some(false)) => Ok(vec![
                        Cover::exact(old.shape.with_member(was), REQUIRE),
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
                    if let Some(problem) = self.enum_problem(path.span) {
                        return Err(problem);
                    }
                    let mut covers = vec![Cover::exact(new.shape.with_member(value), ENUM_VALUE)];
                    // As for a rename: the enum's own finding only when the
                    // steps explain its whole change.
                    if !self.unexplained.contains_key(&new.shape) {
                        covers.push(Cover::exact(old.shape.clone(), ENUM_VALUE));
                    }
                    Ok(covers)
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
                if let Some(to) = self.renamed(&old.shape, member) {
                    return Err(error(
                        path.span,
                        format!(
                            "`{member}` is renamed to `{to}` in this migration; `retype` names the member as it is now, `{to}`"
                        ),
                    ));
                }
                let was = self.old_name(&new.shape, member);
                let target = |service: &Service, shape: &ShapeId, name: &str| match service
                    .shapes
                    .get(shape)
                {
                    Some(Shape::Structure(s)) | Some(Shape::Union(s)) => {
                        s.member(name).map(|m| m.target.clone())
                    }
                    _ => None,
                };
                // A built-in is written `string` and lowered to `crucible.api#String`.
                let named = |id: &ShapeId| {
                    id.name() == to.name
                        || (id.is_prelude() && id.name().eq_ignore_ascii_case(&to.name))
                };
                match (
                    target(self.old, &old.shape, was),
                    target(self.new, &new.shape, member),
                ) {
                    (Some(left), Some(now)) if named(&now) && left != now => Ok(vec![
                        Cover::exact(old.shape.with_member(was), RETYPE),
                        Cover::exact(new.shape.with_member(member), RETYPE),
                        // The checker reports the change on the type it left.
                        Cover::exact(left, RETYPE),
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

    /// Renames that account for a paginated operation's whole pagination
    /// change declare it: renaming the input token (or the output token,
    /// items or page size) changes the contract, and the rename step is
    /// what converts the member for older callers. A change the renames do
    /// not explain stays undeclared.
    fn pagination_covers(&self) -> Vec<Cover> {
        if self.renames.is_empty() {
            return Vec::new();
        }
        // A pagination path's first segment is a member of the operation's
        // input or output; what follows is below it.
        let renamed = |shape: &Option<ShapeId>, path: &str| -> String {
            let (head, rest) = path
                .split_once('.')
                .map_or((path, None), |(h, r)| (h, Some(r)));
            let head = shape
                .as_ref()
                .and_then(|shape| self.renamed(shape, head))
                .unwrap_or(head);
            rest.map_or_else(|| head.to_owned(), |rest| format!("{head}.{rest}"))
        };
        self.old
            .operations
            .iter()
            .filter_map(|old| {
                let before = old.paginated.as_ref()?;
                let now = self
                    .new
                    .operations
                    .iter()
                    .find(|op| op.id == old.id)?
                    .paginated
                    .as_ref()?;
                let mapped = Paginated {
                    input_token: renamed(&old.input, &before.input_token),
                    output_token: renamed(&old.output, &before.output_token),
                    items: before.items.as_deref().map(|p| renamed(&old.output, p)),
                    page_size: before.page_size.as_deref().map(|p| renamed(&old.input, p)),
                };
                (before != now && &mapped == now).then(|| Cover::exact(old.id.clone(), PAGINATION))
            })
            .collect()
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

/// One migration of the chain, with the two releases it joins.
struct Link<'c> {
    /// The source file that declares it.
    file: usize,
    migration: &'c Migration,
    doc: Option<&'c Doc>,
    /// Its `from` release, frozen.
    old: std::sync::Arc<Service>,
    /// The release it serves, frozen; `None` for the current model.
    newer: Option<std::sync::Arc<Service>>,
}

impl Link<'_> {
    /// The release the migration serves, given the current model.
    fn newer<'s>(&'s self, current: &'s Service) -> &'s Service {
        self.newer.as_deref().unwrap_or(current)
    }
}

/// The chain of migrations back from the service's version, newest first,
/// each `from` a release frozen beside the baseline
/// (`releases/<version>.json`).
fn chain<'c>(
    package: &Package,
    compiled: &'c Compiled,
    own: usize,
    loaded: &crate::Releases,
) -> Result<Vec<Link<'c>>, String> {
    let (Some(service), Some(baseline)) = (&compiled.service, &package.baseline) else {
        return Ok(Vec::new());
    };
    let releases = baseline.parent().unwrap_or(baseline);
    let migrations: Vec<(usize, &Migration, Option<&Doc>)> = compiled
        .parsed
        .iter()
        .take(own)
        .enumerate()
        .flat_map(|(file, parsed)| parsed.file.items.iter().map(move |item| (file, item)))
        .filter_map(|(file, item)| match &item.decl {
            Decl::Migration(migration) => Some((file, migration, item.doc.as_ref())),
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    let mut newer: Option<std::sync::Arc<Service>> = None;
    let mut seen = BTreeSet::new();
    while let Some(&(file, migration, doc)) = migrations.iter().find(|(_, migration, _)| {
        migration.version.value == newer.as_deref().unwrap_or(service).version
    }) {
        let name = migration.from.value.clone();
        if !seen.insert(name.clone()) {
            return Err(format!("the migrations from \"{name}\" form a cycle"));
        }
        let path = releases.join(crate::release::file_name(&name)?);
        if !path.is_file() {
            return Err(format!(
                "release \"{name}\", which the migration to \"{}\" comes from, is not frozen at {}; freeze it with `crucible freeze` (`--from <git ref>` for a past release)",
                migration.version.value,
                path.display()
            ));
        }
        let old = loaded.load(&path)?;
        // The chain follows releases by name: a release frozen under another
        // version's name would serve one version's contract as another's.
        if old.version != name {
            return Err(format!(
                "the frozen release {} declares version \"{}\"; its file must be named for it",
                path.display(),
                old.version
            ));
        }
        let next = Some(std::sync::Arc::clone(&old));
        out.push(Link {
            file,
            migration,
            doc,
            old,
            newer: std::mem::replace(&mut newer, next),
        });
    }
    Ok(out)
}

/// An operation a migration drops without `remove operation`.
struct Dropped {
    operation: ShapeId,
    /// A `compatibility.operation` exception in the manifest accepts it.
    excepted: bool,
    /// At the migration's `from`: an error, or a warning when excepted.
    diagnostic: Diagnostic,
}

/// The operations `link`'s `from` release has, which the release it serves
/// does not, and which no `remove operation` step keeps serving. Nothing
/// would answer them at `from` and before, so each is refused unless the
/// manifest excepts `compatibility.operation` for it: then the older
/// releases stop serving it, which a warning says (decision 70). The
/// manifest's exceptions apply to every migration, as frozen releases do
/// not record them (decision 68); its settings do not.
fn dropped(link: &Link, current: &Service, config: &Config) -> Vec<Dropped> {
    let removed: BTreeSet<&str> = link
        .migration
        .steps
        .iter()
        .filter_map(|step| match &step.kind {
            StepKind::RemoveOperation(name) => Some(name.name.as_str()),
            _ => None,
        })
        .collect();
    let (from, to) = (&link.migration.from, &link.migration.version.value);
    link.old
        .operations
        .iter()
        .filter(|old| {
            !link
                .newer(current)
                .operations
                .iter()
                .any(|op| op.id == old.id)
        })
        .filter(|old| !removed.contains(old.name()))
        .map(|old| {
            let (name, id) = (old.name(), old.id.to_string());
            let excepted = config
                .exceptions
                .iter()
                .any(|exception| exception.rule == "compatibility.operation" && exception.shape == id);
            let diagnostic = if excepted {
                Diagnostic::warning(
                    from.span,
                    format!(
                        "{name} is not in \"{to}\" and this migration has no `remove operation {name}`; under the exception for `compatibility.operation`, \"{}\" and the releases before it no longer serve it",
                        from.value
                    ),
                )
            } else {
                Diagnostic::new(
                    from.span,
                    format!(
                        "{name} is not in \"{to}\" and this migration has no `remove operation {name}`, so nothing would serve it at \"{}\"; declare `remove operation {name}` and its handler, or except `compatibility.operation` for {id} to stop serving it there and before",
                        from.value
                    ),
                )
            };
            Dropped {
                operation: old.id.clone(),
                excepted,
                diagnostic,
            }
        })
        .collect()
}

/// The older versions `compiled` still serves, oldest first: the chain of
/// migrations back from the service's version. `guard` has checked the
/// newest migration's steps; an older one's were checked when it was the
/// newest. `config` is the manifest's policy, whose exceptions accept an
/// operation dropped without `remove operation` ([`dropped`]).
pub fn versions(
    package: &Package,
    compiled: &Compiled,
    own: usize,
    config: &Config,
) -> Result<Vec<ApiVersion>, String> {
    versions_in(package, compiled, own, config, &crate::Releases::default())
}

/// [`versions`], reading frozen releases through `releases`: the build
/// passes the ones its guard loaded.
pub fn versions_in(
    package: &Package,
    compiled: &Compiled,
    own: usize,
    config: &Config,
    releases: &crate::Releases,
) -> Result<Vec<ApiVersion>, String> {
    let Some(service) = compiled.service.as_ref() else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for link in chain(package, compiled, own, releases)? {
        let to = &link.migration.version.value;
        let mut retired = Vec::new();
        for drop in dropped(&link, service, config) {
            if !drop.excepted {
                return Err(format!(
                    "the migration to \"{to}\": {}",
                    drop.diagnostic.message
                ));
            }
            retired.push(drop.operation);
        }
        let context = Context::new(
            &compiled.namespace,
            &link.old,
            link.newer(service),
            link.migration,
        );
        let mut steps = Vec::new();
        let mut sunset = None;
        let mut sunset_docs = None;
        for step in &link.migration.steps {
            // A sunset is the release's, not a conversion: it and its
            // documentation go on the version.
            if let StepKind::Sunset(date) = &step.kind {
                sunset = Some(date.value.clone());
                sunset_docs = doc_text(step.doc.as_ref());
                continue;
            }
            steps.push(context.lower(step).map_err(|diagnostic| {
                format!("the migration to \"{to}\": {}", diagnostic.message)
            })?);
        }
        out.push(ApiVersion {
            name: link.migration.from.value.clone(),
            service: Box::new((*link.old).clone()),
            steps,
            sunset,
            sunset_docs,
            docs: doc_text(link.doc),
            retired,
            external: false,
        });
    }
    out.reverse();
    Ok(out)
}

/// A `///` comment's text, as the IR keeps documentation.
fn doc_text(doc: Option<&Doc>) -> Option<String> {
    doc.map(|doc| doc.text.clone())
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
    /// A checked step, lowered with its paths resolved, both its sides and
    /// its documentation.
    fn lower(&self, step: &Step) -> Result<VersionStep, Diagnostic> {
        let docs = doc_text(step.doc.as_ref());
        let step = &step.kind;
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
                    docs,
                }
            }
            StepKind::Require { path, how } => {
                let (old, new) = self.both(path)?;
                let member = new.member.clone().unwrap_or_default();
                // Empty when the member kept its name.
                let from = match self.old_name(&new.shape, &member) {
                    was if was == member => String::new(),
                    was => was.to_owned(),
                };
                match how {
                    RequireHow::Default(value) => {
                        let (default, _) = self.require_default(&new, value);
                        let Some(Shape::Structure(owner)) = self.new.shapes.get(&new.shape) else {
                            unreachable!("a checked requirement targets a structure member")
                        };
                        let target = owner.member(&member).expect("a checked requirement member");
                        // Model literals accept epoch seconds and model member
                        // names; conversions inject protocol document values.
                        let default = self.new.wire_example(target, &default, None);
                        VersionStep::Require {
                            old: old.shape,
                            new: new.shape,
                            member,
                            from,
                            default,
                            docs,
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
                            from,
                        },
                        is_async: hook.is_async,
                        docs,
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
                docs,
            },
            StepKind::Add {
                path,
                name: None,
                downgrade,
                ..
            } => {
                let (old, new) = self.both(path)?;
                VersionStep::AddEnumValue {
                    old: old.shape,
                    new: new.shape,
                    value: new.member.unwrap_or_default(),
                    downgrade: downgrade.name.clone(),
                    docs,
                }
            }
            StepKind::Move { operation, .. } => {
                let older = self::operation(self.old, &operation.name)
                    .expect("a checked move names an operation of the baseline");
                VersionStep::Move {
                    operation: op_id(&operation.name, self.new)
                        .unwrap_or_else(|| self.id(&operation.name)),
                    old_method: older.method.clone(),
                    old_uri: older.uri.clone(),
                    docs,
                }
            }
            StepKind::Reshape {
                path,
                members,
                into,
                hook,
            } => {
                let (old, new) = self.both(path)?;
                VersionStep::Hooked {
                    kind: "reshape".into(),
                    operations: operations_reaching(self.new, &new.shape),
                    hook: Hook::Reshape {
                        old: old.shape,
                        new: new.shape,
                        members: members.iter().map(|member| member.name.clone()).collect(),
                        into: into.name.clone(),
                    },
                    is_async: hook.is_async,
                    docs,
                }
            }
            StepKind::Retype { path, hook, .. } => {
                let (old, new) = self.both(path)?;
                let member = new.member.clone().unwrap_or_default();
                // Empty when the member kept its name.
                let from = match self.old_name(&new.shape, &member) {
                    was if was == member => String::new(),
                    was => was.to_owned(),
                };
                VersionStep::Hooked {
                    kind: "retype".into(),
                    operations: operations_reaching(self.new, &new.shape),
                    hook: Hook::Retype {
                        member,
                        from,
                        old: old.shape,
                        new: new.shape,
                        by: match hook {
                            Some(_) => RetypeBy::Hook,
                            None => RetypeBy::Conversion,
                        },
                    },
                    is_async: hook.is_some_and(|hook| hook.is_async),
                    docs,
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
                    docs,
                }
            }
            StepKind::Behavior(path) => VersionStep::Behavior {
                operation: op_id(&first(path), self.new).unwrap_or_else(|| self.id(&first(path))),
                name: path
                    .parts
                    .get(1)
                    .map_or_else(String::new, |p| p.name.clone()),
                docs,
            },
            StepKind::Sunset(_) => unreachable!("handled by the caller"),
        })
    }
}
