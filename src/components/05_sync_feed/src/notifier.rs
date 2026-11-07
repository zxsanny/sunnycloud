use std::collections::HashMap;
use std::future::pending;
use std::sync::{Arc, Mutex, PoisonError};

use contracts::{FeedNotifier, Seq};
use tokio::sync::watch;
use uuid::Uuid;

struct Entry {
    sender: Arc<watch::Sender<Seq>>,
    waiters: usize,
}

/// The in-process wake of held reads: one watch channel per account that has at least one waiter, holding the
/// highest `seq` notified since the entry was created. An entry leaves the map with its last subscription, so
/// the map is as large as the number of accounts being waited on and `notify` for any other account touches
/// nothing but a lookup.
#[derive(Default)]
pub struct InProcessNotifier {
    entries: Mutex<HashMap<Uuid, Entry>>,
}

/// A registration for one account's wakes. Dropping it removes the account's entry when it was the last.
pub struct Subscription {
    notifier: Arc<InProcessNotifier>,
    account: Uuid,
    sender: Arc<watch::Sender<Seq>>,
    receiver: watch::Receiver<Seq>,
}

impl InProcessNotifier {
    pub fn new() -> Arc<Self> {
        Arc::default()
    }

    /// Registers before the caller's first read, so a commit between that read and the park is not lost. A
    /// notify made earlier is not seen: the first read covers it.
    pub fn subscribe(self: &Arc<Self>, account: Uuid) -> Subscription {
        let mut entries = self.entries();
        let entry = entries.entry(account).or_insert_with(|| Entry {
            sender: Arc::new(watch::channel(0).0),
            waiters: 0,
        });
        entry.waiters += 1;
        let sender = entry.sender.clone();
        let receiver = sender.subscribe();
        drop(entries);
        Subscription {
            notifier: self.clone(),
            account,
            sender,
            receiver,
        }
    }

    /// Accounts with at least one waiter.
    pub fn waiting_accounts(&self) -> usize {
        self.entries().len()
    }

    pub fn waiters(&self, account: Uuid) -> usize {
        self.entries()
            .get(&account)
            .map_or(0, |entry| entry.waiters)
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, Entry>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl FeedNotifier for InProcessNotifier {
    fn notify(&self, account: Uuid, seq: Seq) {
        if let Some(entry) = self.entries().get(&account) {
            entry
                .sender
                .send_modify(|notified| *notified = (*notified).max(seq));
        }
    }
}

impl Subscription {
    /// Completes when a notify arrived since the last completion (or since the subscription).
    pub async fn notified(&mut self) {
        if self.receiver.changed().await.is_err() {
            // Unreachable: this subscription keeps the sender alive. Pending, not a spin, if it ever happens.
            pending().await
        }
    }

    /// The highest `seq` notified for the account since its entry was created.
    pub fn notified_seq(&self) -> Seq {
        *self.sender.borrow()
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let mut entries = self.notifier.entries();
        if let Some(entry) = entries.get_mut(&self.account) {
            entry.waiters -= 1;
            if entry.waiters == 0 {
                entries.remove(&self.account);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCOUNT: Uuid = Uuid::from_u128(1);
    const OTHER: Uuid = Uuid::from_u128(2);

    #[test]
    fn the_entry_is_removed_when_the_last_waiter_leaves() {
        // Arrange
        let notifier = InProcessNotifier::new();
        let first = notifier.subscribe(ACCOUNT);
        let second = notifier.subscribe(ACCOUNT);

        // Act
        drop(first);
        let while_one_remains = notifier.waiting_accounts();
        drop(second);

        // Assert
        assert_eq!(while_one_remains, 1);
        assert_eq!(notifier.waiting_accounts(), 0);
        assert_eq!(notifier.waiters(ACCOUNT), 0);
    }

    #[test]
    fn a_notify_without_waiters_leaves_no_entry() {
        // Arrange
        let notifier = InProcessNotifier::new();

        // Act
        for index in 0..1000 {
            notifier.notify(Uuid::from_u128(index), 5);
        }

        // Assert
        assert_eq!(notifier.waiting_accounts(), 0);
    }

    #[test]
    fn subscriptions_leaving_from_many_threads_leave_no_entry_behind() {
        // Arrange
        let notifier = InProcessNotifier::new();

        // Act
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    for _ in 0..2000 {
                        drop((notifier.subscribe(ACCOUNT), notifier.subscribe(ACCOUNT)));
                    }
                });
            }
        });

        // Assert
        assert_eq!(notifier.waiting_accounts(), 0);
    }

    #[tokio::test]
    async fn a_notify_wakes_only_that_accounts_waiters_and_records_the_highest_seq() {
        // Arrange
        let notifier = InProcessNotifier::new();
        let mut waiting = [notifier.subscribe(ACCOUNT), notifier.subscribe(ACCOUNT)];
        let mut other = notifier.subscribe(OTHER);

        // Act
        notifier.notify(ACCOUNT, 7);
        notifier.notify(ACCOUNT, 4);

        // Assert
        for subscription in &mut waiting {
            subscription.notified().await;
            assert_eq!(subscription.notified_seq(), 7);
        }
        assert!(
            tokio::time::timeout(std::time::Duration::ZERO, other.notified())
                .await
                .is_err()
        );
        assert_eq!(other.notified_seq(), 0);
    }
}
