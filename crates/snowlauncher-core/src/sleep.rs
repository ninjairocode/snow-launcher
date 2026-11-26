//! System suspend and resume.
//!
//! After the machine wakes up, Battle.net often fails to reconnect. The
//! daemon watches for these events so it can restart Battle.net cleanly.

use std::future::Future;

use crate::config::SleepConfig;
use crate::desktop::Notification;

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

/// The warning shown after resume when games were left running, or `None`
/// if no game is running.
///
/// The game's connection to its server does not survive a suspend, but the
/// game can take many minutes to notice, so the user must be told.
// TODO(i18n): translate (pt-BR) once gettext is in place (v0.6).
pub fn lost_connection_notice(game_titles: &[&str]) -> Option<Notification> {
    let (summary, which) = match game_titles {
        [] => return None,
        [title] => (format!("{title} lost its connection"), "The game is"),
        _ => (
            format!("{} lost their connection", game_titles.join(", ")),
            "The games are",
        ),
    };
    let body = format!(
        "The computer was suspended while playing. {which} still open but no \
         longer connected to the server, and may take several minutes to \
         notice. Close it and open it again."
    );
    Some(Notification {
        summary,
        body,
        urgent: true,
    })
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

    #[test]
    fn no_notice_without_games() {
        assert_eq!(lost_connection_notice(&[]), None);
    }

    #[test]
    fn notice_names_the_game_and_is_urgent() {
        let notice = lost_connection_notice(&["World of Warcraft"]).unwrap();
        assert_eq!(notice.summary, "World of Warcraft lost its connection");
        assert!(notice.body.contains("The game is still open"));
        assert!(notice.body.contains("Close it"));
        assert!(notice.urgent);
    }

    #[test]
    fn notice_lists_several_games() {
        let notice = lost_connection_notice(&["World of Warcraft", "Diablo IV"]).unwrap();
        assert_eq!(
            notice.summary,
            "World of Warcraft, Diablo IV lost their connection"
        );
        assert!(notice.body.contains("The games are"));
    }

    #[test]
    fn maps_logind_signal_argument() {
        assert_eq!(
            SleepEvent::from_prepare_for_sleep(true),
            SleepEvent::Suspending
        );
        assert_eq!(
            SleepEvent::from_prepare_for_sleep(false),
            SleepEvent::Resumed
        );
    }
}
