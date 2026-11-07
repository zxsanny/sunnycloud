use std::collections::HashSet;

use hyper::Method;
use uuid::Uuid;

use super::client::H2;
use super::stack::{SERVER_INFO, Stack};

const CLIENT_IDS: [&str; 3] = [
    "00000000-0000-4000-8000-000000000001",
    "01900000-0000-7000-8000-000000000002",
    "not-an-id",
];

#[tokio::test]
async fn a_thousand_answers_carry_distinct_uuid_v7_ids_and_never_a_client_supplied_one() {
    // Arrange
    let stack = Stack::start(&[]).await;
    let mut connection = H2::connect(stack.addr).await;
    let kinds: [(&str, &str); 8] = [
        ("GET", SERVER_INFO),
        ("GET", "/probe/problem/404"),
        ("GET", "/probe/problem/401"),
        ("GET", "/probe/problem/413"),
        ("GET", "/probe/problem/426"),
        ("GET", "/probe/problem/500"),
        (Method::OPTIONS.as_str(), SERVER_INFO),
        (Method::HEAD.as_str(), SERVER_INFO),
    ];
    let mut ids = HashSet::new();
    let mut problem_bodies = 0;

    // Act
    let mut replies = Vec::new();
    for round in 0..1000 {
        let (method, path) = kinds[round % kinds.len()];
        let supplied = CLIENT_IDS[round % CLIENT_IDS.len()];
        let headers: &[(&str, &str)] = if round % 3 == 0 {
            &[("x-request-id", supplied)]
        } else {
            &[]
        };
        replies.push((round, connection.send(method, path, headers).await));
    }

    // Assert
    for (round, reply) in &replies {
        let id = reply.request_id();
        let parsed = Uuid::parse_str(id).expect("the id is a UUID");
        assert_eq!(parsed.get_version_num(), 7, "round {round}: {id}");
        assert!(ids.insert(id.to_owned()), "round {round}: {id} repeats");
        assert!(
            !CLIENT_IDS.contains(&id),
            "round {round} echoed the client id"
        );
        if reply
            .headers
            .get("content-type")
            .is_some_and(|value| value == "application/problem+json")
        {
            assert_eq!(reply.json()["request_id"], id, "round {round}");
            problem_bodies += 1;
        }
    }
    assert_eq!(
        problem_bodies, 625,
        "5 of the 8 kinds answer a problem body"
    );
    assert_eq!(ids.len(), 1000);
    assert!(
        replies
            .iter()
            .any(|(_, reply)| reply.status.as_u16() == 426)
    );
    stack.stop().await;
}
