//! How long frames take to draw, logged now and then so a change of frame
//! rate can be decided from numbers rather than guessed.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How many of the latest frames a summary covers.
pub const KEPT: usize = 256;

/// How often a summary is due.
pub const EVERY: Duration = Duration::from_secs(10);

/// The latest frames' durations.
#[derive(Debug)]
pub struct FrameStats {
    took: VecDeque<Duration>,
    since: Instant,
}

/// One summary: the median, the 95th percentile, the slowest, and how many
/// frames it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub p50: Duration,
    pub p95: Duration,
    pub max: Duration,
    pub frames: usize,
}

impl FrameStats {
    /// Nothing recorded yet, the first summary due [`EVERY`] after `now`.
    #[must_use]
    pub fn new(now: Instant) -> Self {
        Self {
            took: VecDeque::with_capacity(KEPT),
            since: now,
        }
    }

    /// Records one frame, returning a summary when one is due, and starting
    /// afresh after it.
    pub fn record(&mut self, took: Duration, now: Instant) -> Option<Summary> {
        if self.took.len() == KEPT {
            self.took.pop_front();
        }
        self.took.push_back(took);

        if now.saturating_duration_since(self.since) < EVERY {
            return None;
        }

        let mut sorted: Vec<Duration> = self.took.drain(..).collect();
        sorted.sort();
        self.since = now;

        let at = |share: f32| {
            let last = sorted.len() - 1;
            sorted[((last as f32) * share).round() as usize]
        };
        Some(Summary {
            p50: at(0.50),
            p95: at(0.95),
            max: sorted[sorted.len() - 1],
            frames: sorted.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_summary_comes_every_ten_seconds_and_starts_afresh() {
        let start = Instant::now();
        let mut stats = FrameStats::new(start);
        for ms in 1..=100 {
            assert_eq!(
                stats.record(Duration::from_millis(ms), start + Duration::from_millis(ms)),
                None
            );
        }
        let summary = stats
            .record(Duration::from_millis(1), start + Duration::from_secs(10))
            .expect("ten seconds have passed");
        assert_eq!(summary.frames, 101);
        assert_eq!(summary.max, Duration::from_millis(100));
        assert_eq!(summary.p50, Duration::from_millis(50));
        assert_eq!(summary.p95, Duration::from_millis(95));
        assert_eq!(
            stats.record(Duration::from_millis(1), start + Duration::from_secs(11)),
            None
        );
    }

    #[test]
    fn only_the_last_frames_are_kept() {
        let start = Instant::now();
        let mut stats = FrameStats::new(start);
        for _ in 0..1000 {
            stats.record(Duration::from_millis(1), start);
        }
        let summary = stats
            .record(Duration::from_millis(1), start + Duration::from_secs(10))
            .expect("due");
        assert_eq!(summary.frames, KEPT);
    }
}
