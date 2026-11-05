use std::fs;
use std::path::Path;

const ALLOWED_DEPENDENCIES: &[&str] = &["icu_casemap", "icu_normalizer", "sc_clock", "thiserror"];
const FORBIDDEN_IN_SOURCE: &[&str] = &[
    "std::fs",
    "std::net",
    "std::process",
    "std::env",
    "std::thread",
    "std::io",
    "SystemTime",
    "Instant",
    "tokio",
    "sqlx",
    "async fn",
    ".await",
    "static mut",
    "thread_local",
    "OnceLock",
    "lazy_static",
];

fn manifest_section(manifest: &str, header: &str) -> Vec<String> {
    manifest
        .lines()
        .skip_while(|l| l.trim() != header)
        .skip(1)
        .take_while(|l| !l.trim_start().starts_with('['))
        .filter_map(|l| l.split('=').next().map(|k| k.trim().to_owned()))
        .filter(|k| !k.is_empty() && !k.starts_with('#'))
        .collect()
}

#[test]
fn direct_dependencies_are_the_pinned_unicode_libraries_the_clock_and_thiserror_only() {
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("manifest");
    let mut deps = manifest_section(&manifest, "[dependencies]");
    deps.sort();
    assert_eq!(
        deps, ALLOWED_DEPENDENCIES,
        "no I/O, database or async runtime dependency"
    );
}

#[test]
fn library_sources_do_no_io_and_hold_no_global_state() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut scanned = 0;
    for entry in fs::read_dir(src).expect("src dir") {
        let path = entry.expect("entry").path();
        let text = fs::read_to_string(&path).expect("source file");
        let production = text.split("#[cfg(test)]").next().unwrap_or(&text);
        for needle in FORBIDDEN_IN_SOURCE {
            assert!(
                !production.contains(needle),
                "{} uses {needle}",
                path.display()
            );
        }
        assert!(
            text.lines().count() <= 500,
            "{} is over 500 lines",
            path.display()
        );
        scanned += 1;
    }
    assert!(scanned >= 10);
}
