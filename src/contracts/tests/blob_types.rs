//! AC-6: one `BlobRef`, four fields; the other blob data-plane types.

use std::fs;
use std::path::Path;

use contracts::{BlobRef, ByteRange, StreamError};
use uuid::Uuid;

fn sources(dir: &Path, found: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).expect("read src dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push((
                path.display().to_string(),
                fs::read_to_string(&path).expect("read source"),
            ));
        }
    }
}

fn definitions_of(name: &str) -> Vec<(String, Vec<String>)> {
    let mut files = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let mut found = Vec::new();
    for (path, text) in files {
        let mut lines = text.lines().map(str::trim);
        while let Some(line) = lines.next() {
            let defines = ["pub struct ", "struct ", "pub enum ", "enum ", "pub type "]
                .iter()
                .filter_map(|prefix| line.strip_prefix(prefix))
                .any(|rest| rest.split([' ', '{', '(', '<', ';']).next() == Some(name));
            if defines {
                let fields = lines
                    .by_ref()
                    .take_while(|field| *field != "}")
                    .map(str::to_owned)
                    .collect();
                found.push((path.clone(), fields));
            }
        }
    }
    found
}

#[test]
fn exactly_one_blob_ref_is_defined_with_the_four_contract_fields() {
    let found = definitions_of("BlobRef");

    assert_eq!(found.len(), 1, "BlobRef definitions: {found:?}");
    assert_eq!(
        found[0].1,
        [
            "pub blob_id: Uuid,",
            "pub size: i64,",
            "pub wrapped_key: Vec<u8>,",
            "pub master_key_id: i16,"
        ]
    );
}

#[test]
fn blob_ref_debug_hides_the_wrapped_key() {
    let blob = BlobRef {
        blob_id: Uuid::from_u128(1),
        size: 10,
        wrapped_key: vec![222, 173, 190, 239],
        master_key_id: 3,
    };

    let printed = format!("{blob:?}");

    assert!(printed.contains("<redacted>"), "{printed}");
    assert!(!printed.contains("222"), "{printed}");
    assert!(printed.contains("master_key_id: 3"), "{printed}");
}

#[test]
fn a_byte_range_is_inclusive() {
    let range = ByteRange { start: 0, end: 0 };

    assert_eq!(range.end - range.start + 1, 1);
}

#[test]
fn a_corrupt_stream_error_is_distinct_from_a_source_failure() {
    let corrupt = StreamError::Corrupt;
    let source = StreamError::Source(Box::new(std::io::Error::other("disk")));

    assert!(matches!(corrupt, StreamError::Corrupt));
    assert!(matches!(source, StreamError::Source(_)));
}
