//! A role catalogue built at run time from the IR, laid out as
//! `crucible-codegen-rust` lays out the one it generates (bits in
//! `Permission::ALL` order, scopes and roles in the service's order,
//! coverage from `crucible_ir::roles`), so the differential test can run
//! `RoleAuthorizer` over models generated per case. `same` compares one
//! with a generated catalogue, which keeps the two layouts equal.
#![allow(dead_code)]

use crucible_ir::ir::{ScopeTarget, Service, Shape, Tier};
use crucible_server_runtime_rust::permissions::{PermissionInfo, ResourceInfo};
use crucible_server_runtime_rust::roles::{
    PermissionSet, RoleCatalogue, RoleInfo, ScopeInfo, ScopeKind,
};

fn leak<T>(value: T) -> &'static T {
    Box::leak(Box::new(value))
}

fn text(value: &str) -> &'static str {
    Box::leak(value.to_owned().into_boxed_str())
}

fn texts(values: impl IntoIterator<Item = impl AsRef<str>>) -> &'static [&'static str] {
    Box::leak(
        values
            .into_iter()
            .map(|v| text(v.as_ref()))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
}

fn slice<T>(values: Vec<T>) -> &'static [T] {
    Box::leak(values.into_boxed_slice())
}

fn tier(tier: Tier) -> crucible_server_runtime_rust::permissions::Tier {
    use crucible_server_runtime_rust::permissions::Tier as T;
    match tier {
        Tier::Public => T::Public,
        Tier::PermissionOnly => T::PermissionOnly,
        Tier::NotGrantable => T::NotGrantable,
        Tier::Internal => T::Internal,
    }
}

/// `service`'s catalogue, leaked.
pub fn catalogue(service: &Service) -> &'static RoleCatalogue {
    let mut resources: Vec<(&crucible_ir::ShapeId, &'static ResourceInfo)> = Vec::new();
    let mut permissions: Vec<(&crucible_ir::ShapeId, String, &'static PermissionInfo)> = Vec::new();
    for (id, declared) in &service.permissions {
        let info: &'static ResourceInfo = leak(ResourceInfo {
            name: text(&declared.name),
            identifiers: texts(&declared.identifiers),
            not_found: "",
        });
        resources.push((id, info));
        for p in declared.permissions.iter().filter(|p| p.alias_of.is_none()) {
            permissions.push((
                id,
                p.symbol.clone(),
                leak(PermissionInfo {
                    resource: info,
                    symbol: text(&p.symbol),
                    value: text(&p.value),
                    aliases: &[],
                    tier: tier(p.tier),
                    denied: "",
                }),
            ));
        }
    }
    let bit = |resource: &crucible_ir::ShapeId, symbol: &str| {
        permissions
            .iter()
            .position(|(id, s, _)| *id == resource && s == symbol)
            .expect("a granted permission is declared")
    };
    let info_of = |id: &crucible_ir::ShapeId| {
        resources
            .iter()
            .find(|(r, _)| *r == id)
            .map(|(_, info)| *info)
            .unwrap()
    };
    let principals: Vec<(String, String)> = service
        .role_settings
        .principals
        .as_ref()
        .and_then(|id| match service.shapes.get(id) {
            Some(Shape::Enum(e)) => Some(
                e.values
                    .iter()
                    .map(|v| (v.name.clone(), v.value.clone()))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default();
    let roles = service
        .roles
        .iter()
        .map(|role| RoleInfo {
            symbol: text(&role.symbol),
            value: text(&role.value),
            description: text(role.docs.as_deref().map(str::trim).unwrap_or_default()),
            internal: role.internal,
            scopes: slice(
                role.scopes
                    .iter()
                    .filter_map(|s| service.role_scopes.iter().position(|x| x.symbol == *s))
                    .map(|i| i as u16)
                    .collect(),
            ),
            holders: texts(role.holders.iter().map(|name| {
                principals
                    .iter()
                    .find(|(n, _)| n == name)
                    .map_or(name.clone(), |(_, v)| v.clone())
            })),
            assigned_by: slice(
                role.assigned_by
                    .iter()
                    .filter_map(|s| service.roles.iter().position(|x| x.symbol == *s))
                    .map(|i| i as u16)
                    .collect(),
            ),
            grants: PermissionSet::with_bits(
                role.effective
                    .iter()
                    .map(|g| bit(&g.resource, &g.permission)),
            ),
            by_service: PermissionSet::with_bits(
                role.by_service
                    .iter()
                    .map(|g| bit(&g.resource, &g.permission)),
            ),
        })
        .collect();
    let scopes = service
        .role_scopes
        .iter()
        .map(|scope| {
            let coverage = crucible_ir::roles::coverage(service, &scope.on);
            let (kind, resource) = match &scope.on {
                ScopeTarget::Service => (ScopeKind::Service, String::new()),
                ScopeTarget::Instance(id) => (ScopeKind::Instance, id.name().to_owned()),
                ScopeTarget::Every(id) => (ScopeKind::Every, id.name().to_owned()),
            };
            ScopeInfo {
                symbol: text(&scope.symbol),
                value: text(&scope.value),
                description: text(scope.docs.as_deref().map(str::trim).unwrap_or_default()),
                internal: scope.internal,
                kind,
                resource: text(&resource),
                identifiers: texts(crucible_ir::roles::scope_identifiers(service, &scope.on)),
                instances: slice(coverage.instances.iter().map(|id| info_of(id)).collect()),
                every: slice(coverage.every.iter().map(|id| info_of(id)).collect()),
            }
        })
        .collect();
    let grantable = PermissionSet::with_bits(
        permissions
            .iter()
            .enumerate()
            .filter(|(_, (_, _, p))| {
                matches!(
                    p.tier,
                    crucible_server_runtime_rust::permissions::Tier::Public
                        | crucible_server_runtime_rust::permissions::Tier::PermissionOnly
                )
            })
            .map(|(bit, _)| bit),
    );
    leak(RoleCatalogue {
        roles: slice(roles),
        scopes: slice(scopes),
        permissions: slice(permissions.iter().map(|(_, _, p)| *p).collect()),
        principals: texts(principals.iter().map(|(_, v)| v)),
        custom: service.role_settings.custom,
        grantable,
    })
}

/// Whether two catalogues state the same thing: every role and scope
/// field, coverage and permissions by resource name and value (the 403
/// and 404 messages are the permission registry's, not the catalogue's).
pub fn same(a: &RoleCatalogue, b: &RoleCatalogue) -> Result<(), String> {
    if a.roles != b.roles {
        return Err(format!("roles differ:\n{:#?}\n{:#?}", a.roles, b.roles));
    }
    let name = |p: &&PermissionInfo| (p.resource.name, p.value, p.tier);
    if a.permissions.iter().map(name).collect::<Vec<_>>()
        != b.permissions.iter().map(name).collect::<Vec<_>>()
    {
        return Err("permissions differ".into());
    }
    let names = |rs: &[&ResourceInfo]| rs.iter().map(|r| r.name).collect::<Vec<_>>();
    for (x, y) in a.scopes.iter().zip(b.scopes) {
        if (
            x.symbol,
            x.value,
            x.description,
            x.internal,
            x.kind,
            x.resource,
            x.identifiers,
        ) != (
            y.symbol,
            y.value,
            y.description,
            y.internal,
            y.kind,
            y.resource,
            y.identifiers,
        ) || names(x.instances) != names(y.instances)
            || names(x.every) != names(y.every)
        {
            return Err(format!("scope {} differs", x.symbol));
        }
    }
    if a.scopes.len() != b.scopes.len()
        || a.principals != b.principals
        || a.custom != b.custom
        || a.grantable != b.grantable
    {
        return Err("the catalogue's settings differ".into());
    }
    Ok(())
}
