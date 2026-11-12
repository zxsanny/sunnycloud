//! IT-11-6 over the generated matrix (AC-58): N of N refusals, the passing controls, and reads that never change
//! state.

use super::document::Document;
use super::plans::{CsrfPlan, csrf_plan};
use super::run::{csrf_controls, csrf_refusals, reads, refused_sets};
use super::run_idor::failing_operations;
use super::target::Target;

/// Operations whose request the document cannot describe validly, so a case that needs the handler to run cannot:
/// the parameter is declared `{"type": "string"}` with no pattern, and the server refuses a value of another
/// shape with 400 `invalid_request` or 422 `invalid_cursor`. Reported as gaps of the document, not worked around.
/// `X-Blake3` must be 64 hex digits.
pub const SCHEMA_GAPS: [&str; 1] = ["PUT /api/v1/uploads/{upload_id}/blocks/{index}"];

/// As [`SCHEMA_GAPS`], for reads: `cursor` must be `epoch.seq` and `user_code` a device user code.
pub const READ_SCHEMA_GAPS: [&str; 2] = ["GET /api/v1/changes", "GET /api/v1/device-grants"];

fn plan(target: &Target) -> (Document, CsrfPlan) {
    let doc = Document::production();
    let plan = csrf_plan(&doc, &|owner| target.has_case_data(owner))
        .unwrap_or_else(|problems| panic!("the CSRF generator found gaps: {problems:#?}"));
    (doc, plan)
}

#[tokio::test]
async fn every_cookie_mutation_without_a_same_origin_proof_is_403_csrf_rejected() {
    // Arrange
    let target = Target::stub().await;
    let (doc, plan) = plan(&target);
    let sets = refused_sets(&target.public_url).len();

    // Act
    let outcome = csrf_refusals(&doc, &target, &plan).await;

    // Assert
    println!(
        "CSRF matrix N = {} operations x {sets} header sets = {}",
        plan.checked.len(),
        outcome.cases
    );
    outcome.report("refusals");
    assert!(!plan.checked.is_empty());
    assert_eq!(outcome.cases, plan.checked.len() * sets);
    assert_eq!(outcome.failures, []);
    target.stop().await;
}

#[tokio::test]
async fn a_same_origin_proof_or_a_bearer_passes_the_layer_on_every_checked_operation() {
    // Arrange
    let target = Target::stub().await;
    let (doc, plan) = plan(&target);

    // Act
    let outcome = csrf_controls(&doc, &target, &plan).await;

    // Assert
    outcome.report("controls");
    assert_eq!(outcome.cases, plan.checked.len() * 3);
    assert_eq!(failing_operations(&outcome), SCHEMA_GAPS);
    target.stop().await;
}

#[tokio::test]
async fn no_get_or_head_changes_state_except_the_named_exemption() {
    // Arrange
    let target = Target::stub().await;
    let (doc, plan) = plan(&target);

    // Act
    let outcome = reads(&doc, &target, &plan).await;

    // Assert
    outcome.report("reads");
    let exempt: Vec<String> = plan.exempt.iter().map(|o| o.label()).collect();
    assert_eq!(exempt, ["GET /api/v1/auth/providers/{provider}/callback"]);
    assert_eq!(outcome.cases, plan.reads.len());
    assert_eq!(failing_operations(&outcome), READ_SCHEMA_GAPS);
    for failure in &outcome.failures {
        assert!(
            failure.detail.starts_with("the read did not run"),
            "{failure:?}"
        );
    }
    target.stop().await;
}

#[test]
fn every_state_changing_operation_is_checked_or_has_no_cookie_caller() {
    // Arrange
    let doc = Document::production();

    // Act
    let plan = csrf_plan(&doc, &super::samples::has_case_data).expect("no gaps");

    // Assert
    let mutations = doc.operations.iter().filter(|o| o.state_changing).count();
    let checked = plan.checked.iter().filter(|o| o.state_changing).count();
    assert_eq!(
        checked + plan.cookieless.len() + plan.exempt.len(),
        mutations
    );
    for operation in &plan.cookieless {
        assert!(!operation.accepts_cookie(), "{}", operation.label());
    }
}
