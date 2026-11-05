//! Reads the crate's own type definitions and walks every type reachable from given roots.

use std::collections::{BTreeMap, BTreeSet};

use ipc_schema::METHODS;

pub const TYPE_SOURCES: [&str; 4] = [
    include_str!("../../src/ids.rs"),
    include_str!("../../src/types.rs"),
    include_str!("../../src/params.rs"),
    include_str!("../../src/error.rs"),
];

/// Types the walk understands without a definition.
pub const PRIMITIVES: [&str; 8] = [
    "bool", "String", "u32", "u64", "i64", "f64", "Option", "Vec",
];

/// Words that must not appear in a type, field or variant name.
pub const FORBIDDEN_WORDS: [&str; 4] = ["token", "secret", "credential", "password"];

/// `PageToken` is the opaque paging cursor of the contract, not a credential.
pub const ALLOWED_TYPE_NAMES: [&str; 1] = ["PageToken"];

/// Roots in addition to the method table: the notification payload and the error type.
pub const EXTRA_ROOTS: [&str; 2] = ["StatusEvent", "ControlError"];

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    Punct(char),
}

pub fn tokenize(source: &str) -> Vec<Token> {
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

pub fn is_open(token: &Token) -> bool {
    matches!(token, Token::Punct('{' | '(' | '[' | '<'))
}

pub fn is_close(token: &Token) -> bool {
    matches!(token, Token::Punct('}' | ')' | ']' | '>'))
}

/// Index just past the group that opens at `tokens[start]`.
pub fn group_end(tokens: &[Token], start: usize) -> usize {
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
    panic!("unbalanced group in type source");
}

/// Removes every `#[...]` attribute.
pub fn strip_attributes(tokens: Vec<Token>) -> Vec<Token> {
    let mut kept = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i] == Token::Punct('#') && tokens.get(i + 1) == Some(&Token::Punct('[')) {
            i = group_end(&tokens, i + 1);
        } else {
            kept.push(tokens[i].clone());
            i += 1;
        }
    }
    kept
}

pub fn split_top_level(tokens: &[Token]) -> Vec<&[Token]> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (i, token) in tokens.iter().enumerate() {
        if is_open(token) {
            depth += 1;
        } else if is_close(token) {
            depth -= 1;
        } else if *token == Token::Punct(',') && depth == 0 {
            parts.push(&tokens[start..i]);
            start = i + 1;
        }
    }
    if start < tokens.len() {
        parts.push(&tokens[start..]);
    }
    parts
}

pub fn idents(tokens: &[Token]) -> Vec<String> {
    tokens
        .iter()
        .filter_map(|t| match t {
            Token::Ident(name) => Some(name.clone()),
            Token::Punct(_) => None,
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub type_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Variant {
    pub name: String,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone)]
pub struct Definition {
    pub is_enum: bool,
    pub fields: Vec<Field>,
    pub variants: Vec<Variant>,
}

pub fn skip_visibility(mut tokens: &[Token]) -> &[Token] {
    if tokens.first() == Some(&Token::Ident("pub".to_owned())) {
        tokens = &tokens[1..];
        if tokens.first() == Some(&Token::Punct('(')) {
            tokens = &tokens[group_end(tokens, 0)..];
        }
    }
    tokens
}

pub fn parse_named_fields(body: &[Token]) -> Vec<Field> {
    split_top_level(body)
        .into_iter()
        .map(|part| {
            let part = skip_visibility(part);
            assert_eq!(
                part.get(1),
                Some(&Token::Punct(':')),
                "field expected in {part:?}"
            );
            Field {
                name: idents(&part[..1])[0].clone(),
                type_names: idents(&part[2..]),
            }
        })
        .collect()
}

pub fn parse_tuple_fields(body: &[Token]) -> Vec<Field> {
    split_top_level(body)
        .into_iter()
        .enumerate()
        .map(|(index, part)| Field {
            name: index.to_string(),
            type_names: idents(skip_visibility(part)),
        })
        .collect()
}

pub fn parse_variants(body: &[Token]) -> Vec<Variant> {
    split_top_level(body)
        .into_iter()
        .map(|part| {
            let name = idents(&part[..1])[0].clone();
            let fields = match part.get(1) {
                Some(Token::Punct('{')) => parse_named_fields(&part[2..part.len() - 1]),
                Some(Token::Punct('(')) => parse_tuple_fields(&part[2..part.len() - 1]),
                _ => Vec::new(),
            };
            Variant { name, fields }
        })
        .collect()
}

pub fn parse_definitions(sources: &[&str]) -> BTreeMap<String, Definition> {
    let mut definitions = BTreeMap::new();
    for source in sources {
        let tokens = strip_attributes(tokenize(source));
        for (i, token) in tokens.iter().enumerate() {
            let keyword = match token {
                Token::Ident(word) if word == "struct" || word == "enum" => word.as_str(),
                _ => continue,
            };
            let Some(Token::Ident(name)) = tokens.get(i + 1) else {
                continue;
            };
            let rest = &tokens[i + 2..];
            assert_ne!(
                rest.first(),
                Some(&Token::Punct('<')),
                "generic type {name} is not supported by the walk"
            );
            let definition = match (keyword, rest.first()) {
                ("enum", Some(Token::Punct('{'))) => Definition {
                    is_enum: true,
                    fields: Vec::new(),
                    variants: parse_variants(&rest[1..group_end(rest, 0) - 1]),
                },
                ("struct", Some(Token::Punct('{'))) => Definition {
                    is_enum: false,
                    fields: parse_named_fields(&rest[1..group_end(rest, 0) - 1]),
                    variants: Vec::new(),
                },
                ("struct", Some(Token::Punct('('))) => Definition {
                    is_enum: false,
                    fields: parse_tuple_fields(&rest[1..group_end(rest, 0) - 1]),
                    variants: Vec::new(),
                },
                ("struct", _) => Definition {
                    is_enum: false,
                    fields: Vec::new(),
                    variants: Vec::new(),
                },
                _ => continue,
            };
            definitions.insert(name.clone(), definition);
        }
    }
    definitions
}

pub fn words(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    for c in name.chars() {
        if c == '_' || (c.is_uppercase() && !current.is_empty()) {
            if !current.is_empty() {
                words.push(current.to_lowercase());
            }
            current = String::new();
        }
        if c != '_' {
            current.push(c);
        }
    }
    if !current.is_empty() {
        words.push(current.to_lowercase());
    }
    words
}

pub fn forbidden(name: &str) -> bool {
    words(name)
        .iter()
        .any(|w| FORBIDDEN_WORDS.contains(&w.as_str()))
}

#[derive(Debug, Default)]
pub struct Walk {
    pub visited: BTreeSet<String>,
    pub violations: Vec<String>,
}

pub fn walk(sources: &[&str], root_types: &[String]) -> Walk {
    let definitions = parse_definitions(sources);
    let mut result = Walk::default();
    let mut pending: Vec<(String, String)> = root_types
        .iter()
        .flat_map(|root| idents(&tokenize(root)))
        .map(|name| (name, "a method table or root entry".to_owned()))
        .collect();
    while let Some((name, via)) = pending.pop() {
        if PRIMITIVES.contains(&name.as_str()) || !result.visited.insert(name.clone()) {
            continue;
        }
        if forbidden(&name) && !ALLOWED_TYPE_NAMES.contains(&name.as_str()) {
            result
                .violations
                .push(format!("type {name} (reached from {via})"));
        }
        let Some(definition) = definitions.get(&name) else {
            result
                .violations
                .push(format!("unresolved type {name} (reached from {via})"));
            continue;
        };
        let mut owned_fields: Vec<(String, &Field)> = definition
            .fields
            .iter()
            .map(|f| (name.clone(), f))
            .collect();
        for variant in &definition.variants {
            if forbidden(&variant.name) {
                result
                    .violations
                    .push(format!("variant {name}::{}", variant.name));
            }
            owned_fields.extend(
                variant
                    .fields
                    .iter()
                    .map(|f| (format!("{name}::{}", variant.name), f)),
            );
        }
        for (owner, field) in owned_fields {
            if forbidden(&field.name) {
                result
                    .violations
                    .push(format!("field {owner}.{}", field.name));
            }
            for type_name in &field.type_names {
                pending.push((type_name.clone(), format!("{owner}.{}", field.name)));
            }
        }
    }
    result.visited.retain(|name| definitions.contains_key(name));
    result
}

pub fn real_roots() -> Vec<String> {
    let mut roots: Vec<String> = METHODS
        .iter()
        .flat_map(|m| [m.params.to_owned(), m.result.to_owned()])
        .collect();
    roots.extend(EXTRA_ROOTS.map(str::to_owned));
    roots
}
