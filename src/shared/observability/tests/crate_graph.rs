//! AC-9 (the half this crate can prove): no edge to `contracts`, and the helpers it reads do not depend on it.
//! The other half (`contracts` re-exports the same two traits) is the test of the `contracts` task, because a
//! dev-dependency on `contracts` here would itself be the forbidden edge.

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;
use std::sync::Arc;

use sc_observability::{
    CheckFuture, HealthContributor, HealthItem, HealthStatus, ReadinessCheck, ReadinessItem,
    ReadinessRegistry,
};
use serde_json::Value;

fn dependency_names() -> BTreeMap<String, BTreeSet<String>> {
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--manifest-path",
        ])
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .output()
        .expect("cargo metadata runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value = serde_json::from_slice(&output.stdout).expect("metadata json");
    let name_of = |id: &str| -> String {
        metadata["packages"]
            .as_array()
            .expect("packages")
            .iter()
            .find(|package| package["id"] == id)
            .and_then(|package| package["name"].as_str())
            .expect("package name")
            .to_owned()
    };
    metadata["resolve"]["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .map(|node| {
            let deps = node["deps"]
                .as_array()
                .expect("deps")
                .iter()
                .map(|dep| name_of(dep["pkg"].as_str().expect("pkg")))
                .collect();
            (name_of(node["id"].as_str().expect("id")), deps)
        })
        .collect()
}

fn reachable(graph: &BTreeMap<String, BTreeSet<String>>, from: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![from.to_owned()];
    while let Some(name) = stack.pop() {
        for dep in graph.get(&name).into_iter().flatten() {
            if seen.insert(dep.clone()) {
                stack.push(dep.clone());
            }
        }
    }
    seen
}

#[test]
fn ac9_sc_observability_has_no_edge_to_contracts_and_the_helpers_do_not_depend_on_it() {
    let graph = dependency_names();

    let own = reachable(&graph, "sc_observability");
    assert!(
        !own.contains("contracts"),
        "sc_observability reaches contracts: {own:?}"
    );
    for helper in [
        "sc_db",
        "sc_audit",
        "sc_ratelimit",
        "sc_clock",
        "sc_error",
        "sc_config",
    ] {
        let below = reachable(&graph, helper);
        assert!(
            !below.contains("sc_observability"),
            "{helper} reaches sc_observability (cycle)"
        );
        assert!(!below.contains("contracts"), "{helper} reaches contracts");
    }
}

struct Recording;

impl ReadinessCheck for Recording {
    fn name(&self) -> &str {
        "recording"
    }

    fn check(&self) -> CheckFuture<'_, ReadinessItem> {
        Box::pin(async { ReadinessItem::ok() })
    }
}

impl HealthContributor for Recording {
    fn name(&self) -> &str {
        "recording"
    }

    fn check(&self) -> CheckFuture<'_, HealthItem> {
        Box::pin(async { HealthItem::new(HealthStatus::Degraded, "slow") })
    }
}

#[test]
fn ac9_both_traits_are_object_safe_send_sync_and_registrable_through_this_crate() {
    fn assert_send_sync<T: Send + Sync + ?Sized>() {}
    assert_send_sync::<dyn ReadinessCheck>();
    assert_send_sync::<dyn HealthContributor>();

    let mut registry = ReadinessRegistry::new();
    registry.add(Arc::new(Recording));
    let contributor: Arc<dyn HealthContributor> = Arc::new(Recording);

    assert_eq!(ReadinessCheck::name(&Recording), "recording");
    assert_eq!(contributor.name(), "recording");
}
