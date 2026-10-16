//! The older releases a package serves, each the current model projected
//! back to it (docs/api-versions-clients.md, 5b; decisions 68-71 in
//! docs/idl/decisions.md), and every problem found building them.
//!
//! The chain is built newest first. For the migration `r_i -> r_i+1`, each
//! step's newer side resolves in the projection of `r_i+1` (the current
//! model for the newest), its older side in frozen `r_i`; undo builds the
//! projection of `r_i`; every step is checked again against the two
//! projections, so what a step names on its older side survives; and the
//! projection is compared with its frozen release by the guard's own
//! comparison. Names an older release still uses are refused where they
//! are reused. Every problem is collected, with its place in the source.

use std::collections::{BTreeMap, BTreeSet};

use crucible_ir::ShapeId;
use crucible_ir::ir::{ApiVersion, Hook, Service, Shape, VersionStep};
use crucible_ir::versions::{has, named, subjects};
use crucible_policy::{Config, Exception, Finding, Severity as PolicySeverity};

use super::{Context, Guard, Link, capitalized, chain, doc_text, dropped, render};
use crate::ast::{Decl, Migration, OpEntry, Step, StepKind};
use crate::compile::{Compiled, FileDiagnostic, Package};
use crate::diagnostic::Diagnostic;
use crate::lexer::TokenKind;
use crate::span::Span;

/// The older releases a package serves, oldest first, and every problem
/// found building and checking them.
#[derive(Debug, Clone, Default)]
pub struct Versions {
    /// Each older release, the steps that take it to the next and its
    /// frozen model.
    pub versions: Vec<ApiVersion>,
    /// Every problem, each at its place: an error fails the build.
    pub diagnostics: Vec<FileDiagnostic>,
    /// The rule and id of each finding of the projection check among
    /// `diagnostics`, by index, so [`Versions::without`] can leave out what
    /// the guard reports already.
    findings: BTreeMap<usize, (String, String)>,
}

impl Versions {
    /// Whether the versions fail the build.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|item| item.diagnostic.is_error())
    }

    /// Every problem, one per line, for a build log.
    pub fn render(&self, compiled: &Compiled) -> String {
        render(&self.diagnostics, compiled)
    }

    /// The versions, or every problem rendered when one is an error: for a
    /// tool that wants the versions or a message.
    pub fn into_result(self, compiled: &Compiled) -> Result<Vec<ApiVersion>, String> {
        if self.has_errors() {
            Err(self.render(compiled))
        } else {
            Ok(self.versions)
        }
    }

    /// Leaves out what `guard` reports already: the same problem at the
    /// same place (an operation the newest migration drops, a step of that
    /// migration), and a finding of the projection check the guard reports
    /// as an undeclared error. For the baseline a missing step is both
    /// (decision 68). A finding the guard's settings make a warning stays:
    /// the check runs at error.
    pub fn without(&mut self, guard: &Guard) {
        let undeclared: BTreeSet<(&str, &str)> = guard
            .undeclared
            .iter()
            .filter(|finding| finding.severity == PolicySeverity::Error)
            .map(|finding| (finding.rule.as_str(), finding.shape.as_str()))
            .collect();
        let mut kept = Vec::with_capacity(self.diagnostics.len());
        let mut findings = BTreeMap::new();
        for (index, item) in std::mem::take(&mut self.diagnostics)
            .into_iter()
            .enumerate()
        {
            let finding = self.findings.remove(&index);
            if guard.problems.contains(&item) {
                continue;
            }
            if let Some((rule, id)) = &finding
                && undeclared.contains(&(rule.as_str(), id.as_str()))
            {
                continue;
            }
            if let Some(finding) = finding {
                findings.insert(kept.len(), finding);
            }
            kept.push(item);
        }
        self.diagnostics = kept;
        self.findings = findings;
    }

    fn push(&mut self, file: Option<usize>, diagnostic: Diagnostic) {
        let item = FileDiagnostic { file, diagnostic };
        // One problem found two ways is shown once.
        if !self.diagnostics.contains(&item) {
            self.diagnostics.push(item);
        }
    }
}

/// The older versions `compiled` still serves, oldest first: the chain of
/// migrations back from the service's version, each release the current
/// model projected back to it, checked against its frozen self. `config`
/// is the manifest's policy: its exceptions apply to every release, its
/// settings to none (decision 68).
pub fn versions(package: &Package, compiled: &Compiled, own: usize, config: &Config) -> Versions {
    versions_in(package, compiled, own, config, &crate::Releases::default())
}

/// [`versions`], reading frozen releases through `releases`: the build
/// passes the ones its guard loaded, and an editor keeps them across
/// compiles.
pub fn versions_in(
    package: &Package,
    compiled: &Compiled,
    own: usize,
    config: &Config,
    releases: &crate::Releases,
) -> Versions {
    let mut out = Versions::default();
    let Some(service) = compiled.service.as_ref() else {
        return out;
    };
    let links = match chain(package, compiled, own, releases) {
        Ok(links) => links,
        Err(problem) => {
            out.push(None, Diagnostic::new(Span::default(), problem));
            return out;
        }
    };
    // Both families at error, with the manifest's exceptions (decision 68).
    let strict = Config {
        exceptions: config.exceptions.clone(),
        ..Config::default()
    };
    let mut shown = BTreeSet::new();
    let mut refused = BTreeSet::new();
    let mut projected: Option<Service> = None;
    for link in &links {
        let newer = projected.as_ref().unwrap_or(service);
        let mut release = Release {
            compiled,
            link,
            current: service,
            newer,
            config,
            strict: &strict,
            shown: &mut shown,
            refused: &mut refused,
            explained: BTreeSet::new(),
            out: &mut out,
        };
        let (version, projection) = release.build();
        out.versions.push(version);
        projected = Some(projection);
    }
    out.versions.reverse();
    out
}

/// One migration of the chain, `r_i -> r_i+1`, as the projection of `r_i`
/// is built and checked.
struct Release<'a, 'c> {
    compiled: &'c Compiled,
    link: &'a Link<'c>,
    current: &'c Service,
    /// The projection of `r_i+1`, or the current model.
    newer: &'a Service,
    config: &'a Config,
    strict: &'a Config,
    /// The findings shown so far, by rule and id, newest release first.
    shown: &'a mut BTreeSet<(String, String)>,
    /// The operations dropped without `remove operation` or an exception,
    /// and their own shapes, which that refusal explains at this release
    /// and every older one.
    refused: &'a mut BTreeSet<ShapeId>,
    /// The shapes a step that no longer resolves names, which its
    /// diagnostic explains at this release.
    explained: BTreeSet<ShapeId>,
    out: &'a mut Versions,
}

/// A checked step, lowered, with its source.
struct Lowered<'c> {
    step: &'c Step,
    lowered: VersionStep,
}

/// A step whose newer side the newer projection no longer has, though the
/// release it was written for did: what removed it since is named, and the
/// projection check decides whether the step is inert (decision 70).
struct Lost<'c> {
    step: &'c Step,
    lowered: VersionStep,
    gone: Vec<ShapeId>,
    exceptions: Vec<Exception>,
}

impl<'c> Release<'_, 'c> {
    fn name(&self) -> &'c str {
        &self.link.migration.from.value
    }

    fn to(&self) -> &'c str {
        &self.link.migration.version.value
    }

    /// The migration from the baseline, which the guard checks too.
    fn newest(&self) -> bool {
        self.link.newer.is_none()
    }

    /// How messages name the projection being built.
    fn projection_label(&self) -> String {
        format!("release \"{}\" as the current model sees it", self.name())
    }

    fn newer_label(&self) -> String {
        if self.newest() {
            "the current model".to_owned()
        } else {
            format!("release \"{}\" as the current model sees it", self.to())
        }
    }

    fn older_label(&self) -> String {
        if self.newest() {
            "the baseline".to_owned()
        } else {
            format!("release \"{}\"", self.name())
        }
    }

    fn error(&mut self, span: Span, message: String) {
        self.out
            .push(Some(self.link.file), Diagnostic::new(span, message));
    }

    /// The release's version and its projection.
    fn build(&mut self) -> (ApiVersion, Service) {
        let link = self.link;
        let migration: &'c Migration = link.migration;
        let compiled = self.compiled;
        let namespace = &compiled.namespace;

        // An operation dropped without `remove operation`.
        let mut retired = Vec::new();
        // What a refused drop takes from this projection and the older ones
        // is that refusal's.
        for drop in dropped(link, self.current, self.config) {
            if drop.excepted {
                retired.push(drop.operation.clone());
            } else if let Some(op) = link
                .old
                .operations
                .iter()
                .find(|op| op.id == drop.operation)
            {
                self.refused.insert(op.id.clone());
                self.refused
                    .extend(op.input.iter().chain(&op.output).cloned());
            }
            self.out.push(Some(link.file), drop.diagnostic);
        }

        // Each step, its newer side resolved in the newer projection.
        let context = Context::new(namespace, &link.old, self.newer, migration)
            .labels(self.older_label(), self.newer_label());
        let written = (!self.newest()).then(|| {
            Context::new(namespace, &link.old, link.newer(self.current), migration)
                .labels(self.older_label(), format!("release \"{}\"", self.to()))
        });
        let mut steps: Vec<Lowered<'c>> = Vec::new();
        let mut lost: Vec<Lost<'c>> = Vec::new();
        let mut sunset = None;
        let mut sunset_docs = None;
        for step in &migration.steps {
            // A sunset is the release's, not a conversion: it and its
            // documentation go on the version.
            if let StepKind::Sunset(date) = &step.kind {
                sunset = Some(date.value.clone());
                sunset_docs = doc_text(step.doc.as_ref());
                continue;
            }
            match context.lower(step) {
                Ok(lowered) => steps.push(Lowered { step, lowered }),
                Err(problems) => {
                    let problems = match written.as_ref().map(|written| written.lower(step)) {
                        // It held when it was written: a name it restores
                        // is taken since, or something it names is gone.
                        Some(Ok(lowered)) => {
                            let lowered = Lowered { step, lowered };
                            if !self.collisions(&lowered).0.is_empty() {
                                steps.push(lowered);
                            } else if let Some(lost_step) = self.lost(lowered, problems) {
                                lost.push(lost_step);
                            }
                            continue;
                        }
                        // Wrong as written, against the releases it joins.
                        Some(Err(written)) => written,
                        None => problems,
                    };
                    for problem in problems {
                        self.out.push(Some(link.file), problem);
                    }
                }
            }
        }

        // Names the release still uses, reused since (decision 69), at the
        // new declaration. What the projection check finds where the step
        // would restore is the collision's.
        let mut collided = BTreeSet::new();
        for (index, step) in steps.iter().enumerate() {
            let (found, roots) = self.collisions(step);
            if found.is_empty() {
                continue;
            }
            collided.insert(index);
            self.explained.extend(roots);
            for (id, message) in found {
                match self.declared(&id) {
                    Some((file, span)) => self.out.push(Some(file), Diagnostic::new(span, message)),
                    None => self.error(step_span(step.step), message),
                }
            }
        }

        let frozen: &Service = &link.old;
        // A step that would restore a name twice is left out of the undo:
        // its collision is the problem to report.
        let mut version = ApiVersion {
            name: self.name().to_owned(),
            service: Box::new(frozen.clone()),
            release: Box::new(frozen.clone()),
            steps: steps
                .iter()
                .enumerate()
                .filter(|(index, _)| !collided.contains(index))
                .map(|(_, step)| step.lowered.clone())
                .collect(),
            sunset,
            sunset_docs,
            docs: doc_text(link.doc),
            retired,
            external: false,
        };
        let mut errors = crucible_ir::Diagnostics::default();
        let mut warnings = crucible_ir::Diagnostics::default();
        let projection =
            crucible_ir::versions::undo(self.newer, &version, &mut errors, &mut warnings);
        version.steps = steps.iter().map(|step| step.lowered.clone()).collect();

        // What undo could not do, at the step that asked for it.
        let mut broken = collided.clone();
        for problem in errors.iter() {
            match self.step_naming(&steps, &problem.shape) {
                Some(index) if collided.contains(&index) => {}
                Some(index) => {
                    broken.insert(index);
                    let span = step_span(steps[index].step);
                    self.error(span, problem.message.clone());
                }
                None => self.error(link.migration.from.span, problem.message.clone()),
            }
        }
        // The IR's own checks: a collision without a message of its own
        // still fails.
        if collided.is_empty() {
            let mut problems = crucible_ir::Diagnostics::default();
            crucible_ir::validate::service(&projection, &mut problems);
            for problem in problems.iter() {
                let message = format!("{}: {}", self.projection_label(), problem.message);
                match self.declared(&problem.shape) {
                    Some((file, span)) => self.out.push(Some(file), Diagnostic::new(span, message)),
                    None => self.error(link.migration.from.span, message),
                }
            }
        }
        self.readded(&steps, &lost, &projection);
        self.examples(&projection, &warnings);

        // What each step names on its older side survives (decision 70).
        let again = Context::new(namespace, &projection, self.newer, migration)
            .labels(self.projection_label(), self.newer_label());
        for (index, step) in steps.iter().enumerate() {
            if broken.contains(&index) {
                continue;
            }
            if let Err(problems) = again.checked_step(&step.step.kind) {
                for problem in problems {
                    let message = format!(
                        "{}; what a step names on its older side must survive while release \"{}\" is supported: keep it, `@deprecated`, until that release is dropped",
                        problem.message,
                        self.name()
                    );
                    self.error(problem.span, message);
                }
            }
        }

        self.check(&steps, &collided, &lost, &projection);
        (version, projection)
    }

    /// A step that held against the release it was written for, whose
    /// newer side the newer projection lacks; `None` once it is reported.
    /// A step that fails for another reason since reports `problems`.
    fn lost(&mut self, step: Lowered<'c>, problems: Vec<Diagnostic>) -> Option<Lost<'c>> {
        let Lowered { step, lowered } = step;
        let gone: Vec<ShapeId> = named(&lowered)
            .newer
            .into_iter()
            .filter(|id| !has(self.newer, id))
            .collect();
        if gone.is_empty() {
            for problem in problems {
                self.out.push(Some(self.link.file), problem);
            }
            return None;
        }
        let exceptions: Vec<Exception> = self
            .config
            .exceptions
            .iter()
            .filter(|exception| {
                gone.iter().any(|id| {
                    exception.shape == id.to_string() || exception.shape == id.root().to_string()
                })
            })
            .cloned()
            .collect();
        // Shape identity is the shape id (decision 71): a shape renamed since
        // is recorded nowhere, so nothing can name its new id for the step.
        // An operation gone since was retired, which the check judges.
        let next = self.link.newer(self.current);
        if let Some(missing) = gone
            .iter()
            .map(ShapeId::root)
            .find(|root| !has(self.newer, root) && next.shapes.contains_key(root))
        {
            let message = format!(
                "`{}` names {missing}, which {} does not have: a shape renamed or removed since a step named it is not the shape the step converts, and nothing records its new id; keep {} while release \"{}\" is supported, or drop that release",
                self.text(step),
                self.newer_label(),
                missing.name(),
                self.name()
            );
            self.error(step_span(step), message);
            // What the projection check finds there is this.
            self.explained.insert(missing.clone());
            self.explained
                .extend(named(&lowered).older.iter().map(ShapeId::root));
            return None;
        }
        Some(Lost {
            step,
            lowered,
            gone,
            exceptions,
        })
    }

    /// Decision 69: each name the release still uses that the newer model
    /// gives to something else, by the new declaration's id, with its
    /// message; and the shapes the step would restore, whose differences
    /// the collision explains.
    fn collisions(&self, step: &Lowered<'c>) -> (Vec<(ShapeId, String)>, BTreeSet<ShapeId>) {
        let name = self.name().to_owned();
        let text = self.text(step.step);
        let to = self.to().to_owned();
        let mut found = Vec::new();
        match &step.lowered {
            VersionStep::Rename {
                new, from, to: now, ..
            } => {
                let older = new.with_member(from);
                if has(self.newer, &older) {
                    found.push((
                        older.clone(),
                        format!(
                            "`{}.{from}` is the older name of `{}.{now}`, which release \"{name}\" still uses (`{text}` in the migration to \"{to}\"): a request of that release would mean both; give the new member another name while \"{name}\" is supported",
                            new.name(),
                            new.name()
                        ),
                    ));
                }
            }
            VersionStep::Hooked {
                hook: Hook::RemoveOperation { operation },
                ..
            } => {
                if self.newer.operations.iter().any(|op| &op.id == operation) {
                    found.push((
                        operation.clone(),
                        format!(
                            "{} names an operation release \"{name}\" still serves as removed (`{text}` in the migration to \"{to}\"): that release would hold two operations named {}; give the new one another name while \"{name}\" is supported",
                            operation.name(),
                            operation.name()
                        ),
                    ));
                }
                if let Some(older) = self
                    .link
                    .old
                    .operations
                    .iter()
                    .find(|op| &op.id == operation)
                {
                    found.extend(self.routes(operation, &older.method, &older.uri, &text));
                }
            }
            VersionStep::Move {
                operation,
                old_method,
                old_uri,
                ..
            } => found.extend(self.routes(operation, old_method, old_uri, &text)),
            _ => {}
        }
        let (restored, copied) = self.restored_ids(&step.lowered, &text);
        found.extend(restored);
        let named = named(&step.lowered);
        let mut roots: BTreeSet<ShapeId> = named
            .older
            .iter()
            .chain(&named.newer)
            .map(ShapeId::root)
            .collect();
        roots.extend(copied);
        roots.extend(found.iter().map(|(id, _)| id.root()));
        (found, roots)
    }

    /// The operations of the newer model routed where `operation` was at
    /// the release: an older request would reach two.
    fn routes(
        &self,
        operation: &ShapeId,
        method: &str,
        uri: &crucible_ir::ir::Uri,
        text: &str,
    ) -> Vec<(ShapeId, String)> {
        let name = self.name();
        self.newer
            .operations
            .iter()
            .filter(|op| &op.id != operation)
            .filter(|op| crucible_ir::validate::same_route(&op.method, &op.uri, method, uri))
            .map(|op| {
                (
                    op.id.clone(),
                    format!(
                        "{} is routed at `{method} \"{uri}\"`, where release \"{name}\" still routes {} (`{text}` in the migration to \"{}\"): an older request would reach both; route it elsewhere while \"{name}\" is supported",
                        op.name(),
                        operation.name(),
                        self.to()
                    ),
                )
            })
            .collect()
    }

    /// The ids a definition `step` restores reaches, which the newer model
    /// declared again as a different shape since the release after this
    /// one dropped them: the restored definition and the new one would
    /// share an id. Only those ids: one that never left is the current
    /// shape at every release.
    /// Also every shape it copies from the release.
    fn restored_ids(
        &self,
        step: &VersionStep,
        text: &str,
    ) -> (Vec<(ShapeId, String)>, BTreeSet<ShapeId>) {
        let frozen: &Service = &self.link.old;
        let next = self.link.newer(self.current);
        // What undo copies from the release whole, as `undo` does.
        let mut copied: Vec<ShapeId> = Vec::new();
        let mut reached: Vec<ShapeId> = Vec::new();
        match step {
            VersionStep::Hooked {
                hook: Hook::Reshape { old, .. },
                ..
            } => copied.push(old.clone()),
            VersionStep::Hooked {
                hook: Hook::RemoveOperation { operation },
                ..
            } => {
                if let Some(op) = frozen.operations.iter().find(|op| &op.id == operation) {
                    copied.extend(op.input.iter().chain(&op.output).cloned());
                    copied.extend(
                        op.errors
                            .iter()
                            .filter(|error| !self.newer.shapes.contains_key(*error))
                            .cloned(),
                    );
                }
            }
            VersionStep::Hooked {
                hook: Hook::Retype {
                    old, member, from, ..
                },
                ..
            } => {
                let older = if from.is_empty() { member } else { from };
                if let Some(Shape::Structure(s) | Shape::Union(s)) = frozen.shapes.get(old)
                    && let Some(target) = s.member(older)
                {
                    reached.push(target.target.clone());
                }
            }
            _ => {}
        }
        let mut seen = BTreeSet::new();
        while let Some(id) = copied.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            let Some(shape) = frozen.shapes.get(&id) else {
                continue;
            };
            for member in shape.members() {
                if self.newer.shapes.contains_key(&member.target) {
                    reached.push(member.target.clone());
                } else {
                    copied.push(member.target.clone());
                }
            }
        }
        let name = self.name();
        let mut out = Vec::new();
        let mut checked = BTreeSet::new();
        for id in reached {
            if id.is_prelude() || !checked.insert(id.clone()) || next.shapes.contains_key(&id) {
                continue;
            }
            let (Some(then), Some(now)) = (frozen.shapes.get(&id), self.newer.shapes.get(&id))
            else {
                continue;
            };
            if structure(then) != structure(now) {
                out.push((
                    id.clone(),
                    format!(
                        "{} is declared again as a different shape, but release \"{name}\" still has the {} that `{text}` restores there: the two would share the id; give the new shape another name while \"{name}\" is supported",
                        id.name(),
                        id.name()
                    ),
                ));
            }
        }
        (out, seen)
    }

    /// Decision 69: a member the release had, which the next release
    /// dropped and the newer model declares again. It would be the older
    /// member at this release, out of its place in the release's line.
    fn readded(&mut self, steps: &[Lowered<'c>], lost: &[Lost<'c>], projection: &Service) {
        let frozen: &Service = &self.link.old;
        let next = self.link.newer(self.current);
        // A member renamed is not removed; one whose rename is lost since
        // has its own diagnostic.
        let renamed: BTreeSet<ShapeId> = steps
            .iter()
            .map(|step| &step.lowered)
            .chain(lost.iter().map(|lost| &lost.lowered))
            .filter_map(|step| match step {
                VersionStep::Rename { old, from, .. } => Some(old.with_member(from)),
                _ => None,
            })
            .collect();
        // What a reshape or a removal restores whole, which nothing added
        // since reaches (decision 74).
        let mut whole: BTreeSet<ShapeId> = BTreeSet::new();
        for step in steps {
            match &step.lowered {
                VersionStep::Hooked {
                    hook: Hook::Reshape { old, .. },
                    ..
                } => {
                    whole.insert(old.clone());
                }
                VersionStep::Hooked {
                    hook: Hook::RemoveOperation { operation },
                    ..
                } => {
                    if let Some(op) = frozen.operations.iter().find(|op| &op.id == operation) {
                        whole.extend(op.input.iter().chain(&op.output).cloned());
                    }
                }
                _ => {}
            }
        }
        let mut found = Vec::new();
        for (id, shape) in &frozen.shapes {
            let (Shape::Structure(s) | Shape::Union(s)) = shape else {
                continue;
            };
            if !next.shapes.contains_key(id) || whole.contains(id) {
                continue;
            }
            for member in &s.members {
                let member_id = id.with_member(&member.name);
                if renamed.contains(&member_id) || has(next, &member_id) {
                    continue;
                }
                if has(projection, &member_id) && has(self.current, &member_id) {
                    found.push(member_id);
                }
            }
        }
        let name = self.name().to_owned();
        for id in found {
            let member = id.member().unwrap_or_default().to_owned();
            let message = format!(
                "`{}.{member}` is added again, but release \"{name}\" still has the `{member}` the migration to \"{}\" removed: the two would share a name, and the new one would sit out of place in that release's line; give it another name while \"{name}\" is supported",
                id.root().name(),
                self.to()
            );
            match self.declared(&id) {
                Some((file, span)) => self.out.push(Some(file), Diagnostic::new(span, message)),
                None => self.error(self.link.migration.from.span, message),
            }
        }
    }

    /// The examples the projection left out, each a warning at the example.
    fn examples(&mut self, projection: &Service, warnings: &crucible_ir::Diagnostics) {
        for warning in warnings.iter() {
            let titles = self
                .newer
                .operations
                .iter()
                .chain(&self.link.old.operations)
                .chain(&projection.operations)
                .filter(|op| op.id == warning.shape)
                .flat_map(|op| op.examples.iter().map(|example| example.title.as_str()));
            let title = titles
                .into_iter()
                .find(|title| {
                    warning
                        .message
                        .starts_with(&format!("example {title:?} does not hold"))
                })
                .map(str::to_owned);
            let at = title
                .and_then(|title| self.example(&warning.shape, &title))
                .or_else(|| self.declared(&warning.shape));
            let diagnostic = |span| Diagnostic::warning(span, warning.message.clone());
            match at {
                Some((file, span)) => self.out.push(Some(file), diagnostic(span)),
                None => self.out.push(
                    Some(self.link.file),
                    diagnostic(self.link.migration.from.span),
                ),
            }
        }
    }

    /// Decision 68: the projection compared with its frozen release, both
    /// families at error, with the manifest's exceptions. A finding is an
    /// undo gap or a break the package neither declared nor excepted, at
    /// the step that names what it is about, else at the migration.
    fn check(
        &mut self,
        steps: &[Lowered<'c>],
        collided: &BTreeSet<usize>,
        lost: &[Lost<'c>],
        projection: &Service,
    ) {
        let report = crucible_policy::diff(&self.link.old, projection, self.strict);
        // A lost step's older side, and the findings there, open or excepted.
        let mut open: BTreeMap<usize, Vec<Finding>> = BTreeMap::new();
        let mut excepted: BTreeMap<usize, Vec<Finding>> = BTreeMap::new();
        let mut rest = Vec::new();
        for finding in report.findings {
            // An authorization change is reported, never refused: the guard
            // says so where it was made.
            if finding.severity != PolicySeverity::Error {
                continue;
            }
            let id = finding.shape.parse::<ShapeId>().ok();
            if id.as_ref().is_some_and(|id| {
                self.refused.contains(&id.root()) || self.explained.contains(&id.root())
            }) {
                continue;
            }
            let owner = id.as_ref().and_then(|id| {
                lost.iter()
                    .position(|lost| named(&lost.lowered).older.contains(id))
            });
            match (owner, finding.exception.is_some()) {
                (Some(index), true) => excepted.entry(index).or_default().push(finding),
                (Some(index), false) => open.entry(index).or_default().push(finding),
                (None, true) => {}
                (None, false) => rest.push((id, finding)),
            }
        }
        for (index, lost) in lost.iter().enumerate() {
            self.inert(
                lost,
                open.remove(&index).unwrap_or_default(),
                excepted.contains_key(&index),
            );
        }
        for (id, finding) in rest {
            let key = (finding.rule.clone(), finding.shape.clone());
            if !self.shown.insert(key.clone()) {
                continue;
            }
            let step = id.as_ref().and_then(|id| self.step_naming(steps, id));
            if step.is_some_and(|index| collided.contains(&index)) {
                continue;
            }
            let span = step.map_or(self.link.migration.from.span, |index| {
                step_span(steps[index].step)
            });
            let message = format!(
                "{} is not its frozen release: [{}] {}: {}; declare the change in the migration to \"{}\", or except `{}` for {} in the manifest",
                capitalized(&self.projection_label()),
                finding.rule,
                finding.shape,
                finding.message,
                self.to(),
                finding.rule,
                finding.shape
            );
            self.error(span, message);
            let at = self.out.diagnostics.len() - 1;
            self.out.findings.insert(at, key);
        }
    }

    /// Decision 70: a step whose newer side is gone since. Its older side
    /// is still the release's, so the release would lose it unseen, unless
    /// the manifest excepts that too: then the step is inert.
    fn inert(&mut self, lost: &Lost<'c>, open: Vec<Finding>, excepted: bool) {
        let gone = lost
            .gone
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let removed = match lost.exceptions.as_slice() {
            [] => "removed since".to_owned(),
            exceptions => format!(
                "removed under the exception for {}",
                exceptions
                    .iter()
                    .map(|exception| format!("`{}` ({})", exception.rule, exception.reason))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        let text = self.text(lost.step);
        let name = self.name();
        let span = step_span(lost.step);
        if open.is_empty() {
            let older = if excepted {
                format!(", and release \"{name}\"'s side of it is excepted too")
            } else {
                String::new()
            };
            let message = format!(
                "`{text}` is inert: {} no longer has {gone}, {removed}{older}, so the step converts nothing at release \"{name}\"",
                self.newer_label()
            );
            self.out
                .push(Some(self.link.file), Diagnostic::warning(span, message));
            return;
        }
        for finding in &open {
            self.shown
                .insert((finding.rule.clone(), finding.shape.clone()));
        }
        let older: BTreeSet<&str> = open.iter().map(|finding| finding.shape.as_str()).collect();
        let rules: BTreeSet<&str> = open.iter().map(|finding| finding.rule.as_str()).collect();
        let message = format!(
            "`{text}`: {} no longer has {gone}, {removed}, but release \"{name}\" still has {}, which its clients would lose unseen; except {} for {} too, which leaves the step inert, or keep {gone} while release \"{name}\" is supported",
            self.newer_label(),
            older.into_iter().collect::<Vec<_>>().join(", "),
            rules
                .into_iter()
                .map(|rule| format!("`{rule}`"))
                .collect::<Vec<_>>()
                .join(", "),
            open.iter()
                .map(|finding| finding.shape.as_str())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join(", ")
        );
        self.error(span, message);
    }

    /// The step whose subjects or named ids are `id`, or hold it.
    fn step_naming(&self, steps: &[Lowered<'c>], id: &ShapeId) -> Option<usize> {
        let ids = |step: &VersionStep| {
            let named = named(step);
            let mut ids = subjects(step);
            ids.extend(named.older);
            ids.extend(named.newer);
            ids
        };
        steps
            .iter()
            .position(|step| ids(&step.lowered).contains(id))
            .or_else(|| {
                let root = id.root();
                steps
                    .iter()
                    .position(|step| ids(&step.lowered).iter().any(|named| named.root() == root))
            })
    }

    /// Where the current model declares `id`: its name, in the file that
    /// declares it.
    fn declared(&self, id: &ShapeId) -> Option<(usize, Span)> {
        let location = self.compiled.source_map.get(id)?;
        let file = self.compiled.files.get(location.file)?;
        let name = id.member().unwrap_or(id.name());
        let span = self.compiled.parsed[location.file]
            .tokens
            .iter()
            .find(|token| {
                token.kind == TokenKind::Ident
                    && token.span.start >= location.span.start
                    && token.span.end <= location.span.end
                    && token.span.text(&file.text) == name
            })
            .map_or(location.span, |token| token.span);
        Some((location.file, span))
    }

    /// Where operation `operation` declares its example `title`.
    fn example(&self, operation: &ShapeId, title: &str) -> Option<(usize, Span)> {
        for (file, parsed) in self.compiled.parsed.iter().enumerate() {
            for item in &parsed.file.items {
                let Decl::Operation(op) = &item.decl else {
                    continue;
                };
                if op.name.name != operation.name() {
                    continue;
                }
                for entry in &op.entries {
                    if let OpEntry::Example(example) = entry
                        && example.title.value == title
                    {
                        return Some((file, example.title.span));
                    }
                }
            }
        }
        None
    }

    /// A step as written, on one line, for messages.
    fn text(&self, step: &Step) -> String {
        let source = &self.compiled.files[self.link.file].text;
        let text = source
            .get(step.span.start as usize..step.span.end as usize)
            .unwrap_or_default();
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

/// Where a diagnostic about a step goes: the path or name it starts with.
fn step_span(step: &Step) -> Span {
    match &step.kind {
        StepKind::Rename { path, .. }
        | StepKind::Require { path, .. }
        | StepKind::Add { path, .. }
        | StepKind::Reshape { path, .. }
        | StepKind::Retype { path, .. }
        | StepKind::Behavior(path) => path.span,
        StepKind::Move { operation, .. } => operation.span,
        StepKind::RemoveOperation(name) => name.span,
        StepKind::Sunset(date) => date.span,
    }
}

/// A shape as JSON, its documentation left out: two definitions that
/// differ only there are the same shape.
fn structure(shape: &Shape) -> serde_json::Value {
    fn strip(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("docs");
                map.values_mut().for_each(strip);
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut value = serde_json::to_value(shape).unwrap_or_default();
    strip(&mut value);
    value
}
