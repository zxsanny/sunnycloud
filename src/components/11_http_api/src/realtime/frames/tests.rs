use super::*;

fn hex(text: &str) -> Vec<u8> {
    let digits: Vec<u8> = text.bytes().filter(u8::is_ascii_hexdigit).collect();
    digits
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).expect("ascii"), 16).expect("hex"))
        .collect()
}

fn counting(start: u8, len: u8) -> Vec<u8> {
    (0..len).map(|offset| start.wrapping_add(offset)).collect()
}

fn id_of(start: u8) -> Uuid {
    Uuid::from_slice(&counting(start, 16)).expect("16 bytes")
}

fn hash(start: u8) -> [u8; 32] {
    counting(start, 32).try_into().expect("32 bytes")
}

#[test]
fn a_subscribe_of_the_contract_unpacks_to_its_resume_point() {
    // Arrange
    let bytes = hex("01 0100000000000000 2a00000000000000");

    // Act
    let frame = decode_client(&bytes);

    // Assert
    assert_eq!(frame, Ok(ClientFrame::Subscribe { epoch: 1, seq: 42 }));
}

#[test]
fn a_ping_of_the_contract_unpacks_to_its_nonce() {
    // Arrange
    let bytes = hex("02 07000000");

    // Act
    let frame = decode_client(&bytes);

    // Assert
    assert_eq!(frame, Ok(ClientFrame::Ping { nonce: 7 }));
}

#[test]
fn hello_pong_and_reset_pack_to_the_bytes_of_the_contract() {
    // Arrange
    let hello = hex("20 0100 1e00 00040000");
    let pong = hex("24 07000000");
    let reset = hex("23 02 0100000000000000 8403000000000000");

    // Act
    let packed = (
        encode_hello(30, 1024),
        encode_pong(7),
        encode_reset(ResetReason::BelowPurgeHorizon, 1, 900),
    );

    // Assert
    assert_eq!(packed, (Ok(hello), Ok(pong), Ok(reset)));
}

#[test]
fn a_live_file_packs_to_the_131_bytes_of_the_contract() {
    // Arrange
    let frame = ChangeFrame {
        epoch: 1,
        seq: 42,
        node_id: id_of(0x01),
        folder: false,
        thumb: 1,
        name: "IMG_0001.HEIC".to_owned(),
        parent_id: Some(id_of(0x30)),
        revision_id: Some(id_of(0x50)),
        size: Some(2_345_678),
        content_hash: Some(hash(0x20)),
        media_type: Some("image/heic".to_owned()),
        deleted: false,
        end: true,
    };
    let expected = hex(
        "2101000000000000002a0000000000000001020304050607 08090a0b0c0d0e0f10 00 01 0d2bfffadf8fff7fa888a3f542ec \
         5f 303132333435363738393a3b3c3d3e3f 505152535455565758595a5b5c5d5e5f ceca230000000000 \
         202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f 08696e9c0ffe950bb0",
    );

    // Act
    let packed = encode_change(&frame);

    // Assert
    assert_eq!(packed, Ok(expected));
}

#[test]
fn a_tombstone_folder_with_a_ukrainian_name_packs_to_the_61_bytes_of_the_contract() {
    // Arrange
    let frame = ChangeFrame {
        epoch: 1,
        seq: 43,
        node_id: id_of(0x02),
        folder: true,
        thumb: 0,
        name: "Старі фото".to_owned(),
        parent_id: Some(id_of(0x30)),
        revision_id: None,
        size: None,
        content_hash: None,
        media_type: None,
        deleted: true,
        end: false,
    };
    let expected = hex(
        "21 0100000000000000 2b00000000000000 02030405060708090a0b0c0d0e0f1011 01 00 \
         08 83e59cab21fc6760 21 303132333435363738393a3b3c3d3e3f",
    );

    // Act
    let packed = encode_change(&frame);

    // Assert
    assert_eq!(packed, Ok(expected));
}

#[test]
fn a_folder_with_nothing_optional_packs_to_39_bytes() {
    // Arrange
    let frame = ChangeFrame {
        epoch: 1,
        seq: 44,
        node_id: id_of(0x03),
        folder: true,
        thumb: 0,
        name: "a".to_owned(),
        parent_id: None,
        revision_id: None,
        size: None,
        content_hash: None,
        media_type: None,
        deleted: false,
        end: false,
    };
    let expected = hex(
        "21 0100000000000000 2c00000000000000 030405060708090a0b0c0d0e0f101112 01 00 021061 00",
    );

    // Act
    let packed = encode_change(&frame);

    // Assert
    assert_eq!(packed, Ok(expected));
}

#[test]
fn bytes_that_are_not_a_client_frame_are_refused_with_their_kind() {
    // Arrange
    let truncated = hex("01 0100000000000000 2a000000");
    let trailing = hex("02 07000000 ff");

    // Act
    let errors = [
        decode_client(&[]),
        decode_client(&[0x7e, 0]),
        decode_client(&[HELLO, 1, 0]),
        decode_client(&[0x00]),
        decode_client(&truncated),
        decode_client(&trailing),
    ];

    // Assert
    assert_eq!(errors[0], Err(FrameError::Empty));
    assert_eq!(errors[1], Err(FrameError::UnknownType(0x7e)));
    assert_eq!(errors[2], Err(FrameError::ServerOnlyType(HELLO)));
    assert_eq!(errors[3], Err(FrameError::UnknownType(0x00)));
    assert!(matches!(errors[4], Err(FrameError::Malformed(_))));
    assert!(matches!(errors[5], Err(FrameError::Malformed(_))));
}

const VECTORS: &str = include_str!("../../../tests/fixtures/realtime_frames.txt");

/// The hex line `name` of the shared vector file, as bytes (`-` is the empty message).
fn vector(name: &str) -> Vec<u8> {
    let line = VECTORS
        .lines()
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| line.strip_prefix(name)?.strip_prefix(' '))
        .unwrap_or_else(|| panic!("no vector `{name}`"));
    if line == "-" { Vec::new() } else { hex(line) }
}

fn vector_names(prefix: &str) -> Vec<&'static str> {
    VECTORS
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.split(' ').next())
        .filter(|name| name.starts_with(prefix))
        .collect()
}

#[test]
fn the_shared_vector_file_holds_what_the_server_packs_and_reads() {
    // Arrange
    let change = |seq: u64, id: u8, folder: bool, name: &str| ChangeFrame {
        epoch: 1,
        seq,
        node_id: id_of(id),
        folder,
        thumb: 0,
        name: name.to_owned(),
        parent_id: None,
        revision_id: None,
        size: None,
        content_hash: None,
        media_type: None,
        deleted: false,
        end: false,
    };
    let file = ChangeFrame {
        thumb: 1,
        parent_id: Some(id_of(0x30)),
        revision_id: Some(id_of(0x50)),
        size: Some(2_345_678),
        content_hash: Some(hash(0x20)),
        media_type: Some("image/heic".to_owned()),
        end: true,
        ..change(42, 0x01, false, "IMG_0001.HEIC")
    };
    let tombstone = ChangeFrame {
        parent_id: Some(id_of(0x30)),
        deleted: true,
        ..change(43, 0x02, true, "Старі фото")
    };

    // Act
    let packed = [
        ("hello", encode_hello(30, 1024)),
        ("pong", encode_pong(7)),
        ("thumb_state_ready", encode_thumb_state(hash(0x20), 2)),
        (
            "reset_below_horizon",
            encode_reset(ResetReason::BelowPurgeHorizon, 1, 900),
        ),
        ("change_file", encode_change(&file)),
        ("change_folder_tombstone", encode_change(&tombstone)),
        (
            "change_root_child_minimal",
            encode_change(&change(44, 0x03, true, "a")),
        ),
    ];
    let read = [
        decode_client(&vector("subscribe")),
        decode_client(&vector("ping")),
    ];

    // Assert
    for (name, bytes) in packed {
        assert_eq!(bytes, Ok(vector(name)), "{name}");
    }
    assert_eq!(read[0], Ok(ClientFrame::Subscribe { epoch: 1, seq: 42 }));
    assert_eq!(read[1], Ok(ClientFrame::Ping { nonce: 7 }));
}

#[test]
fn every_hostile_client_vector_is_refused_and_the_server_ones_are_server_frames() {
    // Arrange
    let client = vector_names("bad_client_");
    let server = vector_names("bad_server_");

    // Act
    let refused: Vec<_> = client
        .iter()
        .map(|name| decode_client(&vector(name)))
        .collect();

    // Assert
    assert_eq!(client.len(), 6);
    assert!(refused.iter().all(Result::is_err), "{refused:?}");
    assert_eq!(server.len(), 3);
    assert!(
        server
            .iter()
            .all(|name| (HELLO..=0x3f).contains(&vector(name)[0]))
    );
}

#[test]
fn the_probe_of_the_vector_file_is_the_packbin_scheme_both_languages_were_checked_with() {
    // Arrange
    use packbin::{insert, list, times};
    let scheme = MapScheme::new(
        0x7f,
        vec![
            u8("kind"),
            u32("n"),
            u64("big"),
            utf8("name"),
            list("items", u16("item")),
            flags(
                4,
                "f",
                vec![u16("heading"), utf8("label"), group(6, "on", vec![])],
            ),
            u8("count"),
            times(8, "count", vec![u32("lat"), u32("lon")]),
            u8("tail"),
        ],
    );
    let mut values = Values::new();
    let mut put = |name: &str, value: Value| insert(&mut values, name, Some(value));
    put("kind", Value::U8(9));
    put("n", Value::U32(300));
    put("big", Value::U64(u64::MAX));
    put("name", Value::Str("zxsanny".to_owned()));
    put("items", Value::List(vec![Value::U16(1), Value::U16(2)]));
    put("heading", Value::U16(90));
    put("on", Value::U8(1));
    put("count", Value::U8(2));
    put("lat", Value::List(vec![Value::U32(10), Value::U32(30)]));
    put("lon", Value::List(vec![Value::U32(20), Value::U32(40)]));
    put("tail", Value::U8(7));

    // Act
    let packed = pack(&scheme, &values);

    // Assert
    assert_eq!(packed, Ok(vector("probe_all")));
}
