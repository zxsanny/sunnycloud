//! AC-1 (listing), AC-4 (signatures only, server-only crate) and the HTTP-free half of AC-6.
//! Parts 2 and 3 add their names beside `PART_ONE`; the check is "0 names missing".

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

const PART_ONE: &[&str] = &[
    "Janitor",
    "AccountLifecycleHook",
    "BlobOwnerCleanup",
    "DoctorCheck",
    "Authenticator",
    "PrincipalRevalidator",
    "AccountGuard",
    "SignInService",
    "SessionService",
    "DeviceGrantService",
    "InviteService",
    "OperatorIdentityOps",
    "AccountAdmin",
    "AdminService",
    "AccountService",
    "AccountDeletionService",
    "DevSeed",
    "QuotaLedger",
    "QuotaAdmin",
];

fn source_files(dir: &Path, files: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).expect("read src dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            source_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let text = fs::read_to_string(&path).expect("read source");
            files.push((path.display().to_string(), text));
        }
    }
}

fn crate_sources() -> Vec<(String, String)> {
    let mut files = Vec::new();
    source_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    files
}

#[test]
fn every_part_one_port_is_a_public_trait_of_the_crate() {
    let declared: BTreeSet<String> = crate_sources()
        .iter()
        .flat_map(|(_, text)| {
            text.lines()
                .map(str::trim)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter_map(|line| {
            line.strip_prefix("pub trait ").map(|rest| {
                rest.split([':', ' ', '{'])
                    .next()
                    .unwrap_or_default()
                    .to_owned()
            })
        })
        .collect();

    let missing: Vec<_> = PART_ONE
        .iter()
        .filter(|name| !declared.contains(**name))
        .collect();

    assert!(missing.is_empty(), "missing ports: {missing:?}");
}

#[test]
fn the_re_exported_ports_are_the_sc_observability_traits() {
    use std::any::TypeId;

    assert_eq!(
        TypeId::of::<dyn contracts::HealthContributor>(),
        TypeId::of::<dyn sc_observability::HealthContributor>()
    );
    assert_eq!(
        TypeId::of::<dyn contracts::ReadinessCheck>(),
        TypeId::of::<dyn sc_observability::ReadinessCheck>()
    );
}

#[test]
fn the_crate_has_no_unimplemented_body() {
    let todo = ["todo", "!("].concat();
    let unimplemented = ["unimplemented", "!("].concat();

    let hits: Vec<_> = crate_sources()
        .into_iter()
        .filter(|(_, text)| text.contains(&todo) || text.contains(&unimplemented))
        .map(|(path, _)| path)
        .collect();

    assert!(hits.is_empty(), "stub bodies in {hits:?}");
}

#[test]
fn the_crate_names_no_http_status_number_or_http_type() {
    let forbidden_words = ["StatusCode", "axum", "hyper", "sqlx", "http"];
    let mut hits = Vec::new();

    for (path, text) in crate_sources() {
        for (number, line) in text.lines().enumerate() {
            let words = line.split(|c: char| !c.is_alphanumeric() && c != '_');
            for word in words {
                let status =
                    word.len() == 3 && word.parse::<u16>().is_ok_and(|n| (100..600).contains(&n));
                if status || forbidden_words.contains(&word) {
                    hits.push(format!("{path}:{}: {word}", number + 1));
                }
            }
        }
    }

    assert!(hits.is_empty(), "HTTP mentions: {hits:?}");
}

struct Package {
    dir: String,
    deps: BTreeSet<String>,
}

fn workspace_packages() -> BTreeMap<String, Package> {
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--no-deps",
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
    let root = metadata["workspace_root"]
        .as_str()
        .expect("workspace_root")
        .to_owned();
    metadata["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .map(|package| {
            let manifest = package["manifest_path"].as_str().expect("manifest_path");
            let dir = Path::new(manifest)
                .strip_prefix(&root)
                .ok()
                .and_then(Path::parent)
                .and_then(Path::to_str)
                .expect("manifest inside the workspace")
                .replace('\\', "/");
            let deps = package["dependencies"]
                .as_array()
                .expect("dependencies")
                .iter()
                .map(|dep| dep["name"].as_str().expect("dependency name").to_owned())
                .collect();
            (
                package["name"].as_str().expect("name").to_owned(),
                Package { dir, deps },
            )
        })
        .collect()
}

#[test]
fn contracts_depends_directly_only_on_helpers_and_plain_libraries() {
    let packages = workspace_packages();
    let allowed = [
        "async-trait",
        "sc_clock",
        "sc_db",
        "sc_error",
        "sc_ids",
        "sc_observability",
        "serde_json",
        "thiserror",
        "tokio",
        "uuid",
    ];

    let deps = &packages["contracts"].deps;
    let unexpected: Vec<_> = deps
        .iter()
        .filter(|dep| !allowed.contains(&dep.as_str()))
        .collect();

    assert!(
        unexpected.is_empty(),
        "unexpected direct dependencies: {unexpected:?}"
    );
    for forbidden in [
        "sqlx",
        "axum",
        "hyper",
        "http",
        "tower",
        "reqwest",
        "media-shared",
    ] {
        assert!(
            !deps.contains(forbidden),
            "contracts depends on {forbidden}"
        );
    }
}

#[test]
fn only_server_crates_depend_on_contracts() {
    let packages = workspace_packages();

    let offenders: Vec<_> = packages
        .iter()
        .filter(|(_, package)| package.deps.contains("contracts"))
        .filter(|(_, package)| !(package.dir.starts_with("components/") || package.dir == "server"))
        .map(|(name, package)| format!("{name} ({})", package.dir))
        .collect();

    assert!(
        offenders.is_empty(),
        "non-server crates depend on contracts: {offenders:?}"
    );
}
