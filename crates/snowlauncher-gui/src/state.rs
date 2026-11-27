//! What the window shows, from what `snowd` and the house report. Pure:
//! the window only displays what these functions decide.

use std::path::PathBuf;

use snowlauncher_core::dbus::GameConnection;
use snowlauncher_core::house::BATTLENET_EXE;
use snowlauncher_wine::daemon_client::DaemonState;
use snowlauncher_wine::house::{BattlenetRun, load_record, record_path};
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

/// Which illustration goes with a house state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Illustration {
    Empty,
    Ready,
}

/// Illustration, title and description for the house part of the window.
pub fn house_text(house: &House) -> (Illustration, &'static str, String) {
    match house {
        House::Missing => (
            Illustration::Empty,
            "No house yet",
            "Create Snow Launcher's Battle.net house with `snowctl house create`.".to_owned(),
        ),
        House::NoBattlenet => (
            Illustration::Empty,
            "Battle.net is not installed",
            "Install it with `snowctl house install-battlenet --installer PATH`.".to_owned(),
        ),
        House::Ready { prefix, .. } => (
            Illustration::Ready,
            "The house is ready",
            format!("Battle.net lives in {}.", prefix.display()),
        ),
    }
}

/// One game of the library, as its row shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameCard {
    pub name: String,
    pub detail: String,
    /// `None` until Battle.net's log reveals it: "Play" stays disabled.
    pub launch_code: Option<String>,
}

pub fn game_cards(games: &[InstalledGame]) -> Vec<GameCard> {
    games
        .iter()
        .map(|game| GameCard {
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

/// The toast after an action (`what`: "Battle.net", "World of Warcraft").
pub fn action_message(what: &str, result: &Result<BattlenetRun, String>) -> String {
    match result {
        Ok(BattlenetRun::Started) => format!("Starting Battle.net for {what}…"),
        Ok(BattlenetRun::PassedToRunning) => format!("{what}: request sent to Battle.net"),
        Err(problem) => format!("Could not open {what}: {problem}"),
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
        assert_eq!(house_text(&House::Missing).0, Illustration::Empty);
        assert!(house_text(&House::Missing).2.contains("house create"));
        assert!(
            house_text(&House::NoBattlenet)
                .2
                .contains("install-battlenet")
        );
        let ready = House::Ready {
            prefix: PathBuf::from("/pfx"),
            library: PathBuf::from("/games"),
        };
        assert_eq!(house_text(&ready).0, Illustration::Ready);
        assert!(ready.is_ready() && !House::NoBattlenet.is_ready());
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
        let cards = game_cards(&[game(Some("WoWF"), Some("1.60")), game(None, Some("1.60"))]);
        assert_eq!(cards[0].detail, "Version 1.60");
        assert_eq!(cards[0].launch_code.as_deref(), Some("WoWF"));
        assert!(cards[1].detail.contains("Open Battle.net once"));
        assert_eq!(cards[1].launch_code, None);
    }

    #[test]
    fn action_messages() {
        assert_eq!(
            action_message("World of Warcraft", &Ok(BattlenetRun::PassedToRunning)),
            "World of Warcraft: request sent to Battle.net"
        );
        assert!(
            action_message("Battle.net", &Err("umu-run not found".to_owned())).contains("umu-run")
        );
    }
}
