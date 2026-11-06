use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::OnceCell;

use crate::error::CliError;

type Built = Arc<dyn Any + Send + Sync>;
type Build = Box<
    dyn Fn(ServiceContext) -> Pin<Box<dyn Future<Output = Result<Built, CliError>> + Send>>
        + Send
        + Sync,
>;

struct Slot {
    build: Build,
    built: OnceCell<Built>,
}

/// Services (or anything else a command needs), each built the first time a command asks for it and then
/// shared for the life of the process.
///
/// A builder runs at most once when it succeeds; a failing builder is retried by the next `get`. Builders
/// receive the context, so one service can ask for another. Builders must not ask for the service they build
/// (that would wait on itself).
#[derive(Clone, Default)]
pub struct ServiceContext {
    slots: Arc<HashMap<TypeId, Slot>>,
}

/// Collects the builders; `finish` freezes them into a [`ServiceContext`].
#[derive(Default)]
pub struct ContextBuilder {
    slots: HashMap<TypeId, Slot>,
}

impl ContextBuilder {
    /// Registers the builder of `T` (a type or a `dyn` trait); a later registration of the same `T` replaces it.
    pub fn register<T, F, Fut>(mut self, build: F) -> Self
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(ServiceContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Arc<T>, CliError>> + Send + 'static,
    {
        let build: Build = Box::new(move |context| {
            let future = build(context);
            Box::pin(async move { future.await.map(|service| Arc::new(service) as Built) })
        });
        self.slots.insert(
            TypeId::of::<T>(),
            Slot {
                build,
                built: OnceCell::new(),
            },
        );
        self
    }

    pub fn finish(self) -> ServiceContext {
        ServiceContext {
            slots: Arc::new(self.slots),
        }
    }
}

impl ServiceContext {
    pub fn builder() -> ContextBuilder {
        ContextBuilder::default()
    }

    /// The service `T`, built on first use.
    pub async fn get<T>(&self) -> Result<Arc<T>, CliError>
    where
        T: ?Sized + Send + Sync + 'static,
    {
        let slot = self
            .slots
            .get(&TypeId::of::<T>())
            .ok_or(CliError::NotRegistered(type_name::<T>()))?;
        let built = slot
            .built
            .get_or_try_init(|| (slot.build)(self.clone()))
            .await?;
        built
            .downcast_ref::<Arc<T>>()
            .cloned()
            .ok_or(CliError::NotRegistered(type_name::<T>()))
    }
}
