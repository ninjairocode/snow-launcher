//! Names and values of `snowd`'s D-Bus interface, shared by the daemon
//! (server) and its clients (`snowctl`, later the GUI).
//!
//! The interface lives on the **session** bus, so only the user's own
//! programs can see it. It only has read-only properties for now; nothing
//! can tell the daemon to do anything through it.

use std::fmt;
use std::str::FromStr;

/// Well-known bus name. The `1` is the interface's major version.
pub const BUS_NAME: &str = "io.github.ninjairocode.SnowLauncher1";
pub const OBJECT_PATH: &str = "/io/github/ninjairocode/SnowLauncher1";
pub const INTERFACE: &str = "io.github.ninjairocode.SnowLauncher1";

/// Property names, as seen on the bus.
pub mod property {
    pub const VERSION: &str = "Version";
    pub const PREFIX: &str = "Prefix";
    pub const RUNNING_GAMES: &str = "RunningGames";
    pub const SUSPEND_INHIBITED: &str = "SuspendInhibited";
    pub const GAME_CONNECTION: &str = "GameConnection";
}

/// State of the games' connections, as published in `GameConnection`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GameConnection {
    /// No game running.
    #[default]
    NoGame,
    /// A game is running and its connections look fine.
    Ok,
    /// A game is running but the local network is down.
    NetworkDown,
    /// A game's connection died; the user was warned.
    Lost,
}

impl GameConnection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoGame => "no-game",
            Self::Ok => "ok",
            Self::NetworkDown => "network-down",
            Self::Lost => "lost",
        }
    }
}

impl fmt::Display for GameConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown game connection state `{0}`")]
pub struct UnknownGameConnection(pub String);

impl FromStr for GameConnection {
    type Err = UnknownGameConnection;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        [Self::NoGame, Self::Ok, Self::NetworkDown, Self::Lost]
            .into_iter()
            .find(|state| state.as_str() == text)
            .ok_or_else(|| UnknownGameConnection(text.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_connection_round_trips_through_text() {
        for state in [
            GameConnection::NoGame,
            GameConnection::Ok,
            GameConnection::NetworkDown,
            GameConnection::Lost,
        ] {
            assert_eq!(state.as_str().parse::<GameConnection>(), Ok(state));
        }
        assert!("asleep".parse::<GameConnection>().is_err());
    }

    #[test]
    fn names_are_consistent() {
        assert_eq!(INTERFACE, BUS_NAME);
        assert_eq!(OBJECT_PATH, format!("/{}", BUS_NAME.replace('.', "/")));
    }
}
