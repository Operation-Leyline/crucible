//! What one `RoleAuthorizer` decision costs: one read of the principal's
//! bindings from an in-memory store (a map lookup and an `Arc` clone), then
//! per check the bindings' identifier-prefix comparisons and bitset tests
//! against the catalogue generated from `tests/models/roles-leyline`.
//! Prints heap allocations per decision first (deterministic), then
//! criterion timings.
//!
//! `cargo bench --manifest-path e2e/Cargo.toml --bench roles`

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashMap;
use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use criterion::Criterion;
use crucible_server_runtime_rust::permissions::{Check, ResourceRef, Verdict};
use crucible_server_runtime_rust::roles::{
    BindingSource, ByService, RoleAuthorizer, RoleBinding, Root, Unavailable,
};
use futures_util::FutureExt;

include!(concat!(env!("OUT_DIR"), "/roles_leyline.rs"));

use leyline::permissions::{GraphHistoryPermission, GraphHistoryRef, GraphPermission, GraphRef};
use leyline::roles::{CATALOGUE, Role, Scope};

struct Counting;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

// SAFETY: forwards to the system allocator, counting on the way.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

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
}

fn binding(role: Role, scope: Scope, ids: &[&str]) -> RoleBinding {
    RoleBinding::new(role.as_str(), scope.as_str(), ids.iter().copied())
}

fn decide(authorizer: &RoleAuthorizer<Store>, caller: &Caller, checks: &[Check]) -> Vec<Verdict> {
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
    let authorizer = RoleAuthorizer::new(&CATALOGUE, store);
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
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        for _ in 0..ROUNDS {
            black_box(decide(&authorizer, &member, black_box(checks)));
        }
        let allocations = (ALLOCATIONS.load(Ordering::Relaxed) - before) as f64 / ROUNDS as f64;
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
