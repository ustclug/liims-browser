use std::time::{Duration, Instant};

#[derive(Debug, PartialEq)]
pub enum IdleState {
    Inactive,
    Active,
    Warning(u64),
    Expired,
}

pub struct ActivityClock {
    last_activity: Instant,
    active: bool,
    timeout: Duration,
    warning: Duration,
}

impl ActivityClock {
    pub fn new(timeout: u64, warning: u64) -> Self {
        Self {
            last_activity: Instant::now(),
            active: false,
            timeout: Duration::from_secs(timeout),
            warning: Duration::from_secs(warning),
        }
    }
    pub fn activity(&mut self, now: Instant) {
        self.last_activity = now;
    }
    pub fn start(&mut self, now: Instant) {
        self.active = true;
        self.activity(now);
    }
    pub fn reset(&mut self) {
        self.active = false;
    }
    pub fn state(&self, now: Instant) -> IdleState {
        if !self.active {
            return IdleState::Inactive;
        }
        let elapsed = now.saturating_duration_since(self.last_activity);
        if elapsed >= self.timeout {
            return IdleState::Expired;
        }
        let remaining = self.timeout - elapsed;
        if remaining <= self.warning {
            IdleState::Warning(remaining.as_secs_f64().ceil() as u64)
        } else {
            IdleState::Active
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn warns_then_expires_and_input_restarts_countdown() {
        let now = Instant::now();
        let mut clock = ActivityClock::new(60, 15);
        assert_eq!(
            clock.state(now + Duration::from_secs(3600)),
            IdleState::Inactive
        );
        clock.start(now);
        assert_eq!(
            clock.state(now + Duration::from_secs(44)),
            IdleState::Active
        );
        assert_eq!(
            clock.state(now + Duration::from_secs(45)),
            IdleState::Warning(15)
        );
        assert_eq!(
            clock.state(now + Duration::from_secs(60)),
            IdleState::Expired
        );
        clock.activity(now + Duration::from_secs(50));
        assert_eq!(
            clock.state(now + Duration::from_secs(60)),
            IdleState::Active
        );
        clock.reset();
        assert_eq!(
            clock.state(now + Duration::from_secs(1000)),
            IdleState::Inactive
        );
    }
}
