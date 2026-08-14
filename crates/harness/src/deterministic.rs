use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use chrono::{DateTime, Duration, Utc};
use eas_mail_mcp::{Clock, IdGenerator};

/// Clock fixed at one instant for expiry and report tests.
#[derive(Debug, Clone)]
pub struct FixedClock {
    now: DateTime<Utc>,
}

impl FixedClock {
    /// Creates a fixed UTC clock.
    #[must_use]
    pub const fn new(now: DateTime<Utc>) -> Self {
        Self { now }
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.now
    }
}

/// Clock that tests can advance without sleeping.
#[derive(Debug, Clone)]
pub struct ManualClock {
    timestamp: Arc<AtomicI64>,
}

impl ManualClock {
    /// Creates a controllable UTC clock.
    #[must_use]
    pub fn new(now: DateTime<Utc>) -> Self {
        Self { timestamp: Arc::new(AtomicI64::new(now.timestamp())) }
    }

    /// Advances the current time by a deterministic duration.
    pub fn advance(&self, duration: Duration) {
        self.timestamp.fetch_add(duration.num_seconds(), Ordering::Relaxed);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> DateTime<Utc> {
        DateTime::from_timestamp(self.timestamp.load(Ordering::Relaxed), 0)
            .unwrap_or(DateTime::UNIX_EPOCH)
    }
}

/// Deterministic monotonically increasing reference generator.
#[derive(Debug, Default)]
pub struct SequenceIds {
    next: AtomicU64,
}

impl IdGenerator for SequenceIds {
    fn next(&self) -> String {
        format!("{:016x}", self.next.fetch_add(1, Ordering::Relaxed))
    }
}
