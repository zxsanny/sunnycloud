use sc_ids::{BLOCK_SIZE, FileHash, FileHasher, HashError, derive_client_key, hash_block, rollup};
use uuid::{Uuid, Variant, Version};

const CHILD_ENV: &str = "SC_IDS_PRINT_CLIENT_KEY";
const CHILD_MARK: &str = "CLIENT_KEY=";

fn sample_block() -> Vec<u8> {
    (0..4096u32).map(|i| (i % 253) as u8).collect()
}

fn file_hash_of(block: &[u8]) -> FileHash {
    rollup(&[hash_block(block)])
}

fn fixed_inputs() -> (Uuid, &'static str, FileHash) {
    (
        Uuid::from_u128(0x0190_c5f0_0000_7000_8000_0000_0000_0001),
        "content://media/external/images/media/42",
        FileHash::from_bytes([7; 32]),
    )
}

#[test]
fn bit_flip_changes_block_and_file_hashes() {
    let block = sample_block();
    let mut flipped = block.clone();
    flipped[1000] ^= 0b0000_0100;

    assert_ne!(hash_block(&block), hash_block(&flipped));
    assert_ne!(file_hash_of(&block), file_hash_of(&flipped));
}

#[test]
fn derive_client_key_is_stable_with_version_and_variant_bits() {
    let (device, asset, file) = fixed_inputs();

    let first = derive_client_key(device, asset, &file);
    let second = derive_client_key(device, asset, &file);

    assert_eq!(first, second);
    assert_eq!(first.get_version(), Some(Version::Custom));
    assert_eq!(first.get_variant(), Variant::RFC4122);
}

#[test]
fn derive_client_key_depends_on_every_input() {
    let (device, asset, file) = fixed_inputs();
    let base = derive_client_key(device, asset, &file);

    assert_ne!(
        base,
        derive_client_key(Uuid::from_u128(device.as_u128() + 1), asset, &file)
    );
    assert_ne!(base, derive_client_key(device, "other-asset", &file));
    assert_ne!(
        base,
        derive_client_key(device, asset, &FileHash::from_bytes([8; 32]))
    );
}

#[test]
fn derive_client_key_is_equal_across_processes() {
    if std::env::var_os(CHILD_ENV).is_some() {
        let (device, asset, file) = fixed_inputs();
        println!("{CHILD_MARK}{}", derive_client_key(device, asset, &file));
        return;
    }

    let exe = std::env::current_exe().expect("test binary path");
    let output = std::process::Command::new(exe)
        .args([
            "--exact",
            "derive_client_key_is_equal_across_processes",
            "--nocapture",
        ])
        .env(CHILD_ENV, "1")
        .output()
        .expect("spawn the test binary as a second process");
    assert!(output.status.success(), "child failed: {output:?}");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 output");
    let child_key = stdout
        .lines()
        .find_map(|line| line.strip_prefix(CHILD_MARK))
        .expect("child printed the key");

    let (device, asset, file) = fixed_inputs();
    assert_eq!(
        child_key,
        derive_client_key(device, asset, &file).to_string()
    );
}

#[test]
fn block_of_two_mib_plus_one_byte_is_rejected_without_state_change() {
    let mut hasher = FileHasher::new();
    let oversize = vec![0u8; BLOCK_SIZE + 1];

    assert_eq!(hasher.update(&oversize), Err(HashError::BlockTooLarge));

    // The rejected call consumed nothing, so a full block still fits.
    hasher
        .update(&oversize[..BLOCK_SIZE])
        .expect("full block fits");
    assert_eq!(hasher.update(&[0]), Err(HashError::BlockTooLarge));
}

#[test]
fn zero_length_file_has_empty_block_list_and_empty_rollup() {
    let (blocks, file) = FileHasher::new().finish();

    assert!(blocks.is_empty());
    assert_eq!(file, rollup(&[]));
}

#[test]
fn finish_closes_a_pending_partial_block() {
    let mut hasher = FileHasher::new();
    hasher.update(b"abc").expect("fits");
    hasher.update(b"def").expect("fits");

    let (blocks, file) = hasher.finish();

    assert_eq!(blocks, vec![hash_block(b"abcdef")]);
    assert_eq!(file, rollup(&blocks));
}

#[test]
fn rollup_depends_on_block_order() {
    let a = hash_block(b"a");
    let b = hash_block(b"b");

    assert_ne!(rollup(&[a, b]), rollup(&[b, a]));
}
