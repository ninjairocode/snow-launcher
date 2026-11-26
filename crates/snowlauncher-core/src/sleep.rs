//! System suspend and resume.
//!
//! After the machine wakes up, Battle.net often fails to reconnect. The
//! daemon watches for these events so it can restart Battle.net cleanly.

use std::future::Future;

use crate::config::SleepConfig;

/// A change in the system's sleep state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepEvent {
    /// The system is about to suspend or hibernate.
    Suspending,
    /// The system has just woken up.
    Resumed,
}

impl SleepEvent {
    /// Converts the argument of logind's `PrepareForSleep(start)` signal:
    /// `true` before sleeping, `false` after waking up.
    pub fn from_prepare_for_sleep(start: bool) -> Self {
        if start {
            Self::Suspending
        } else {
            Self::Resumed
        }
    }
}

/// Source of sleep events. The real implementation listens to logind over
/// D-Bus; tests use a scripted list of events.
pub trait SleepMonitor {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Waits for the next event. `Ok(None)` means the source is gone
    /// (for example, the D-Bus connection was closed) and no more events
    /// will arrive.
    ///
    /// Written as `-> impl Future` rather than `async fn` so implementations
    /// are required to return a `Send` future, usable from any Tokio task.
    fn next_event(
        &mut self,
    ) -> impl Future<Output = Result<Option<SleepEvent>, Self::Error>> + Send;
}

/// Whether automatic suspend must be blocked right now.
pub fn should_inhibit_suspend(games_running: bool, config: &SleepConfig) -> bool {
    games_running && !config.allow_suspend_while_playing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inhibits_only_while_playing_and_not_allowed() {
        let default = SleepConfig::default();
        let allowed = SleepConfig {
            allow_suspend_while_playing: true,
        };
        assert!(should_inhibit_suspend(true, &default));
        assert!(!should_inhibit_suspend(false, &default));
        assert!(!should_inhibit_suspend(true, &allowed));
        assert!(!should_inhibit_suspend(false, &allowed));
    }
}
