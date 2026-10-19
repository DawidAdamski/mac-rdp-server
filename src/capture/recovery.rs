//! Pacing for recovery failures (F-007.21, F-007.24).
//!
//! Recovery retries with backoff for as long as the stream stays unreachable,
//! and every attempt deserves a log line — the absence of one was half the
//! diagnosis time for the display-sleep freeze (F-007 §8c). But a display that
//! sleeps for an hour would then produce a warning every few seconds forever,
//! which is the flooding F-007.5 and AC-7 exist to prevent.
//!
//! So failures are paced: the first one is loud, repeats are quiet, and once
//! per interval a loud line says how many were swallowed in between. The same
//! bargain the rejection log already strikes, applied to recovery.
//!
//! Pure — no ScreenCaptureKit, no clock of its own — so all of it is unit
//! tested without a capture session.

use std::time::{Duration, Instant};

/// How a single failed recovery attempt should be reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Report {
    /// Worth a warning: the first failure, or the first one after `interval`.
    Loud {
        /// Failures swallowed since the previous loud line. Belongs in that
        /// line, so the log never implies fewer attempts than really happened.
        suppressed: u64,
    },
    /// A repeat of something already reported; debug level is enough.
    Quiet,
}

/// Decides, for each failed recovery attempt, whether to say it out loud.
#[derive(Debug)]
pub(crate) struct FailureReporter {
    interval: Duration,
    last_loud: Option<Instant>,
    suppressed: u64,
}

impl FailureReporter {
    pub(crate) const fn new(interval: Duration) -> Self {
        Self {
            interval,
            last_loud: None,
            suppressed: 0,
        }
    }

    /// Records one failure and says how to report it.
    ///
    /// The first failure after construction or [`reset`](Self::reset) is always
    /// loud: a recovery that starts failing is news, and waiting out an
    /// interval before mentioning it would reproduce the silence this exists to
    /// end.
    pub(crate) fn observe(&mut self, now: Instant) -> Report {
        let due = self
            .last_loud
            .is_none_or(|last| now.saturating_duration_since(last) >= self.interval);

        if due {
            let suppressed = self.suppressed;
            self.suppressed = 0;
            self.last_loud = Some(now);
            Report::Loud { suppressed }
        } else {
            self.suppressed = self.suppressed.saturating_add(1);
            Report::Quiet
        }
    }

    /// Forgets the history, so the next failure is loud again.
    ///
    /// Called after a successful rebuild: the next failure is a *new* fault,
    /// not a continuation of the one that just healed.
    pub(crate) fn reset(&mut self) {
        self.last_loud = None;
        self.suppressed = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTERVAL: Duration = Duration::from_secs(30);

    fn reporter() -> FailureReporter {
        FailureReporter::new(INTERVAL)
    }

    #[test]
    fn f_007_21_the_first_failure_is_always_loud() {
        let mut r = reporter();
        assert_eq!(
            r.observe(Instant::now()),
            Report::Loud { suppressed: 0 },
            "a recovery that starts failing must be visible immediately"
        );
    }

    #[test]
    fn f_007_21_repeats_inside_the_interval_are_quiet() {
        let mut r = reporter();
        let t0 = Instant::now();
        r.observe(t0);
        for n in 1..10 {
            assert_eq!(
                r.observe(t0 + Duration::from_secs(n)),
                Report::Quiet,
                "a display asleep for minutes must not fill the log"
            );
        }
    }

    #[test]
    fn f_007_21_the_next_loud_line_carries_the_suppressed_count() {
        let mut r = reporter();
        let t0 = Instant::now();
        r.observe(t0);
        for n in 1..=5 {
            r.observe(t0 + Duration::from_secs(n));
        }
        assert_eq!(
            r.observe(t0 + INTERVAL),
            Report::Loud { suppressed: 5 },
            "the loud line must not imply fewer attempts than really happened"
        );
    }

    #[test]
    fn f_007_21_a_loud_line_resets_the_count() {
        let mut r = reporter();
        let t0 = Instant::now();
        r.observe(t0);
        r.observe(t0 + Duration::from_secs(1));
        assert_eq!(r.observe(t0 + INTERVAL), Report::Loud { suppressed: 1 });
        assert_eq!(
            r.observe(t0 + INTERVAL + INTERVAL),
            Report::Loud { suppressed: 0 },
            "counts must not accumulate across loud lines"
        );
    }

    #[test]
    fn f_007_24_success_makes_the_next_failure_loud_again() {
        let mut r = reporter();
        let t0 = Instant::now();
        r.observe(t0);
        r.observe(t0 + Duration::from_secs(1));
        r.reset();
        assert_eq!(
            r.observe(t0 + Duration::from_secs(2)),
            Report::Loud { suppressed: 0 },
            "a fault after a healed one is new, not a continuation"
        );
    }

    #[test]
    fn f_007_24_a_clock_that_does_not_advance_still_terminates() {
        // `Instant` arithmetic saturates rather than panicking, and the
        // reporter must stay well behaved when every observation lands on the
        // same instant — which is what a coarse clock under load looks like.
        let mut r = reporter();
        let t0 = Instant::now();
        assert_eq!(r.observe(t0), Report::Loud { suppressed: 0 });
        for _ in 0..100 {
            assert_eq!(r.observe(t0), Report::Quiet);
        }
    }
}
