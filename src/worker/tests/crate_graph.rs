//! AC-5, crate part: the dependency graph of the worker (normal and build edges, every transitive crate) and
//! the symbols of its binary. Image checks are in `deploy/tests/check-images.sh`.
//!
//! The transitive set comes from `cargo tree -p sunnycloud-worker`, which resolves features for the worker
//! alone; `cargo metadata` unifies the features of all workspace members (the server turns on the `db`
//! feature of `sc_observability`), so it would show crates the worker binary does not link. For the same
//! reason the binary scanned for symbols is built here with `-p sunnycloud-worker` into its own target
//! folder, as the image build does, not the binary of a workspace-wide `cargo test`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

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
    /// The name of every crate the worker links, as `cargo tree` resolves it for the worker alone.
    fn reachable(&self) -> BTreeSet<String> {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
        let output = Command::new(cargo)
            .args([
                "tree",
                "--locked",
                "-p",
                "sunnycloud-worker",
                "--edges",
                "normal,build",
                "--target",
                "all",
                "--prefix",
                "none",
                "--format",
                "{p}",
            ])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("cargo tree starts");
        assert!(
            output.status.success(),
            "cargo tree failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("cargo tree is UTF-8")
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .map(str::to_owned)
            .collect()
    }

    fn is_component(&self, name: &str) -> bool {
        self.dir
            .get(name)
            .is_some_and(|folder| folder.starts_with("components/"))
    }
}

/// The worker binary as `cargo build -p sunnycloud-worker` links it, built once per test run.
fn isolated_binary() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| {
        let workspace_binary = Path::new(env!("CARGO_BIN_EXE_sunnycloud-worker"));
        let target_dir = workspace_binary
            .parent()
            .and_then(Path::parent)
            .expect("the binary is in <target>/<profile>")
            .join("ac5-isolated");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
        let output = Command::new(cargo)
            .args([
                "build",
                "--locked",
                "-p",
                "sunnycloud-worker",
                "--target-dir",
            ])
            .arg(&target_dir)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("cargo build starts");
        assert!(
            output.status.success(),
            "cargo build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let profile = workspace_binary
            .parent()
            .and_then(Path::file_name)
            .expect("profile folder");
        target_dir
            .join(profile)
            .join(workspace_binary.file_name().expect("binary name"))
    })
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
        .iter()
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
fn ac5_no_sqlx_sc_db_or_audit_crate_in_the_graph() {
    let graph = load();

    let reached = graph.reachable();

    let found: Vec<&String> = reached.iter().filter(|n| is_database_path(n)).collect();
    assert!(found.is_empty(), "database crates in the graph: {found:?}");
}

#[test]
fn ac5_no_sqlx_symbol_in_the_binary() {
    let binary = std::fs::read(isolated_binary()).expect("binary is readable");

    let hit = binary.windows(4).any(|window| window == b"sqlx");

    assert!(!hit, "the binary names sqlx");
}

#[test]
fn ac5_no_media_library_symbol_in_the_binary() {
    let binary = std::fs::read(isolated_binary()).expect("binary is readable");

    // The library files a dynamic link names, and one API function of each library for a static link. The bare
    // word `libheif` is no needle: it is a field of the decoder protocol's capabilities message.
    let needles = [
        "libvips.so",
        "libheif.so",
        "libavcodec.so",
        "libavformat.so",
        "vips_init",
        "heif_context_alloc",
        "avcodec_open2",
        "av_read_frame",
    ];
    let hits: Vec<&str> = needles
        .into_iter()
        .filter(|needle| {
            binary
                .windows(needle.len())
                .any(|window| window == needle.as_bytes())
        })
        .collect();

    assert!(
        hits.is_empty(),
        "media library files or functions in the binary: {hits:?}"
    );
}
