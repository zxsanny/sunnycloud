//! The 64-byte file header: layout, round trip, and refusal of anything else.

use contracts::BlobError;
use sunnycloud_blobstore::{FORMAT_VERSION, HEADER_LEN, Header, NONCE_PREFIX_LEN};

use crate::support::prefix;

fn header() -> Header {
    Header {
        nonce_prefix: prefix(),
    }
}

#[test]
fn the_header_is_64_bytes_with_magic_version_prefix_and_zero_pad() {
    let bytes = header().encode();

    assert_eq!(bytes.len(), 64);
    assert_eq!(&bytes[..8], b"SUNNYBLB");
    assert_eq!(&bytes[8..12], &FORMAT_VERSION.to_be_bytes());
    assert_eq!(&bytes[12..12 + NONCE_PREFIX_LEN], &prefix());
    assert!(bytes[12 + NONCE_PREFIX_LEN..].iter().all(|byte| *byte == 0));
    assert_eq!(FORMAT_VERSION, 1);
}

#[test]
fn a_header_round_trips() {
    assert_eq!(
        Header::decode(&header().encode()).expect("decode"),
        header()
    );
}

#[test]
fn a_generated_header_has_a_fresh_prefix_each_time() {
    let first = Header::generate().expect("generate");
    let second = Header::generate().expect("generate");

    assert_ne!(first.nonce_prefix, second.nonce_prefix);
    assert_ne!(first.nonce_prefix, [0; NONCE_PREFIX_LEN]);
}

#[test]
fn a_wrong_length_magic_version_or_pad_is_corrupt() {
    let good = header().encode();
    let mut bad_magic = good;
    bad_magic[0] ^= 1;
    let mut bad_version = good;
    bad_version[11] = 2;
    let mut bad_pad = good;
    bad_pad[HEADER_LEN - 1] = 1;

    for bytes in [
        &bad_magic[..],
        &bad_version[..],
        &bad_pad[..],
        &good[..63],
        &[][..],
    ] {
        assert!(matches!(Header::decode(bytes), Err(BlobError::Corrupt)));
    }
    let mut longer = good.to_vec();
    longer.push(0);
    assert!(matches!(Header::decode(&longer), Err(BlobError::Corrupt)));
}
