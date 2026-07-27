//! Exponential backoff for capture-stream recovery (F-007.3, F-007.12).
//!
//! Pure and macOS-free, so the delay sequence is unit-tested without a live
//! ScreenCaptureKit stream. The point is bounded retries: a transient
//! reconfiguration recovers almost immediately, while a permanent failure
//! (a display that never comes back) settles onto `max` instead of spinning.

use std::time::Duration;

/// A doubling backoff clamped to `[min, max]`, reset after each success.
#[derive(Debug, Clone)]
pub(crate) struct Backoff {
    min: Duration,
    max: Duration,
    next: Duration,
}

impl Backoff {
    /// Starts a fresh sequence at `min`. `max` is the ceiling every later delay
    /// saturates at.
    pub(crate) const fn new(min: Duration, max: Duration) -> Self {
        Self { min, max, next: min }
    }

    /// Returns the delay to wait before the next attempt, then doubles the
    /// stored value for the attempt after, saturating at `max`.
    pub(crate) fn next_delay(&mut self) -> Duration {
        let delay = self.next;
        self.next = self.next.saturating_mul(2).min(self.max);
        delay
    }

    /// Resets to `min` after a successful attempt, so a later failure starts
    /// from the bottom rather than the last (large) delay.
    pub(crate) fn reset(&mut self) {
        self.next = self.min;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_millis(500);
    const MAX: Duration = Duration::from_secs(5);

    #[test]
    fn doubles_from_min_and_saturates_at_max() {
        let mut backoff = Backoff::new(MIN, MAX);
        let seq: Vec<_> = (0..7).map(|_| backoff.next_delay()).collect();
        assert_eq!(
            seq,
            vec![
                Duration::from_millis(500),
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4),
                Duration::from_secs(5), // 8s clamped to the 5s ceiling
                Duration::from_secs(5),
                Duration::from_secs(5),
            ]
        );
    }

    #[test]
    fn reset_returns_to_min() {
        let mut backoff = Backoff::new(MIN, MAX);
        backoff.next_delay();
        backoff.next_delay();
        backoff.next_delay();
        backoff.reset();
        assert_eq!(
            backoff.next_delay(),
            MIN,
            "after a success the next delay starts from min again"
        );
    }

    #[test]
    fn never_exceeds_max_even_after_many_failures() {
        let mut backoff = Backoff::new(MIN, MAX);
        for _ in 0..64 {
            assert!(backoff.next_delay() <= MAX);
        }
    }
}
