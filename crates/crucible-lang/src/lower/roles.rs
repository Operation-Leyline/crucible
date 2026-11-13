//! Roles and scopes in `.cru` (docs/roles.md §4): `scope` and `role`
//! declarations, the service's `roles`, `scopes` and `principals` clauses,
//! and `@roleTypes` and `@scopeTypes`.
//!
//! This module resolves names and checks what only the syntax can get wrong
//! (an entry written twice, a name that is no resource, an annotation that
//! does not apply); everything that holds of the IR, coverage included, is
//! `crucible_ir::validate::roles`, run once the service is built and its
//! roles settled (`crucible_ir::roles::settle`).

use std::collections::BTreeSet;

use crucible_ir::ShapeId;
use crucible_ir::ir::{
    Grant, GrantedSet, Role, RoleSettings, Scope, ScopeTarget as IrScopeTarget, Service,
};
use crucible_ir::validate::{FindingSeverity, RoleFinding, RoleList, RoleSite};

use super::{DeclRef, Lowerer, Position};
use crate::ast::{
    ClauseValue, Decl, GrantList, Ident, Item, RoleDecl, RoleEntryKind, ScopeDecl, ScopeTarget,
    Value,
};
use crate::compile::{FileDiagnostic, Location};
use crate::diagnostic::Diagnostic;

/// What the service block says about roles.
#[derive(Default)]
pub(super) struct RoleClauses<'a> {
    /// The file of the service block.
    pub file: usize,
    pub roles: Vec<&'a Ident>,
    pub scopes: Vec<&'a Ident>,
    /// `custom` and `direct` after the `roles` list.
    pub flags: Vec<&'a Ident>,
    pub principals: Option<&'a Ident>,
}

impl<'a> Lowerer<'a> {
    /// The service's roles and scopes, in its lists' order, and its role
    /// settings. A name that is not a role (scope) is reported here; one a
    /// role names that the service does not list is the shared check's.
    pub(super) fn lower_roles(
        &mut self,
        clauses: &RoleClauses<'a>,
    ) -> (Vec<Role>, Vec<Scope>, RoleSettings) {
        let file = clauses.file;
        let mut settings = RoleSettings::default();
        for flag in &clauses.flags {
            let slot = if flag.name == "custom" {
                &mut settings.custom
            } else {
                &mut settings.direct
            };
            if *slot {
                self.error(file, flag.span, format!("`{}` is given twice", flag.name));
            }
            *slot = true;
        }
        if let Some(name) = clauses.principals {
            match self.decl(&name.name).map(|d| &d.item.decl) {
                Some(Decl::Enum(_)) => {
                    let id = self.ref_id(&name.name);
                    self.visit(&id);
                    self.role_sites.insert(
                        RoleSite::Shape(id.clone()),
                        Location {
                            file,
                            span: name.span,
                        },
                    );
                    settings.principals = Some(id);
                }
                _ => self.error(
                    file,
                    name.span,
                    format!(
                        "roles.principals: `{}` is not an enum; `principals` names the enum of principal kinds whose values `holders` lists",
                        name.name
                    ),
                ),
            }
        }
        let mut scopes = Vec::new();
        for name in &clauses.scopes {
            match self.decl(&name.name) {
                Some(decl) if matches!(decl.item.decl, Decl::Scope(_)) => {
                    let Decl::Scope(scope) = &decl.item.decl else {
                        unreachable!("matched")
                    };
                    scopes.push(self.lower_scope(decl, scope));
                }
                _ => self.error(file, name.span, format!("`{}` is not a scope", name.name)),
            }
        }
        let mut roles = Vec::new();
        for name in &clauses.roles {
            match self.decl(&name.name) {
                Some(decl) if matches!(decl.item.decl, Decl::Role(_)) => {
                    let Decl::Role(role) = &decl.item.decl else {
                        unreachable!("matched")
                    };
                    roles.push(self.lower_role(decl, role));
                }
                _ => self.error(file, name.span, format!("`{}` is not a role", name.name)),
            }
        }
        settings.role_types = self.linked_enum("roleTypes");
        settings.scope_types = self.linked_enum("scopeTypes");
        self.report_unlisted_roles();
        (roles, scopes, settings)
    }

    fn site(&mut self, site: RoleSite, file: usize, span: crate::span::Span) {
        self.role_sites.insert(site, Location { file, span });
    }

    /// `@deprecated` and `@since`, the annotations a role or scope takes.
    fn role_notes(&mut self, file: usize, item: &Item, what: &str) -> crucible_ir::ir::DocNotes {
        let mut notes = Vec::new();
        for annotation in &item.annotations {
            match annotation.name.name.as_str() {
                "deprecated" | "since" => notes.push(annotation.clone()),
                other => self.error(
                    file,
                    annotation.span,
                    format!("`@{other}` does not apply to a {what}; a {what} takes `@deprecated` and `@since`"),
                ),
            }
        }
        self.annotations(file, &notes, &[], Position::EnumValue)
            .notes
    }

    fn lower_scope(&mut self, decl: DeclRef<'a>, scope: &'a ScopeDecl) -> Scope {
        let file = decl.file;
        let item = decl.item;
        let internal = self
            .modifiers(file, item, &["internal"])
            .contains("internal");
        let notes = self.role_notes(file, item, "scope");
        self.site(
            RoleSite::Scope(scope.name.name.clone()),
            file,
            scope.name.span,
        );
        let on = match &scope.target {
            ScopeTarget::Service(_) => IrScopeTarget::Service,
            ScopeTarget::Resource { name, every } => {
                if !matches!(
                    self.decl(&name.name).map(|d| &d.item.decl),
                    Some(Decl::Resource(_))
                ) && !name.is_missing()
                {
                    self.error(
                        file,
                        name.span,
                        format!(
                            "`{}` is not a resource; a scope is `on` a resource, `Resource(*)` or `service`",
                            name.name
                        ),
                    );
                }
                let id = self.ref_id(&name.name);
                if *every {
                    IrScopeTarget::Every(id)
                } else {
                    IrScopeTarget::Instance(id)
                }
            }
        };
        Scope {
            symbol: scope.name.name.clone(),
            value: scope.value.value.clone(),
            on,
            internal,
            docs: self.doc(file, item.doc.as_ref()),
            notes,
        }
    }

    fn lower_role(&mut self, decl: DeclRef<'a>, role: &'a RoleDecl) -> Role {
        let file = decl.file;
        let item = decl.item;
        let symbol = role.name.name.clone();
        let internal = self
            .modifiers(file, item, &["internal"])
            .contains("internal");
        let notes = self.role_notes(file, item, "role");
        self.site(RoleSite::Role(symbol.clone()), file, role.name.span);
        let mut lists: [Option<Vec<String>>; 4] = Default::default();
        let mut grants = Vec::new();
        for entry in &role.entries {
            let (list, names) = match &entry.kind {
                RoleEntryKind::At(names) => (RoleList::At, names),
                RoleEntryKind::Includes(names) => (RoleList::Includes, names),
                RoleEntryKind::Holders(names) => (RoleList::Holders, names),
                RoleEntryKind::AssignedBy(names) => (RoleList::AssignedBy, names),
                RoleEntryKind::Grants(grant) => {
                    let resource = &grant.resource;
                    if resource.is_missing() {
                        continue;
                    }
                    if !matches!(
                        self.decl(&resource.name).map(|d| &d.item.decl),
                        Some(Decl::Resource(_))
                    ) {
                        self.error(
                            file,
                            resource.span,
                            format!(
                                "roles.grant: `{}` is not a resource; a role grants a resource's permissions: `grants Resource [permission, ..]`",
                                resource.name
                            ),
                        );
                        continue;
                    }
                    let permissions = match &grant.permissions {
                        GrantList::Named(names) => {
                            GrantedSet::Named(names.iter().map(|n| n.name.clone()).collect())
                        }
                        GrantList::All { except, .. } => GrantedSet::All {
                            except: except.iter().map(|n| n.name.clone()).collect(),
                        },
                    };
                    self.site(
                        RoleSite::Grant {
                            role: symbol.clone(),
                            index: grants.len(),
                        },
                        file,
                        entry.span,
                    );
                    grants.push(Grant {
                        resource: self.ref_id(&resource.name),
                        permissions,
                        by_service: grant.by_service.is_some(),
                        docs: self.doc(file, grant.doc.as_ref()),
                    });
                    continue;
                }
            };
            let slot = &mut lists[list as usize];
            if slot.is_some() {
                self.error(
                    file,
                    entry.keyword.span,
                    format!(
                        "`{symbol}` has two `{0}` entries; list every name in one `{0} [..]`",
                        list.keyword()
                    ),
                );
                continue;
            }
            let mut seen = BTreeSet::new();
            let mut out = Vec::new();
            for name in names {
                if !seen.insert(name.name.as_str()) {
                    self.error(file, name.span, format!("`{}` is listed twice", name.name));
                    continue;
                }
                self.site(
                    RoleSite::Name {
                        role: symbol.clone(),
                        list,
                        index: out.len(),
                    },
                    file,
                    name.span,
                );
                out.push(name.name.clone());
            }
            *slot = Some(out);
        }
        let [scopes, includes, holders, assigned_by] = lists.map(Option::unwrap_or_default);
        Role {
            symbol,
            value: role.value.value.clone(),
            internal,
            docs: self.doc(file, item.doc.as_ref()),
            notes,
            scopes,
            includes,
            grants,
            holders,
            assigned_by,
            effective: Vec::new(),
            by_service: Vec::new(),
        }
    }

    /// The service's `@roleTypes` (`@scopeTypes`) enum: at most one, a
    /// string enum of its own package.
    fn linked_enum(&mut self, annotation: &str) -> Option<ShapeId> {
        let mut found: Option<ShapeId> = None;
        let decls: Vec<DeclRef<'a>> = self.decls.values().copied().collect();
        for decl in decls {
            let Decl::Enum(e) = &decl.item.decl else {
                continue;
            };
            let Some(given) = decl
                .item
                .annotations
                .iter()
                .find(|a| a.name.name == annotation)
            else {
                continue;
            };
            let file = decl.file;
            let problem = if decl.ns != self.ns {
                Some(format!(
                    "roles.types: `{}` is declared in `{}`; `@{annotation}` marks an enum of the service's own package",
                    e.name.name, decl.ns
                ))
            } else if let Some(first) = &found {
                Some(format!(
                    "roles.types: a service has one `@{annotation}` enum, and `{}` is one",
                    first.name()
                ))
            } else if e.base.is_some() {
                Some(format!(
                    "roles.types: `@{annotation}` applies to a string enum"
                ))
            } else if !given.args.is_empty() {
                Some(format!("`@{annotation}` takes no arguments"))
            } else {
                None
            };
            if let Some(problem) = problem {
                self.error(file, given.span, problem);
                continue;
            }
            let id = self.ref_id(&e.name.name);
            self.visit(&id);
            self.site(RoleSite::Shape(id.clone()), file, given.span);
            found = Some(id);
        }
        found
    }

    /// A role or scope of the package that no service lists: part of no
    /// catalogue, so nothing generated would know it.
    fn report_unlisted_roles(&mut self) {
        let mut listed: BTreeSet<&str> = BTreeSet::new();
        for decl in self.decls.values() {
            let Decl::Service(block) = &decl.item.decl else {
                continue;
            };
            if decl.ns != self.ns {
                continue;
            }
            for clause in &block.clauses {
                if let ("roles" | "scopes", ClauseValue::Value(Value::List(values, _))) =
                    (clause.name.name.as_str(), &clause.value)
                {
                    listed.extend(values.iter().filter_map(|value| match value {
                        Value::Ident(name) => Some(name.name.as_str()),
                        _ => None,
                    }));
                }
            }
        }
        let mut unlisted = Vec::new();
        for decl in self.decls.values() {
            if decl.ns != self.ns {
                continue;
            }
            let (name, list) = match &decl.item.decl {
                Decl::Role(role) => (&role.name, "roles"),
                Decl::Scope(scope) => (&scope.name, "scopes"),
                _ => continue,
            };
            if !listed.contains(name.name.as_str()) {
                unlisted.push((decl.file, name.span, name.name.clone(), list));
            }
        }
        for (file, span, name, list) in unlisted {
            self.warning(
                file,
                span,
                format!(
                    "`{name}` is not part of the service: list it in the service's `{list} [..]`"
                ),
            );
        }
    }

    /// The shared role checks, each finding reported where its site was
    /// declared.
    pub(super) fn check_roles(&mut self, service: &Service) {
        for finding in crucible_ir::validate::roles(service) {
            self.report_role_finding(finding);
        }
    }

    fn report_role_finding(&mut self, finding: RoleFinding) {
        let declaration = match &finding.site {
            RoleSite::Name { role, .. } | RoleSite::Grant { role, .. } => {
                Some(RoleSite::Role(role.clone()))
            }
            _ => None,
        };
        let location = self
            .role_sites
            .get(&finding.site)
            .or_else(|| declaration.and_then(|site| self.role_sites.get(&site)))
            .or_else(|| match &finding.site {
                RoleSite::Permission(id) | RoleSite::Shape(id) | RoleSite::Operation(id) => self
                    .permission_sites
                    .get(id)
                    .or_else(|| self.source_map.get(id)),
                _ => None,
            })
            .copied();
        let message = finding.text();
        let (file, span, message) = match location {
            Some(Location { file, span }) => (Some(file), span, message),
            None => (None, crate::span::Span::default(), finding.to_string()),
        };
        let diagnostic = match finding.severity {
            FindingSeverity::Error => Diagnostic::new(span, message),
            FindingSeverity::Advice => Diagnostic::advice(span, message),
            _ => Diagnostic::warning(span, message),
        };
        self.diagnostics.push(FileDiagnostic { file, diagnostic });
    }
}
