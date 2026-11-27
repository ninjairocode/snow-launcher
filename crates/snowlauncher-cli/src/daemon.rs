//! Asking the running `snowd` what it sees, over its D-Bus interface.

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};
use snowlauncher_core::dbus::{BUS_NAME, GameConnection, INTERFACE, OBJECT_PATH, property};
use snowlauncher_wine::instance::{default_lock_path, running_instance};

/// What `snowd` reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonView {
    pub version: String,
    pub prefix: String,
    pub running_games: Vec<String>,
    pub suspend_inhibited: bool,
    pub game_connection: String,
}

#[derive(Debug)]
pub enum DaemonState {
    Running(DaemonView),
    /// Holds the single-instance lock but has no D-Bus interface: an
    /// older snowd, from before the interface existed.
    RunningWithoutInterface {
        pid: Option<u32>,
    },
    NotRunning,
    /// Could not ask (no session bus, unexpected error...).
    Unknown(anyhow::Error),
}

/// Queries `snowd`. Never fails: problems become [`DaemonState::Unknown`].
pub fn query() -> DaemonState {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => return DaemonState::Unknown(error.into()),
    };
    match runtime.block_on(query_async()) {
        Ok(Some(view)) => DaemonState::Running(view),
        Ok(None) => match default_lock_path().map(|path| running_instance(&path)) {
            Some(Ok(Some(instance))) => DaemonState::RunningWithoutInterface { pid: instance.pid },
            _ => DaemonState::NotRunning,
        },
        Err(error) => DaemonState::Unknown(error),
    }
}

async fn query_async() -> Result<Option<DaemonView>> {
    let bus = zbus::Connection::session()
        .await
        .context("could not connect to the session D-Bus")?;
    let proxy = zbus::proxy::Builder::<zbus::Proxy<'_>>::new(&bus)
        .destination(BUS_NAME)?
        .path(OBJECT_PATH)?
        .interface(INTERFACE)?
        // Read fresh values; do not subscribe to changes.
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await?;

    // The first read tells whether anyone owns the name.
    let version: String = match proxy.get_property(property::VERSION).await {
        Ok(version) => version,
        Err(error) if nobody_owns_the_name(&error) => return Ok(None),
        Err(error) => return Err(error).context("could not read snowd's status"),
    };
    Ok(Some(DaemonView {
        version,
        prefix: proxy.get_property(property::PREFIX).await?,
        running_games: proxy.get_property(property::RUNNING_GAMES).await?,
        suspend_inhibited: proxy.get_property(property::SUSPEND_INHIBITED).await?,
        game_connection: proxy.get_property(property::GAME_CONNECTION).await?,
    }))
}

/// The bus answers "no such name" when snowd is not running, either as a
/// standard D-Bus error or as a raw method error with the same name.
fn nobody_owns_the_name(error: &zbus::Error) -> bool {
    const NOT_RUNNING: [&str; 2] = [
        "org.freedesktop.DBus.Error.ServiceUnknown",
        "org.freedesktop.DBus.Error.NameHasNoOwner",
    ];
    match error {
        zbus::Error::FDO(fdo) => matches!(
            **fdo,
            zbus::fdo::Error::ServiceUnknown(_) | zbus::fdo::Error::NameHasNoOwner(_)
        ),
        zbus::Error::MethodError(name, _, _) => NOT_RUNNING.contains(&name.as_str()),
        _ => false,
    }
}

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
            let _ = writeln!(out, "snowd:      unknown ({error:#})");
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
