use sc_error::ErrorCode;
use std::collections::HashSet;

// Independent copy of the contract table (helper 02), written out by hand.
const EXPECTED: &[(u16, &[&str])] = &[
    (400, &["invalid_request"]),
    (401, &["unauthenticated", "password_required"]),
    (403, &["account_not_active", "forbidden"]),
    (
        404,
        &[
            "parent_not_found",
            "not_found",
            "content_unavailable",
            "revision_obsolete",
            "thumbnail_not_ready",
            "thumbnail_unavailable",
        ],
    ),
    (
        409,
        &[
            "name_conflict",
            "edited_since",
            "cycle",
            "need_blocks",
            "block_conflict",
            "last_admin",
            "session_cap_exceeded",
            "staged_cap_reached",
            "lease_lost",
        ],
    ),
    (
        410,
        &["upload_session_gone", "parent_deleted", "resync_required"],
    ),
    (413, &["size_too_large", "output_too_large"]),
    (416, &["range_not_satisfiable"]),
    (
        422,
        &[
            "invalid_name",
            "invalid_cursor",
            "not_a_folder",
            "blocklist_mismatch",
            "commit_id_reused",
            "batch_too_large",
            "target_not_shareable",
            "block_hash_mismatch",
            "block_length_mismatch",
            "output_type_rejected",
            "output_kind_rejected",
        ],
    ),
    (426, &["upgrade_required"]),
    (429, &["rate_limited"]),
    (500, &["internal"]),
    (503, &["service_unavailable", "overloaded"]),
    (507, &["quota_exceeded", "free_space_floor"]),
];

#[test]
fn every_code_has_exactly_the_contract_status() {
    let mut expected_count = 0;
    for (status, wires) in EXPECTED {
        for wire in *wires {
            expected_count += 1;
            let code = ErrorCode::from_wire(wire)
                .unwrap_or_else(|| panic!("code {wire} missing from ErrorCode"));
            assert_eq!(code.status(), *status, "status of {wire}");
        }
    }
    assert_eq!(
        ErrorCode::ALL.len(),
        expected_count,
        "ErrorCode has codes outside the contract table"
    );
}

#[test]
fn wire_codes_are_unique_snake_case_and_round_trip() {
    let mut seen = HashSet::new();
    for code in ErrorCode::ALL {
        let wire = code.as_str();
        assert!(seen.insert(wire), "duplicate wire code {wire}");
        assert!(!wire.is_empty());
        assert!(
            wire.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
            "{wire} is not snake_case"
        );
        assert!(!wire.starts_with('_') && !wire.ends_with('_') && !wire.contains("__"));
        assert_eq!(ErrorCode::from_wire(wire), Some(*code));
        assert!(!code.title().is_empty());
        assert!((400..600).contains(&code.status()));
    }
}

#[test]
fn unknown_wire_code_is_not_mapped() {
    assert_eq!(ErrorCode::from_wire("brand_new"), None);
    assert_eq!(ErrorCode::from_wire(""), None);
    assert_eq!(ErrorCode::from_wire("Not_Found"), None);
}

#[test]
fn quota_and_free_space_floor_are_507() {
    assert_eq!(ErrorCode::QuotaExceeded.status(), 507);
    assert_eq!(ErrorCode::FreeSpaceFloor.status(), 507);
}
