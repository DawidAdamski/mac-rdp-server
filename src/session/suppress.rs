//! When to pause sending and when to repaint the whole desktop (F-008.8).
//!
//! Pure state machine over successive readings of the client's "suppress
//! output" flag (MS-RDPBCGR 3.3.5.11.2). No IronRDP types and no clock of its
//! own: the caller passes `now`, so every rule below is unit-tested.

use std::time::{Duration, Instant};

/// Minimum spacing between two full refreshes (F-008.9).
pub const REFRESH_SPACING: Duration = Duration::from_secs(1);

/// What the display loop should do right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuppressState {
    /// Send updates as usual.
    Idle,
    /// The client asked for a pause: receive frames, encode nothing (F-008.4).
    Suppressed,
    /// Repaint the whole desktop now (F-008.6).
    RefreshDue,
}

/// A change of the pause state, for the log (F-008.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    Suppressed,
    Resumed {
        /// How long the pause lasted.
        paused_for: Duration,
        /// Frames received and dropped during the pause.
        skipped_frames: u64,
    },
}

/// Tracks the pause flag across frames and reactivations.
#[derive(Debug, Clone, Default)]
pub struct SuppressTracker {
    /// Set while the client has asked for a pause, with when it started.
    suppressed_since: Option<Instant>,
    skipped_frames: u64,
    /// A resume happened and the full refresh has not started yet.
    refresh_pending: bool,
    last_refresh: Option<Instant>,
}

impl SuppressTracker {
    /// Feeds the current value of the client's flag. Returns the transition,
    /// if the value changed since the previous reading.
    pub fn observe(&mut self, suppressed: bool, now: Instant) -> Option<Transition> {
        match (self.suppressed_since, suppressed) {
            (None, true) => {
                self.suppressed_since = Some(now);
                self.skipped_frames = 0;
                // A new pause cancels a refresh that has not gone out yet.
                self.refresh_pending = false;
                Some(Transition::Suppressed)
            }
            (Some(since), false) => {
                self.suppressed_since = None;
                self.refresh_pending = true;
                Some(Transition::Resumed {
                    paused_for: now.saturating_duration_since(since),
                    skipped_frames: std::mem::take(&mut self.skipped_frames),
                })
            }
            _ => None,
        }
    }

    /// Counts a frame dropped during a pause.
    pub fn frame_skipped(&mut self) {
        if self.suppressed_since.is_some() {
            self.skipped_frames = self.skipped_frames.saturating_add(1);
        }
    }

    #[must_use]
    pub fn state(&self, now: Instant) -> SuppressState {
        if self.suppressed_since.is_some() {
            SuppressState::Suppressed
        } else if self.refresh_pending && self.spacing_elapsed(now) {
            SuppressState::RefreshDue
        } else {
            SuppressState::Idle
        }
    }

    /// The caller has started the full refresh.
    pub fn refresh_started(&mut self, now: Instant) {
        self.refresh_pending = false;
        self.last_refresh = Some(now);
    }

    /// A refresh is waiting for the spacing window to end (F-008.9).
    #[must_use]
    pub fn refresh_deferred(&self, now: Instant) -> bool {
        self.suppressed_since.is_none() && self.refresh_pending && !self.spacing_elapsed(now)
    }

    fn spacing_elapsed(&self, now: Instant) -> bool {
        self.last_refresh
            .is_none_or(|last| now.saturating_duration_since(last) >= REFRESH_SPACING)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    /// F-008.11, F-008.8: a new connection starts with no pause and nothing due.
    #[test]
    fn f008_11_initial_state_is_idle() {
        let tracker = SuppressTracker::default();
        assert_eq!(tracker.state(Instant::now()), SuppressState::Idle);
    }

    /// F-008.11, F-008.6: pause → resume ⇒ exactly one `RefreshDue`.
    #[test]
    fn f008_11_pause_then_resume_gives_one_refresh() {
        let t0 = Instant::now();
        let mut tracker = SuppressTracker::default();

        assert_eq!(tracker.observe(true, t0), Some(Transition::Suppressed));
        assert_eq!(tracker.state(at(t0, 10)), SuppressState::Suppressed);
        tracker.frame_skipped();
        tracker.frame_skipped();

        assert_eq!(
            tracker.observe(false, at(t0, 2_000)),
            Some(Transition::Resumed {
                paused_for: Duration::from_secs(2),
                skipped_frames: 2
            })
        );
        assert_eq!(tracker.state(at(t0, 2_000)), SuppressState::RefreshDue);

        tracker.refresh_started(at(t0, 2_000));
        assert_eq!(tracker.state(at(t0, 5_000)), SuppressState::Idle, "only one refresh");
    }

    /// F-008.11, F-008.9: pause → resume → pause before the refresh went out ⇒
    /// the refresh is cancelled.
    #[test]
    fn f008_9_a_new_pause_cancels_a_pending_refresh() {
        let t0 = Instant::now();
        let mut tracker = SuppressTracker::default();
        tracker.observe(true, t0);
        tracker.observe(false, at(t0, 100));
        tracker.observe(true, at(t0, 110));
        assert_eq!(tracker.state(at(t0, 120)), SuppressState::Suppressed);

        // Still paused much later: nothing was queued behind the pause.
        assert_eq!(tracker.observe(true, at(t0, 5_000)), None);
        assert_eq!(tracker.state(at(t0, 5_000)), SuppressState::Suppressed);
    }

    /// F-008.11, F-008.9: two resumes less than 1 s apart ⇒ the second refresh
    /// is deferred to the end of the window, not lost.
    #[test]
    fn f008_9_a_second_resume_within_a_second_is_deferred() {
        let t0 = Instant::now();
        let mut tracker = SuppressTracker::default();
        tracker.observe(true, t0);
        tracker.observe(false, at(t0, 100));
        tracker.refresh_started(at(t0, 100));

        tracker.observe(true, at(t0, 300));
        tracker.observe(false, at(t0, 400));
        assert_eq!(tracker.state(at(t0, 400)), SuppressState::Idle, "inside the 1 s window");
        assert!(tracker.refresh_deferred(at(t0, 400)));

        assert_eq!(
            tracker.state(at(t0, 1_100)),
            SuppressState::RefreshDue,
            "at the end of it"
        );
    }

    /// F-008.11: without a pause there is never a refresh.
    #[test]
    fn f008_11_no_pause_never_refreshes() {
        let t0 = Instant::now();
        let mut tracker = SuppressTracker::default();
        for ms in (0..10_000).step_by(50) {
            assert_eq!(tracker.observe(false, at(t0, ms)), None);
            tracker.frame_skipped();
            assert_eq!(tracker.state(at(t0, ms)), SuppressState::Idle);
        }
    }

    /// F-008.2: frames are counted only during a pause, and the count restarts
    /// with each pause.
    #[test]
    fn f008_2_skipped_frames_are_counted_per_pause() {
        let t0 = Instant::now();
        let mut tracker = SuppressTracker::default();
        tracker.frame_skipped();
        tracker.observe(true, t0);
        tracker.frame_skipped();
        let first = tracker.observe(false, at(t0, 10));
        assert!(matches!(first, Some(Transition::Resumed { skipped_frames: 1, .. })));

        tracker.observe(true, at(t0, 20));
        let second = tracker.observe(false, at(t0, 30));
        assert!(matches!(second, Some(Transition::Resumed { skipped_frames: 0, .. })));
    }
}
