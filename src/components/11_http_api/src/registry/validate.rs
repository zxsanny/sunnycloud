use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use super::classes::{AuthClass, BodyClass, HttpMethod, ListenerKind, Timing};
use super::naming::{lint_path, path_variables};
use super::spec::{ParamLocation, RouteDef, RouteSpec};

const VAULT_PREFIX: &str = "/api/v1/vaults";
const INTERNAL_PREFIXES: [&str; 2] = ["/api/v1/worker-jobs/", "/internal/v1/test/"];
/// A GET that changes state by protocol, named because the "GET never mutates" check exempts it.
const GET_MUTATION_EXEMPT: [&str; 1] = ["SignInService::complete"];

/// Every way a registry can be wrong, one line each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryError(pub Vec<String>);

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "route registry is invalid: {}", self.0.join("; "))
    }
}

impl std::error::Error for RegistryError {}

/// Checks that every operation is fully and consistently declared: owner, summary, parameters for every
/// path variable, auth class matching the listener, body class matching the method, names, and one
/// definition per (method, path).
pub fn validate(defs: &[RouteDef]) -> Result<(), RegistryError> {
    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();
    for def in defs {
        let spec = &def.spec;
        let label = format!("{} {}", spec.method.key(), spec.path);
        if !seen.insert((spec.path, spec.method)) {
            problems.push(format!("{label}: declared twice"));
        }
        check_declaration(spec, &label, &mut problems);
        check_placement(spec, &label, &mut problems);
        check_params(spec, &label, &mut problems);
        check_timing(spec, &label, &mut problems);
    }
    check_schemas(defs, &mut problems);
    if problems.is_empty() {
        Ok(())
    } else {
        Err(RegistryError(problems))
    }
}

fn check_declaration(spec: &RouteSpec, label: &str, problems: &mut Vec<String>) {
    if spec.owner.is_empty() || spec.owner.contains(char::is_whitespace) {
        problems.push(format!("{label}: owner port method is missing"));
    }
    if spec.summary.is_empty() {
        problems.push(format!("{label}: summary is missing"));
    }
    if !matches!(spec.success, 200 | 201 | 202 | 204 | 302) {
        problems.push(format!(
            "{label}: success status {} is not declared",
            spec.success
        ));
    }
    if matches!(spec.success, 204 | 302) && spec.response.is_some() {
        problems.push(format!(
            "{label}: a {} answer has no body schema",
            spec.success
        ));
    }
    let reads = matches!(spec.method, HttpMethod::Get | HttpMethod::Head);
    if reads && spec.body != BodyClass::None {
        problems.push(format!("{label}: a {} has no body", spec.method.key()));
    }
    if !reads && spec.body == BodyClass::None && spec.method != HttpMethod::Delete {
        problems.push(format!(
            "{label}: body class None needs GET, HEAD or DELETE"
        ));
    }
    if spec.body == BodyClass::None && spec.request.is_some() {
        problems.push(format!("{label}: body class None has a request schema"));
    }
    if reads && spec.state_changing && !GET_MUTATION_EXEMPT.contains(&spec.owner) {
        problems.push(format!(
            "{label}: a {} is declared state-changing",
            spec.method.key()
        ));
    }
    if spec.method == HttpMethod::Head && spec.state_changing {
        problems.push(format!("{label}: HEAD never changes state"));
    }
}

fn check_placement(spec: &RouteSpec, label: &str, problems: &mut Vec<String>) {
    for violation in lint_path(spec.path) {
        problems.push(format!("{label}: {violation}"));
    }
    match spec.listener {
        ListenerKind::Public => {
            if spec.auth == AuthClass::Worker {
                problems.push(format!(
                    "{label}: a Worker route is served by the internal listener only"
                ));
            }
            if INTERNAL_PREFIXES
                .iter()
                .any(|prefix| spec.path.starts_with(prefix))
            {
                problems.push(format!(
                    "{label}: this path belongs to the internal listener"
                ));
            }
        }
        ListenerKind::Internal => {
            if spec.auth != AuthClass::Worker {
                problems.push(format!(
                    "{label}: the internal listener serves Worker routes only"
                ));
            }
            if !INTERNAL_PREFIXES
                .iter()
                .any(|prefix| spec.path.starts_with(prefix))
            {
                problems.push(format!("{label}: not a worker or test-clock path"));
            }
        }
    }
    if spec.path.starts_with(VAULT_PREFIX) && !spec.owner.starts_with("VaultService::") {
        problems.push(format!(
            "{label}: /api/v1/vaults is reserved for VaultService"
        ));
    }
}

fn check_timing(spec: &RouteSpec, label: &str, problems: &mut Vec<String>) {
    let reads = matches!(spec.method, HttpMethod::Get | HttpMethod::Head);
    match spec.timing {
        Timing::Request => {}
        Timing::LongPoll => {
            let has_wait = spec
                .params
                .iter()
                .any(|param| param.location == ParamLocation::Query && param.name == "wait");
            if !reads || !has_wait {
                problems.push(format!(
                    "{label}: a long-poll is a GET with a `wait` query parameter"
                ));
            }
        }
        Timing::Download => {
            if !reads {
                problems.push(format!("{label}: a download is a GET or HEAD"));
            }
        }
    }
}

fn looks_like_id(name: &str) -> bool {
    name == "id" || name.ends_with("_id") || name == "token" || name.ends_with("_key")
}

fn check_params(spec: &RouteSpec, label: &str, problems: &mut Vec<String>) {
    let mut declared = BTreeSet::new();
    for param in spec.params {
        if !declared.insert((param.location.key(), param.name)) {
            problems.push(format!("{label}: parameter {} declared twice", param.name));
        }
        if param.location != ParamLocation::Body && !param.id && looks_like_id(param.name) {
            problems.push(format!(
                "{label}: parameter {} looks like an id but is not in x-id-params",
                param.name
            ));
        }
        if param.location == ParamLocation::Path
            && !spec.path.contains(&format!("{{{}}}", param.name))
        {
            problems.push(format!(
                "{label}: path parameter {} is not in the path",
                param.name
            ));
        }
    }
    for variable in path_variables(spec.path) {
        if !declared.contains(&("path", variable)) {
            problems.push(format!("{label}: path variable {variable} is not declared"));
        }
    }
}

fn check_schemas(defs: &[RouteDef], problems: &mut Vec<String>) {
    let mut named = BTreeMap::new();
    for spec in defs.iter().map(|def| &def.spec) {
        for schema in [spec.request, spec.response].into_iter().flatten() {
            let built = (schema.build)();
            if let Some(previous) = named.insert(schema.name, built.clone())
                && previous != built
            {
                problems.push(format!(
                    "schema {} is defined twice differently",
                    schema.name
                ));
            }
        }
    }
}
