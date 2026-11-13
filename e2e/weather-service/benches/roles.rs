//! What one `RoleAuthorizer` decision costs: one read of the principal's
//! bindings from an in-memory store (a map lookup and an `Arc` clone), then
//! per check the bindings' identifier-prefix comparisons and bitset tests
//! against the catalogue generated from `tests/models/roles-leyline`.
//! Prints heap allocations per decision first (deterministic; counted by
//! `stats_alloc`'s instrumented system allocator, so no `unsafe` here),
//! then criterion timings.
//!
//! `cargo bench --manifest-path e2e/Cargo.toml --bench roles`

use std::alloc::System;
use std::collections::HashMap;
use std::hint::black_box;
use std::sync::Arc;

use criterion::Criterion;
use crucible_server_runtime_rust::permissions::{Check, ResourceRef, Verdict, Visibility};
use crucible_server_runtime_rust::roles::{
    BindingSource, ByService, RoleAuthorizer, RoleBinding, Root, ServiceGrants, ServiceHatch,
    Unavailable,
};
use futures_util::FutureExt;
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

include!(concat!(env!("OUT_DIR"), "/roles_leyline.rs"));

use leyline::permissions::{GraphHistoryPermission, GraphHistoryRef, GraphPermission, GraphRef};
use leyline::roles::{Role, Scope};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

#[derive(Clone)]
struct Caller {
    id: &'static str,
    kind: &'static str,
}

/// Bindings by principal, as a service's cache would keep them.
struct Store(HashMap<&'static str, Arc<[RoleBinding]>>);

impl BindingSource for Store {
    type Principal = Caller;

    fn kind<'p>(&self, caller: &'p Caller) -> Option<&'p str> {
        Some(caller.kind)
    }

    async fn bindings(
        &self,
        caller: &Caller,
        _: Root<'_>,
        _: bool,
    ) -> Result<Arc<[RoleBinding]>, Unavailable> {
        Ok(self
            .0
            .get(caller.id)
            .cloned()
            .unwrap_or_else(|| Arc::from([])))
    }
}

impl ServiceGrants for Store {
    async fn decide_by_service(
        &self,
        _: &Caller,
        grant: ByService<'_>,
    ) -> Result<bool, Unavailable> {
        // The history of the binding's graph.
        let graph = grant.binding.identifiers.get(1).map(String::as_str);
        let history = grant
            .target
            .identifier("historyId")
            .and_then(|t| t.as_one());
        Ok(graph.is_some() && history.map(|h| h.strip_prefix("h-")) == Some(graph))
    }

    async fn visible_by_service(
        &self,
        _: &Caller,
        grant: ByService<'_>,
    ) -> Result<Visibility, Unavailable> {
        // The history of the binding's graph.
        Ok(Visibility::Only(
            grant
                .binding
                .identifiers
                .get(1)
                .map(|graph| format!("h-{graph}"))
                .into_iter()
                .collect(),
        ))
    }
}

fn binding(role: Role, scope: Scope, ids: &[&str]) -> RoleBinding {
    RoleBinding::new(role.as_str(), scope.as_str(), ids.iter().copied())
}

fn decide(
    authorizer: &RoleAuthorizer<Store, ServiceHatch>,
    caller: &Caller,
    checks: &[Check],
) -> Vec<Verdict> {
    authorizer
        .decide_for(caller, checks, false)
        .now_or_never()
        .expect("an in-memory source never waits")
        .unwrap()
}

fn main() {
    // A member with the bindings a Leyline member typically holds.
    let store = Store(HashMap::from([(
        "member",
        Arc::from(vec![
            binding(Role::UsageViewer, Scope::Organization, &["o-1"]),
            binding(Role::GraphReader, Scope::AllGraphs, &["o-1"]),
            binding(Role::GraphAdmin, Scope::Graph, &["o-1", "g-7"]),
        ]),
    )]));
    let authorizer = leyline::roles::authorizer(store);
    let member = Caller {
        id: "member",
        kind: "user",
    };
    let check = |p: GraphPermission, graph: &str| {
        Check::handler(p, ResourceRef::from(GraphRef::new("o-1", graph)))
    };
    let cases: Vec<(&str, Vec<Check>)> = vec![
        ("allowed", vec![check(GraphPermission::Read, "g-1")]),
        ("denied", vec![check(GraphPermission::Write, "g-1")]),
        (
            "hidden (another tenant)",
            vec![Check::handler(
                GraphPermission::Read,
                ResourceRef::from(GraphRef::new("o-2", "g-1")),
            )],
        ),
        (
            "two checks, one read",
            vec![
                check(GraphPermission::Read, "g-7"),
                check(GraphPermission::Manage, "g-7"),
            ],
        ),
        (
            "by service (history of g-7)",
            vec![Check::handler(
                GraphHistoryPermission::Read,
                ResourceRef::from(GraphHistoryRef::new("o-1", "h-g-7")),
            )],
        ),
    ];
    assert_eq!(
        decide(&authorizer, &member, &cases[0].1),
        [Verdict::Allowed]
    );
    assert_eq!(decide(&authorizer, &member, &cases[1].1), [Verdict::Denied]);
    assert_eq!(decide(&authorizer, &member, &cases[2].1), [Verdict::Hidden]);
    assert_eq!(
        decide(&authorizer, &member, &cases[4].1),
        [Verdict::Allowed]
    );

    const ROUNDS: u64 = 10_000;
    for (name, checks) in &cases {
        let region = Region::new(GLOBAL);
        for _ in 0..ROUNDS {
            black_box(decide(&authorizer, &member, black_box(checks)));
        }
        let change = region.change();
        let allocations = (change.allocations + change.reallocations) as f64 / ROUNDS as f64;
        println!("{name:<30} {allocations:>4} allocations per decision");
    }

    let mut criterion = Criterion::default().configure_from_args();
    for (name, checks) in &cases {
        criterion.bench_function(&format!("role_authorizer/{name}"), |b| {
            b.iter(|| decide(&authorizer, &member, black_box(checks)))
        });
    }
    criterion.final_summary();
}
