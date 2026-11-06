//! AC-4: each error built from a real I/O failure, a real database failure and a real cipher failure formats
//! with ids only. The failures are produced, not constructed: a missing file, a full device, a directory read
//! as a file, a violated constraint, a ciphertext opened under the wrong key.

use std::error::Error;
use std::io;

use chacha20poly1305::aead::{self, Aead, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use contracts::{BlobError, KeyError};
use sc_db::{DbError, RunError, Scope};
use sunnycloud_blobstore::{
    BlobFault, BlobIds, blob_error_from_chunk, blob_error_from_db, blob_error_from_io,
    blob_error_from_unwrap, key_error_from_cipher, key_error_from_db, key_error_from_io,
};
use uuid::Uuid;

use crate::support::{Fixture, WRAPPED_KEY, account, blob};

const KEY_BYTE: u8 = 0xAB;

fn ids() -> BlobIds {
    BlobIds {
        blob_id: Some(Uuid::new_v4()),
        account_id: Some(Uuid::new_v4()),
    }
}

fn missing_file() -> io::Error {
    let path = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::read(path).expect_err("the file does not exist")
}

fn full_device() -> io::Error {
    std::fs::write("/dev/full", b"x").expect_err("/dev/full refuses every write")
}

fn directory_read_as_file() -> io::Error {
    std::fs::read(std::env::temp_dir()).expect_err("a directory is not a file")
}

fn wrong_key_failure() -> aead::Error {
    let sealed = XChaCha20Poly1305::new(&Key::from([KEY_BYTE; 32]))
        .encrypt(&XNonce::default(), b"payload".as_slice())
        .expect("seal");
    XChaCha20Poly1305::new(&Key::from([KEY_BYTE + 1; 32]))
        .decrypt(&XNonce::default(), sealed.as_slice())
        .expect_err("a wrong key does not open")
}

/// Every text an operator can see of `error`: Display and Debug of it and of each cause behind it.
fn texts(error: &(dyn Error + 'static)) -> Vec<String> {
    let mut all = Vec::new();
    let mut next = Some(error);
    while let Some(current) = next {
        all.push(current.to_string());
        all.push(format!("{current:?}"));
        next = current.source();
    }
    all
}

/// The forms key bytes and the stored wrapped key could take in text.
fn leaked_key_forms() -> Vec<String> {
    let hex = format!("{KEY_BYTE:02x}").repeat(4);
    vec![
        hex.clone(),
        hex.to_uppercase(),
        format!("{KEY_BYTE}, {KEY_BYTE}"),
        String::from_utf8_lossy(WRAPPED_KEY).into_owned(),
        "sentinel".to_owned(),
    ]
}

fn assert_clean(error: &(dyn Error + 'static), forbidden: &[String]) {
    for text in texts(error) {
        for needle in forbidden.iter().chain(&leaked_key_forms()) {
            assert!(!text.contains(needle), "`{text}` contains `{needle}`");
        }
    }
}

fn fault_of<'a>(error: &'a (dyn Error + 'static)) -> &'a BlobFault {
    error
        .source()
        .and_then(|source| source.downcast_ref::<BlobFault>())
        .expect("the payload is a BlobFault")
}

/// Real constraint violations through the API role: an insert that repeats a stored id, and one with a bad state.
async fn database_failures() -> (Vec<DbError>, Uuid, Uuid) {
    let fixture = Fixture::new().await;
    let (account_id, blob_id) = (Uuid::new_v4(), Uuid::new_v4());
    let mut conn = fixture.owner_conn().await;
    account(&mut conn, account_id).await;
    blob(&mut conn, blob_id, account_id, "committed", Some(1))
        .await
        .expect("fixture blob");
    drop(conn);
    let mut failures = Vec::new();
    for (id, state) in [(blob_id, "committed"), (Uuid::new_v4(), "bogus")] {
        let failed = fixture
            .api
            .run(Scope::Account(account_id), async |tx| {
                sqlx::query(
                    "INSERT INTO blobs (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
                     VALUES ($1, $2, $3, 1, $4, 1, 'epoch'::timestamptz)",
                )
                .bind(id)
                .bind(account_id)
                .bind(state)
                .bind(WRAPPED_KEY)
                .execute(tx.conn())
                .await
                .map_err(DbError::from)
            })
            .await;
        let Err(RunError::Closure(error)) = failed else {
            panic!("the insert violates a constraint, got {failed:?}");
        };
        failures.push(error);
    }
    fixture.finish().await;
    (failures, account_id, blob_id)
}

#[test]
fn a_missing_file_is_blob_missing_and_key_file_missing() {
    // Arrange
    let ids = ids();

    // Act
    let blob_error = blob_error_from_io(missing_file(), ids);
    let key_error = key_error_from_io(missing_file(), ids);

    // Assert
    assert!(matches!(blob_error, BlobError::BlobMissing));
    assert_eq!(blob_error.to_string(), "blob file missing");
    assert!(matches!(key_error, KeyError::FileMissing));
    assert_eq!(key_error.to_string(), "master key file missing");
}

#[test]
fn a_full_device_is_no_space() {
    // Arrange
    let error = full_device();

    // Act
    let converted = blob_error_from_io(error, ids());

    // Assert
    assert!(matches!(converted, BlobError::NoSpace));
    assert_eq!(converted.to_string(), "no space left on the blob volume");
}

#[test]
fn another_io_failure_is_io_carrying_the_ids_and_the_kind_only() {
    // Arrange
    let ids = ids();
    let (blob_id, account_id) = (ids.blob_id.unwrap(), ids.account_id.unwrap());
    let directory = std::env::temp_dir().display().to_string();

    // Act
    let blob_error = blob_error_from_io(directory_read_as_file(), ids);
    let key_error = key_error_from_io(directory_read_as_file(), ids);

    // Assert
    assert!(matches!(blob_error, BlobError::Io(_)));
    assert!(matches!(key_error, KeyError::Unexpected(_)));
    let expected = format!("i/o failure (is a directory) blob {blob_id} account {account_id}");
    assert_eq!(fault_of(&blob_error).to_string(), expected);
    assert_eq!(fault_of(&key_error).to_string(), expected);
    assert_eq!(fault_of(&blob_error).ids(), ids);
    assert_eq!(
        format!("{blob_error:?}"),
        format!(
            "Io(BlobFault {{ ids: BlobIds {{ blob_id: Some({blob_id}), account_id: Some({account_id}) }}, cause: Io(IsADirectory) }})"
        )
    );
    assert_clean(&blob_error, &[directory.clone(), "os error".to_owned()]);
    assert_clean(&key_error, &[directory, "os error".to_owned()]);
}

#[tokio::test]
async fn a_violated_constraint_is_io_for_a_blob_and_unexpected_for_a_key() {
    // Arrange
    let (failures, account_id, blob_id) = database_failures().await;
    let ids = BlobIds {
        blob_id: Some(blob_id),
        account_id: Some(account_id),
    };
    let classes = ["unique violation", "check violation"];

    for (failure, class) in failures.into_iter().zip(classes) {
        // Act
        let blob_error = blob_error_from_db(RunError::Closure(failure.clone()), ids);
        let key_error = key_error_from_db(RunError::Db(failure), ids);

        // Assert
        let expected = format!("database failure ({class}) blob {blob_id} account {account_id}");
        assert!(matches!(blob_error, BlobError::Io(_)));
        assert!(matches!(key_error, KeyError::Unexpected(_)));
        assert_eq!(fault_of(&blob_error).to_string(), expected);
        assert_eq!(fault_of(&key_error).to_string(), expected);
        assert_clean(
            &blob_error,
            &["blobs_pkey".to_owned(), "SQLSTATE".to_owned()],
        );
        assert_clean(
            &key_error,
            &["blobs_pkey".to_owned(), "SQLSTATE".to_owned()],
        );
    }
}

#[test]
fn a_cipher_failure_is_corrupt_for_a_chunk_and_key_unavailable_for_an_unwrap() {
    // Arrange
    let chunk_failure = wrong_key_failure();
    let unwrap_failure = wrong_key_failure();

    // Act
    let chunk = blob_error_from_chunk(chunk_failure);
    let unwrap = blob_error_from_unwrap(unwrap_failure);

    // Assert
    assert!(matches!(chunk, BlobError::Corrupt));
    assert_eq!(chunk.to_string(), "stored blob is corrupt");
    assert!(matches!(unwrap, BlobError::KeyUnavailable));
    assert_eq!(unwrap.to_string(), "master key unavailable");
    assert_clean(&chunk, &[]);
    assert_clean(&unwrap, &[]);
}

#[test]
fn a_cipher_failure_in_key_handling_is_unexpected_carrying_the_ids_only() {
    // Arrange
    let ids = ids();
    let (blob_id, account_id) = (ids.blob_id.unwrap(), ids.account_id.unwrap());

    // Act
    let error = key_error_from_cipher(wrong_key_failure(), ids);

    // Assert
    assert!(matches!(error, KeyError::Unexpected(_)));
    assert_eq!(
        fault_of(&error).to_string(),
        format!("cipher failure blob {blob_id} account {account_id}")
    );
    assert_clean(&error, &[]);
}

#[test]
fn a_failure_without_ids_formats_without_them() {
    // Arrange
    let ids = BlobIds::default();

    // Act
    let error = blob_error_from_io(directory_read_as_file(), ids);

    // Assert
    assert_eq!(fault_of(&error).to_string(), "i/o failure (is a directory)");
}
