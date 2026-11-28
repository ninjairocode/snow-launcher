//! What the window shows, from what `snowd` and the house report. Pure:
//! the window only displays what these functions decide.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use snowlauncher_core::battlenet_log::StartupStage;
use snowlauncher_core::dbus::GameConnection;
use snowlauncher_core::house::BATTLENET_EXE;
use snowlauncher_wine::daemon_client::DaemonState;
use snowlauncher_wine::house::{BattlenetRun, PlayRun, load_record, record_path};
use snowlauncher_wine::library::InstalledGame;

// TODO(i18n): translate (pt-BR) once gettext is in place (v0.6).

/// One row of the status list: a title and its current value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusRow {
    pub title: &'static str,
    pub value: String,
}

/// The four status rows, from what `snowd` reports.
pub fn status_rows(daemon: &DaemonState) -> Vec<StatusRow> {
    let row = |title, value: &str| StatusRow {
        title,
        value: value.to_owned(),
    };
    let view = match daemon {
        DaemonState::Running(view) => view,
        DaemonState::RunningWithoutInterface { .. } => {
            return vec![row(
                "snowd",
                "running, but too old to report its status (update it)",
            )];
        }
        DaemonState::NotRunning => {
            return vec![row("snowd", "not running (see `snowctl service install`)")];
        }
        DaemonState::Unknown(_) => return vec![row("snowd", "unknown (could not ask it)")],
    };
    let games = if view.running_games.is_empty() {
        "none".to_owned()
    } else {
        view.running_games.join(", ")
    };
    let suspend = if view.suspend_inhibited {
        "blocked while playing"
    } else {
        "allowed"
    };
    let connection = match view.game_connection.parse::<GameConnection>() {
        Ok(GameConnection::NoGame) => "no game running",
        Ok(GameConnection::Ok) => "ok",
        Ok(GameConnection::NetworkDown) => "network is down",
        Ok(GameConnection::Lost) => "lost: close the game and open it again",
        // A newer snowd may report states this build does not know.
        Err(_) => view.game_connection.as_str(),
    };
    vec![
        row("snowd", &format!("running (v{})", view.version)),
        row("Games", &games),
        row("Automatic suspend", suspend),
        row("Game connection", connection),
    ]
}

/// The state of Snow Launcher's own Battle.net prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum House {
    /// No house recorded.
    Missing,
    /// The house exists but Battle.net is not installed in it.
    NoBattlenet,
    Ready {
        prefix: PathBuf,
        library: PathBuf,
    },
}

impl House {
    pub fn is_ready(&self) -> bool {
        matches!(self, House::Ready { .. })
    }
}

/// Reads the house record and checks for Battle.net. Errors count as
/// "missing": the window must never fail because of this.
pub fn read_house() -> House {
    let Some(record) = record_path().and_then(|path| load_record(&path).ok().flatten()) else {
        return House::Missing;
    };
    if record.prefix.join(BATTLENET_EXE).is_file() {
        House::Ready {
            prefix: record.prefix,
            library: record.library,
        }
    } else {
        House::NoBattlenet
    }
}

/// Battle.net in the house, as the window shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattlenetView {
    Closed,
    Starting,
    LoggingIn,
    /// Not ready after [`LOGIN_WAIT`]: the login screen is waiting.
    WaitingForLogin,
    Ready,
}

/// A session that is not ready after this is waiting for the user to log
/// in (experiment E10: a saved login is ready in ~6 s).
pub const LOGIN_WAIT: Duration = Duration::from_secs(30);

/// Follows Battle.net sessions across refreshes, to know which log is the
/// current session's. Right after Battle.net starts, its new log may not
/// exist yet and the newest one is the previous session's (maybe "ready").
/// So the newest log seen while Battle.net was closed never counts.
#[derive(Debug, Default)]
pub struct SessionTracker {
    old_log: Option<PathBuf>,
    started_at: Option<Instant>,
    /// The current session's log and the last stage read from it.
    session: Option<(PathBuf, StartupStage)>,
}

impl SessionTracker {
    /// `read_stage` is only called when needed: a session already ready
    /// is not read again.
    pub fn update(
        &mut self,
        running: bool,
        newest_log: Option<PathBuf>,
        now: Instant,
        read_stage: impl FnOnce(&Path) -> Option<StartupStage>,
    ) -> BattlenetView {
        if !running {
            self.old_log = newest_log;
            self.started_at = None;
            self.session = None;
            return BattlenetView::Closed;
        }
        let started_at = *self.started_at.get_or_insert(now);
        let stage = match newest_log.filter(|log| Some(log) != self.old_log.as_ref()) {
            None => None,
            Some(log) => match &self.session {
                Some((known, StartupStage::Ready)) if *known == log => Some(StartupStage::Ready),
                _ => {
                    let stage = read_stage(&log);
                    self.session = stage.map(|stage| (log, stage));
                    stage
                }
            },
        };
        match stage {
            Some(StartupStage::Ready) => BattlenetView::Ready,
            _ if now.saturating_duration_since(started_at) >= LOGIN_WAIT => {
                BattlenetView::WaitingForLogin
            }
            Some(StartupStage::AgentReady) => BattlenetView::LoggingIn,
            _ => BattlenetView::Starting,
        }
    }
}

/// Which illustration goes with a house state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Illustration {
    Empty,
    /// The doghouse with this many lights on (0 to 3).
    Lights(usize),
}

/// Illustration, title and description for the house part of the window.
pub fn house_text(house: &House, battlenet: BattlenetView) -> (Illustration, &'static str, String) {
    let (illustration, title, text) = match (house, battlenet) {
        (House::Missing, _) => (
            Illustration::Empty,
            "No house yet",
            "Create Snow Launcher's Battle.net house with `snowctl house create`.",
        ),
        (House::NoBattlenet, _) => (
            Illustration::Empty,
            "Battle.net is not installed",
            "Install it with `snowctl house install-battlenet --installer PATH`.",
        ),
        (House::Ready { .. }, BattlenetView::Closed) => (
            Illustration::Lights(0),
            "Battle.net is closed",
            "Open it with the button above, or press Play on a game.",
        ),
        (House::Ready { .. }, BattlenetView::Starting) => (
            Illustration::Lights(1),
            "Starting Battle.net…",
            "Step 1 of 3: opening the app.",
        ),
        (House::Ready { .. }, BattlenetView::LoggingIn) => (
            Illustration::Lights(2),
            "Logging in…",
            "Step 2 of 3: the Battle.net Agent is ready.",
        ),
        (House::Ready { .. }, BattlenetView::WaitingForLogin) => (
            Illustration::Lights(2),
            "Waiting for you to log in",
            "Log in in Battle.net's own window.",
        ),
        (House::Ready { .. }, BattlenetView::Ready) => (
            Illustration::Lights(3),
            "Battle.net is ready",
            "Logged in. Pick a game below.",
        ),
    };
    (illustration, title, text.to_owned())
}

/// One game of the library, as its row shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameCard {
    pub name: String,
    pub detail: String,
    /// `None` until Battle.net's log reveals it: "Play" stays disabled.
    pub launch_code: Option<String>,
    /// The icon read from the game's own executable, if any.
    pub icon: Option<PathBuf>,
}

/// One card per game; `icon` finds a game's icon (IO, kept out of here).
pub fn game_cards(
    games: &[InstalledGame],
    icon: impl Fn(&InstalledGame) -> Option<PathBuf>,
) -> Vec<GameCard> {
    games
        .iter()
        .map(|game| GameCard {
            icon: icon(game),
            name: game.name.clone(),
            detail: match (&game.launch_code, &game.build.version) {
                (None, _) => {
                    "Open Battle.net once, logged in, so Snow learns how to launch it".to_owned()
                }
                (Some(_), Some(version)) => format!("Version {version}"),
                (Some(_), None) => "Installed".to_owned(),
            },
            launch_code: game.launch_code.clone(),
        })
        .collect()
}

/// The toast after "Open Battle.net".
pub fn open_message(result: &Result<BattlenetRun, String>) -> String {
    match result {
        Ok(BattlenetRun::Started) => "Starting Battle.net…".to_owned(),
        Ok(BattlenetRun::PassedToRunning) => "Battle.net: window brought back".to_owned(),
        Err(problem) => format!("Could not open Battle.net: {problem}"),
    }
}

/// The toast after "Play" on `game`.
pub fn play_message(game: &str, result: &Result<PlayRun, String>) -> String {
    match result {
        Ok(PlayRun::Requested) => format!("{game}: asked Battle.net to open it"),
        Ok(PlayRun::RequestedAfterLogin) => {
            format!("{game}: Battle.net logged in, asked it to open the game")
        }
        Err(problem) => format!("Could not open {game}: {problem}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use snowlauncher_wine::daemon_client::DaemonView;

    fn running(games: &[&str], inhibited: bool, connection: &str) -> DaemonState {
        DaemonState::Running(DaemonView {
            version: "0.1.0".to_owned(),
            prefix: "/pfx".to_owned(),
            running_games: games.iter().map(|g| (*g).to_owned()).collect(),
            suspend_inhibited: inhibited,
            game_connection: connection.to_owned(),
        })
    }

    #[test]
    fn shows_what_snowd_sees() {
        let rows = status_rows(&running(&["World of Warcraft"], true, "lost"));
        let values: Vec<&str> = rows.iter().map(|row| row.value.as_str()).collect();
        assert_eq!(
            values,
            [
                "running (v0.1.0)",
                "World of Warcraft",
                "blocked while playing",
                "lost: close the game and open it again",
            ]
        );
    }

    #[test]
    fn a_missing_daemon_is_one_row_with_a_hint() {
        let rows = status_rows(&DaemonState::NotRunning);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].value.contains("snowctl service install"));
    }

    #[test]
    fn unknown_connection_states_are_shown_as_is() {
        let rows = status_rows(&running(&[], false, "hibernating"));
        assert_eq!(rows[3].value, "hibernating");
    }

    #[test]
    fn each_house_state_has_its_illustration_and_hint() {
        let closed = BattlenetView::Closed;
        assert_eq!(house_text(&House::Missing, closed).0, Illustration::Empty);
        assert!(
            house_text(&House::Missing, closed)
                .2
                .contains("house create")
        );
        assert!(
            house_text(&House::NoBattlenet, closed)
                .2
                .contains("install-battlenet")
        );
        let ready = House::Ready {
            prefix: PathBuf::from("/pfx"),
            library: PathBuf::from("/games"),
        };
        assert!(ready.is_ready() && !House::NoBattlenet.is_ready());
        let lights: Vec<Illustration> = [
            BattlenetView::Closed,
            BattlenetView::Starting,
            BattlenetView::LoggingIn,
            BattlenetView::Ready,
        ]
        .into_iter()
        .map(|view| house_text(&ready, view).0)
        .collect();
        assert_eq!(
            lights,
            (0..=3).map(Illustration::Lights).collect::<Vec<_>>()
        );
    }

    fn log(name: &str) -> Option<PathBuf> {
        Some(PathBuf::from(name))
    }

    #[test]
    fn the_previous_sessions_log_never_counts() {
        let mut tracker = SessionTracker::default();
        let t0 = Instant::now();
        let never = |_: &Path| -> Option<StartupStage> { panic!("must not read") };
        // Closed; the newest log is an old, ready session.
        assert_eq!(
            tracker.update(false, log("old"), t0, never),
            BattlenetView::Closed
        );
        // Started, but its log is not there yet: not "ready".
        assert_eq!(
            tracker.update(true, log("old"), t0, never),
            BattlenetView::Starting
        );
        // Its own log appears.
        let view = tracker.update(true, log("new"), t0 + Duration::from_secs(3), |_| {
            Some(StartupStage::AgentReady)
        });
        assert_eq!(view, BattlenetView::LoggingIn);
        let view = tracker.update(true, log("new"), t0 + Duration::from_secs(6), |_| {
            Some(StartupStage::Ready)
        });
        assert_eq!(view, BattlenetView::Ready);
        // Ready sessions are not read again.
        let later = t0 + Duration::from_secs(600);
        assert_eq!(
            tracker.update(true, log("new"), later, never),
            BattlenetView::Ready
        );
    }

    #[test]
    fn a_session_already_running_uses_the_newest_log() {
        let mut tracker = SessionTracker::default();
        let view = tracker.update(true, log("current"), Instant::now(), |_| {
            Some(StartupStage::Ready)
        });
        assert_eq!(view, BattlenetView::Ready);
    }

    #[test]
    fn not_ready_after_a_while_is_waiting_for_login() {
        let mut tracker = SessionTracker::default();
        let t0 = Instant::now();
        let agent = |_: &Path| Some(StartupStage::AgentReady);
        assert_eq!(
            tracker.update(true, log("s"), t0, agent),
            BattlenetView::LoggingIn
        );
        let view = tracker.update(true, log("s"), t0 + LOGIN_WAIT, agent);
        assert_eq!(view, BattlenetView::WaitingForLogin);
        // Logging in afterwards makes it ready.
        let view = tracker.update(true, log("s"), t0 + LOGIN_WAIT * 2, |_| {
            Some(StartupStage::Ready)
        });
        assert_eq!(view, BattlenetView::Ready);
    }

    fn game(code: Option<&str>, version: Option<&str>) -> InstalledGame {
        InstalledGame {
            name: "World of Warcraft".to_owned(),
            path: PathBuf::from("/games/World of Warcraft"),
            build: snowlauncher_core::library::BuildInfo {
                product: "wowf".to_owned(),
                version: version.map(str::to_owned),
            },
            launch_code: code.map(str::to_owned),
        }
    }

    #[test]
    fn a_game_without_launch_code_explains_why() {
        let games = [game(Some("WoWF"), Some("1.60")), game(None, Some("1.60"))];
        let cards = game_cards(&games, |_| Some(PathBuf::from("/cache/wowf.png")));
        assert_eq!(cards[0].icon.as_deref(), Some(Path::new("/cache/wowf.png")));
        assert_eq!(cards[0].detail, "Version 1.60");
        assert_eq!(cards[0].launch_code.as_deref(), Some("WoWF"));
        assert!(cards[1].detail.contains("Open Battle.net once"));
        assert_eq!(cards[1].launch_code, None);
    }

    #[test]
    fn action_messages() {
        assert_eq!(
            play_message("World of Warcraft", &Ok(PlayRun::Requested)),
            "World of Warcraft: asked Battle.net to open it"
        );
        assert!(
            play_message("World of Warcraft", &Ok(PlayRun::RequestedAfterLogin))
                .contains("logged in")
        );
        assert!(open_message(&Err("umu-run not found".to_owned())).contains("umu-run"));
        assert_eq!(
            open_message(&Ok(BattlenetRun::Started)),
            "Starting Battle.net…"
        );
    }
}
