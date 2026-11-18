//! AZ-3926 AC-4 (AC-59): the generated matrices cover the four vault operations, account B gets 404 on each with
//! A's vault id, and a matrix input that drops one vault operation the server serves fails the coverage check.

use super::document::{Document, registry_routes};
use super::plans::{csrf_plan, idor_plan, unlisted};
use super::run::csrf_refusals;
use super::run_idor::two_accounts;
use super::target::Target;

const VAULT_OPERATIONS: [&str; 4] = [
    "GET /api/v1/vaults/{id}",
    "POST /api/v1/vaults",
    "POST /api/v1/vaults/{id}/unlock-failures",
    "PUT /api/v1/vaults/{id}/header",
];

fn is_vault(label: &str) -> bool {
    label.contains("/api/v1/vaults")
}

#[tokio::test]
async fn account_b_gets_404_on_all_four_vault_operations_with_as_vault_id() {
    // Arrange
    let target = Target::stub().await;
    let doc = Document::production();
    let cases: Vec<_> = idor_plan(&doc, &|owner| target.has_per_account_fake(owner))
        .expect("no IDOR gaps")
        .into_iter()
        .filter(|case| is_vault(&case.operation.label()))
        .collect();

    // Act
    let outcome = two_accounts(&doc, &target, &cases).await;

    // Assert
    let mut labels: Vec<String> = cases.iter().map(|case| case.operation.label()).collect();
    labels.sort();
    assert_eq!(labels, VAULT_OPERATIONS);
    assert_eq!(outcome.cases, 4);
    assert_eq!(outcome.failures, []);
    target.stop().await;
}

#[tokio::test]
async fn the_csrf_matrix_checks_the_three_vault_mutations_and_reads_the_vault() {
    // Arrange
    let target = Target::stub().await;
    let doc = Document::production();
    let plan = csrf_plan(&doc, &|owner| target.has_case_data(owner)).expect("no CSRF gaps");

    // Act
    let checked: Vec<String> = plan
        .checked
        .iter()
        .map(|operation| operation.label())
        .filter(|label| is_vault(label))
        .collect();
    let reads: Vec<String> = plan
        .reads
        .iter()
        .map(|operation| operation.label())
        .filter(|label| is_vault(label))
        .collect();
    let outcome = csrf_refusals(&doc, &target, &plan).await;

    // Assert
    assert_eq!(
        checked,
        [
            "POST /api/v1/vaults",
            "PUT /api/v1/vaults/{id}/header",
            "POST /api/v1/vaults/{id}/unlock-failures",
        ]
    );
    assert_eq!(reads, ["GET /api/v1/vaults/{id}"]);
    assert_eq!(outcome.failures, []);
    target.stop().await;
}

#[tokio::test]
async fn a_matrix_input_without_one_vault_operation_fails_the_coverage_check() {
    // Arrange
    let target = Target::stub().await;
    let served = target.served.as_ref().expect("the Mode S document");
    let without_replace: Vec<_> = registry_routes()
        .into_iter()
        .filter(|def| def.spec.path != "/api/v1/vaults/{id}/header")
        .collect();
    let input = Document::of(&without_replace);

    // Act
    let gaps = unlisted(&input, served);
    let complete = unlisted(&Document::production(), served);

    // Assert
    assert_eq!(
        gaps,
        ["PUT /api/v1/vaults/{id}/header is served but not in the matrix input"]
    );
    assert_eq!(complete, Vec::<String>::new());
    target.stop().await;
}
