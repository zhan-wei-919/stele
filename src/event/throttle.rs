//! Redraw throttling for IO-driven wake storms.

use std::time::{Duration, Instant};

/// Tracks redraw cadence and pending deferred work.
#[derive(Debug)]
pub(crate) struct RedrawThrottle {
    last_redraw: Option<Instant>,
    dirty: bool,
    pending_deadline: bool,
    min_interval: Duration,
}

impl RedrawThrottle {
    /// Creates a throttle with the given redraw interval.
    pub(crate) fn new(min_interval: Duration) -> Self {
        Self {
            last_redraw: None,
            dirty: false,
            pending_deadline: false,
            min_interval,
        }
    }

    /// Returns whether a redraw can happen immediately.
    pub(crate) fn should_redraw_now(&self) -> bool {
        self.last_redraw
            .map(|last| last.elapsed() >= self.min_interval)
            .unwrap_or(true)
    }

    /// Marks the frame as dirty.
    pub(crate) fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Clears the dirty flag.
    pub(crate) fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    /// Returns whether deferred work is waiting for the next redraw.
    pub(crate) fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Records a redraw request timestamp.
    pub(crate) fn record_redraw(&mut self) {
        self.last_redraw = Some(Instant::now());
    }

    /// Returns the elapsed interval since the previous redraw request.
    pub(crate) fn elapsed_since_last_redraw(&self) -> Option<Duration> {
        self.last_redraw.map(|last| last.elapsed())
    }

    /// Returns the remaining delay before the deadline can fire.
    pub(crate) fn deadline_delay(&self) -> Duration {
        self.last_redraw
            .map(|last| self.min_interval.saturating_sub(last.elapsed()))
            .unwrap_or(Duration::ZERO)
    }

    /// Returns whether a deadline timer is already scheduled.
    pub(crate) fn pending_deadline(&self) -> bool {
        self.pending_deadline
    }

    /// Marks a deadline timer as pending.
    pub(crate) fn start_deadline(&mut self) {
        self.pending_deadline = true;
    }

    /// Marks the deadline timer as no longer pending.
    pub(crate) fn finish_deadline(&mut self) {
        self.pending_deadline = false;
    }
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;

    use super::RedrawThrottle;

    #[test]
    fn first_redraw_is_immediate() {
        let throttle = RedrawThrottle::new(Duration::from_millis(16));
        assert!(throttle.should_redraw_now());
    }

    #[test]
    fn redraw_is_deferred_inside_interval() {
        let mut throttle = RedrawThrottle::new(Duration::from_millis(16));
        throttle.record_redraw();
        assert!(!throttle.should_redraw_now());
    }

    #[test]
    fn redraw_becomes_available_after_interval() {
        let mut throttle = RedrawThrottle::new(Duration::from_millis(1));
        throttle.record_redraw();
        thread::sleep(Duration::from_millis(2));
        assert!(throttle.should_redraw_now());
    }
}
