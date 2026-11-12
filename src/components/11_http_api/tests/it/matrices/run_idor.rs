//! The one runner, two-account half (AC-59): B's credential with A's id, then with a random id, for every
//! generated (operation, ID parameter) pair; the admin refusal (G-S6); and A's `account_id` in a body.

use std::collections::HashMap;

use serde_json::Value;

use super::document::{Document, Operation};
use super::plans::{Category, IdorCase};
use super::run::{Failure, Headers, Outcome, bearer, code, cookie, own_request};
use super::synth::build;
use super::target::{Seen, Target, comparable};
use super::world::ACCOUNT_A;
use crate::auth::support::MEMBER_ACCOUNT;
use crate::worker::support::Reply;

/// Pairs whose uniform answer is not 404, because the port's outcome for an id the caller does not own is
/// another one: 03 maps `UploadNotFound` to 410 and keys staged uploads per account, a commit or delete names
/// a base revision that is never current (409), a batch fails per entry inside a 200, 06 lists none of the
/// caller's links on another node, and 01's `SessionService::revoke` has no not-found outcome.
pub const UNIFORM_NOT_404: [(&str, &str, u16); 12] = [
    ("UploadService::get_upload", "upload_id", 410),
    ("UploadService::abort_upload", "upload_id", 410),
    ("UploadService::put_block", "upload_id", 410),
    ("UploadService::stage_small", "staged_key", 200),
    ("CommitService::commit_upload", "upload_id", 410),
    ("CommitService::commit_upload", "base_revision", 409),
    ("CommitService::commit_batch", "staged_key", 200),
    ("CommitService::commit_batch", "parent_id", 200),
    ("CommitService::commit_batch", "base_revision", 200),
    ("TreeService::delete_node", "base_revision", 409),
    ("SessionService::revoke", "id", 204),
    ("ShareService::list", "node_id", 200),
];

fn expected_status(case: &IdorCase) -> u16 {
    UNIFORM_NOT_404
        .iter()
        .find(|(owner, param, _)| *owner == case.operation.owner && *param == case.param.name)
        .map_or(404, |(_, _, status)| *status)
}

/// The answer to compare, with the id the request carried replaced: an echo of the caller's own key is no leak.
fn shape(reply: &Reply, id: &str) -> (u16, Value) {
    let text = String::from_utf8_lossy(&reply.body).replace(id, "<id>");
    comparable(&Reply {
        status: reply.status,
        headers: reply.headers.clone(),
        body: text.into_bytes(),
    })
}

/// One request of `case` with `focus` as the parameter's value and B's own ids everywhere else.
async fn call(
    doc: &Document,
    target: &Target,
    case: &IdorCase,
    own: &HashMap<String, String>,
    focus: &str,
    extra: &Headers,
) -> Result<Reply, String> {
    let built = build(doc, &case.operation, Some(&case.param), &mut |param| {
        if *param == case.param {
            focus.to_owned()
        } else {
            own.get(&param.name).cloned().unwrap_or_default()
        }
    })?;
    target.prepare(&case.operation.owner);
    Ok(target.send(&built, extra).await)
}

/// Mode S: every per-account fake call came with B's account, A's account reached no port, and A's id reached a
/// per-account fake (so the 404 is the account scoping, not an earlier refusal).
fn account_scoping(seen: &Seen, a_id: &str) -> Option<String> {
    let a_account = ACCOUNT_A.to_string();
    if let Some(call) = seen
        .gate_calls
        .iter()
        .find(|call| call.account != MEMBER_ACCOUNT)
    {
        return Some(format!(
            "{} received account {}: {call:?}",
            call.key, call.account
        ));
    }
    if let Some(line) = seen.log.iter().find(|line| line.contains(&a_account)) {
        return Some(format!("A's account reached a port: {line}"));
    }
    let reached = seen
        .gate_calls
        .iter()
        .any(|call| !call.owned && call.ids.iter().any(|id| id.to_string() == a_id));
    (!reached).then(|| "A's id reached no per-account fake that refused it".to_owned())
}

fn no_port_call(seen: Option<Seen>) -> Option<String> {
    let seen = seen?;
    (!seen.log.is_empty() || !seen.gate_calls.is_empty())
        .then(|| format!("ports were called: {:?} {:?}", seen.log, seen.gate_calls))
}

async fn run_case(doc: &Document, target: &Target, case: &IdorCase) -> Result<(), String> {
    let mut own = HashMap::new();
    for param in case
        .operation
        .id_params
        .iter()
        .filter(|p| **p != case.param)
    {
        own.insert(param.name.clone(), target.id_b(&case.operation, param)?);
    }
    let a_id = target.id_a(&case.operation, &case.param)?;
    let member = bearer(&target.credentials.member);
    let mark = target.mark();
    match case.category {
        Category::Account | Category::Admin | Category::Worker => {
            let random = target.random_id();
            let with_a = call(doc, target, case, &own, &a_id, &vec![member.clone()]).await?;
            let with_random = call(doc, target, case, &own, &random, &vec![member]).await?;
            let (a, r) = (shape(&with_a, &a_id), shape(&with_random, &random));
            if a != r {
                return Err(format!("A's id answers {a:?}, a random id {r:?}"));
            }
            let expected = match case.category {
                Category::Account => expected_status(case),
                _ => 403,
            };
            if with_a.status != expected {
                return Err(format!(
                    "{} {}, expected {expected}",
                    with_a.status,
                    code(&with_a)
                ));
            }
            let seen = target.seen_since(mark);
            let problem = match (case.category, seen) {
                (Category::Account, Some(seen)) => account_scoping(&seen, &a_id),
                (_, seen) => no_port_call(seen),
            };
            problem.map_or(Ok(()), Err)
        }
        Category::Capability => {
            let with_b = call(doc, target, case, &own, &a_id, &vec![member]).await?;
            let anonymous = call(doc, target, case, &own, &a_id, &Vec::new()).await?;
            let (b, none) = (comparable(&with_b), comparable(&anonymous));
            if b != none {
                return Err(format!("B's credential answers {b:?}, none {none:?}"));
            }
            if with_b.status >= 400 {
                return Err(format!(
                    "the token was not served: {} {}",
                    with_b.status,
                    code(&with_b)
                ));
            }
            let b_account = MEMBER_ACCOUNT.to_string();
            match target.seen_since(mark) {
                Some(seen)
                    if seen.log.iter().any(|line| line.contains(&b_account))
                        || seen.gate_calls.iter().any(|c| c.account == MEMBER_ACCOUNT) =>
                {
                    Err(format!("B's account reached a port: {:?}", seen.log))
                }
                _ => Ok(()),
            }
        }
    }
}

/// AC-4: every generated pair, by category.
pub async fn two_accounts(doc: &Document, target: &Target, cases: &[IdorCase]) -> Outcome {
    let mut outcome = Outcome::default();
    for case in cases {
        outcome.cases += 1;
        if let Err(detail) = run_case(doc, target, case).await {
            outcome.failures.push(Failure {
                operation: case.operation.label(),
                detail: format!("[{} in {}] {detail}", case.param.name, case.param.location),
            });
        }
    }
    outcome
}

/// NFT-SEC-14 step 6: a JSON body of an account-scoped operation with A's `account_id` added is refused as an
/// unknown member (every request schema closes its members) before any port call.
pub async fn body_account(doc: &Document, target: &Target, account_a: &str) -> Outcome {
    let mut outcome = Outcome::default();
    for operation in doc.operations.iter().filter(|o| is_account_scoped(o)) {
        let Ok(built) = own_request(doc, target, operation) else {
            outcome.fail(operation, "no request".to_owned());
            continue;
        };
        let Some(built) = built.with_member("account_id", Value::String(account_a.to_owned()))
        else {
            continue;
        };
        target.prepare(&operation.owner);
        let mark = target.mark();
        let reply = target
            .send(&built, &[bearer(&target.credentials.member)])
            .await;
        outcome.cases += 1;
        if reply.status != 400 || code(&reply) != "invalid_request" {
            outcome.fail(operation, format!("{} {}", reply.status, code(&reply)));
        }
        if let Some(detail) = no_port_call(target.seen_since(mark)) {
            outcome.fail(operation, detail);
        }
    }
    outcome
}

/// AC-5: every `Admin` operation refuses B (no admin) with 403, by bearer and by cookie with a same-origin
/// proof, before any port call.
pub async fn admin_refusals(doc: &Document, target: &Target) -> Outcome {
    let mut outcome = Outcome::default();
    let member = &target.credentials.member;
    for operation in doc.operations.iter().filter(|o| o.auth == "Admin") {
        let built = match own_request(doc, target, operation) {
            Ok(built) => built,
            Err(error) => {
                outcome.fail(operation, error);
                continue;
            }
        };
        let credentials: [(&str, Headers); 2] = [
            ("bearer", vec![bearer(member)]),
            (
                "cookie",
                vec![
                    cookie(target, member),
                    ("origin".to_owned(), target.public_url.clone()),
                ],
            ),
        ];
        for (name, extra) in credentials {
            let mark = target.mark();
            let reply = target.send(&built, &extra).await;
            outcome.cases += 1;
            if reply.status != 403 || code(&reply) != "forbidden" {
                outcome.fail(
                    operation,
                    format!("{name}: {} {}", reply.status, code(&reply)),
                );
            }
            if let Some(detail) = no_port_call(target.seen_since(mark)) {
                outcome.fail(operation, format!("{name}: {detail}"));
            }
        }
    }
    outcome
}

/// The operations of `outcome`'s failures, for comparing with a named list.
pub fn failing_operations(outcome: &Outcome) -> Vec<String> {
    let mut labels: Vec<String> = outcome
        .failures
        .iter()
        .map(|failure| failure.operation.clone())
        .collect();
    labels.sort();
    labels.dedup();
    labels
}

pub fn is_account_scoped(operation: &Operation) -> bool {
    matches!(
        operation.auth.as_str(),
        "Session" | "SessionOrDevice" | "SessionOnly"
    )
}
