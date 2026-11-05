//! AC-4 (ADR-009): `media-shared` is a types crate. Its dependency list, read from
//! `cargo metadata` (direct dependencies, workspace members) and `cargo tree -p media-shared`
//! (every transitive crate with the features this package alone enables), has no sqlx, no
//! HTTP client or server, no `contracts`, no component crate and no async runtime I/O.
//!
//! The same rules are checked in `src/server/tests/crate_graph.rs` for sqlx; this test is the
//! full list for the crate itself.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

const PACKAGE: &str = "media-shared";

/// Direct production dependencies: serde-level crates, `Uuid` and `FileHash`.
const ALLOWED_DIRECT: [&str; 3] = ["sc_ids", "serde", "uuid"];

/// Workspace crates the graph may contain: the crate itself and `sc_ids` with its `sc_clock`.
const ALLOWED_WORKSPACE: [&str; 3] = [PACKAGE, "sc_ids", "sc_clock"];

const FORBIDDEN_CRATES: [&str; 11] = [
    "sqlx",
    "reqwest",
    "hyper",
    "hyper-util",
    "axum",
    "h2",
    "mio",
    "socket2",
    "rustls",
    "contracts",
    "sc_db",
];

/// Tokio features that open files, sockets, pipes or processes.
const TOKIO_IO_FEATURES: [&str; 6] = ["net", "fs", "io-util", "io-std", "process", "signal"];

#[derive(Debug, Clone)]
struct Resolved {
    name: String,
    features: Vec<String>,
}

#[derive(Debug, Clone)]
struct Graph {
    direct: BTreeSet<String>,
    workspace: BTreeSet<String>,
    resolved: Vec<Resolved>,
}

fn cargo(args: &[&str]) -> String {
    let program = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = Command::new(program)
        .args(args)
        .current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")))
        .output()
        .expect("cargo did not start");
    assert!(
        output.status.success(),
        "cargo {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("cargo output is UTF-8")
}

fn load_graph() -> Graph {
    let metadata: serde_json::Value = serde_json::from_str(&cargo(&[
        "metadata",
        "--no-deps",
        "--format-version",
        "1",
        "--locked",
    ]))
    .expect("cargo metadata output is JSON");
    let packages = metadata["packages"].as_array().expect("packages");
    let workspace = packages
        .iter()
        .map(|package| package["name"].as_str().expect("name").to_owned())
        .collect();
    let this = packages
        .iter()
        .find(|package| package["name"] == PACKAGE)
        .expect("media-shared is a workspace member");
    let direct = this["dependencies"]
        .as_array()
        .expect("dependencies")
        .iter()
        .filter(|dep| dep["kind"].is_null())
        .map(|dep| dep["name"].as_str().expect("dependency name").to_owned())
        .collect();

    let tree = cargo(&[
        "tree",
        "-p",
        PACKAGE,
        "-e",
        "normal,build",
        "--prefix",
        "none",
        "--locked",
        "-f",
        "{p}|{f}",
    ]);
    let resolved = tree
        .lines()
        .filter_map(|line| {
            let (package, features) = line.split_once('|')?;
            Some(Resolved {
                name: package.split_whitespace().next()?.to_owned(),
                features: features
                    .split(',')
                    .filter(|f| !f.is_empty())
                    .map(str::to_owned)
                    .collect(),
            })
        })
        .collect();
    Graph {
        direct,
        workspace,
        resolved,
    }
}

fn violations(graph: &Graph) -> Vec<String> {
    let mut found = Vec::new();
    for dep in &graph.direct {
        if !ALLOWED_DIRECT.contains(&dep.as_str()) {
            found.push(format!("direct dependency {dep} is not allowed"));
        }
    }
    for package in &graph.resolved {
        let name = package.name.as_str();
        if FORBIDDEN_CRATES.contains(&name) {
            found.push(format!("{name} is in the dependency graph"));
        }
        if graph.workspace.contains(name) && !ALLOWED_WORKSPACE.contains(&name) {
            found.push(format!("workspace crate {name} is in the dependency graph"));
        }
        if name == "tokio" {
            for feature in &package.features {
                if TOKIO_IO_FEATURES.contains(&feature.as_str()) {
                    found.push(format!("tokio enables I/O feature {feature}"));
                }
            }
        }
    }
    found
}

fn with_resolved(mut graph: Graph, name: &str, features: &[&str]) -> Graph {
    graph.resolved.push(Resolved {
        name: name.to_owned(),
        features: features.iter().map(|f| (*f).to_owned()).collect(),
    });
    graph
}

#[test]
fn the_dependency_list_has_nothing_forbidden() {
    let graph = load_graph();

    let found = violations(&graph);

    assert!(found.is_empty(), "forbidden dependencies: {found:?}");
}

#[test]
fn the_direct_dependencies_are_serde_uuid_and_sc_ids_only() {
    let graph = load_graph();

    assert_eq!(
        graph.direct,
        ALLOWED_DIRECT.iter().map(|d| (*d).to_owned()).collect()
    );
}

#[test]
fn the_graph_is_not_empty_and_contains_sc_ids() {
    let graph = load_graph();

    let names: Vec<&str> = graph.resolved.iter().map(|r| r.name.as_str()).collect();

    assert!(names.contains(&PACKAGE) && names.contains(&"sc_ids") && names.contains(&"serde"));
}

#[test]
fn sqlx_is_rejected() {
    let graph = with_resolved(load_graph(), "sqlx", &[]);

    assert_eq!(violations(&graph), vec!["sqlx is in the dependency graph"]);
}

#[test]
fn an_http_client_or_server_is_rejected() {
    let graph = with_resolved(with_resolved(load_graph(), "reqwest", &[]), "hyper", &[]);

    assert_eq!(
        violations(&graph),
        vec![
            "reqwest is in the dependency graph",
            "hyper is in the dependency graph"
        ]
    );
}

#[test]
fn contracts_and_a_component_crate_are_rejected() {
    let mut graph = load_graph();
    graph.workspace.insert("sunnycloud-tree".to_owned());
    let graph = with_resolved(
        with_resolved(graph, "contracts", &[]),
        "sunnycloud-tree",
        &[],
    );

    let found = violations(&graph);

    assert!(found.contains(&"contracts is in the dependency graph".to_owned()));
    assert!(
        found.contains(&"workspace crate sunnycloud-tree is in the dependency graph".to_owned())
    );
}

#[test]
fn async_runtime_io_is_rejected() {
    let graph = with_resolved(load_graph(), "tokio", &["net", "time"]);

    assert_eq!(violations(&graph), vec!["tokio enables I/O feature net"]);
}

#[test]
fn a_new_direct_dependency_is_rejected() {
    let mut graph = load_graph();
    graph.direct.insert("tokio".to_owned());

    assert_eq!(
        violations(&graph),
        vec!["direct dependency tokio is not allowed"]
    );
}
