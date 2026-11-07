//! AC-3: the services and the purger take the ports and lists through constructors, keep the list order, and
//! need no global or static registry.

mod support;

use std::fs;
use std::path::Path;
use std::sync::Arc;

use contracts::{AccountAdmin, AccountLifecycleHook, HealthContributor, QuotaAdmin, TreeReader};
use sc_audit::AuditWriter;
use sc_config::Environment;
use sc_testdb::{TestDb, clock_at_start};
use sunnycloud_admin::{
    AccountDeletionServiceImpl, AccountServiceImpl, AdminConfig, AdminServiceImpl, DeletionPurger,
};
use support::doubles::{
    AccountAdminDouble, HealthDouble, HookDouble, QuotaAdminDouble, TreeReaderDouble,
};
use support::settings::load_settings;

const PRODUCTION_HOOK_ORDER: [&str; 6] = ["06", "02", "03", "08", "05", "07a"];

fn config() -> AdminConfig {
    let settings = load_settings(Environment::Test, &[])
        .unwrap_or_else(|error| panic!("the test settings do not load: {error}"));
    AdminConfig::from_settings(&settings)
}

fn accounts() -> Arc<dyn AccountAdmin> {
    Arc::new(AccountAdminDouble)
}

fn quota() -> Arc<dyn QuotaAdmin> {
    Arc::new(QuotaAdminDouble)
}

fn hooks_in_order() -> Vec<Arc<dyn AccountLifecycleHook>> {
    PRODUCTION_HOOK_ORDER
        .iter()
        .map(|name| Arc::new(HookDouble(name)) as Arc<dyn AccountLifecycleHook>)
        .collect()
}

fn same_object<T: ?Sized>(left: &Arc<T>, right: &Arc<T>) -> bool {
    std::ptr::eq(
        Arc::as_ptr(left).cast::<()>(),
        Arc::as_ptr(right).cast::<()>(),
    )
}

#[test]
fn purger_keeps_the_hooks_in_the_order_given() {
    // Arrange
    let hooks = hooks_in_order();
    let expected = hooks.clone();

    // Act
    let purger = DeletionPurger::new(accounts(), hooks, config());

    // Assert
    assert_eq!(purger.hooks().len(), PRODUCTION_HOOK_ORDER.len());
    for (position, hook) in purger.hooks().iter().enumerate() {
        assert!(
            same_object(hook, &expected[position]),
            "hook at position {position} is not hook {}",
            PRODUCTION_HOOK_ORDER[position]
        );
    }
}

#[test]
fn purger_does_not_sort_a_reversed_list() {
    // Arrange
    let mut hooks = hooks_in_order();
    hooks.reverse();
    let expected = hooks.clone();

    // Act
    let purger = DeletionPurger::new(accounts(), hooks, config());

    // Assert
    for (position, hook) in purger.hooks().iter().enumerate() {
        assert!(same_object(hook, &expected[position]));
    }
}

#[tokio::test]
async fn admin_service_holds_its_ports_contributors_and_settings() {
    // Arrange
    let accounts = accounts();
    let quota = quota();
    let contributors: Vec<Arc<dyn HealthContributor>> = vec![
        Arc::new(HealthDouble("database")),
        Arc::new(HealthDouble("blob volume")),
        Arc::new(HealthDouble("janitor lag")),
    ];
    let expected = contributors.clone();
    let config = config();
    let database = TestDb::clone_template().await;
    let api = database.api().await;
    let audit = AuditWriter::new(api.clone(), clock_at_start());

    // Act
    let service = AdminServiceImpl::new(
        accounts.clone(),
        quota.clone(),
        contributors,
        config.clone(),
        audit,
        api,
    );

    // Assert
    assert!(same_object(service.accounts(), &accounts));
    assert!(same_object(service.quota(), &quota));
    assert_eq!(service.config(), &config);
    let names: Vec<&str> = service
        .health_contributors()
        .iter()
        .map(|contributor| contributor.name())
        .collect();
    assert_eq!(names, ["database", "blob volume", "janitor lag"]);
    for (position, contributor) in service.health_contributors().iter().enumerate() {
        assert!(same_object(contributor, &expected[position]));
    }
    drop(service);
    database.drop_db().await;
}

#[test]
fn account_service_holds_the_account_quota_and_tree_ports() {
    // Arrange
    let accounts = accounts();
    let quota = quota();
    let tree: Arc<dyn TreeReader> = Arc::new(TreeReaderDouble);

    // Act
    let service = AccountServiceImpl::new(accounts.clone(), quota.clone(), tree.clone());

    // Assert
    assert!(same_object(service.accounts(), &accounts));
    assert!(same_object(service.quota(), &quota));
    assert!(same_object(service.tree(), &tree));
}

#[test]
fn deletion_service_holds_the_account_port_and_settings() {
    // Arrange
    let accounts = accounts();
    let config = config();

    // Act
    let service = AccountDeletionServiceImpl::new(accounts.clone(), config.clone());

    // Assert
    assert!(same_object(service.accounts(), &accounts));
    assert_eq!(service.config(), &config);
}

/// A registry would be a process-wide `static`, a lazy cell or a thread-local.
#[test]
fn crate_source_has_no_global_or_static_registry() {
    // Arrange
    let source_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let banned_words = [
        "lazy_static",
        "once_cell",
        "OnceLock",
        "OnceCell",
        "LazyLock",
        "thread_local",
    ];

    // Act
    let mut offences = Vec::new();
    for entry in fs::read_dir(&source_dir).expect("src directory is readable") {
        let path = entry.expect("src entry is readable").path();
        let text = fs::read_to_string(&path).expect("source file is readable");
        for (index, line) in text.lines().enumerate() {
            let code = line.trim_start();
            let is_comment = code.starts_with("//");
            let declares_static = ["static ", "pub static ", "pub(crate) static "]
                .iter()
                .any(|prefix| code.starts_with(prefix));
            let uses_banned_word = banned_words.iter().any(|word| code.contains(word));
            if !is_comment && (declares_static || uses_banned_word) {
                offences.push(format!("{}:{}: {code}", path.display(), index + 1));
            }
        }
    }

    // Assert
    assert!(
        offences.is_empty(),
        "global state in the crate: {offences:#?}"
    );
}
