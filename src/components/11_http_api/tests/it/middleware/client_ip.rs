use super::client::h1_from;
use super::stack::Stack;

const TRUSTED_PEER: &str = "127.0.0.2";

#[tokio::test]
async fn a_trusted_peer_gets_the_forwarded_address_and_a_header_list_resolves_to_the_proxy_appended_entry()
 {
    // Arrange
    let stack = Stack::start(&["127.0.0.0/24"]).await;

    // Act
    let single = h1_from(
        TRUSTED_PEER,
        stack.addr,
        "/probe/client-ip",
        &[("x-forwarded-for", "198.51.100.7")],
    )
    .await;
    let spoofed_list = h1_from(
        TRUSTED_PEER,
        stack.addr,
        "/probe/client-ip",
        &[("x-forwarded-for", "1.1.1.1, 198.51.100.7")],
    )
    .await;
    let without_header = h1_from(TRUSTED_PEER, stack.addr, "/probe/client-ip", &[]).await;

    // Assert
    assert_eq!(single.text(), "198.51.100.7");
    assert_eq!(spoofed_list.text(), "198.51.100.7");
    assert_eq!(without_header.text(), TRUSTED_PEER);
    stack.stop().await;
}

#[tokio::test]
async fn an_untrusted_peer_keeps_its_address_while_it_rotates_the_forwarded_header() {
    // Arrange
    let stack = Stack::start(&["10.0.0.0/24"]).await;
    let mut seen = Vec::new();

    // Act
    for round in 0..20 {
        let forged = format!("198.51.100.{round}");
        let reply = h1_from(
            TRUSTED_PEER,
            stack.addr,
            "/probe/client-ip",
            &[("x-forwarded-for", forged.as_str())],
        )
        .await;
        seen.push(reply.text());
    }

    // Assert
    assert_eq!(seen.len(), 20);
    assert!(seen.iter().all(|address| address == TRUSTED_PEER));
    stack.stop().await;
}

#[tokio::test]
async fn an_empty_trusted_list_ignores_the_header_from_every_peer() {
    // Arrange
    let stack = Stack::start(&[]).await;

    // Act
    let reply = h1_from(
        "127.0.0.1",
        stack.addr,
        "/probe/client-ip",
        &[("x-forwarded-for", "198.51.100.7")],
    )
    .await;

    // Assert
    assert_eq!(reply.text(), "127.0.0.1");
    stack.stop().await;
}
