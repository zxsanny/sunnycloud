//! AC-5: `Plan` carries no free text. Reads the type definitions in `policy.rs` (the same
//! source walk as `ipc_schema`'s `schema_walk`) and follows every field type reachable from
//! `Plan`: each must be a boolean, a fixed-width integer or an enumeration whose variants
//! carry nothing. A new field of any other type is found without registration.

use std::collections::{BTreeMap, BTreeSet};

const POLICY_SOURCE: &str = include_str!("../src/policy.rs");

/// Leaf types that cannot hold text.
const BOUNDED_LEAVES: [&str; 5] = ["bool", "u8", "u16", "u32", "i32"];

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    Punct(char),
}

fn tokenize(source: &str) -> Vec<Token> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '"' {
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                i += if chars[i] == '\\' { 2 } else { 1 };
            }
            i += 1;
        } else if c.is_alphanumeric() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            tokens.push(Token::Ident(chars[start..i].iter().collect()));
        } else {
            if !c.is_whitespace() {
                tokens.push(Token::Punct(c));
            }
            i += 1;
        }
    }
    tokens
}

fn is_open(token: &Token) -> bool {
    matches!(token, Token::Punct('{' | '(' | '['))
}

fn is_close(token: &Token) -> bool {
    matches!(token, Token::Punct('}' | ')' | ']'))
}

/// Index just past the group that opens at `tokens[start]`.
fn group_end(tokens: &[Token], start: usize) -> usize {
    let mut depth = 0;
    for (offset, token) in tokens[start..].iter().enumerate() {
        if is_open(token) {
            depth += 1;
        } else if is_close(token) {
            depth -= 1;
            if depth == 0 {
                return start + offset + 1;
            }
        }
    }
    panic!("unbalanced group");
}

/// Splits the inside of a group at commas of depth 0 (angle brackets are not counted: a
/// generic type such as `Option<u8>` has no comma in this crate's definitions).
fn split_top_level(tokens: &[Token]) -> Vec<&[Token]> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (index, token) in tokens.iter().enumerate() {
        if is_open(token) {
            depth += 1;
        } else if is_close(token) {
            depth -= 1;
        } else if depth == 0 && *token == Token::Punct(',') {
            parts.push(&tokens[start..index]);
            start = index + 1;
        }
    }
    if start < tokens.len() {
        parts.push(&tokens[start..]);
    }
    parts
}

fn idents(tokens: &[Token]) -> Vec<String> {
    tokens
        .iter()
        .filter_map(|token| match token {
            Token::Ident(name) => Some(name.clone()),
            Token::Punct(_) => None,
        })
        .collect()
}

/// Drops attributes (`# [ ... ]`) and `pub`.
fn strip_decoration(tokens: &[Token]) -> Vec<Token> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i] == Token::Punct('#') && tokens.get(i + 1) == Some(&Token::Punct('[')) {
            i = group_end(tokens, i + 1);
        } else if tokens[i] == Token::Ident("pub".to_owned()) {
            i += 1;
        } else {
            out.push(tokens[i].clone());
            i += 1;
        }
    }
    out
}

/// One field or variant payload: the type identifiers it mentions.
#[derive(Debug, Default)]
struct Definition {
    is_enum: bool,
    /// Per field (struct) or per variant (enum): `(label, identifiers of its type)`.
    members: Vec<(String, Vec<String>)>,
}

fn definitions(source: &str) -> BTreeMap<String, Definition> {
    let tokens = tokenize(source);
    let mut found = BTreeMap::new();
    for (index, token) in tokens.iter().enumerate() {
        let keyword = match token {
            Token::Ident(word) if word == "struct" || word == "enum" => word.as_str(),
            _ => continue,
        };
        let Some(Token::Ident(name)) = tokens.get(index + 1) else {
            continue;
        };
        let Some(open) = (index + 2..tokens.len()).find(|&i| is_open(&tokens[i])) else {
            continue;
        };
        let end = group_end(&tokens, open);
        let body = strip_decoration(&tokens[open + 1..end - 1]);
        let mut definition = Definition {
            is_enum: keyword == "enum",
            members: Vec::new(),
        };
        for part in split_top_level(&body) {
            definition.members.push(member(part, definition.is_enum));
        }
        found.insert(name.clone(), definition);
    }
    found
}

fn member(part: &[Token], is_enum: bool) -> (String, Vec<String>) {
    if is_enum {
        let label = idents(&part[..1]).remove(0);
        let rest = &part[1..];
        if rest.is_empty() {
            return (label, Vec::new());
        }
        // Variant payload: a tuple `( T )` or a record `{ a: T, b: U }`.
        let payload = &rest[1..rest.len() - 1];
        let types = if rest[0] == Token::Punct('{') {
            split_top_level(payload)
                .into_iter()
                .flat_map(field_type)
                .collect()
        } else {
            idents(payload)
        };
        (
            label,
            if types.is_empty() {
                vec!["()".to_owned()]
            } else {
                types
            },
        )
    } else {
        let label = idents(&part[..1]).remove(0);
        (label, field_type(part))
    }
}

fn field_type(field: &[Token]) -> Vec<String> {
    let colon = field
        .iter()
        .position(|token| *token == Token::Punct(':'))
        .expect("field has a type");
    idents(&field[colon + 1..])
}

/// Every reason the walk from `root` finds that a type can hold free text.
fn violations(source: &str, root: &str) -> Vec<String> {
    let defs = definitions(source);
    let mut found = Vec::new();
    let mut visited = BTreeSet::new();
    let mut queue = vec![root.to_owned()];
    while let Some(name) = queue.pop() {
        if !visited.insert(name.clone()) {
            continue;
        }
        let Some(definition) = defs.get(&name) else {
            found.push(format!("type {name} has no definition"));
            continue;
        };
        for (label, types) in &definition.members {
            for ty in types {
                if BOUNDED_LEAVES.contains(&ty.as_str()) {
                    continue;
                }
                if let Some(target) = defs.get(ty) {
                    let payload_free = target.members.iter().all(|(_, t)| t.is_empty());
                    if target.is_enum && !payload_free && name != root {
                        found.push(format!("enum {ty} carries a payload ({name}.{label})"));
                    }
                    queue.push(ty.clone());
                } else {
                    found.push(format!("{name}.{label} has type {ty}"));
                }
            }
        }
    }
    found
}

#[test]
fn plan_fields_are_enumerations_booleans_or_bounded_numbers() {
    let found = violations(POLICY_SOURCE, "Plan");

    assert!(found.is_empty(), "free-text capable fields: {found:?}");
}

#[test]
fn the_walk_reaches_every_plan_field_type() {
    let defs = definitions(POLICY_SOURCE);
    let plan = defs.get("Plan").expect("Plan is defined");

    let labels: Vec<&str> = plan
        .members
        .iter()
        .map(|(label, _)| label.as_str())
        .collect();
    let transcode_types: Vec<&String> = plan
        .members
        .iter()
        .filter(|(label, _)| label == "Transcode")
        .flat_map(|(_, types)| types)
        .collect();

    assert!(plan.is_enum);
    assert_eq!(labels, ["Remux", "Transcode"]);
    for expected in ["Encoder", "u16", "bool", "AudioPlan"] {
        assert!(
            transcode_types.iter().any(|ty| ty.as_str() == expected),
            "{expected} not among {transcode_types:?}"
        );
    }
}

#[test]
fn a_string_field_added_to_a_plan_copy_is_found() {
    let mutated = POLICY_SOURCE.replacen(
        "faststart: bool,",
        "faststart: bool,\n        extra_args: String,",
        1,
    );

    let found = violations(&mutated, "Plan");

    assert_eq!(found, vec!["Plan.Transcode has type String".to_owned()]);
}

#[test]
fn a_string_list_or_option_added_to_a_plan_copy_is_found() {
    let mutated = POLICY_SOURCE.replacen(
        "faststart: bool,",
        "faststart: bool,\n        filters: Vec<String>,\n        label: Option<String>,",
        1,
    );

    let found = violations(&mutated, "Plan");

    assert!(
        found.contains(&"Plan.Transcode has type Vec".to_owned()),
        "{found:?}"
    );
    assert!(
        found.contains(&"Plan.Transcode has type String".to_owned()),
        "{found:?}"
    );
    assert!(
        found.contains(&"Plan.Transcode has type Option".to_owned()),
        "{found:?}"
    );
}

#[test]
fn an_enum_variant_with_a_string_payload_is_found() {
    let mutated = POLICY_SOURCE.replacen(
        "    Aac,\n    None,",
        "    Aac,\n    None,\n    Custom(String),",
        1,
    );

    let found = violations(&mutated, "Plan");

    assert!(
        found
            .iter()
            .any(|v| v.contains("AudioPlan") && v.contains("Custom")),
        "{found:?}"
    );
}
