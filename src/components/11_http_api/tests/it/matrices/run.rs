//! The one runner, CSRF half: it sends the generated cases to a [`Target`] (Mode S or a real stack) and returns
//! every case that did not answer as `api_v1_surface.md` requires.

use super::document::{Document, Operation};
use super::plans::CsrfPlan;
use super::synth::{Built, build};
use super::target::{Seen, Target};
use crate::worker::support::Reply;

pub const FOREIGN_ORIGIN: &str = "https://evil.example";

/// A case that did not hold: the operation and what was wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub operation: String,
    pub detail: String,
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub cases: usize,
    pub failures: Vec<Failure>,
}

impl Outcome {
    /// Prints the case count and every failure, for the CI log.
    pub fn report(&self, what: &str) {
        println!(
            "{what}: {} cases, {} failed",
            self.cases,
            self.failures.len()
        );
        for failure in &self.failures {
            println!("  {}: {}", failure.operation, failure.detail);
        }
    }

    pub fn fail(&mut self, operation: &Operation, detail: String) {
        self.failures.push(Failure {
            operation: operation.label(),
            detail,
        });
    }
}

pub type Headers = Vec<(String, String)>;

fn pairs(list: &[(&str, &str)]) -> Headers {
    list.iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

/// The header sets of IT-11-6 that prove no same origin: a foreign origin, a sibling subdomain of the public
/// host, `null`, `Sec-Fetch-Site` cross-site, same-site and none, and neither header.
pub fn refused_sets(public_url: &str) -> Vec<(&'static str, Headers)> {
    let host = public_url
        .split_once("://")
        .map_or(public_url, |(_, rest)| rest)
        .trim_end_matches('/');
    let sibling = format!("https://app.{host}");
    vec![
        ("Origin foreign", pairs(&[("origin", FOREIGN_ORIGIN)])),
        ("Origin sibling subdomain", pairs(&[("origin", &sibling)])),
        ("Origin null", pairs(&[("origin", "null")])),
        (
            "Sec-Fetch-Site cross-site",
            pairs(&[("sec-fetch-site", "cross-site")]),
        ),
        (
            "Sec-Fetch-Site same-site",
            pairs(&[("sec-fetch-site", "same-site")]),
        ),
        ("Sec-Fetch-Site none", pairs(&[("sec-fetch-site", "none")])),
        ("neither header", Vec::new()),
    ]
}

/// The session token the operation's class accepts: the admin's for `Admin`, B's otherwise.
pub fn session_of<'t>(target: &'t Target, operation: &Operation) -> &'t str {
    if operation.auth == "Admin" {
        &target.credentials.admin
    } else {
        &target.credentials.member
    }
}

pub fn cookie(target: &Target, token: &str) -> (String, String) {
    (
        "cookie".to_owned(),
        format!("{}={token}", target.credentials.cookie),
    )
}

pub fn bearer(token: &str) -> (String, String) {
    ("authorization".to_owned(), format!("Bearer {token}"))
}

/// The request of `operation` with B's own ids in every ID parameter it needs.
pub fn own_request(
    doc: &Document,
    target: &Target,
    operation: &Operation,
) -> Result<Built, String> {
    let mut failure = None;
    let built = build(doc, operation, None, &mut |param| {
        target.id_b(operation, param).unwrap_or_else(|error| {
            failure = Some(error);
            String::new()
        })
    })?;
    failure.map_or(Ok(built), Err)
}

pub fn code(reply: &Reply) -> String {
    serde_json::from_slice::<serde_json::Value>(&reply.body)
        .ok()
        .and_then(|body| body["code"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Mode S: what reached a port while the cases ran, as a failure detail; `None` when nothing did.
fn port_calls(seen: Option<Seen>) -> Option<String> {
    let seen = seen?;
    if seen.log.is_empty() && seen.gate_calls.is_empty() && seen.state_changes == 0 {
        return None;
    }
    Some(format!(
        "ports were called ({} state changes): {:?} {:?}",
        seen.state_changes, seen.log, seen.gate_calls
    ))
}

/// AC-1: every checked operation with a valid session cookie and each refused header set is 403
/// `csrf_rejected`, and no port is called.
pub async fn csrf_refusals(doc: &Document, target: &Target, plan: &CsrfPlan) -> Outcome {
    let mut outcome = Outcome::default();
    let mark = target.mark();
    for operation in &plan.checked {
        let built = match own_request(doc, target, operation) {
            Ok(built) => built,
            Err(error) => {
                outcome.fail(operation, error);
                continue;
            }
        };
        for (name, headers) in refused_sets(&target.public_url) {
            let mut extra = vec![cookie(target, session_of(target, operation))];
            extra.extend(headers);
            let reply = target.send(&built, &extra).await;
            outcome.cases += 1;
            if reply.status != 403 || code(&reply) != "csrf_rejected" {
                outcome.fail(
                    operation,
                    format!("{name}: {} {}", reply.status, code(&reply)),
                );
            }
        }
    }
    if let (Some(detail), Some(first)) = (port_calls(target.seen_since(mark)), plan.checked.first())
    {
        outcome.fail(first, detail);
    }
    outcome
}

/// AC-2: `Origin` equal to the public URL, `Sec-Fetch-Site: same-origin`, and a bearer with a foreign origin
/// each pass the layer, and the scripted port answers 2xx.
pub async fn csrf_controls(doc: &Document, target: &Target, plan: &CsrfPlan) -> Outcome {
    let mut outcome = Outcome::default();
    for operation in &plan.checked {
        let token = session_of(target, operation).to_owned();
        let controls: [(&str, Headers); 3] = [
            (
                "Origin public URL",
                vec![
                    cookie(target, &token),
                    ("origin".to_owned(), target.public_url.clone()),
                ],
            ),
            (
                "Sec-Fetch-Site same-origin",
                vec![
                    cookie(target, &token),
                    ("sec-fetch-site".to_owned(), "same-origin".to_owned()),
                ],
            ),
            (
                "bearer, foreign origin",
                vec![
                    bearer(&token),
                    ("origin".to_owned(), FOREIGN_ORIGIN.to_owned()),
                ],
            ),
        ];
        for (name, extra) in controls {
            let built = match own_request(doc, target, operation) {
                Ok(built) => built,
                Err(error) => {
                    outcome.fail(operation, error);
                    break;
                }
            };
            target.prepare(&operation.owner);
            let reply = target.send(&built, &extra).await;
            outcome.cases += 1;
            if !(200..300).contains(&reply.status) {
                outcome.fail(
                    operation,
                    format!("{name}: {} {}", reply.status, code(&reply)),
                );
            }
        }
    }
    outcome
}

/// AC-3: every GET and HEAD with B's cookie and cross-site headers leaves the state counter unchanged. A public
/// listener read must also succeed, or the check would prove nothing about what its handler does.
pub async fn reads(doc: &Document, target: &Target, plan: &CsrfPlan) -> Outcome {
    let mut outcome = Outcome::default();
    for operation in &plan.reads {
        let built = match own_request(doc, target, operation) {
            Ok(built) => built,
            Err(error) => {
                outcome.fail(operation, error);
                continue;
            }
        };
        target.prepare(&operation.owner);
        let extra = vec![
            cookie(target, session_of(target, operation)),
            ("sec-fetch-site".to_owned(), "cross-site".to_owned()),
            ("origin".to_owned(), FOREIGN_ORIGIN.to_owned()),
        ];
        let mark = target.mark();
        let reply = target.send(&built, &extra).await;
        outcome.cases += 1;
        if let Some(seen) = target.seen_since(mark)
            && seen.state_changes != 0
        {
            outcome.fail(
                operation,
                format!("{} state changes: {:?}", seen.state_changes, seen.log),
            );
        }
        if !operation.is_internal() && reply.status >= 400 {
            outcome.fail(
                operation,
                format!("the read did not run: {} {}", reply.status, code(&reply)),
            );
        }
    }
    outcome
}
