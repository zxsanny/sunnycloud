//! AZ-3654 AC-5, AZ-3664 AC-6, AZ-3662 AC-5, AZ-3665 AC-5 and AZ-3661 AC-5: every operation of the first-run flow (F1),
//! the sign-in flow (F2), the share-link flow (F11), the account-deletion flow (F13) and the media flow (F10) has a
//! production caller, and the abuse limiter is built once. The search covers the non-test sources of the workspace;
//! a caller that exists only in a test (a `tests` directory or `tests.rs`) does not count.

use std::fs;
use std::path::{Path, PathBuf};

/// (operation, the call as it is written at the caller, the directory under `src/` where its caller lives).
const FIRST_RUN: [(&str, &str, &str); 7] = [
    (
        "Migrator::run_pending",
        "Migrator::run_pending(",
        "server/src",
    ),
    (
        "Migrator::check_version",
        "Migrator::check_version(",
        "server/src",
    ),
    (
        "OperatorIdentityOps::ensure_setup_token",
        ".ensure_setup_token(",
        "server/src",
    ),
    (
        "OperatorIdentityOps::create_admin_invite",
        ".create_admin_invite(",
        "server/src",
    ),
    (
        "SignInService::list_providers",
        ".list_providers(",
        "components/11_http_api/src",
    ),
    (
        "SignInService::begin",
        ".begin(",
        "components/11_http_api/src/registry/signin",
    ),
    (
        "SignInService::complete",
        ".complete(",
        "components/11_http_api/src/registry/signin",
    ),
];

/// The sign-in flow (F2): the routes of 11 call 01's services, 12 registers the hooks and builds the one limiter.
const SIGN_IN: [(&str, &str, &str); 10] = [
    (
        "SignInService::exchange_mobile_code",
        ".exchange_mobile_code(",
        "components/11_http_api/src/registry/signin",
    ),
    (
        "SignInService::begin_link",
        ".begin_link(",
        "components/11_http_api/src/registry/signin",
    ),
    (
        "SignInService::complete_link",
        ".complete_link(",
        "components/11_http_api/src/registry/signin",
    ),
    (
        "SessionService::refresh",
        ".refresh(",
        "components/11_http_api/src/registry/signin",
    ),
    (
        "SessionService::revoke",
        ".revoke(",
        "components/11_http_api/src/registry/sessions",
    ),
    (
        "Authenticator::authenticate",
        ".authenticate(",
        "components/11_http_api/src/auth",
    ),
    (
        "Authenticator::require",
        ".require(",
        "components/11_http_api/src/auth",
    ),
    (
        "InviteService::create_invite",
        ".create_invite(",
        "server/src",
    ),
    (
        "AccountLifecycleHook::initialize",
        "hook.initialize(",
        "components/01_identity/src",
    ),
    ("the hook list of 12", "AccountHooks", "server/src/root"),
];

/// The share-link flow (F11): the owner routes and the public routes of 11 call 06 and the download service of 03.
const SHARE_LINKS: [(&str, &str, &str); 7] = [
    (
        "ShareService::create",
        ".create(",
        "components/11_http_api/src/registry/sharing",
    ),
    (
        "ShareService::list",
        ".list(",
        "components/11_http_api/src/registry/sharing",
    ),
    (
        "ShareService::revoke",
        ".revoke(",
        "components/11_http_api/src/registry/sharing",
    ),
    (
        "PublicShareService::open",
        ".open(",
        "components/11_http_api/src/registry/sharing",
    ),
    (
        "PublicShareService::verify_password",
        ".verify_password(",
        "components/11_http_api/src/registry/sharing",
    ),
    (
        "PublicShareService::authorize_content",
        ".authorize_content(",
        "components/11_http_api/src/registry/sharing",
    ),
    (
        "DownloadService from the public content route",
        ".open_revision_content(",
        "components/11_http_api/src/registry/sharing",
    ),
];

/// The account-deletion flow (F13): the route and `admin user delete` ask, 12 schedules the purger and the sweeper, and
/// 07's purge steps begin and finalize the deletion.
const DELETION: [(&str, &str, &str); 6] = [
    (
        "AccountDeletionService::request (route)",
        ".request(",
        "components/11_http_api/src/registry/account",
    ),
    (
        "AccountDeletionService::request_as_operator (admin user delete)",
        ".request_as_operator(",
        "server/src",
    ),
    (
        "DeletionPurger on the scheduler",
        "account_delete_interval",
        "server/src/root",
    ),
    (
        "AccountAdmin::begin_deletion",
        ".begin_deletion(",
        "components/07_admin/src",
    ),
    (
        "AccountAdmin::finalize_deletion",
        ".finalize_deletion(",
        "components/07_admin/src",
    ),
    (
        "GcSweeper on the scheduler",
        "gc_interval",
        "server/src/root",
    ),
];

/// The media flow (F10, second half): the worker route stores outputs through 08, 02's listing and 11's routes read the
/// derived state and files through 03, 12 schedules the derived janitor, and the supervisor's `main` builds the slot loop
/// with the real API client and the real decoder client.
const MEDIA: [(&str, &str, &str); 9] = [
    (
        "WorkerJobs::accept_output (internal route)",
        ".accept_output(",
        "components/11_http_api/src/registry/worker",
    ),
    (
        "DerivedStore::accept_output (from WorkerJobs)",
        ".accept_output(",
        "components/08_media_jobs/src",
    ),
    (
        "ThumbnailStateSource::states (02 listing)",
        ".states(",
        "components/02_tree/src",
    ),
    (
        "ThumbnailStateSource::states (11 route)",
        ".states(",
        "components/11_http_api/src/registry/media",
    ),
    (
        "DerivedOpener::open_derived (thumbnail and transcode routes)",
        ".open_derived(",
        "components/11_http_api/src/registry/media",
    ),
    (
        "DerivedOpener::open_derived_for (share thumbnail)",
        ".open_derived_for(",
        "components/11_http_api/src/registry/sharing",
    ),
    (
        "DownloadService::open_blob_content (thumbnail and transcode routes)",
        ".open_blob_content(",
        "components/11_http_api/src/registry/media",
    ),
    (
        "DerivedJanitor on the scheduler",
        "DerivedJanitor::new(",
        "server/src/root",
    ),
    (
        "the supervisor's slot loop with the real API client",
        "WorkerApiClient::new(",
        "worker/src",
    ),
];

fn production_sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|error| panic!("read {}: {error}", dir.display()))
    {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            // Test code does not count as a caller, and build output is not source.
            if !path
                .file_name()
                .is_some_and(|name| name == "tests" || name == "target")
            {
                production_sources(&path, found);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && !path.file_name().is_some_and(|name| name == "tests.rs")
        {
            found.push(path);
        }
    }
}

fn missing_callers(table: &[(&'static str, &str, &str)]) -> Vec<&'static str> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    table
        .iter()
        .filter(|(_, call, dir)| {
            let mut files = Vec::new();
            production_sources(&src.join(dir), &mut files);
            !files.iter().any(|file| {
                fs::read_to_string(file)
                    .unwrap_or_else(|error| panic!("read {}: {error}", file.display()))
                    .contains(call)
            })
        })
        .map(|(name, _, _)| *name)
        .collect()
}

#[test]
fn every_operation_of_the_first_run_flow_has_a_production_caller() {
    // Arrange and Act
    let missing = missing_callers(&FIRST_RUN);

    // Assert
    assert!(missing.is_empty(), "no production caller for {missing:?}");
}

#[test]
fn every_operation_of_the_sign_in_flow_has_a_production_caller() {
    // Arrange and Act
    let missing = missing_callers(&SIGN_IN);

    // Assert
    assert!(missing.is_empty(), "no production caller for {missing:?}");
}

#[test]
fn every_operation_of_the_share_link_flow_has_a_production_caller() {
    // Arrange and Act
    let missing = missing_callers(&SHARE_LINKS);

    // Assert
    assert!(missing.is_empty(), "no production caller for {missing:?}");
}

#[test]
fn every_operation_of_the_account_deletion_flow_has_a_production_caller() {
    // Arrange and Act
    let missing = missing_callers(&DELETION);

    // Assert
    assert!(missing.is_empty(), "no production caller for {missing:?}");
}

#[test]
fn every_operation_of_the_media_flow_has_a_production_caller() {
    // Arrange and Act
    let missing = missing_callers(&MEDIA);

    // Assert
    assert!(missing.is_empty(), "no production caller for {missing:?}");
}

#[test]
fn the_supervisor_main_runs_the_worker_and_the_worker_builds_slot_loops_over_real_decoder_slots() {
    // Arrange
    let worker = Path::new(env!("CARGO_MANIFEST_DIR")).join("../worker/src");
    let read = |name: &str| {
        fs::read_to_string(worker.join(name)).unwrap_or_else(|error| panic!("read {name}: {error}"))
    };

    // Act
    let (main, compose) = (read("main.rs"), read("compose.rs"));

    // Assert
    assert!(
        main.contains("sunnycloud_worker::execute("),
        "main does not start the worker"
    );
    assert!(compose.contains("SlotLoop::new("), "no slot loop is built");
    assert!(compose.contains("Slot::new("), "no decoder slot is built");
}

#[test]
fn the_abuse_limiter_is_constructed_once_and_in_12() {
    // Arrange
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    production_sources(&src, &mut files);

    // Act
    let builders: Vec<PathBuf> = files
        .into_iter()
        .filter(|file| {
            fs::read_to_string(file)
                .unwrap_or_else(|error| panic!("read {}: {error}", file.display()))
                .contains("AbuseLimiter::new(")
        })
        .collect();

    // Assert
    assert_eq!(builders.len(), 1, "{builders:?}");
    assert!(
        builders[0].ends_with("server/src/root/base.rs"),
        "{builders:?}"
    );
}
