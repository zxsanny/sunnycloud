//! The in-process `ThumbNotifier`: one broadcast channel for the whole process. Every realtime socket subscribes
//! and keeps the events of its own account, so there is no per-account map to maintain; a socket that falls
//! behind the channel is told it lagged and resets its client.

use std::sync::Arc;

use contracts::{ThumbEvent, ThumbNotifier, ThumbSubscription, ThumbWake, async_trait};
use tokio::sync::broadcast::{self, error::RecvError};

/// Events kept for a slow subscriber before it is told it lagged.
pub const THUMB_EVENT_CAPACITY: usize = 1_024;

pub struct InProcessThumbNotifier {
    sender: broadcast::Sender<ThumbEvent>,
}

impl InProcessThumbNotifier {
    pub fn new() -> Arc<Self> {
        Self::with_capacity(THUMB_EVENT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Arc<Self> {
        Arc::new(InProcessThumbNotifier {
            sender: broadcast::channel(capacity).0,
        })
    }
}

impl ThumbNotifier for InProcessThumbNotifier {
    /// No subscriber is not an error: nobody is connected to hear it.
    fn notify(&self, event: ThumbEvent) {
        let _delivered = self.sender.send(event).is_ok();
    }

    fn subscribe(&self) -> Box<dyn ThumbSubscription> {
        Box::new(Subscription {
            receiver: self.sender.subscribe(),
        })
    }
}

struct Subscription {
    receiver: broadcast::Receiver<ThumbEvent>,
}

#[async_trait]
impl ThumbSubscription for Subscription {
    async fn next(&mut self) -> ThumbWake {
        match self.receiver.recv().await {
            Ok(event) => ThumbWake::Event(event),
            Err(RecvError::Lagged(_)) => ThumbWake::Lagged,
            // The notifier is gone: nothing will ever be announced again.
            Err(RecvError::Closed) => std::future::pending().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use contracts::ThumbState;
    use sc_ids::FileHash;
    use uuid::Uuid;

    use super::*;

    fn event(n: u8) -> ThumbEvent {
        ThumbEvent {
            account: Uuid::from_u128(1),
            content_hash: FileHash::from_bytes([n; 32]),
            state: ThumbState::Ready,
        }
    }

    #[tokio::test]
    async fn every_subscriber_sees_the_events_announced_after_it_subscribed() {
        // Arrange
        let notifier = InProcessThumbNotifier::new();
        notifier.notify(event(0));
        let (mut one, mut two) = (notifier.subscribe(), notifier.subscribe());

        // Act
        notifier.notify(event(1));
        notifier.notify(event(2));

        // Assert
        assert_eq!(one.next().await, ThumbWake::Event(event(1)));
        assert_eq!(one.next().await, ThumbWake::Event(event(2)));
        assert_eq!(two.next().await, ThumbWake::Event(event(1)));
    }

    #[tokio::test]
    async fn a_subscriber_that_falls_behind_the_capacity_is_told_it_lagged_then_sees_the_newest() {
        // Arrange
        let notifier = InProcessThumbNotifier::with_capacity(2);
        let mut slow = notifier.subscribe();

        // Act
        for n in 0..5 {
            notifier.notify(event(n));
        }
        let first = slow.next().await;
        let after = slow.next().await;

        // Assert
        assert_eq!(first, ThumbWake::Lagged);
        assert_eq!(after, ThumbWake::Event(event(3)));
    }
}
