//! AC-4: every `TreeError` variant converts through `sc_error` to exactly the code and status of the
//! specification; a database failure is the unexpected tier and leaks no database text.

use std::error::Error;

use sc_db::{DbError, RunError, Scope};
use sc_error::{BusinessParams, CorrelationId, DomainError, ErrorCode};
use sqlx::Connection;
use sunnycloud_tree::TreeError;
use uuid::Uuid;

use crate::support::{Fixture, account, node_row};

fn request_id() -> CorrelationId {
    CorrelationId::new(Uuid::new_v4())
}

fn business_variants() -> Vec<(TreeError, &'static str, u16)> {
    vec![
        (TreeError::NameConflict, "name_conflict", 409),
        (TreeError::InvalidName, "invalid_name", 422),
        (TreeError::Cycle, "cycle", 409),
        (TreeError::EditedSince, "edited_since", 409),
        (TreeError::ParentDeleted, "parent_deleted", 410),
        (TreeError::ParentNotFound, "parent_not_found", 404),
        (TreeError::NotAFolder, "not_a_folder", 422),
        (TreeError::InvalidCursor, "invalid_cursor", 422),
        (TreeError::BatchTooLarge, "batch_too_large", 422),
        (TreeError::NotFound, "not_found", 404),
    ]
}

#[test]
fn every_business_variant_maps_to_its_code_and_status() {
    for (error, wire, status) in business_variants() {
        // Arrange
        let name = format!("{error:?}");

        // Act
        let domain = error.into_domain(request_id());

        // Assert
        assert!(
            matches!(domain, DomainError::Business(_)),
            "{name} is not tier 2"
        );
        assert_eq!(domain.code().as_str(), wire, "{name}");
        assert_eq!(domain.code().status(), status, "{name}");
    }
}

#[test]
fn error_code_agrees_with_the_domain_code() {
    for (error, wire, status) in business_variants() {
        // Arrange
        let code = error.error_code();

        // Act
        let domain_code = error.into_domain(request_id()).code();

        // Assert
        assert_eq!((code.as_str(), code.status()), (wire, status));
        assert_eq!(domain_code, code);
    }
}

#[test]
fn a_business_variant_carries_no_parameters_in_its_body() {
    for (error, _, _) in business_variants() {
        // Arrange
        let id = request_id();

        // Act
        let body = error.into_domain(id).problem(id);

        // Assert
        assert_eq!(body.params, BusinessParams::default());
        assert!(body.problems.is_empty());
        assert_eq!(body.request_id, id);
    }
}

/// A real unique violation raised by the API role on a duplicate node id.
async fn database_failure() -> (DbError, Uuid) {
    let fixture = Fixture::new().await;
    let (account_id, id) = (Uuid::new_v4(), Uuid::new_v4());
    let mut conn = fixture.owner_conn().await;
    account(&mut conn, account_id).await;
    node_row(&mut conn, id, account_id, None, "folder", "", "")
        .await
        .expect("fixture root");
    conn.close().await.expect("close fixture connection");
    let failed = fixture
        .api
        .run(Scope::Account(account_id), async |tx| {
            node_row(tx.conn(), id, account_id, None, "folder", "", "")
                .await
                .map_err(DbError::from)
        })
        .await;
    let Err(RunError::Closure(error)) = failed else {
        panic!("the insert repeats a node id, got {failed:?}");
    };
    fixture.finish().await;
    (error, id)
}

#[tokio::test]
async fn a_database_failure_is_internal_and_keeps_its_cause_out_of_the_body() {
    // Arrange
    let (cause, node_id) = database_failure().await;
    let cause_text = cause.to_string();
    let id = request_id();

    // Act
    let domain = TreeError::from(cause).into_domain(id);

    // Assert
    let DomainError::Unexpected {
        source,
        correlation_id,
    } = &domain
    else {
        panic!("expected tier 3, got {domain:?}");
    };
    assert_eq!(source.to_string(), cause_text);
    assert_eq!(*correlation_id, id);
    assert_eq!(domain.code(), ErrorCode::Internal);
    assert_eq!(domain.code().status(), 500);
    let shown = [
        domain.to_string(),
        format!("{:?}", domain.problem(id)),
        TreeError::Database(DbError::CheckViolation).to_string(),
    ];
    for text in shown {
        assert!(!text.contains(&cause_text), "{text}");
        assert!(!text.contains(&node_id.to_string()), "{text}");
        assert!(!text.contains("nodes_pkey"), "{text}");
    }
}

#[test]
fn the_database_cause_is_the_source_of_the_error() {
    // Arrange
    let error = TreeError::Database(DbError::Unavailable);

    // Act
    let source = error.source().map(ToString::to_string);

    // Assert
    assert_eq!(source.as_deref(), Some("database unavailable"));
}
