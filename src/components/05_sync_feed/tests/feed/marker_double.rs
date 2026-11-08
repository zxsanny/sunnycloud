//! A double of the `EpochMarker` port: the file implementation is 04's and 05 may not depend on it.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use contracts::{EpochMarker, EpochMarkerError, async_trait};

/// Keeps the marker value and counts the writes; while `failing` is set every write is an I/O error.
#[derive(Default)]
pub struct MarkerDouble {
    value: Mutex<Option<i64>>,
    writes: Mutex<u32>,
    failing: AtomicBool,
}

impl MarkerDouble {
    pub fn failing() -> Self {
        MarkerDouble {
            failing: AtomicBool::new(true),
            ..MarkerDouble::default()
        }
    }

    pub fn holding(epoch: i64) -> Self {
        MarkerDouble {
            value: Mutex::new(Some(epoch)),
            ..MarkerDouble::default()
        }
    }

    pub fn set_failing(&self, failing: bool) {
        self.failing.store(failing, Ordering::SeqCst);
    }

    pub fn value(&self) -> Option<i64> {
        *self.value.lock().expect("marker lock")
    }

    pub fn writes(&self) -> u32 {
        *self.writes.lock().expect("marker lock")
    }
}

#[async_trait]
impl EpochMarker for MarkerDouble {
    async fn write(&self, epoch: i64) -> Result<(), EpochMarkerError> {
        if self.failing.load(Ordering::SeqCst) {
            return Err(EpochMarkerError::Io(Box::new(std::io::Error::other(
                "marker write refused",
            ))));
        }
        *self.value.lock().expect("marker lock") = Some(epoch);
        *self.writes.lock().expect("marker lock") += 1;
        Ok(())
    }

    async fn read(&self) -> Result<Option<i64>, EpochMarkerError> {
        Ok(self.value())
    }
}
