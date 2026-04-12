//! Async-side SceneDiff send throttling.

use std::time::{Duration, Instant};

/// Tracks the minimum interval between SceneDiff sends.
#[derive(Debug)]
pub(crate) struct RedrawThrottle {
    last_send: Option<Instant>,
    min_interval: Duration,
}

impl RedrawThrottle {
    /// Creates a throttle with the requested minimum send interval.
    pub(crate) fn new(min_interval: Duration) -> Self {
        Self {
            last_send: None,
            min_interval,
        }
    }

    /// Returns whether a new SceneDiff may be sent immediately.
    pub(crate) fn ready_now(&self) -> bool {
        self.last_send
            .map(|last_send| last_send.elapsed() >= self.min_interval)
            .unwrap_or(true)
    }

    /// Returns the remaining delay before the next send is allowed.
    pub(crate) fn delay_until_ready(&self) -> Duration {
        self.last_send
            .map(|last_send| self.min_interval.saturating_sub(last_send.elapsed()))
            .unwrap_or(Duration::ZERO)
    }

    /// Records the timestamp of a sent SceneDiff.
    pub(crate) fn record_send(&mut self) {
        self.last_send = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;

    use super::RedrawThrottle;

    #[test]
    fn first_send_is_immediate() {
        let throttle = RedrawThrottle::new(Duration::from_millis(16));
        assert!(throttle.ready_now());
    }

    #[test]
    fn send_is_deferred_inside_interval() {
        let mut throttle = RedrawThrottle::new(Duration::from_millis(16));
        throttle.record_send();
        assert!(!throttle.ready_now());
    }

    #[test]
    fn send_becomes_ready_after_interval() {
        let mut throttle = RedrawThrottle::new(Duration::from_millis(1));
        throttle.record_send();
        thread::sleep(Duration::from_millis(2));
        assert!(throttle.ready_now());
    }
}
