//! AZ-3789: the composition root built for `serve` against PostgreSQL 18 and a blob volume. AC-2 (late lists in
//! their documented order), AC-3 (one limiter, one scheduler), AC-4 (only what a command needs, each service
//! once), AC-6 (no vault item in a default build; the crate-graph half is in `crate_graph`). AZ-4311 AC-3 (03's
//! commit effects hold the one shared media effect).

mod root_support;

use std::any::type_name;
use std::sync::Arc;

use contracts::{MediaAdmin, Scrubber};
use media_api::MediaAdminService;
use sc_ratelimit::AbuseLimiter;
use sunnycloud_blobstore::MasterKeyring;
use sunnycloud_server::root::{
    AccountHooks, Admin, BlobCleanups, Blobstore, CommitEffects, DoctorChecks, Feed,
    HealthContributors, Identity, Media, MediaPlanning, Quota, ReadinessChecks, Sharing, Transfer,
    Tree,
};

use root_support::{Root, address};

/// 06, (20 in a `vault` build), 02, 03, 08, 05, 07a: each entry is the owner's own hook instance.
#[tokio::test]
async fn hook_list_follows_the_documented_order_and_reaches_the_owners_hooks() {
    // Arrange
    let root = Root::new().await;

    // Act
    let hooks = root
        .services
        .get::<AccountHooks>()
        .await
        .expect("hook list");

    // Assert
    let services = &root.services;
    let sharing = services.get::<Sharing>().await.expect("06");
    let tree = services.get::<Tree>().await.expect("02");
    let transfer = services.get::<Transfer>().await.expect("03");
    let media = services.get::<Media>().await.expect("08");
    let feed = services.get::<Feed>().await.expect("05");
    let quota = services.get::<Quota>().await.expect("07a");
    let mut expected = vec![("06", address(&sharing.lifecycle))];
    #[cfg(feature = "vault")]
    {
        let vault = services
            .get::<sunnycloud_server::root::Vault>()
            .await
            .expect("20");
        expected.push(("20", address(&vault.lifecycle)));
    }
    expected.extend([
        ("02", address(&tree.lifecycle)),
        ("03", address(&transfer.lifecycle)),
        ("08", address(&media.lifecycle)),
        ("05", address(&feed.lifecycle)),
        ("07a", address(&quota.admin)),
    ]);
    let actual: Vec<_> = hooks
        .0
        .iter()
        .map(|hook| (hook.owner, address(&hook.item)))
        .collect();
    assert_eq!(actual, expected);
    root.finish().await;
}

#[tokio::test]
async fn cleanup_list_is_02_then_08() {
    // Arrange
    let root = Root::new().await;

    // Act
    let cleanups = root.services.get::<BlobCleanups>().await.expect("cleanups");

    // Assert
    let tree = root.services.get::<Tree>().await.expect("02");
    let media = root.services.get::<Media>().await.expect("08");
    let actual: Vec<_> = cleanups
        .0
        .iter()
        .map(|entry| (entry.owner, address(&entry.item)))
        .collect();
    assert_eq!(
        actual,
        [
            ("02", address(&tree.lifecycle)),
            ("08", address(&media.lifecycle))
        ]
    );
    root.finish().await;
}

/// AZ-4311 AC-3: one 08 media effect for every account, and 03 is built with that instance.
#[tokio::test]
async fn transfer_commit_effects_hold_the_one_shared_media_effect() {
    // Arrange
    let root = Root::new().await;
    let effects = root
        .services
        .get::<CommitEffects>()
        .await
        .expect("commit effects");
    let media = Arc::clone(&effects.0[0].item);
    let before = Arc::strong_count(&media);

    // Act
    root.services.get::<Transfer>().await.expect("03");

    // Assert
    let owners: Vec<_> = effects.0.iter().map(|entry| entry.owner).collect();
    assert_eq!(owners, ["08"]);
    assert!(
        Arc::strong_count(&media) > before,
        "03 holds the list's media effect"
    );
    let again = root
        .services
        .get::<CommitEffects>()
        .await
        .expect("commit effects");
    assert!(Arc::ptr_eq(&effects, &again));
    root.finish().await;
}

#[tokio::test]
async fn health_and_doctor_lists_hold_the_owners_items() {
    // Arrange
    let root = Root::new().await;

    // Act
    let health = root
        .services
        .get::<HealthContributors>()
        .await
        .expect("health");
    let doctor = root.services.get::<DoctorChecks>().await.expect("doctor");

    // Assert
    let health: Vec<_> = health.0.iter().map(|item| item.name().to_owned()).collect();
    let doctor: Vec<_> = doctor.0.iter().map(|check| check.name()).collect();
    assert_eq!(health, ["blobstore", "media"]);
    assert_eq!(
        doctor,
        [
            "database",
            "master_key",
            "blob_volume",
            "blob_retention",
            "blob_files_missing",
            "quota_drift",
            "media_worker",
            "identity_providers",
            "identity_setup"
        ]
    );
    root.finish().await;
}

#[tokio::test]
async fn identity_and_sharing_hold_the_one_limiter_of_the_context() {
    // Arrange
    let root = Root::new().await;
    let limiter = root.services.get::<AbuseLimiter>().await.expect("limiter");
    let before = Arc::strong_count(&limiter);

    // Act
    root.services.get::<Identity>().await.expect("01");
    let after_identity = Arc::strong_count(&limiter);
    root.services.get::<Sharing>().await.expect("06");
    let after_sharing = Arc::strong_count(&limiter);

    // Assert
    assert!(after_identity > before, "01 holds the context's limiter");
    assert!(
        after_sharing > after_identity,
        "06 holds the context's limiter"
    );
    let again = root.services.get::<AbuseLimiter>().await.expect("limiter");
    assert!(Arc::ptr_eq(&limiter, &again));
    root.finish().await;
}

#[tokio::test]
async fn key_check_builds_the_keyring_and_nothing_of_another_component() {
    // Arrange
    let root = Root::new().await;

    // Act
    root.services.get::<MasterKeyring>().await.expect("keyring");

    // Assert
    let mut expected = vec![
        type_name::<sc_config::ConfigSources>(),
        type_name::<sunnycloud_server::ServeSettings>(),
        type_name::<sc_db::Db>(),
        type_name::<sc_audit::AuditWriter>(),
        type_name::<dyn sc_clock::Clock>(),
        type_name::<MasterKeyring>(),
    ];
    expected.sort_unstable();
    assert_eq!(root.services.built(), expected);
    root.finish().await;
}

#[tokio::test]
async fn media_admin_builds_without_the_tree_or_the_blobs_and_media_holds_the_same_instance() {
    // Arrange
    let root = Root::new().await;
    let services = &root.services;

    // Act
    let admin = services.get::<dyn MediaAdmin>().await.expect("media admin");
    let stats = admin.stats().await.expect("stats on an empty queue");

    // Assert
    let built = services.built();
    assert!(stats.counts.iter().all(|item| item.count == 0));
    assert!(!built.contains(&type_name::<Tree>()));
    assert!(!built.contains(&type_name::<Blobstore>()));
    let media = services.get::<Media>().await.expect("08");
    assert!(Arc::ptr_eq(
        &media.admin,
        &services
            .get::<MediaAdminService>()
            .await
            .expect("admin service")
    ));
    root.finish().await;
}

#[tokio::test]
async fn the_scrubber_service_is_the_instance_04_holds() {
    // Arrange
    let root = Root::new().await;

    // Act
    let scrubber = root.services.get::<dyn Scrubber>().await.expect("scrubber");
    let blobs = root.services.get::<Blobstore>().await.expect("04");

    // Assert
    assert!(std::ptr::addr_eq(
        Arc::as_ptr(&scrubber),
        Arc::as_ptr(&blobs.scrubber)
    ));
    root.finish().await;
}

#[tokio::test]
async fn serve_builds_every_component_once_although_several_consumers_receive_it() {
    // Arrange
    let root = Root::new().await;
    let services = &root.services;

    // Act
    services.get::<ReadinessChecks>().await.expect("readiness");
    services.get::<DoctorChecks>().await.expect("doctor");
    services.get::<Admin>().await.expect("07");
    let tree = services.get::<Tree>().await.expect("02");
    let blobs = services.get::<Blobstore>().await.expect("04");

    // Assert: every later request answers the instance the consumers were built with.
    let planning = services.get::<MediaPlanning>().await.expect("08 planning");
    let media = services.get::<Media>().await.expect("08");
    let hooks = services.get::<AccountHooks>().await.expect("hooks");
    let cleanups = services.get::<BlobCleanups>().await.expect("cleanups");
    assert!(Arc::ptr_eq(
        &tree,
        &services.get::<Tree>().await.expect("02")
    ));
    assert!(Arc::ptr_eq(
        &blobs,
        &services.get::<Blobstore>().await.expect("04")
    ));
    let hook_of = |owner| {
        let hook = hooks
            .0
            .iter()
            .find(|hook| hook.owner == owner)
            .expect("hook");
        address(&hook.item)
    };
    assert_eq!(hook_of("02"), address(&cleanups.0[0].item));
    assert_eq!(hook_of("08"), address(&cleanups.0[1].item));
    assert_eq!(hook_of("08"), address(&media.lifecycle));
    assert!(Arc::ptr_eq(
        &planning,
        &services.get::<MediaPlanning>().await.expect("08")
    ));
    let built = services.built();
    let mut unique = built.clone();
    unique.dedup();
    assert_eq!(built, unique);
    root.finish().await;
}

#[tokio::test]
async fn the_keyring_port_is_the_keyring_04_holds_and_the_blob_services_share_it() {
    // Arrange
    let root = Root::new().await;

    // Act
    let port = root
        .services
        .get::<dyn contracts::MasterKeyring>()
        .await
        .expect("keyring port");
    let blobs = root.services.get::<Blobstore>().await.expect("04");

    // Assert
    assert!(std::ptr::addr_eq(
        Arc::as_ptr(&port),
        Arc::as_ptr(&blobs.keyring)
    ));
    root.finish().await;
}

#[tokio::test]
async fn the_pre_flight_list_holds_every_pre_flight_tagged_check_of_the_doctor_list() {
    // Arrange
    let root = Root::new().await;

    // Act
    let doctor = root.services.get::<DoctorChecks>().await.expect("doctor");
    let early = root
        .services
        .get::<sunnycloud_server::root::PreFlightChecks>()
        .await
        .expect("pre-flight");

    // Assert
    let mut tagged: Vec<_> = doctor
        .0
        .iter()
        .filter(|check| check.tags().contains(&contracts::DoctorTag::PreFlight))
        .map(|check| check.name())
        .collect();
    let mut listed: Vec<_> = early.0.iter().map(|check| check.name()).collect();
    tagged.sort_unstable();
    listed.sort_unstable();
    assert_eq!(listed, tagged);
    root.finish().await;
}

#[tokio::test]
async fn the_operator_and_invite_ports_are_the_identity_invites_04_holds() {
    // Arrange
    let root = Root::new().await;

    // Act
    let ops = root
        .services
        .get::<dyn contracts::OperatorIdentityOps>()
        .await
        .expect("operator ops");
    let invites = root
        .services
        .get::<dyn contracts::InviteService>()
        .await
        .expect("invite service");
    let identity = root
        .services
        .get::<sunnycloud_server::root::Identity>()
        .await
        .expect("01");

    // Assert
    assert!(std::ptr::addr_eq(
        Arc::as_ptr(&ops),
        Arc::as_ptr(&identity.invites)
    ));
    assert!(std::ptr::addr_eq(
        Arc::as_ptr(&invites),
        Arc::as_ptr(&identity.invites)
    ));
    root.finish().await;
}
