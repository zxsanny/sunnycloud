//! AC-6: a blob path is built from a parsed server-generated id only.

use std::path::{Component, Path, PathBuf};

use sunnycloud_blobstore::{FormatError, blob_path, parse_blob_id};
use uuid::Uuid;

const HOSTILE: [&str; 13] = [
    "../../etc/passwd",
    "00/../..",
    "not-a-uuid",
    "",
    "..",
    "/etc/passwd",
    "{11112222-3333-4444-8555-666677778888}",
    "11112222-3333-4444-8555-666677778888/../x",
    "11112222-3333-4444-8555-666677778888\0",
    "11112222333344448555666677778888",
    "urn:uuid:11112222-3333-4444-8555-666677778888",
    "AAAAAAAA-3333-4444-8555-666677778888",
    "11112222-3333-1444-8555-666677778888",
];

#[test]
fn hostile_strings_produce_no_blob_id_and_so_no_path() {
    for text in HOSTILE {
        assert_eq!(
            parse_blob_id(text),
            Err(FormatError::InvalidBlobId),
            "{text:?}"
        );
    }
}

#[test]
fn a_uppercase_spelling_of_a_valid_id_is_refused() {
    let id = Uuid::new_v4();

    assert_eq!(
        parse_blob_id(&id.to_string().to_uppercase()),
        Err(FormatError::InvalidBlobId)
    );
    assert_eq!(
        parse_blob_id(&id.braced().to_string()),
        Err(FormatError::InvalidBlobId)
    );
    assert_eq!(
        parse_blob_id(&id.simple().to_string()),
        Err(FormatError::InvalidBlobId)
    );
}

#[test]
fn a_lowercase_hyphenated_v4_id_parses_back_to_itself() {
    let id = Uuid::new_v4();

    assert_eq!(parse_blob_id(&id.to_string()), Ok(id));
}

#[test]
fn the_path_is_blobs_two_prefix_levels_and_the_id() {
    let id = parse_blob_id("11112222-3333-4444-8555-666677778888").expect("id");

    let path = blob_path(Path::new("/srv/data"), id);

    assert_eq!(
        path,
        PathBuf::from("/srv/data/blobs/11/11/11112222-3333-4444-8555-666677778888")
    );
}

#[test]
fn every_generated_id_stays_under_the_root_with_normal_components_only() {
    let root = Path::new("/srv/data");

    for _ in 0..200 {
        let path = blob_path(root, Uuid::new_v4());

        let relative = path.strip_prefix(root).expect("under the root");
        assert!(
            relative
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
        );
        assert_eq!(relative.components().count(), 4);
    }
}
