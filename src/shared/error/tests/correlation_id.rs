use sc_error::{CorrelationId, uuid_v7};
use uuid::{Uuid, Variant};

const FRESH: u128 = 0x0192_8f3a_7b00_7abc_8def_0123_4567_89ab;

fn fresh() -> Uuid {
    Uuid::from_u128(FRESH)
}

#[test]
fn well_formed_header_is_kept() {
    let header = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";

    let id = CorrelationId::from_request(Some(header), || panic!("must not generate"));

    assert_eq!(id.to_string(), header);
}

#[test]
fn missing_or_malformed_header_takes_the_fresh_id() {
    for header in [
        None,
        Some(""),
        Some("not-a-uuid"),
        Some("6ba7b8109dad11d180b400c04fd430c8"),
        Some("{6ba7b810-9dad-11d1-80b4-00c04fd430c8}"),
        Some("6ba7b810-9dad-11d1-80b4-00c04fd430c8-extra"),
        Some("6ba7b810-9dad-11d1-80b4-00c04fd430cZ"),
    ] {
        let id = CorrelationId::from_request(header, fresh);

        assert_eq!(id.as_uuid(), fresh(), "header {header:?}");
    }
}

#[test]
fn uuid_v7_sets_version_variant_and_timestamp() {
    let random = [0xFF; 10];

    let id = uuid_v7(0x0192_8f3a_7b00, random);

    assert_eq!(id.get_version_num(), 7);
    assert_eq!(id.get_variant(), Variant::RFC4122);
    assert_eq!(id.as_u128() >> 80, 0x0192_8f3a_7b00);
    // 12 + 62 random bits survive, the 6 reserved bits do not
    assert_eq!((id.as_u128() >> 64) & 0x0FFF, 0x0FFF);
    assert_eq!(id.as_u128() & ((1 << 62) - 1), (1 << 62) - 1);
}

#[test]
fn uuid_v7_masks_timestamp_to_48_bits_and_keeps_zero_random() {
    let id = uuid_v7(u64::MAX, [0; 10]);

    assert_eq!(id.as_u128() >> 80, 0xFFFF_FFFF_FFFF);
    assert_eq!(id.get_version_num(), 7);
    assert_eq!(id.as_u128() & ((1 << 62) - 1), 0);
}

#[test]
fn generated_id_is_accepted_when_echoed_back() {
    let first = CorrelationId::from_request(None, || uuid_v7(1_700_000_000_000, [7; 10]));

    let echoed =
        CorrelationId::from_request(Some(&first.to_string()), || panic!("must not generate"));

    assert_eq!(echoed, first);
}
