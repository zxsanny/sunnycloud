//! The two generators. Each reads only the generated document and puts every operation (CSRF) or every
//! (operation, ID parameter) pair (IDOR) into exactly one kind of case; anything it cannot give a case is a
//! problem naming the operation and, for IDOR, the parameter. A non-empty problem list fails CI.

use super::document::{Document, IdParam, Operation};
use super::synth::check_placement;

/// GETs that change state by protocol. `api_v1_surface.md` Open point 3: the provider callback consumes the
/// state row and creates a session, and its registry row names `SignInService::complete` as the one GET allowed
/// to be declared state-changing; the generated matrix must exempt the same name.
pub const MUTATING_GET_EXEMPTIONS: [&str; 1] = ["SignInService::complete"];

/// The CSRF cases of one document.
pub struct CsrfPlan {
    /// Unsafe methods a session cookie can authenticate: the CSRF layer checks each.
    pub checked: Vec<Operation>,
    /// GET and HEAD operations, each checked for leaving the state counter alone.
    pub reads: Vec<Operation>,
    /// GETs exempted by name from the read check.
    pub exempt: Vec<Operation>,
    /// State-changing operations no cookie authenticates (`Public`, `DeviceOnly`, `Worker`): layer 9 has no caller
    /// to check, so a cross-origin page cannot ride a session on them.
    pub cookieless: Vec<Operation>,
}

pub fn csrf_plan(
    doc: &Document,
    case_data: &dyn Fn(&str) -> bool,
) -> Result<CsrfPlan, Vec<String>> {
    let mut plan = CsrfPlan {
        checked: Vec::new(),
        reads: Vec::new(),
        exempt: Vec::new(),
        cookieless: Vec::new(),
    };
    let mut problems = Vec::new();
    for operation in &doc.operations {
        let label = operation.label();
        if operation.is_read() {
            if operation.state_changing {
                if MUTATING_GET_EXEMPTIONS.contains(&operation.owner.as_str()) {
                    plan.exempt.push(operation.clone());
                } else {
                    problems.push(format!(
                        "{label} ({}) is a state-changing read with no named exemption",
                        operation.owner
                    ));
                }
                continue;
            }
            if !operation.is_internal() && !case_data(&operation.owner) {
                problems.push(format!(
                    "{label} has no case: no case data for {}",
                    operation.owner
                ));
                continue;
            }
            plan.reads.push(operation.clone());
        } else if operation.unsafe_method() && operation.accepts_cookie() {
            if case_data(&operation.owner) {
                plan.checked.push(operation.clone());
            } else {
                problems.push(format!(
                    "{label} has no case: no case data for {}, so its passing controls cannot run",
                    operation.owner
                ));
            }
        } else if operation.unsafe_method() || operation.state_changing {
            plan.cookieless.push(operation.clone());
        } else {
            problems.push(format!(
                "{label} has a method the CSRF matrix does not know"
            ));
        }
    }
    for owner in MUTATING_GET_EXEMPTIONS {
        if !plan.exempt.iter().any(|operation| operation.owner == owner) {
            problems.push(format!("the exemption {owner} names no state-changing GET"));
        }
    }
    if problems.is_empty() {
        Ok(plan)
    } else {
        Err(problems)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// B's session with A's id: the account scopes the call, so the answer is the one for a missing id.
    Account,
    /// B is no admin (G-S6): 403 before any port call, whatever the id.
    Admin,
    /// Internal listener: a session is no worker, 403 before any port call, whatever the id.
    Worker,
    /// A share token is a capability, not an account's id: B's credential must change nothing.
    Capability,
}

#[derive(Debug, Clone)]
pub struct IdorCase {
    pub operation: Operation,
    pub param: IdParam,
    pub category: Category,
}

fn category(auth: &str) -> Option<Category> {
    match auth {
        "Session" | "SessionOrDevice" | "SessionOnly" => Some(Category::Account),
        "Admin" => Some(Category::Admin),
        "Worker" => Some(Category::Worker),
        "Public" | "ShareGrant" => Some(Category::Capability),
        _ => None,
    }
}

pub fn idor_plan(
    doc: &Document,
    per_account: &dyn Fn(&str) -> bool,
) -> Result<Vec<IdorCase>, Vec<String>> {
    let mut cases = Vec::new();
    let mut problems = Vec::new();
    for operation in &doc.operations {
        problems.extend(check_placement(doc, operation));
        for param in &operation.id_params {
            let missing = |why: String| {
                format!(
                    "{}: ID parameter {} ({}) has no case: {why}",
                    operation.label(),
                    param.name,
                    param.location
                )
            };
            match category(&operation.auth) {
                None => problems.push(missing(format!(
                    "no two-account case for auth class {}",
                    operation.auth
                ))),
                Some(Category::Account) if !per_account(&operation.owner) => problems.push(
                    missing(format!("no per-account fake for {}", operation.owner)),
                ),
                Some(category) => cases.push(IdorCase {
                    operation: operation.clone(),
                    param: param.clone(),
                    category,
                }),
            }
        }
    }
    if problems.is_empty() {
        Ok(cases)
    } else {
        Err(problems)
    }
}
