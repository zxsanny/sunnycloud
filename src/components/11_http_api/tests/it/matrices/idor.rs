//! IT-11-10 and IT-11-11 over the generated two-account matrix (AC-59, NFT-SEC-13, NFT-SEC-14) and the admin
//! refusal of G-S6.

use super::csrf::SCHEMA_GAPS;
use super::document::Document;
use super::plans::{Category, idor_plan};
use super::run_idor::{admin_refusals, body_account, failing_operations, two_accounts};
use super::target::Target;
use super::world::ACCOUNT_A;

#[tokio::test]
async fn another_accounts_id_answers_like_a_random_one_on_every_pair() {
    // Arrange
    let target = Target::stub().await;
    let doc = Document::production();
    let cases = idor_plan(&doc, &|owner| target.has_per_account_fake(owner))
        .unwrap_or_else(|problems| panic!("the IDOR generator found gaps: {problems:#?}"));

    // Act
    let outcome = two_accounts(&doc, &target, &cases).await;

    // Assert
    for category in [
        Category::Account,
        Category::Admin,
        Category::Worker,
        Category::Capability,
    ] {
        let n = cases
            .iter()
            .filter(|case| case.category == category)
            .count();
        println!("IDOR {category:?}: {n} pairs");
    }
    outcome.report("two-account matrix");
    assert_eq!(outcome.cases, cases.len());
    assert_eq!(failing_operations(&outcome), SCHEMA_GAPS);
    target.stop().await;
}

#[tokio::test]
async fn an_account_id_in_a_body_is_refused_before_any_port_call() {
    // Arrange
    let target = Target::stub().await;
    let doc = Document::production();

    // Act
    let outcome = body_account(&doc, &target, &ACCOUNT_A.to_string()).await;

    // Assert
    outcome.report("body account_id");
    assert!(outcome.cases > 0);
    assert_eq!(outcome.failures, []);
    target.stop().await;
}

#[tokio::test]
async fn every_admin_operation_refuses_a_member_with_403_and_no_port_call() {
    // Arrange
    let target = Target::stub().await;
    let doc = Document::production();
    let admin_operations = doc.operations.iter().filter(|o| o.auth == "Admin").count();

    // Act
    let outcome = admin_refusals(&doc, &target).await;

    // Assert
    outcome.report("admin refusals");
    assert!(admin_operations > 0);
    assert_eq!(outcome.cases, admin_operations * 2);
    assert_eq!(outcome.failures, []);
    target.stop().await;
}
