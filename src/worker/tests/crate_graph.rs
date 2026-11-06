//! AC-5, crate part: the resolved dependency graph of the worker (normal and build edges, every transitive
//! crate) and the symbols of its binary. Image checks are in `deploy/tests/check-images.sh`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;
use std::process::Command;

const BLOCKED_REASON: &str = "blocked: sc_observability depends on sc_db and sc_audit, which link sqlx (open flagged concern of AZ-3868, owners of E-09 and E-00); enable when the dependency is split";

struct Graph {
    /// Package name -> workspace-relative manifest folder, for every package.
    dir: BTreeMap<String, String>,
    /// Package name -> names of its normal and build dependencies.
    edges: BTreeMap<String, BTreeSet<String>>,
}

fn load() -> Graph {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata starts");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata is JSON");
    let root = metadata["workspace_root"].as_str().expect("workspace_root");
    let mut name_of = BTreeMap::new();
    let mut dir = BTreeMap::new();
    for package in metadata["packages"].as_array().expect("packages") {
        let id = package["id"].as_str().expect("id").to_owned();
        let name = package["name"].as_str().expect("name").to_owned();
        let manifest = package["manifest_path"].as_str().expect("manifest_path");
        let folder = Path::new(manifest)
            .parent()
            .and_then(|folder| folder.strip_prefix(root).ok())
            .map(|folder| folder.display().to_string())
            .unwrap_or_default();
        name_of.insert(id, name.clone());
        dir.insert(name, folder);
    }
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for node in metadata["resolve"]["nodes"].as_array().expect("nodes") {
        let from = &name_of[node["id"].as_str().expect("node id")];
        let targets = edges.entry(from.clone()).or_default();
        for dep in node["deps"].as_array().expect("deps") {
            let shipped = dep["dep_kinds"]
                .as_array()
                .expect("dep_kinds")
                .iter()
                .any(|kind| kind["kind"].as_str() != Some("dev"));
            if shipped {
                targets.insert(name_of[dep["pkg"].as_str().expect("pkg")].clone());
            }
        }
    }
    Graph { dir, edges }
}

impl Graph {
    /// Every crate the worker reaches, with the first crate after the worker on the way to it.
    fn reachable(&self) -> BTreeMap<String, String> {
        let mut first_hop = BTreeMap::new();
        let mut queue = VecDeque::new();
        for dep in &self.edges["sunnycloud-worker"] {
            first_hop.insert(dep.clone(), dep.clone());
            queue.push_back(dep.clone());
        }
        while let Some(current) = queue.pop_front() {
            let hop = first_hop[&current].clone();
            for dep in self.edges.get(&current).into_iter().flatten() {
                if !first_hop.contains_key(dep) {
                    first_hop.insert(dep.clone(), hop.clone());
                    queue.push_back(dep.clone());
                }
            }
        }
        first_hop
    }

    fn is_component(&self, name: &str) -> bool {
        self.dir
            .get(name)
            .is_some_and(|folder| folder.starts_with("components/"))
    }
}

/// Crates that give a process a database, the audit log writer or a media library.
fn is_database_path(name: &str) -> bool {
    name == "sc_db" || name == "sc_audit" || name.starts_with("sqlx")
}

fn is_media_library(name: &str) -> bool {
    ["ffmpeg", "vips", "libheif", "image", "av-codec"]
        .iter()
        .any(|part| name.contains(part))
}

#[test]
fn ac5_no_contracts_component_server_or_media_library_crate_in_the_graph() {
    let graph = load();

    let reached = graph.reachable();

    let found: Vec<&String> = reached
        .keys()
        .filter(|name| {
            name.as_str() == "contracts"
                || name.as_str() == "sunnycloud-server"
                || graph.is_component(name)
                || is_media_library(name)
        })
        .collect();
    assert!(found.is_empty(), "forbidden crates in the graph: {found:?}");
}

#[test]
fn ac5_the_worker_declares_no_database_dependency_itself() {
    let graph = load();

    let direct = &graph.edges["sunnycloud-worker"];

    let found: Vec<&String> = direct.iter().filter(|n| is_database_path(n)).collect();
    assert!(found.is_empty(), "direct database dependencies: {found:?}");
}

#[test]
fn ac5_database_crates_reach_the_worker_only_through_sc_observability() {
    let graph = load();

    let reached = graph.reachable();

    let other_paths: Vec<(&String, &String)> = reached
        .iter()
        .filter(|(name, hop)| is_database_path(name) && hop.as_str() != "sc_observability")
        .collect();
    assert!(
        other_paths.is_empty(),
        "database crates through another dependency: {other_paths:?}"
    );
}

#[test]
#[ignore = "blocked: sc_observability depends on sc_db and sc_audit, which link sqlx (open flagged concern of AZ-3868, owners of E-09 and E-00); enable when the dependency is split"]
fn ac5_no_sqlx_sc_db_or_audit_crate_in_the_graph() {
    let graph = load();

    let reached = graph.reachable();

    let found: Vec<&String> = reached.keys().filter(|n| is_database_path(n)).collect();
    assert!(found.is_empty(), "{BLOCKED_REASON}: {found:?}");
}

#[test]
#[ignore = "blocked: sc_observability depends on sc_db and sc_audit, which link sqlx (open flagged concern of AZ-3868, owners of E-09 and E-00); enable when the dependency is split"]
fn ac5_no_sqlx_symbol_in_the_binary() {
    let binary =
        std::fs::read(env!("CARGO_BIN_EXE_sunnycloud-worker")).expect("binary is readable");

    let hit = binary.windows(4).any(|window| window == b"sqlx");

    assert!(!hit, "{BLOCKED_REASON}");
}

#[test]
fn ac5_no_media_library_symbol_in_the_binary() {
    let binary =
        std::fs::read(env!("CARGO_BIN_EXE_sunnycloud-worker")).expect("binary is readable");

    let hits: Vec<&str> = ["libvips", "libheif", "libavcodec", "libavformat"]
        .into_iter()
        .filter(|needle| {
            binary
                .windows(needle.len())
                .any(|window| window == needle.as_bytes())
        })
        .collect();

    assert!(
        hits.is_empty(),
        "media library names in the binary: {hits:?}"
    );
}
