//! AC-3: the methods that join the caller's transaction take `&mut DbTx`, and nothing else does.
//!
//! A `DbTx` is handed out only by `sc_db::Db::run`, which needs a live database, so the calls themselves are
//! made by the component tests that own a database. This file proves the signatures: every listed method
//! accepts the caller's `&mut DbTx` through `&dyn Port`, and the source scan proves the list is exact.

use std::fs;
use std::path::Path;

use contracts::{
    AccountAdmin, AccountGuard, AccountLifecycleHook, AccountState, BlobOwnerCleanup, BlobPlan,
    BlobRegistry, CommitEffect, Due, EntryOutcome, FeedHorizon, FeedStamp, GcQueue, GcReason,
    PublishedEntry, QuotaLedger, RegisterState, Role, TouchedNodes, WrittenOrPlanned,
};
use sc_db::DbTx;
use uuid::Uuid;

/// Compiles only while each listed method takes `&mut DbTx`. Never called.
#[allow(dead_code)]
async fn joined_methods(
    tx: &mut DbTx,
    guard: &dyn AccountGuard,
    admin: &dyn AccountAdmin,
    ledger: &dyn QuotaLedger,
    hook: &dyn AccountLifecycleHook,
    cleanup: &dyn BlobOwnerCleanup,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let account = Uuid::nil();
    guard.ensure_active(tx, account).await?;
    admin.lock_active_admins(tx).await?;
    admin.set_role(tx, account, Role::User).await?;
    admin.set_state(tx, account, AccountState::Active).await?;
    admin.begin_deletion(tx, account).await?;
    admin.finalize_deletion(tx, account).await?;
    ledger.reserve(tx, account, 1).await?;
    ledger.release_reservation(tx, account, 1).await?;
    ledger.adjust_used(tx, account, 1).await?;
    hook.initialize(tx, account).await?;
    cleanup.remove_owner_rows(tx, account).await?;
    Ok(())
}

/// The part-2 methods that join the transaction. Never called.
#[allow(dead_code)]
async fn joined_methods_part_two(
    tx: &mut DbTx,
    stamp: &dyn FeedStamp,
    horizon: &dyn FeedHorizon,
    effect: &dyn CommitEffect,
    entry: &PublishedEntry,
    outcomes: &[EntryOutcome],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let account = Uuid::nil();
    stamp
        .stamp(tx, account, TouchedNodes(vec![account]))
        .await?;
    horizon.raise_purge_horizon(tx, account, 1).await?;
    effect.on_begin(tx).await?;
    effect.on_entry(tx, entry).await?;
    effect.on_batch(tx, outcomes).await?;
    Ok(())
}

/// The part-3 methods that join the transaction. Never called.
#[allow(dead_code)]
async fn joined_methods_part_three(
    tx: &mut DbTx,
    registry: &dyn BlobRegistry,
    queue: &dyn GcQueue,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let blob_id = Uuid::nil();
    let plan = BlobPlan {
        blob_id,
        wrapped_key: Vec::new(),
        master_key_id: 1,
    };
    registry
        .register_in_tx(
            tx,
            WrittenOrPlanned::Planned(plan),
            RegisterState::Uploading,
        )
        .await?;
    registry.mark_committed_in_tx(tx, blob_id, 1).await?;
    registry.reinstate_in_tx(tx, blob_id).await?;
    registry.remove_row_in_tx(tx, blob_id).await?;
    queue
        .schedule_in_tx(tx, blob_id, GcReason::Deleted, Due::Now)
        .await?;
    Ok(())
}

fn sources(dir: &Path, found: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("read src dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(fs::read_to_string(&path).expect("read source"));
        }
    }
}

/// `Trait::method` of every trait method whose signature mentions `&mut DbTx`.
fn methods_taking_a_transaction() -> Vec<String> {
    let mut files = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let mut found = Vec::new();
    for text in files {
        let mut current = String::new();
        let mut signature = String::new();
        for line in text.lines().map(str::trim) {
            if let Some(rest) = line.strip_prefix("pub trait ") {
                current = rest
                    .split([':', ' ', '{'])
                    .next()
                    .unwrap_or_default()
                    .to_owned();
            } else if line.starts_with("async fn ")
                || line.starts_with("fn ")
                || !signature.is_empty()
            {
                signature.push_str(line);
                signature.push(' ');
                if line.ends_with(';') {
                    if signature.contains("&mut DbTx") {
                        let name = signature
                            .trim_start_matches("async ")
                            .trim_start_matches("fn ")
                            .split(['(', '<'])
                            .next()
                            .unwrap_or_default();
                        found.push(format!("{current}::{name}"));
                    }
                    signature.clear();
                }
            }
        }
    }
    found.sort();
    found
}

#[test]
fn exactly_the_listed_methods_of_the_three_parts_take_a_transaction() {
    // Arrange
    let expected = [
        "AccountAdmin::begin_deletion",
        "AccountAdmin::finalize_deletion",
        "AccountAdmin::lock_active_admins",
        "AccountAdmin::set_role",
        "AccountAdmin::set_state",
        "AccountGuard::ensure_active",
        "AccountLifecycleHook::initialize",
        "BlobOwnerCleanup::remove_owner_rows",
        "BlobRegistry::mark_committed_in_tx",
        "BlobRegistry::register_in_tx",
        "BlobRegistry::reinstate_in_tx",
        "BlobRegistry::remove_row_in_tx",
        "CommitEffect::on_batch",
        "CommitEffect::on_begin",
        "CommitEffect::on_entry",
        "FeedHorizon::raise_purge_horizon",
        "FeedStamp::stamp",
        "GcQueue::schedule_in_tx",
        "QuotaLedger::adjust_used",
        "QuotaLedger::release_reservation",
        "QuotaLedger::reserve",
    ];

    // Act
    let found = methods_taking_a_transaction();

    // Assert
    assert_eq!(found, expected);
}

/// `Trait::method` of every trait method whose signature names `needle`.
fn methods_naming(needle: &str) -> Vec<String> {
    let mut files = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let mut found = Vec::new();
    for text in files {
        let mut current = String::new();
        let mut signature = String::new();
        for line in text.lines().map(str::trim) {
            if let Some(rest) = line.strip_prefix("pub trait ") {
                current = rest
                    .split([':', ' ', '{'])
                    .next()
                    .unwrap_or_default()
                    .to_owned();
            } else if line.starts_with("async fn ")
                || line.starts_with("fn ")
                || !signature.is_empty()
            {
                signature.push_str(line);
                signature.push(' ');
                if line.ends_with(';') {
                    if signature
                        .split(|c: char| !c.is_alphanumeric())
                        .any(|w| w == needle)
                    {
                        let name = signature
                            .trim_start_matches("async ")
                            .trim_start_matches("fn ")
                            .split(['(', '<'])
                            .next()
                            .unwrap_or_default();
                        found.push(format!("{current}::{name}"));
                    }
                    signature.clear();
                }
            }
        }
    }
    found.sort();
    found
}

#[test]
fn only_the_gc_queue_can_be_given_a_reason_to_write_the_gc_table() {
    // Arrange
    let expected = ["GcQueue::schedule_in_tx"];

    // Act
    let found = methods_naming("GcReason");

    // Assert
    assert_eq!(found, expected);
}
