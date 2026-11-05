//! AC-6: feed-page checker, fixtures v1..v6 and the valid page of IT-13-11.

use sync_core_proto::{
    ChangeItem, ChangePage, ChangeState, Cursor, NodeKind, PageRule, check_page,
};
use uuid::Uuid;

const CURSOR: Cursor = Cursor::new(1, 100);

fn node(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_c5f0_0000_7000_8000_0000_0000_0000 + n)
}

fn item(seq: u64, n: u128) -> ChangeItem {
    ChangeItem {
        node_id: node(n),
        parent_id: None,
        kind: NodeKind::Folder,
        name: format!("n{n}"),
        state: ChangeState::Live,
        revision_id: None,
        size: None,
        content_hash: None,
        seq,
    }
}

fn page(items: Vec<ChangeItem>, next: Cursor) -> ChangePage {
    ChangePage {
        items,
        next_cursor: next,
        has_more: true,
    }
}

fn rule_of(page: &ChangePage) -> PageRule {
    check_page(CURSOR, page)
        .expect_err("page must be refused")
        .rule
}

fn valid_items() -> Vec<ChangeItem> {
    (101..=150).map(|seq| item(seq, u128::from(seq))).collect()
}

#[test]
fn valid_page_passes_unchanged() {
    let valid = page(valid_items(), Cursor::new(1, 150));
    let before = valid.clone();

    assert_eq!(check_page(CURSOR, &valid), Ok(()));
    assert_eq!(valid, before);
}

#[test]
fn empty_page_with_the_same_cursor_passes() {
    assert_eq!(check_page(CURSOR, &page(Vec::new(), CURSOR)), Ok(()));
}

#[test]
fn many_items_sharing_one_seq_ordered_by_node_id_pass() {
    let shared = page(
        vec![item(101, 1), item(101, 2), item(101, 3)],
        Cursor::new(1, 101),
    );

    assert_eq!(check_page(CURSOR, &shared), Ok(()));
}

#[test]
fn v1_items_not_ascending_is_p1() {
    assert_eq!(
        rule_of(&page(vec![item(103, 3), item(102, 2)], Cursor::new(1, 150))),
        PageRule::P1
    );
    assert_eq!(
        rule_of(&page(vec![item(101, 2), item(101, 1)], Cursor::new(1, 150))),
        PageRule::P1
    );
}

#[test]
fn v2_item_at_or_below_the_cursor_is_p2() {
    assert_eq!(
        rule_of(&page(vec![item(100, 1), item(101, 2)], Cursor::new(1, 150))),
        PageRule::P2
    );
    assert_eq!(
        rule_of(&page(vec![item(99, 1)], Cursor::new(1, 150))),
        PageRule::P2
    );
}

#[test]
fn v3_item_past_next_cursor_is_p3() {
    assert_eq!(
        rule_of(&page(vec![item(101, 1), item(151, 2)], Cursor::new(1, 150))),
        PageRule::P3
    );
}

#[test]
fn v4_next_cursor_in_another_epoch_is_p4() {
    assert_eq!(
        rule_of(&page(valid_items(), Cursor::new(2, 150))),
        PageRule::P4
    );
    assert_eq!(
        rule_of(&page(Vec::new(), Cursor::new(0, 200))),
        PageRule::P4
    );
}

#[test]
fn v5_next_cursor_before_the_cursor_is_p5() {
    assert_eq!(rule_of(&page(Vec::new(), Cursor::new(1, 90))), PageRule::P5);
}

#[test]
fn v6_duplicate_seq_and_node_is_p6() {
    assert_eq!(
        rule_of(&page(vec![item(101, 1), item(101, 1)], Cursor::new(1, 150))),
        PageRule::P6
    );
}

#[test]
fn violation_names_the_rule_in_its_message() {
    let error = check_page(CURSOR, &page(Vec::new(), Cursor::new(1, 90))).expect_err("p5");

    assert!(error.to_string().contains("P5"), "{error}");
    assert!(!error.detail.is_empty());
}
