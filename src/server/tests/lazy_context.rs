//! AC-7: services are built on first use, once per process.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use sunnycloud_server::{CliError, ServiceContext};
use support::Harness;

trait Clockwork: Send + Sync {
    fn tick(&self) -> u32;
}

struct Fixed;

impl Clockwork for Fixed {
    fn tick(&self) -> u32 {
        7
    }
}

struct Database;

fn counting_context(builds: &Arc<AtomicUsize>) -> ServiceContext {
    let counter = Arc::clone(builds);
    ServiceContext::builder()
        .register::<Database, _, _>(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            async { Ok(Arc::new(Database)) }
        })
        .finish()
}

#[test]
fn version_never_calls_a_failing_database_builder() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let harness = Harness::new()
        .real_handler()
        .service::<Database, _, _>(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            async { Err(CliError::Usage("database unreachable".into())) }
        });

    let result = harness.run(&["version"]);

    assert_eq!(result.exit, 0, "{}", result.stderr);
    assert!(result.stdout.contains("version: "), "{}", result.stdout);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_service_asked_for_twice_is_built_once() {
    let builds = Arc::new(AtomicUsize::new(0));
    let context = counting_context(&builds);

    let first = context.get::<Database>().await.expect("built");
    let second = context.get::<Database>().await.expect("shared");

    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(builds.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn concurrent_first_requests_build_once() {
    let builds = Arc::new(AtomicUsize::new(0));
    let context = counting_context(&builds);

    let (a, b, c) = tokio::join!(
        context.get::<Database>(),
        context.get::<Database>(),
        context.get::<Database>()
    );

    assert!(a.is_ok() && b.is_ok() && c.is_ok());
    assert_eq!(builds.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_builder_may_ask_for_another_service_and_a_trait_object_is_a_service() {
    let context = ServiceContext::builder()
        .register::<dyn Clockwork, _, _>(|_| async { Ok(Arc::new(Fixed) as Arc<dyn Clockwork>) })
        .register::<u32, _, _>(|services| async move {
            let clock = services.get::<dyn Clockwork>().await?;
            Ok(Arc::new(clock.tick() + 1))
        })
        .finish();

    assert_eq!(*context.get::<u32>().await.expect("built"), 8);
}

#[tokio::test]
async fn an_unregistered_service_is_an_error_and_a_failed_build_is_retried() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&attempts);
    let context = ServiceContext::builder()
        .register::<Database, _, _>(move |_| {
            let attempt = counter.fetch_add(1, Ordering::SeqCst);
            async move {
                if attempt == 0 {
                    Err(CliError::Usage("first attempt fails".into()))
                } else {
                    Ok(Arc::new(Database))
                }
            }
        })
        .finish();

    assert!(matches!(
        context.get::<String>().await,
        Err(CliError::NotRegistered(_))
    ));
    assert!(context.get::<Database>().await.is_err());
    assert!(context.get::<Database>().await.is_ok());
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
}
