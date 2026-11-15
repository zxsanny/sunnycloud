//! AZ-3789 AC-1: `ApiServices` built for `serve` against PostgreSQL 18 and a blob volume holds, for every port 11
//! requires, the one instance the root's builders made for every other consumer (so none is a test double). AC-6:
//! no vault service in a default build.

mod root_support;

use std::sync::Arc;

use sc_clock::Clock;
use sc_observability::Metrics;
use sunnycloud_server::root::{
    self, AccountHooks, Admin, Feed, Identity, Media, MediaPlanning, ReadinessChecks, Sharing,
    Shutdown, Transfer, Tree,
};

use root_support::{Root, address};

#[tokio::test]
async fn every_port_is_the_owners_instance() {
    // Arrange
    let root = Root::new().await;
    let services = &root.services;

    // Act
    let api = root::api_services(services).await.expect("api services");

    // Assert
    let identity = services.get::<Identity>().await.expect("01");
    let tree = services.get::<Tree>().await.expect("02");
    let transfer = services.get::<Transfer>().await.expect("03");
    let feed = services.get::<Feed>().await.expect("05");
    let sharing = services.get::<Sharing>().await.expect("06");
    let admin = services.get::<Admin>().await.expect("07");
    let planning = services.get::<MediaPlanning>().await.expect("08 planning");
    let media = services.get::<Media>().await.expect("08");
    let ports = [
        ("authenticator", address(&api.authenticator)),
        ("principal_revalidator", address(&api.principal_revalidator)),
        ("sign_in", address(&api.sign_in)),
        ("sessions", address(&api.sessions)),
        ("device_grants", address(&api.device_grants)),
        ("invites", address(&api.invites)),
        ("tree", address(&api.tree)),
        ("tree_reader", address(&api.tree_reader)),
        ("uploads", address(&api.uploads)),
        ("commits", address(&api.commits)),
        ("downloads", address(&api.downloads)),
        ("change_feed", address(&api.change_feed)),
        ("shares", address(&api.shares)),
        ("public_shares", address(&api.public_shares)),
        ("admin", address(&api.admin)),
        ("accounts", address(&api.accounts)),
        ("account_deletion", address(&api.account_deletion)),
        ("thumbnail_states", address(&api.thumbnail_states)),
        ("derived", address(&api.derived)),
        ("worker_auth", address(&api.worker_auth)),
        ("worker_jobs", address(&api.worker_jobs)),
    ];
    let owners = [
        ("authenticator", address(&identity.authenticator)),
        ("principal_revalidator", address(&identity.authenticator)),
        ("sign_in", address(&identity.sign_in)),
        ("sessions", address(&identity.sessions)),
        ("device_grants", address(&identity.device_grants)),
        ("invites", address(&identity.invites)),
        ("tree", address(&tree.service)),
        ("tree_reader", address(&tree.reader)),
        ("uploads", address(&transfer.service)),
        ("commits", address(&transfer.service)),
        ("downloads", address(&transfer.service)),
        ("change_feed", address(&feed.change_feed)),
        ("shares", address(&sharing.service)),
        ("public_shares", address(&sharing.service)),
        ("admin", address(&admin.admin)),
        ("accounts", address(&admin.accounts)),
        ("account_deletion", address(&admin.deletion)),
        ("thumbnail_states", address(&planning.thumbnails)),
        ("derived", address(&media.derived)),
        ("worker_auth", address(&media.worker_auth)),
        ("worker_jobs", address(&media.worker_jobs)),
    ];
    assert_eq!(ports, owners);
    root.finish().await;
}

#[tokio::test]
async fn runtime_handles_are_the_contexts_own() {
    // Arrange
    let root = Root::new().await;
    let services = &root.services;

    // Act
    let api = root::api_services(services).await.expect("api services");

    // Assert
    let readiness = services.get::<ReadinessChecks>().await.expect("readiness");
    let metrics = services.get::<Metrics>().await.expect("metrics");
    let clock = services.get::<dyn Clock>().await.expect("clock");
    let shutdown = services.get::<Shutdown>().await.expect("shutdown");
    let checks: Vec<_> = api.readiness.iter().map(address).collect();
    let registered: Vec<_> = readiness.0.iter().map(address).collect();
    assert_eq!(checks, registered);
    assert!(Arc::ptr_eq(&api.metrics, &metrics));
    assert_eq!(address(&api.clock), address(&clock));
    assert!(!api.feed_shutdown.is_cancelled());
    shutdown.feed.cancel();
    assert!(
        api.feed_shutdown.is_cancelled(),
        "11 holds the token 05's ChangeFeed was built with"
    );
    root.finish().await;
}

/// The hook list reaches 01's account creation before 11 receives a port, and assembling it twice builds nothing
/// new.
#[tokio::test]
async fn the_hook_list_is_built_with_the_api_services() {
    // Arrange
    let root = Root::new().await;
    let services = &root.services;

    // Act
    root::api_services(services).await.expect("api services");
    let built = services.built();
    root::api_services(services)
        .await
        .expect("api services again");

    // Assert
    assert!(built.contains(&std::any::type_name::<AccountHooks>()));
    assert_eq!(services.built(), built);
    root.finish().await;
}

#[cfg(not(feature = "vault"))]
#[tokio::test]
async fn a_default_build_holds_no_vault_service() {
    // Arrange
    let root = Root::new().await;

    // Act
    let api = root::api_services(&root.services)
        .await
        .expect("api services");

    // Assert
    assert!(api.vault.is_none());
    root.finish().await;
}

#[cfg(feature = "vault")]
#[tokio::test]
async fn a_vault_build_holds_the_vault_service_of_20() {
    // Arrange
    let root = Root::new().await;

    // Act
    let api = root::api_services(&root.services)
        .await
        .expect("api services");

    // Assert
    let vault = root
        .services
        .get::<sunnycloud_server::root::Vault>()
        .await
        .expect("20");
    let held = api.vault.as_ref().expect("a vault service");
    assert_eq!(address(held), address(&vault.service));
    root.finish().await;
}
