//! Stall detection for the capture stream (F-007.14, F-007.15).
//!
//! `did_stop_with_error` only fires when ScreenCaptureKit *stops* a stream.
//! The failure this project actually hit does not stop it: opening the lid
//! reconfigures the displays, the `SCContentFilter` goes stale, and the stream
//! keeps delivering sample buffers that carry no image buffer — forever. The
//! delegate stays silent, so recovery never triggers and the client stares at a
//! frozen frame (verified 2026-08-07, F-007 §8).
//!
//! This is the second trigger. It reads the counters the capture path already
//! maintains and decides, from two snapshots and a clock, whether the stream is
//! producing nothing but rejects — or, since F-007.30, nothing at all.
//!
//! **The distinction that matters** (F-007.14a) is between a broken stream and
//! an idle desktop, because a naive "no frames for N seconds" rule would rebuild
//! the stream every few seconds on a Mac nobody is touching:
//!
//! | Situation                   | `received` | `skipped` | `rejected` |
//! |-----------------------------|------------|-----------|------------|
//! | Healthy, screen changing    | rises      | —         | —          |
//! | Healthy, desktop idle       | flat       | **rises** | flat       |
//! | Stale filter (the incident) | flat       | —         | **rises**  |
//! | Mute stream (F-007 §8g)     | **flat**   | **flat**  | **flat**   |
//!
//! The third and fourth rows are failures. An idle desktop raises `skipped`,
//! because `frame_from_sample` drops `Idle`/`Blank` frames before they ever
//! count as received — which is exactly what makes the fourth row detectable:
//! a Mac nobody is touching still gets called about thirty times a second, so
//! *no call at all* is not idleness, it is death.
//!
//! The fourth row cost thirteen hours of frozen screen before it was measured
//! (F-007 §8g): the stream stayed alive, ScreenCaptureKit simply stopped
//! calling the delegate, and a watchdog that needed rejections had nothing to
//! count. Reconnecting could not help, because the frame source is shared by
//! the whole process.
//!
//! Pure and macOS-free, so all of it is unit-tested without a live stream.

use std::time::{Duration, Instant};

use super::StatsSnapshot;

/// Counters read from one snapshot. Only the ones that carry the signal.
#[derive(Debug, Clone, Copy)]
struct Counters {
    received: u64,
    rejected: u64,
    /// Idle/Blank frames. Not evidence of health on its own — the client gets
    /// no new picture from one — but evidence that the stream is *calling us*,
    /// which is what tells an untouched Mac apart from a dead stream.
    skipped: u64,
}

impl Counters {
    /// Every call from ScreenCaptureKit, whatever it carried.
    const fn calls(self) -> u64 {
        self.received.saturating_add(self.rejected).saturating_add(self.skipped)
    }
}

/// Why the stream is considered dead.
///
/// Two failures, deliberately not collapsed into one: they are detected from
/// different evidence, and F-007.32 requires the log to say which one fired —
/// for the same reason F-007.16 exists, that the absence of a line was once the
/// whole diagnosis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stalled {
    /// Rejects keep arriving and no frame does (F-007.14). Carries how long the
    /// rejections had been running.
    OnlyRejects(Duration),
    /// Nothing arrives at all: no frame, no idle frame, no reject (F-007.30).
    /// Carries how long the stream had been silent.
    Silent(Duration),
}

/// An open stall window: rejections are arriving and no frame has landed.
#[derive(Debug, Clone, Copy)]
struct Stall {
    /// When the first rejection of this window was observed.
    since: Instant,
    /// When the most recent rejection was observed, used to expire a window
    /// whose rejections went quiet without any frame arriving.
    last_rejection: Instant,
    /// Rejections accumulated since `since`.
    rejections: u64,
}

/// Decides whether the capture stream is stalled — alive, but producing only
/// unusable samples.
#[derive(Debug)]
pub(crate) struct StallWatchdog {
    /// How long rejections must keep arriving with no frame before firing.
    threshold: Duration,
    /// How many rejections must accumulate in the window before firing.
    ///
    /// Guards against a single transient reject on an otherwise idle desktop
    /// keeping a window open until the threshold elapses. It also makes the
    /// watchdog correct at low frame rates: at `--max-fps 1` a broken stream
    /// produces one reject per second, so the count, not the clock, is what
    /// gates the decision.
    min_rejections: u64,
    /// How long with no call of any kind before the stream is declared mute
    /// (F-007.30). Derived from `max_fps` by the caller, because at
    /// `--max-fps 1` a second of quiet is normal and at 30 fps it is not.
    silence: Duration,
    last: Option<Counters>,
    stall: Option<Stall>,
    /// When ScreenCaptureKit last called us, by this watchdog's own reckoning.
    ///
    /// Deliberately not read from `StatsSnapshot::silent_for`, which measures
    /// the same thing for the log: firing has to re-baseline *this* clock, or a
    /// stream that stays mute through a rebuild would trigger another rebuild on
    /// every poll instead of every `silence` (F-007.33).
    last_activity: Option<Instant>,
}

impl StallWatchdog {
    /// # Contract
    ///
    /// The caller MUST call [`observe`](Self::observe) at an interval shorter
    /// than `threshold`. A window of rejections that goes quiet for longer than
    /// `threshold` is treated as stale evidence and restarted, so polling more
    /// slowly than that would keep restarting the window and the watchdog could
    /// never fire. `WATCHDOG_POLL` (500 ms) against a 3 s threshold in
    /// `screen.rs` satisfies this with room to spare.
    pub(crate) const fn new(threshold: Duration, min_rejections: u64, silence: Duration) -> Self {
        Self {
            threshold,
            min_rejections,
            silence,
            last: None,
            stall: None,
            last_activity: None,
        }
    }

    /// Feeds one observation in.
    ///
    /// Returns `Some(_)` exactly once per stall, when the evidence for one of
    /// the two failures is complete; the watchdog then re-baselines so the
    /// caller's recovery attempt is judged on fresh evidence. The elapsed time
    /// travels with the verdict rather than a bare `bool`, so the recovery log
    /// can say how long the stream was dead (F-007.16).
    ///
    /// The first call only establishes a baseline and never fires: counters are
    /// cumulative, so a single reading says nothing about direction.
    pub(crate) fn observe(&mut self, snapshot: &StatsSnapshot, now: Instant) -> Option<Stalled> {
        let current = Counters {
            received: snapshot.received,
            rejected: snapshot.rejected,
            skipped: snapshot.skipped,
        };
        let Some(previous) = self.last.replace(current) else {
            // Nothing to compare against yet, so the silence clock starts here
            // rather than at zero — otherwise the first poll after a rebuild
            // would judge the new stream on the old one's quiet.
            self.last_activity = Some(now);
            return None;
        };

        // F-007.30. Checked before the rejection logic because it subsumes it:
        // a stream that is not calling us at all cannot be producing rejects,
        // and the rejection path would read its flat counters as an idle
        // desktop and say nothing — which is what let a frozen screen sit for
        // thirteen hours (F-007 §8g).
        if current.calls() > previous.calls() {
            self.last_activity = Some(now);
        } else {
            let since = *self.last_activity.get_or_insert(now);
            let silent_for = now.duration_since(since);
            if silent_for >= self.silence {
                // Re-baseline both clocks: the recovery this triggers gets the
                // same fresh start as the rejection path's (F-007.33).
                self.last_activity = Some(now);
                self.stall = None;
                return Some(Stalled::Silent(silent_for));
            }
        }

        // A real frame is the only evidence of health, and it outranks any
        // rejections seen in the same interval.
        if current.received > previous.received {
            self.stall = None;
            return None;
        }

        let new_rejections = current.rejected.saturating_sub(previous.rejected);
        if new_rejections == 0 {
            // Neither healthy nor failing: an idle desktop looks exactly like
            // this. Leave any open window alone rather than reading silence as
            // recovery — only a delivered frame clears a stall.
            return None;
        }

        // A window whose rejections stopped for longer than the threshold, with
        // no frame in between, is stale evidence. Start counting again rather
        // than letting sporadic rejects add up across minutes.
        if self
            .stall
            .is_some_and(|stall| now.duration_since(stall.last_rejection) >= self.threshold)
        {
            self.stall = None;
        }

        let stall = self.stall.get_or_insert(Stall {
            since: now,
            last_rejection: now,
            rejections: 0,
        });
        stall.last_rejection = now;
        stall.rejections = stall.rejections.saturating_add(new_rejections);

        let elapsed = now.duration_since(stall.since);
        if elapsed >= self.threshold && stall.rejections >= self.min_rejections {
            self.reset();
            return Some(Stalled::OnlyRejects(elapsed));
        }
        None
    }

    /// Drops all history.
    ///
    /// Called after a rebuild: the counters are shared across streams, so
    /// without this the new stream would inherit the old one's evidence.
    pub(crate) fn reset(&mut self) {
        self.last = None;
        self.stall = None;
        // Left for the next `observe` to set from its own `now`, so a rebuild
        // is never judged against a clock that started before it (F-007.33).
        self.last_activity = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THRESHOLD: Duration = Duration::from_secs(3);
    const MIN_REJECTIONS: u64 = 10;
    /// Far longer than any gap the rejection tests simulate, so they keep
    /// testing what they were written to test. The silence tests set their own.
    const SILENCE: Duration = Duration::from_secs(60);

    fn watchdog() -> StallWatchdog {
        StallWatchdog::new(THRESHOLD, MIN_REJECTIONS, SILENCE)
    }

    /// A snapshot carrying only the counters the watchdog reads.
    ///
    /// `silent_for` is left at zero: it is the log's instrument (F-007.31), and
    /// the watchdog deliberately keeps its own clock (F-007.33), so a test that
    /// set it would be testing nothing.
    fn snap(received: u64, skipped: u64, rejected: u64) -> StatsSnapshot {
        StatsSnapshot {
            received,
            delivered: 0,
            coalesced: 0,
            skipped,
            rejected,
            bytes: 0,
            silent_for: Duration::ZERO,
        }
    }

    #[test]
    fn the_first_observation_only_establishes_a_baseline() {
        let mut wd = watchdog();
        let t0 = Instant::now();
        assert_eq!(
            wd.observe(&snap(0, 0, 1_000_000), t0),
            None,
            "a single reading of a cumulative counter says nothing about direction"
        );
    }

    #[test]
    fn a_healthy_stream_never_fires() {
        let mut wd = watchdog();
        let t0 = Instant::now();
        for tick in 0..100 {
            let elapsed = Duration::from_millis(500 * tick);
            assert_eq!(wd.observe(&snap(tick * 15, 0, 0), t0 + elapsed), None);
        }
    }

    /// F-007.14a: the case a naive "no frames for N seconds" watchdog gets wrong.
    #[test]
    fn an_idle_desktop_never_fires() {
        let mut wd = watchdog();
        let t0 = Instant::now();
        // `received` frozen at 500 forever; only `skipped` climbs, because
        // Idle/Blank frames are dropped before they count as received.
        for tick in 0..200 {
            let elapsed = Duration::from_millis(500 * tick);
            assert_eq!(
                wd.observe(&snap(500, tick * 15, 0), t0 + elapsed),
                None,
                "an untouched Mac must not look like a broken stream"
            );
        }
    }

    #[test]
    fn a_stream_producing_only_rejects_fires_after_the_threshold() {
        let mut wd = watchdog();
        let t0 = Instant::now();
        wd.observe(&snap(500, 0, 0), t0);

        // 30 fps of empty samples: 15 rejects per 500 ms poll.
        assert_eq!(wd.observe(&snap(500, 0, 15), t0 + Duration::from_millis(500)), None);
        assert_eq!(wd.observe(&snap(500, 0, 30), t0 + Duration::from_millis(1000)), None);

        // The window opened at t0+500ms, so the threshold is met at t0+3500ms.
        assert_eq!(wd.observe(&snap(500, 0, 90), t0 + Duration::from_millis(3000)), None);
        let fired = wd
            .observe(&snap(500, 0, 105), t0 + Duration::from_millis(3500))
            .expect("a stream delivering nothing but rejects must be reported as stalled");
        assert_eq!(
            fired,
            Stalled::OnlyRejects(Duration::from_millis(3000)),
            "measured from the first reject"
        );
    }

    /// The count gate, not the clock, is what makes this correct at low frame
    /// rates: `--max-fps 1` yields one reject per second.
    #[test]
    fn a_slow_stream_fires_on_the_rejection_count_not_the_clock() {
        let mut wd = watchdog();
        let t0 = Instant::now();
        wd.observe(&snap(500, 0, 0), t0);

        for second in 1..MIN_REJECTIONS {
            let now = t0 + Duration::from_secs(second);
            assert_eq!(
                wd.observe(&snap(500, 0, second), now),
                None,
                "past the time threshold at {second}s but only {second} rejects so far"
            );
        }

        assert!(
            wd.observe(&snap(500, 0, MIN_REJECTIONS), t0 + Duration::from_secs(MIN_REJECTIONS))
                .is_some(),
            "the tenth reject completes the evidence"
        );
    }

    #[test]
    fn one_transient_reject_on_an_idle_desktop_does_not_fire() {
        let mut wd = watchdog();
        let t0 = Instant::now();
        wd.observe(&snap(500, 0, 0), t0);
        wd.observe(&snap(500, 0, 1), t0 + Duration::from_millis(500));

        // Nothing more happens for a long time: no frames, no further rejects.
        for tick in 2..120 {
            let now = t0 + Duration::from_millis(500 * tick);
            assert_eq!(
                wd.observe(&snap(500, tick * 15, 1), now),
                None,
                "a single reject is not evidence of a dead stream"
            );
        }
    }

    #[test]
    fn a_delivered_frame_clears_an_open_stall() {
        let mut wd = watchdog();
        let t0 = Instant::now();
        wd.observe(&snap(500, 0, 0), t0);
        wd.observe(&snap(500, 0, 60), t0 + Duration::from_secs(2));

        // A frame lands: whatever was wrong, the stream is producing again.
        wd.observe(&snap(501, 0, 60), t0 + Duration::from_secs(3));

        // The old window is gone, so the clock restarts from the next reject.
        wd.observe(&snap(501, 0, 120), t0 + Duration::from_secs(4));
        assert_eq!(
            wd.observe(&snap(501, 0, 180), t0 + Duration::from_secs(5)),
            None,
            "only 1s has passed since the stall restarted"
        );
        assert!(
            wd.observe(&snap(501, 0, 240), t0 + Duration::from_secs(7)).is_some(),
            "3s after the new first reject the stall is established again"
        );
    }

    #[test]
    fn firing_re_baselines_so_it_does_not_fire_again_immediately() {
        let mut wd = watchdog();
        let t0 = Instant::now();
        // Poll on the real cadence: every 500 ms, 15 rejects per poll.
        let poll = |tick: u64| (snap(500, 0, tick * 15), t0 + Duration::from_millis(500 * tick));

        let mut fired_at = None;
        for tick in 0..=7 {
            let (snapshot, now) = poll(tick);
            if wd.observe(&snapshot, now).is_some() {
                fired_at = Some(tick);
                break;
            }
        }
        // Window opens on tick 1 (the first reject); 3 s later is tick 7.
        assert_eq!(
            fired_at,
            Some(7),
            "the stall must be reported once the threshold is met"
        );

        // The next observation is a fresh baseline, not an instant re-fire.
        let (snapshot, now) = poll(8);
        assert_eq!(
            wd.observe(&snapshot, now),
            None,
            "recovery needs a chance to work before the watchdog judges again"
        );
    }

    #[test]
    fn reset_forgets_a_stall_in_progress() {
        let mut wd = watchdog();
        let t0 = Instant::now();
        wd.observe(&snap(500, 0, 0), t0);
        wd.observe(&snap(500, 0, 60), t0 + Duration::from_secs(2));

        wd.reset();

        assert_eq!(
            wd.observe(&snap(500, 0, 120), t0 + Duration::from_secs(4)),
            None,
            "after a rebuild the new stream must not inherit the old one's evidence"
        );
    }

    /// F-007.30: the failure of F-007 §8g, which the rejection logic cannot
    /// see because there is nothing to count.
    #[test]
    fn f007_30_a_stream_that_stops_calling_at_all_fires_on_silence() {
        let mut wd = StallWatchdog::new(THRESHOLD, MIN_REJECTIONS, Duration::from_secs(5));
        let t0 = Instant::now();
        // Healthy for a while: 15 frames per 500 ms poll.
        for tick in 0..4 {
            assert_eq!(
                wd.observe(&snap(tick * 15, 0, 0), t0 + Duration::from_millis(500 * tick)),
                None
            );
        }

        // And now ScreenCaptureKit goes quiet. Every counter stands still.
        let frozen = 45;
        let quiet_since = Duration::from_millis(1500);
        for tick in 4..13 {
            let now = t0 + Duration::from_millis(500 * tick);
            assert_eq!(
                wd.observe(&snap(frozen, 0, 0), now),
                None,
                "{} s of quiet is not yet proof",
                (Duration::from_millis(500 * tick) - quiet_since).as_secs_f32()
            );
        }

        // The last call was at t0+1500ms, so the window closes at t0+6500ms.
        let fired = wd
            .observe(&snap(frozen, 0, 0), t0 + Duration::from_millis(6500))
            .expect("a stream that has not called us for 5 s is dead, not idle");
        assert_eq!(fired, Stalled::Silent(Duration::from_secs(5)));
    }

    /// F-007.14a again, from the other side: the silence rule must not undo the
    /// one guarantee the rejection rule was built around.
    #[test]
    fn f007_30_an_idle_desktop_is_not_silence() {
        let mut wd = StallWatchdog::new(THRESHOLD, MIN_REJECTIONS, Duration::from_secs(5));
        let t0 = Instant::now();
        // `received` never moves again — nothing on screen changes — but the
        // stream keeps calling with Idle frames, as a live stream does.
        for tick in 0..200 {
            let now = t0 + Duration::from_millis(500 * tick);
            assert_eq!(
                wd.observe(&snap(500, tick * 15, 0), now),
                None,
                "an untouched Mac is not a dead stream, however long it is untouched"
            );
        }
    }

    /// Rejects are calls too, so the stale-filter failure must still be
    /// reported as what it is — otherwise the recovery log would name the wrong
    /// cause (F-007.32).
    #[test]
    fn f007_32_a_rejecting_stream_is_not_reported_as_silent() {
        let mut wd = StallWatchdog::new(THRESHOLD, MIN_REJECTIONS, Duration::from_secs(5));
        let t0 = Instant::now();
        wd.observe(&snap(500, 0, 0), t0);

        let mut verdict = None;
        for tick in 1..40 {
            let now = t0 + Duration::from_millis(500 * tick);
            if let Some(fired) = wd.observe(&snap(500, 0, tick * 15), now) {
                verdict = Some(fired);
                break;
            }
        }
        assert!(
            matches!(verdict, Some(Stalled::OnlyRejects(_))),
            "got {verdict:?}, but rejects keep arriving — the stream is talking, just uselessly"
        );
    }

    /// F-007.33: one failure must not produce a recovery on every poll.
    #[test]
    fn f007_33_silence_fires_once_per_silence_window_not_once_per_poll() {
        let silence = Duration::from_secs(5);
        let mut wd = StallWatchdog::new(THRESHOLD, MIN_REJECTIONS, silence);
        let t0 = Instant::now();
        wd.observe(&snap(10, 0, 0), t0);

        let mut fired_at = Vec::new();
        // 30 s of total silence, polled every 500 ms as the supervisor does.
        for tick in 1..=60 {
            let now = t0 + Duration::from_millis(500 * tick);
            if wd.observe(&snap(10, 0, 0), now).is_some() {
                fired_at.push(tick);
            }
        }
        assert_eq!(
            fired_at,
            vec![10, 20, 30, 40, 50, 60],
            "once per 5 s window, not sixty times"
        );
    }

    /// A rebuild takes time, and during it the counters are flat for reasons
    /// that are not the stream's fault.
    #[test]
    fn f007_33_a_rebuild_is_not_judged_on_the_old_streams_quiet() {
        let mut wd = StallWatchdog::new(THRESHOLD, MIN_REJECTIONS, Duration::from_secs(5));
        let t0 = Instant::now();
        wd.observe(&snap(10, 0, 0), t0);
        // Silent long enough to fire.
        assert!(wd.observe(&snap(10, 0, 0), t0 + Duration::from_secs(6)).is_some());

        // The supervisor rebuilds, which takes a few seconds, and resets us.
        wd.reset();
        let after_rebuild = t0 + Duration::from_secs(9);
        assert_eq!(
            wd.observe(&snap(10, 0, 0), after_rebuild),
            None,
            "the first look at a new stream is a baseline, not a verdict"
        );
        assert_eq!(
            wd.observe(&snap(10, 0, 0), after_rebuild + Duration::from_secs(4)),
            None,
            "4 s into the new stream is still within its own silence window"
        );
        assert!(
            wd.observe(&snap(10, 0, 0), after_rebuild + Duration::from_secs(5))
                .is_some(),
            "but a new stream that never says anything is dead too"
        );
    }

    #[test]
    fn a_quiet_gap_between_rejects_restarts_the_window() {
        let mut wd = watchdog();
        let t0 = Instant::now();
        wd.observe(&snap(500, 0, 0), t0);
        wd.observe(&snap(500, 0, 5), t0 + Duration::from_secs(1));

        // Five rejects, then silence well past the threshold, then rejects again.
        // Without the expiry rule the two bursts would add up and fire here.
        assert_eq!(
            wd.observe(&snap(500, 0, 10), t0 + Duration::from_secs(10)),
            None,
            "the earlier burst is stale evidence and must not count"
        );
    }
}
