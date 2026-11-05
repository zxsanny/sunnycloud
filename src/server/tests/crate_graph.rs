use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

#[derive(Clone)]
struct Package {
    name: String,
    dir: String,
    deps: BTreeSet<String>,
}

type Graph = BTreeMap<String, Package>;

const WORKSPACE_PACKAGES: &[(&str, &str)] = &[
    ("sc_config", "shared/config"),
    ("sc_error", "shared/error"),
    ("sc_clock", "shared/clock"),
    ("sc_audit", "shared/audit"),
    ("sc_ids", "shared/ids_hashing"),
    ("sc_names", "shared/names"),
    ("sc_db", "shared/db"),
    ("sc_ratelimit", "shared/ratelimit"),
    ("sc_observability", "shared/observability"),
    ("contracts", "contracts"),
    ("media-shared", "media-shared"),
    ("sunnycloud-identity", "components/01_identity"),
    ("sunnycloud-tree", "components/02_tree"),
    ("sunnycloud-transfer", "components/03_transfer"),
    ("sunnycloud-blobstore", "components/04_blobstore"),
    ("sunnycloud-sync-feed", "components/05_sync_feed"),
    ("sunnycloud-sharing", "components/06_sharing"),
    ("sunnycloud-admin", "components/07_admin"),
    ("sunnycloud-quota", "components/07a_quota"),
    ("media-api", "components/08_media_jobs"),
    ("sunnycloud-http-api", "components/11_http_api"),
    ("sunnycloud-vault-service", "components/20_vault_service"),
    ("sunnycloud-server", "server"),
    ("sunnycloud-worker", "worker"),
    ("decoder-runner", "decoder"),
    ("sync-core", "sync-core"),
    ("sync-core-wasm", "sync-core-wasm"),
    ("ipc_schema", "desktop/ipc_schema"),
    ("sunnycloud-daemon", "desktop/daemon"),
    ("sunnycloud-settings", "desktop/settings"),
    ("mobile_backup", "mobile_backup"),
    ("vault-core", "vault-core"),
];

const NO_SQLX: &[&str] = &[
    "sunnycloud-worker",
    "decoder-runner",
    "sunnycloud-http-api",
    "sunnycloud-server",
    "media-shared",
];

const WASM_FORBIDDEN: &[&str] = &["sync-core", "tokio", "reqwest", "mio"];

const CLIENTS: &[&str] = &[
    "sync-core",
    "sync-core-wasm",
    "mobile_backup",
    "vault-core",
    "ipc_schema",
    "sunnycloud-daemon",
    "sunnycloud-settings",
];

fn is_component(package: &Package) -> bool {
    package.dir.starts_with("components/")
}

fn workspace_graph() -> Graph {
    static GRAPH: OnceLock<Graph> = OnceLock::new();
    GRAPH.get_or_init(load_graph).clone()
}

fn load_graph() -> Graph {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["metadata", "--no-deps", "--format-version", "1", "--locked"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata did not start");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata output is not JSON");
    let root = metadata["workspace_root"]
        .as_str()
        .expect("workspace_root missing")
        .to_string();
    let mut graph = Graph::new();
    for raw in metadata["packages"].as_array().expect("packages missing") {
        let name = raw["name"].as_str().expect("package name").to_string();
        let manifest = raw["manifest_path"].as_str().expect("manifest_path");
        let dir = Path::new(manifest)
            .strip_prefix(&root)
            .ok()
            .and_then(Path::parent)
            .and_then(Path::to_str)
            .expect("manifest outside the workspace")
            .replace('\\', "/");
        let deps = raw["dependencies"]
            .as_array()
            .expect("dependencies")
            .iter()
            .map(|dep| dep["name"].as_str().expect("dependency name").to_string())
            .collect();
        graph.insert(name.clone(), Package { name, dir, deps });
    }
    graph
}

fn violations(graph: &Graph) -> Vec<String> {
    let mut found = Vec::new();
    for package in graph.values() {
        for dep in &package.deps {
            let target = graph.get(dep);
            if let Some(target) = target {
                if is_component(package) && is_component(target) && package.name != target.name {
                    found.push(format!("component edge {} -> {}", package.name, dep));
                }
                if CLIENTS.contains(&package.name.as_str())
                    && (dep == "contracts" || is_component(target))
                {
                    found.push(format!(
                        "client {} depends on server crate {}",
                        package.name, dep
                    ));
                }
            }
            if dep == "sunnycloud-server" {
                found.push(format!("{} depends on sunnycloud-server", package.name));
            }
            if dep == "wasm-bindgen" && package.name != "sync-core-wasm" {
                found.push(format!("{} depends on wasm-bindgen", package.name));
            }
            if package.name == "sync-core-wasm" && WASM_FORBIDDEN.contains(&dep.as_str()) {
                found.push(format!("sync-core-wasm depends on {dep}"));
            }
            if dep == "sqlx" && NO_SQLX.contains(&package.name.as_str()) {
                found.push(format!("{} depends on sqlx", package.name));
            }
        }
    }
    found
}

fn add_edge(graph: &mut Graph, from: &str, to: &str) {
    graph
        .get_mut(from)
        .unwrap_or_else(|| panic!("{from} missing from the graph"))
        .deps
        .insert(to.to_string());
}

#[test]
fn workspace_has_exactly_the_planned_packages_at_their_folders() {
    let graph = workspace_graph();

    let actual: BTreeMap<_, _> = graph
        .values()
        .map(|package| (package.name.clone(), package.dir.clone()))
        .collect();
    let expected: BTreeMap<_, _> = WORKSPACE_PACKAGES
        .iter()
        .map(|(name, dir)| (name.to_string(), dir.to_string()))
        .collect();

    assert_eq!(actual, expected);
}

#[test]
fn skeleton_graph_has_no_forbidden_edge() {
    let graph = workspace_graph();

    let found = violations(&graph);

    assert!(found.is_empty(), "forbidden edges: {found:?}");
}

#[test]
fn component_to_component_edge_is_rejected() {
    let mut graph = workspace_graph();
    add_edge(&mut graph, "sunnycloud-tree", "sunnycloud-quota");

    let found = violations(&graph);

    assert_eq!(
        found,
        vec!["component edge sunnycloud-tree -> sunnycloud-quota"]
    );
}

#[test]
fn sqlx_in_media_shared_is_rejected() {
    let mut graph = workspace_graph();
    add_edge(&mut graph, "media-shared", "sqlx");

    let found = violations(&graph);

    assert_eq!(found, vec!["media-shared depends on sqlx"]);
}

#[test]
fn client_depending_on_contracts_or_a_component_is_rejected() {
    let mut graph = workspace_graph();
    add_edge(&mut graph, "sync-core", "contracts");
    add_edge(&mut graph, "sunnycloud-daemon", "sunnycloud-tree");

    let found = violations(&graph);

    assert_eq!(
        found,
        vec![
            "client sunnycloud-daemon depends on server crate sunnycloud-tree",
            "client sync-core depends on server crate contracts",
        ]
    );
}

#[test]
fn dependency_on_the_composition_root_is_rejected() {
    let mut graph = workspace_graph();
    add_edge(&mut graph, "sunnycloud-http-api", "sunnycloud-server");

    let found = violations(&graph);

    assert!(found.contains(&"sunnycloud-http-api depends on sunnycloud-server".to_string()));
}

#[test]
fn wasm_bindgen_outside_the_wasm_adapter_is_rejected() {
    let mut graph = workspace_graph();
    add_edge(&mut graph, "sync-core", "wasm-bindgen");

    let found = violations(&graph);

    assert_eq!(found, vec!["sync-core depends on wasm-bindgen"]);
}

#[test]
fn wasm_adapter_depending_on_the_native_client_is_rejected() {
    let mut graph = workspace_graph();
    add_edge(&mut graph, "sync-core-wasm", "sync-core");

    let found = violations(&graph);

    assert_eq!(found, vec!["sync-core-wasm depends on sync-core"]);
}
