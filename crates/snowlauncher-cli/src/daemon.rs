//! The `snowd` block of `snowctl status` (the query lives in
//! `snowlauncher_wine::daemon_client`, shared with the GUI).

use std::fmt::Write as _;
use std::path::Path;

use snowlauncher_core::dbus::GameConnection;
pub use snowlauncher_wine::daemon_client::{DaemonState, query};

/// The `snowd` block of `snowctl status`. `prefix` is the one being shown,
/// to point out when the daemon watches another.
pub fn render(state: &DaemonState, prefix: &Path) -> String {
    let mut out = String::new();
    // Writing to a String cannot fail, so the results are ignored.
    match state {
        DaemonState::NotRunning => {
            let _ = writeln!(
                out,
                "snowd:      not running (see `snowctl service install`)"
            );
        }
        DaemonState::RunningWithoutInterface { pid } => {
            let pid = pid.map(|pid| format!(", pid {pid}")).unwrap_or_default();
            let _ = writeln!(
                out,
                "snowd:      running{pid}, but too old to report its status (update it)"
            );
        }
        DaemonState::Unknown(error) => {
            // Show the whole cause chain on one line.
            let mut text = error.to_string();
            let mut source = std::error::Error::source(error);
            while let Some(cause) = source {
                text.push_str(&format!(": {cause}"));
                source = cause.source();
            }
            let _ = writeln!(out, "snowd:      unknown ({text})");
        }
        DaemonState::Running(view) => {
            let _ = writeln!(out, "snowd:      running (v{})", view.version);
            if Path::new(&view.prefix) != prefix {
                let _ = writeln!(out, "  Watching another prefix: {}", view.prefix);
            }
            let games = if view.running_games.is_empty() {
                "none".to_owned()
            } else {
                view.running_games.join(", ")
            };
            let _ = writeln!(out, "  Games:            {games}");
            let suspend = if view.suspend_inhibited {
                "blocked while playing"
            } else {
                "allowed"
            };
            let _ = writeln!(out, "  Auto-suspend:     {suspend}");
            let connection = match view.game_connection.parse::<GameConnection>() {
                Ok(GameConnection::NoGame) => "no game running".to_owned(),
                Ok(GameConnection::Ok) => "ok".to_owned(),
                Ok(GameConnection::NetworkDown) => "network is down".to_owned(),
                Ok(GameConnection::Lost) => "lost: close the game and open it again".to_owned(),
                // A newer snowd may report states this snowctl does not know.
                Err(_) => view.game_connection.clone(),
            };
            let _ = writeln!(out, "  Game connection:  {connection}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use snowlauncher_wine::daemon_client::DaemonView;

    fn view() -> DaemonView {
        DaemonView {
            version: "0.1.0".to_owned(),
            prefix: "/home/user/Games/battlenet".to_owned(),
            running_games: vec!["World of Warcraft".to_owned()],
            suspend_inhibited: true,
            game_connection: "lost".to_owned(),
        }
    }

    #[test]
    fn shows_what_the_daemon_sees() {
        let out = render(
            &DaemonState::Running(view()),
            Path::new("/home/user/Games/battlenet"),
        );
        assert_eq!(
            out,
            "snowd:      running (v0.1.0)\n\
             \x20 Games:            World of Warcraft\n\
             \x20 Auto-suspend:     blocked while playing\n\
             \x20 Game connection:  lost: close the game and open it again\n"
        );
    }

    #[test]
    fn points_out_a_different_prefix_and_unknown_states() {
        let mut other = view();
        other.game_connection = "hibernating".to_owned();
        let out = render(&DaemonState::Running(other), Path::new("/somewhere/else"));
        assert!(out.contains("Watching another prefix: /home/user/Games/battlenet"));
        assert!(out.contains("Game connection:  hibernating"));
    }

    #[test]
    fn tells_an_old_daemon_apart_from_none() {
        let out = render(
            &DaemonState::RunningWithoutInterface { pid: Some(42) },
            Path::new("/x"),
        );
        assert!(out.contains("running, pid 42, but too old"));
    }

    #[test]
    fn says_when_not_running() {
        let out = render(&DaemonState::NotRunning, Path::new("/x"));
        assert!(out.contains("not running"));
        assert!(out.contains("snowctl service install"));
    }
}
