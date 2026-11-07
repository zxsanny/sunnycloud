use std::fmt;

const API_PREFIX: &str = "/api/v1/";
const TEST_CLOCK_PREFIX: &str = "/internal/v1/test/";
/// The document URL is the one path whose last segment carries a file extension.
const DOCUMENT_PATH: &str = "/api/v1/openapi.json";
const COLON_FORMS: [&str; 2] = ["commits:batch", "revisions:lookup"];
const VERBS: [&str; 12] = [
    "get", "create", "delete", "list", "update", "set", "add", "remove", "fetch", "do", "run",
    "make",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamingViolation {
    OutsideApi,
    TrailingSlash,
    EmptySegment,
    NotKebabCase(String),
    VerbForm(String),
    ColonForm(String),
    SingularCollection(String),
    BadParameter(String),
}

impl fmt::Display for NamingViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NamingViolation::OutsideApi => write!(f, "path is outside {API_PREFIX}"),
            NamingViolation::TrailingSlash => f.write_str("path has a trailing slash"),
            NamingViolation::EmptySegment => f.write_str("path has an empty segment"),
            NamingViolation::NotKebabCase(segment) => {
                write!(f, "segment `{segment}` is not lowercase kebab-case")
            }
            NamingViolation::VerbForm(segment) => write!(f, "segment `{segment}` is a verb form"),
            NamingViolation::ColonForm(segment) => {
                write!(f, "segment `{segment}` uses the colon form")
            }
            NamingViolation::SingularCollection(segment) => {
                write!(f, "collection `{segment}` is not plural")
            }
            NamingViolation::BadParameter(segment) => {
                write!(f, "parameter `{segment}` is not a snake_case name")
            }
        }
    }
}

/// Checks one path template against the naming rules of `api_v1_surface.md`: under `/api/v1` (the test
/// clock under `/internal/v1/test` excepted), plural lowercase kebab nouns, no trailing slash, and the
/// colon form only for `commits:batch` and `revisions:lookup`.
pub fn lint_path(path: &str) -> Vec<NamingViolation> {
    let mut found = Vec::new();
    if path == DOCUMENT_PATH {
        return found;
    }
    let rest = if let Some(rest) = path.strip_prefix(API_PREFIX) {
        rest
    } else if let Some(rest) = path.strip_prefix(TEST_CLOCK_PREFIX) {
        rest
    } else {
        found.push(NamingViolation::OutsideApi);
        return found;
    };
    if rest.ends_with('/') {
        found.push(NamingViolation::TrailingSlash);
    }
    let segments: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
    for (index, segment) in segments.iter().enumerate() {
        if segment.is_empty() {
            found.push(NamingViolation::EmptySegment);
        } else if let Some(name) = segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            if !is_snake_name(name) {
                found.push(NamingViolation::BadParameter((*segment).to_owned()));
            }
        } else {
            lint_literal(segment, &mut found);
            let before_parameter = segments
                .get(index + 1)
                .is_some_and(|next| next.starts_with('{'));
            if before_parameter && !segment.ends_with('s') {
                found.push(NamingViolation::SingularCollection((*segment).to_owned()));
            }
        }
    }
    found
}

fn lint_literal(segment: &str, found: &mut Vec<NamingViolation>) {
    if segment.contains(':') {
        if !COLON_FORMS.contains(&segment) {
            found.push(NamingViolation::ColonForm(segment.to_owned()));
        }
        return;
    }
    if !is_kebab_name(segment) {
        found.push(NamingViolation::NotKebabCase(segment.to_owned()));
        return;
    }
    let first_word = segment.split('-').next().unwrap_or(segment);
    if VERBS.contains(&first_word) {
        found.push(NamingViolation::VerbForm(segment.to_owned()));
    }
}

fn is_kebab_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase())
        && !name.ends_with('-')
        && !name.contains("--")
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_snake_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// The variable names of a path template, in order.
pub(crate) fn path_variables(path: &str) -> Vec<&str> {
    path.split('/')
        .filter_map(|segment| segment.strip_prefix('{')?.strip_suffix('}'))
        .collect()
}
