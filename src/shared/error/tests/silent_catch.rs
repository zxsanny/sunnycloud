//! AC-7: the silent-catch gate. It complements the workspace lints `unused_must_use` and
//! `clippy::let_underscore_must_use` (both deny, type-aware) with a text scan that cannot see types,
//! so it rejects every `let _ =` and every empty `Err(..)` arm. Pattern literals below are split with
//! `concat!` so this file never contains a violation itself.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const LET_DISCARD: &str = concat!("let", " _=");
const LET_TYPED_DISCARD: &str = concat!("let", " _:");

#[derive(Debug, PartialEq, Eq)]
struct Violation {
    line: usize,
    rule: &'static str,
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Drops `//` comments, collapses whitespace (kept only between two identifier characters),
/// and remembers the source line of every kept character.
fn normalise(source: &str) -> (Vec<char>, Vec<usize>) {
    let mut chars: Vec<char> = Vec::new();
    let mut lines: Vec<usize> = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let code = line.split("//").next().unwrap_or_default();
        let mut pending_space = true;
        for c in code.chars() {
            if c.is_whitespace() {
                pending_space = true;
                continue;
            }
            if pending_space && chars.last().is_some_and(|prev| is_ident(*prev)) && is_ident(c) {
                chars.push(' ');
                lines.push(index + 1);
            }
            pending_space = false;
            chars.push(c);
            lines.push(index + 1);
        }
        if chars.last().is_some_and(|prev| is_ident(*prev)) {
            chars.push(' ');
            lines.push(index + 1);
        }
    }
    (chars, lines)
}

fn starts_with_at(chars: &[char], at: usize, pattern: &str) -> bool {
    pattern
        .chars()
        .enumerate()
        .all(|(offset, expected)| chars.get(at + offset) == Some(&expected))
}

fn boundary_before(chars: &[char], at: usize) -> bool {
    at == 0 || !is_ident(chars[at - 1])
}

fn scan(source: &str) -> Vec<Violation> {
    let (chars, lines) = normalise(source);
    let mut found = Vec::new();
    for (at, line) in lines.iter().copied().enumerate() {
        if !boundary_before(&chars, at) {
            continue;
        }
        if starts_with_at(&chars, at, LET_DISCARD) || starts_with_at(&chars, at, LET_TYPED_DISCARD)
        {
            found.push(Violation {
                line,
                rule: "discarded result (underscore binding)",
            });
        }
        if starts_with_at(&chars, at, "Err(") {
            let inner_start = at + 4;
            let Some(close) = chars[inner_start..].iter().position(|c| *c == ')') else {
                continue;
            };
            let inner: String = chars[inner_start..inner_start + close].iter().collect();
            let after = inner_start + close + 1;
            let catch_all = inner == ".." || inner.starts_with('_');
            let empty_arm =
                starts_with_at(&chars, after, "=>{}") || starts_with_at(&chars, after, "=>()");
            if catch_all && empty_arm {
                found.push(Violation {
                    line,
                    rule: "empty catch-all Err arm",
                });
            }
        }
    }
    found
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if path.is_dir() {
            if name != "target" && !name.starts_with('.') {
                collect_files(&path, files)?;
            }
        } else if name.ends_with(".rs") || name == "Cargo.toml" {
            files.push(path);
        }
    }
    Ok(())
}

fn workspace_files() -> io::Result<(PathBuf, Vec<PathBuf>)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let mut files = Vec::new();
    collect_files(&root, &mut files)?;
    files.sort();
    Ok((root, files))
}

fn seeded_discard() -> String {
    format!(
        "fn f() {{\n    {} fallible();\n}}\n",
        concat!("let", " _ =")
    )
}

#[test]
fn seeded_result_discard_is_caught_with_its_line() {
    let found = scan(&seeded_discard());

    assert_eq!(
        found,
        vec![Violation {
            line: 2,
            rule: "discarded result (underscore binding)"
        }]
    );
}

#[test]
fn seeded_variants_are_all_caught() {
    let fixtures = [
        format!("{} fallible();", concat!("let", "  _  =")),
        format!("{} Result<(), E> = fallible();", concat!("let", " _:")),
        format!("{} fallible() {{ run() }}", concat!("if let", " _ =")),
        format!("match r {{ {}{}}}", concat!("Err(", "_) => "), "{}"),
        format!("match r {{ {}{}}}", concat!("Err(", "_) => "), "()"),
        format!("match r {{ {}{}}}", concat!("Err(", "..) => "), "{}"),
        format!("match r {{\n  {}{{\n  }}\n}}", concat!("Err(", "_e) => ")),
    ];

    for fixture in &fixtures {
        assert_eq!(scan(fixture).len(), 1, "not caught: {fixture}");
    }
}

#[test]
fn clean_code_comments_and_named_bindings_are_not_flagged() {
    let clean = [
        "let _guard = lock();".to_owned(),
        "let _x = 1;".to_owned(),
        "let outlet = 1; let toilet_ = 2;".to_owned(),
        format!("// {} fallible();", concat!("let", " _ =")),
        "match r { Err(e) => { log(e); } Ok(_) => {} }".to_owned(),
        format!(
            "match r {{ {}return Err(Failure) }}",
            concat!("Err(", "_) => ")
        ),
        format!("match r {{ {}{{ log(); }} }}", concat!("Err(", "_) => ")),
    ];

    for source in &clean {
        assert_eq!(scan(source), vec![], "false positive: {source}");
    }
}

#[test]
fn seeded_violation_in_a_workspace_file_fails_the_gate() {
    // The gate runs over a "workspace" made of the real file list plus one seeded in-memory file.
    let (_, files) = workspace_files().expect("workspace listing");
    let seeded = ("crates/seeded/src/lib.rs".to_owned(), seeded_discard());
    let mut sources: Vec<(String, String)> = Vec::new();
    for path in files
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
    {
        let text = fs::read_to_string(path).expect("readable source");
        sources.push((path.display().to_string(), text));
    }
    sources.push(seeded);

    let failing: Vec<&str> = sources
        .iter()
        .filter(|(_, text)| !scan(text).is_empty())
        .map(|(name, _)| name.as_str())
        .collect();

    assert_eq!(failing, ["crates/seeded/src/lib.rs"]);
}

#[test]
fn workspace_sources_have_no_silent_discard() {
    let (root, files) = workspace_files().expect("workspace listing");
    let rust_files: Vec<&PathBuf> = files
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect();
    assert!(
        rust_files
            .iter()
            .any(|p| p.ends_with("tests/silent_catch.rs")),
        "scan must reach this crate's own tests"
    );

    let mut report = Vec::new();
    for path in rust_files {
        let text = fs::read_to_string(path).expect("readable source");
        for violation in scan(&text) {
            let relative = path.strip_prefix(&root).unwrap_or(path);
            report.push(format!(
                "{}:{} {}",
                relative.display(),
                violation.line,
                violation.rule
            ));
        }
    }

    assert!(
        report.is_empty(),
        "silent discards found:\n{}",
        report.join("\n")
    );
}

#[test]
fn deny_lints_are_set_and_inherited_by_every_crate() {
    let (root, files) = workspace_files().expect("workspace listing");
    let root_manifest = fs::read_to_string(root.join("Cargo.toml")).expect("root manifest");
    assert!(root_manifest.contains("unused_must_use = \"deny\""));
    assert!(root_manifest.contains("let_underscore_must_use = \"deny\""));

    let mut missing = Vec::new();
    for path in files.iter().filter(|p| p.ends_with("Cargo.toml")) {
        let text = fs::read_to_string(path).expect("readable manifest");
        if !text.contains("[package]") {
            continue;
        }
        if text.contains("\n[workspace]") {
            // Standalone package (cargo-fuzz): it sets the same two denies itself.
            if !(text.contains("unused_must_use = \"deny\"")
                && text.contains("let_underscore_must_use = \"deny\""))
            {
                missing.push(path.display().to_string());
            }
            continue;
        }
        let inherits = text
            .split("[lints]")
            .nth(1)
            .and_then(|rest| rest.lines().map(str::trim).find(|l| !l.is_empty()))
            == Some("workspace = true");
        if !inherits {
            missing.push(path.display().to_string());
        }
    }

    assert!(
        missing.is_empty(),
        "crates not inheriting workspace lints: {missing:?}"
    );
}
