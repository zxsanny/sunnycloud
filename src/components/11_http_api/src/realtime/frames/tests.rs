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

fn id(start: u8) -> Uuid {
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
        node_id: id(0x01),
        folder: false,
        thumb: 1,
        name: "IMG_0001.HEIC".to_owned(),
        parent_id: Some(id(0x30)),
        revision_id: Some(id(0x50)),
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
        node_id: id(0x02),
        folder: true,
        thumb: 0,
        name: "Старі фото".to_owned(),
        parent_id: Some(id(0x30)),
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
        node_id: id(0x03),
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
