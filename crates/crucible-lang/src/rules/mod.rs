//! Policy rules and their exceptions (decisions 45-47): `rule` declarations
//! that a package's model must satisfy, checked on every compile, and
//! `except` declarations that excuse a violation with a reason.
//!
//! Rules run on the lowered model, looking positions up in the source map
//! (decision 61). They are checked against a fixed vocabulary first, so a
//! rule that could never match is an error in the rule, and only then run
//! on the elements the compiled package declares.

mod check;
mod eval;
mod metadata;
mod subject;
mod vocabulary;

use std::collections::{BTreeMap, BTreeSet};

use crucible_smithy::ShapeId;
use crucible_smithy::ir::{PolicyException, Service};

use self::check::Checker;
use self::eval::Evaluator;
use self::subject::{Subject, World};
use self::vocabulary::Kind;
use crate::Parsed;
use crate::ast::{Decl, Expr, Item, Quantifier, Rule, RuleClause};
use crate::compile::{FileDiagnostic, SourceFile, SourceMap};
use crate::diagnostic::Diagnostic;
use crate::span::Span;

pub(crate) struct Inputs<'a> {
    pub package: &'a str,
    pub files: &'a [SourceFile],
    pub parsed: &'a [Parsed],
    pub namespaces: &'a [String],
    pub service: &'a Service,
    pub source_map: &'a SourceMap,
    /// Types named outside any shape's members: they count as referenced.
    pub named_types: &'a BTreeSet<ShapeId>,
    /// Members a role template put in an operation.
    pub template_members: &'a BTreeSet<ShapeId>,
}

#[derive(Default)]
pub(crate) struct Outcome {
    pub diagnostics: Vec<FileDiagnostic>,
    pub exceptions: Vec<PolicyException>,
}

/// How a rule is enforced and who may excuse it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    /// A violation fails the compile, and no package may except it.
    Strict,
    /// A violation fails the compile; the package that declares the rule may
    /// except it.
    Standard,
    /// As `Standard`, and a package that imports the rule may except it too.
    Exceptable,
    /// A violation is reported and never fails the compile.
    Advice,
}

struct RuleDef<'a> {
    name: &'a str,
    package: &'a str,
    file: usize,
    item: &'a Item,
    rule: &'a Rule,
    level: Level,
    /// Set when the rule has problems; it does not run.
    broken: bool,
}

struct Violation {
    rule: usize,
    subject: usize,
    message: String,
}

pub(crate) fn run(inputs: &Inputs<'_>) -> Outcome {
    let mut outcome = Outcome::default();
    let mut defs = collect_rules(inputs, &mut outcome.diagnostics);
    let excepts = collect_exceptions(inputs);
    if defs.is_empty() && excepts.is_empty() {
        return outcome;
    }
    let world = World::new(inputs);
    let meta = metadata::index(inputs.parsed, inputs.namespaces);
    for def in &mut defs {
        check_rule(def, &world, &meta, &mut outcome.diagnostics);
    }
    let evaluator = Evaluator {
        world: &world,
        meta: &meta,
        patterns: Default::default(),
    };
    let mut violations = Vec::new();
    for (index, def) in defs.iter().enumerate() {
        if def.broken {
            continue;
        }
        for clause in &def.rule.clauses {
            violations.extend(apply(index, def, clause, &world, &evaluator, inputs));
        }
    }
    report(inputs, &defs, &excepts, &world, &violations, &mut outcome);
    outcome
}

fn level_of(rule: &Rule) -> (Level, Vec<Diagnostic>) {
    let mut problems = Vec::new();
    let mut level = Level::Standard;
    let mut set = false;
    for flag in &rule.flags {
        let next = match flag.name.as_str() {
            "strict" => Level::Strict,
            "exceptable" => Level::Exceptable,
            "advice" => Level::Advice,
            other => {
                problems.push(Diagnostic::new(
                    flag.span,
                    format!("`{other}` is not a rule level; use `strict`, `exceptable` or `advice`, or none"),
                ));
                continue;
            }
        };
        if set {
            problems.push(Diagnostic::new(
                flag.span,
                "a rule has one level: `strict`, `exceptable` or `advice`",
            ));
        }
        set = true;
        level = next;
    }
    (level, problems)
}

fn collect_rules<'a>(
    inputs: &Inputs<'a>,
    diagnostics: &mut Vec<FileDiagnostic>,
) -> Vec<RuleDef<'a>> {
    let mut defs: Vec<RuleDef<'a>> = Vec::new();
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for (file, parsed) in inputs.parsed.iter().enumerate() {
        let package = inputs
            .namespaces
            .get(file)
            .map_or(inputs.package, String::as_str);
        for item in &parsed.file.items {
            let Decl::Rule(rule) = &item.decl else {
                continue;
            };
            let (level, problems) = level_of(rule);
            let mut broken = !problems.is_empty();
            diagnostics.extend(problems.into_iter().map(|diagnostic| FileDiagnostic {
                file: Some(file),
                diagnostic,
            }));
            if let Some(&earlier) = seen.get(rule.name.name.as_str()) {
                let first: &RuleDef<'_> = &defs[earlier];
                let message = if first.package == package {
                    format!("rule `{}` is declared twice", rule.name.name)
                } else {
                    format!(
                        "rule `{}` shadows the rule of that name in `{}`: an importing package cannot replace a rule",
                        rule.name.name, first.package
                    )
                };
                diagnostics.push(FileDiagnostic {
                    file: Some(file),
                    diagnostic: Diagnostic::new(rule.name.span, message),
                });
                broken = true;
            } else {
                seen.insert(rule.name.name.as_str(), defs.len());
            }
            defs.push(RuleDef {
                name: &rule.name.name,
                package,
                file,
                item,
                rule,
                level,
                broken,
            });
        }
    }
    defs
}

struct ExceptDef<'a> {
    file: usize,
    package: &'a str,
    span: Span,
    rule: &'a crate::ast::Ident,
    target: &'a crate::ast::Ident,
    reason: &'a crate::ast::StrLit,
}

fn collect_exceptions<'a>(inputs: &Inputs<'a>) -> Vec<ExceptDef<'a>> {
    let mut out = Vec::new();
    for (file, parsed) in inputs.parsed.iter().enumerate() {
        let package = inputs
            .namespaces
            .get(file)
            .map_or(inputs.package, String::as_str);
        for item in &parsed.file.items {
            if let Decl::Except(except) = &item.decl {
                out.push(ExceptDef {
                    file,
                    package,
                    span: item.span,
                    rule: &except.rule,
                    target: &except.target,
                    reason: &except.reason,
                });
            }
        }
    }
    out
}

/// The kind a clause ranges over, with the problem if its words name none.
fn clause_kind(clause: &RuleClause) -> Result<Kind, Diagnostic> {
    let words: Vec<&str> = clause.kind.iter().map(|w| w.name.as_str()).collect();
    Kind::from_words(&words).ok_or_else(|| {
        Diagnostic::new(
            clause.span,
            format!(
                "`{}` is not something a rule ranges over; use one of {}",
                words.join(" "),
                Kind::all_words()
            ),
        )
    })
}

fn check_rule(
    def: &mut RuleDef<'_>,
    world: &World<'_>,
    meta: &metadata::MetaIndex,
    diagnostics: &mut Vec<FileDiagnostic>,
) {
    let mut checker = Checker {
        meta,
        world,
        problems: Vec::new(),
    };
    if def.rule.clauses.is_empty() {
        checker.problems.push(Diagnostic::new(
            def.rule.name.span,
            format!(
                "rule `{}` has no clause: write `every <kind> must ..` or `no <kind> ..`",
                def.name
            ),
        ));
    }
    for clause in &def.rule.clauses {
        let kind = match clause_kind(clause) {
            Ok(kind) => kind,
            Err(problem) => {
                checker.problems.push(problem);
                continue;
            }
        };
        if clause.quantifier == Quantifier::Every && clause.requirements.is_empty() {
            checker.problems.push(Diagnostic::new(
                clause.span,
                "an `every` clause says what each must satisfy: add `must ..`",
            ));
        }
        if let Some(condition) = &clause.condition {
            checker.expr(kind, condition);
        }
        for requirement in &clause.requirements {
            checker.expr(kind, requirement);
        }
    }
    if !checker.problems.is_empty() {
        def.broken = true;
    }
    let file = def.file;
    diagnostics.extend(
        checker
            .problems
            .into_iter()
            .map(|diagnostic| FileDiagnostic {
                file: Some(file),
                diagnostic,
            }),
    );
}

fn apply(
    rule: usize,
    def: &RuleDef<'_>,
    clause: &RuleClause,
    world: &World<'_>,
    evaluator: &Evaluator<'_, '_>,
    inputs: &Inputs<'_>,
) -> Vec<Violation> {
    let Ok(kind) = clause_kind(clause) else {
        return Vec::new();
    };
    let text = |expr: &Expr| -> &str { expr.span.text(&inputs.files[def.file].text) };
    let mut out = Vec::new();
    for (index, subject) in world.subjects.iter().enumerate() {
        if !is_kind(subject, kind) {
            continue;
        }
        // A fact the model lacks for this subject skips it for the clause.
        let applies = match &clause.condition {
            Some(condition) => match evaluator.eval(subject, condition) {
                Ok(holds) => holds,
                Err(_) => continue,
            },
            None => true,
        };
        if !applies {
            continue;
        }
        match clause.quantifier {
            Quantifier::No => {
                let scope = match &clause.condition {
                    Some(condition) => format!("{} where {}", kind.words(), text(condition)),
                    None => kind.words().to_owned(),
                };
                out.push(Violation {
                    rule,
                    subject: index,
                    message: format!(
                        "`{}` is a {scope}, which this rule forbids",
                        subject.display
                    ),
                });
            }
            Quantifier::Every => {
                for requirement in &clause.requirements {
                    if let Ok(false) = evaluator.eval(subject, requirement) {
                        out.push(Violation {
                            rule,
                            subject: index,
                            message: format!(
                                "`{}` does not satisfy `must {}`",
                                subject.display,
                                text(requirement)
                            ),
                        });
                    }
                }
            }
        }
    }
    out
}

fn is_kind(subject: &Subject<'_>, kind: Kind) -> bool {
    use self::subject::{Entity, Side};
    match &subject.entity {
        Entity::Service(_) => kind == Kind::Service,
        Entity::Resource { .. } => kind == Kind::Resource,
        Entity::Operation(_) => kind == Kind::Operation,
        Entity::Shape { class, .. } => class.is(kind),
        Entity::Member { sides, .. } => match kind {
            Kind::Member => true,
            Kind::InputMember => sides.contains(&Side::Input),
            Kind::OutputMember => sides.contains(&Side::Output),
            _ => false,
        },
    }
}

/// The names an exception may give for a subject: a shape, operation or
/// resource by its name, and a member only through its shape or operation, so
/// a member's own name never excuses every member that shares it.
fn targets<'a>(subject: &'a Subject<'_>) -> Vec<&'a str> {
    use self::subject::Entity;
    let mut out = Vec::new();
    if !matches!(subject.entity, Entity::Member { .. }) {
        out.push(subject.name.as_str());
    }
    out.extend(subject.aliases.iter().map(String::as_str));
    out
}

fn report(
    inputs: &Inputs<'_>,
    defs: &[RuleDef<'_>],
    excepts: &[ExceptDef<'_>],
    world: &World<'_>,
    violations: &[Violation],
    outcome: &mut Outcome,
) {
    let by_name: BTreeMap<&str, usize> = defs
        .iter()
        .enumerate()
        .map(|(index, def)| (def.name, index))
        .collect();
    // Which exceptions are allowed, and which of them fire.
    let mut used = vec![false; excepts.len()];
    let mut allowed = vec![false; excepts.len()];
    let mut seen: BTreeSet<(&str, &str)> = BTreeSet::new();
    for (index, except) in excepts.iter().enumerate() {
        let error = |message: String| FileDiagnostic {
            file: Some(except.file),
            diagnostic: Diagnostic::new(except.rule.span, message),
        };
        let Some(&rule) = by_name.get(except.rule.name.as_str()) else {
            let known: Vec<&str> = defs.iter().map(|d| d.name).collect();
            outcome.diagnostics.push(error(format!(
                "`except` names an unknown rule `{}`; the rules are {}",
                except.rule.name,
                if known.is_empty() {
                    "none".to_owned()
                } else {
                    known.join(", ")
                }
            )));
            continue;
        };
        let def = &defs[rule];
        let problem = match def.level {
            Level::Strict => Some(format!(
                "rule `{}` is strict: no exception can excuse it",
                def.name
            )),
            Level::Advice => Some(format!(
                "rule `{}` is advice and never fails the compile: there is nothing to except",
                def.name
            )),
            Level::Standard if def.package != except.package => Some(format!(
                "rule `{}` belongs to `{}` and is not `exceptable`: only that package can except it",
                def.name, def.package
            )),
            _ => None,
        };
        if let Some(problem) = problem {
            outcome.diagnostics.push(error(problem));
            continue;
        }
        if except.reason.value.trim().is_empty() {
            outcome.diagnostics.push(FileDiagnostic {
                file: Some(except.file),
                diagnostic: Diagnostic::new(
                    except.reason.span,
                    "an exception needs a reason: say why this violation is accepted",
                ),
            });
            continue;
        }
        if !seen.insert((except.rule.name.as_str(), except.target.name.as_str())) {
            outcome.diagnostics.push(error(format!(
                "`{}` is excepted for `{}` twice",
                except.rule.name, except.target.name
            )));
            continue;
        }
        allowed[index] = true;
    }
    // Exceptions by (rule, target), so a violation finds its own at once.
    let mut by_key: BTreeMap<(&str, &str), Vec<usize>> = BTreeMap::new();
    for (index, except) in excepts.iter().enumerate() {
        by_key
            .entry((except.rule.name.as_str(), except.target.name.as_str()))
            .or_default()
            .push(index);
    }
    for violation in violations {
        let def = &defs[violation.rule];
        let subject = &world.subjects[violation.subject];
        let excused: Vec<usize> = targets(subject)
            .into_iter()
            .filter_map(|target| by_key.get(&(def.name, target)))
            .flatten()
            .copied()
            .filter(|index| allowed[*index])
            .collect();
        if !excused.is_empty() {
            for index in excused {
                used[index] = true;
            }
            continue;
        }
        let owner = if def.package == inputs.package {
            def.name.to_owned()
        } else {
            format!("{}#{}", def.package, def.name)
        };
        let mut message = format!("rule {owner}: {}", violation.message);
        if let Some(doc) = &def.item.doc {
            for line in doc.text.lines() {
                message.push_str("\n  ");
                message.push_str(line);
            }
        }
        let diagnostic = |span| match def.level {
            Level::Advice => Diagnostic::advice(span, message.clone()),
            _ => Diagnostic::new(span, message.clone()),
        };
        let located = inputs
            .source_map
            .get(&subject.id)
            .map(|location| (location.file, location.span))
            .or(subject.declared);
        outcome.diagnostics.push(match located {
            Some((file, span)) => FileDiagnostic {
                file: Some(file),
                diagnostic: diagnostic(span),
            },
            None => FileDiagnostic {
                file: None,
                diagnostic: diagnostic(Span::default()),
            },
        });
    }
    for (index, except) in excepts.iter().enumerate() {
        if !allowed[index] {
            continue;
        }
        if used[index] {
            let def = &defs[by_name[except.rule.name.as_str()]];
            outcome.exceptions.push(PolicyException {
                rule: def.name.to_owned(),
                rule_package: def.package.to_owned(),
                target: except.target.name.clone(),
                reason: except.reason.value.clone(),
                package: except.package.to_owned(),
            });
        } else {
            outcome.diagnostics.push(FileDiagnostic {
                file: Some(except.file),
                diagnostic: Diagnostic::new(
                    except.span,
                    format!(
                        "`except {} for {}` excuses nothing: no element of that name violates the rule; remove it",
                        except.rule.name, except.target.name
                    ),
                ),
            });
        }
    }
    outcome
        .exceptions
        .sort_by(|a, b| (&a.rule, &a.target, &a.package).cmp(&(&b.rule, &b.target, &b.package)));
}
