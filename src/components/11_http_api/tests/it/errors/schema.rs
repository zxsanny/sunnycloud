use std::collections::BTreeSet;

use sc_error::ErrorCode;
use sunnycloud_http_api::problem_schema;

use super::cases::CASES;
use super::probe::{ErrorProbe, fresh_id};

#[tokio::test]
async fn the_problem_schema_describes_every_body_the_cases_produce() {
    // Arrange
    let schema = problem_schema();
    let properties: BTreeSet<&str> = schema["properties"]
        .as_object()
        .expect("properties")
        .keys()
        .map(String::as_str)
        .collect();
    let required: Vec<&str> = schema["required"]
        .as_array()
        .expect("required")
        .iter()
        .map(|name| name.as_str().expect("name"))
        .collect();
    let probe = ErrorProbe::start().await;

    for case in CASES {
        // Act
        let body = probe
            .get(&format!("/probe/errors/{}", case.name), fresh_id())
            .await
            .json();

        // Assert
        let keys: BTreeSet<&str> = body
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert!(keys.is_subset(&properties), "{}: {keys:?}", case.name);
        assert!(
            required.iter().all(|name| keys.contains(name)),
            "{}",
            case.name
        );
    }
    probe.stop().await;
}

#[test]
fn the_problem_schema_lists_every_code_of_the_table() {
    // Arrange
    let schema = problem_schema();

    // Act
    let listed: Vec<&str> = schema["properties"]["code"]["enum"]
        .as_array()
        .expect("enum")
        .iter()
        .map(|code| code.as_str().expect("code"))
        .collect();

    // Assert
    let table: Vec<&str> = ErrorCode::ALL.iter().map(|code| code.as_str()).collect();
    assert_eq!(listed, table);
}
