//! `snowd`: Snow Launcher's background daemon.
//!
//! What it does today, for one Wine prefix:
//! - blocks automatic suspend while a game is running (unless the user
//!   allowed it in `config.toml`);
//! - watches the games' connections while they run and warns when one
//!   dies (e.g. after a suspend or a network outage: the game stays open
//!   but no longer talks to the server);
//! - after resume, logs how long the network took to come back;
//! - shows Snow's item in the desktop tray, with Battle.net and the
//!   games one click away.
//!
//! Restarting Battle.net comes later, once experiment E4 confirms the
//! approach.

mod connections;
mod dbus;
mod gamemode;
mod games;
mod logind;
mod networkmanager;
mod notifications;
mod portal;
mod resume;
mod tray;

use std::future::Future;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use snowlauncher_core::config::Config;
use snowlauncher_core::sleep::{SleepEvent, SleepMonitor};
use snowlauncher_wine::config_file::{choose_prefix, default_config_path, read_config};
use snowlauncher_wine::instance::{self, default_lock_path};
use snowlauncher_wine::prefix::WinePrefix;
use snowlauncher_wine::procfs::ProcfsProcessTable;
use tokio::signal::unix::{SignalKind, signal};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::connections::ConnectionMonitor;
use crate::gamemode::{ConfigReload, DbusGameMode, GameModeSession};
use crate::games::{GameGuard, watch_games};
use crate::logind::LogindSleepMonitor;
use crate::networkmanager::NmNetworkMonitor;
use crate::notifications::DesktopNotifier;
use crate::portal::PortalSuspendInhibitor;
use crate::resume::ResumeWatcher;

/// How long to wait for the network after resume before giving up.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(60);

/// How often to check whether a game started or stopped.
const GAME_POLL_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Parser)]
#[command(name = "snowd", version, about = "Snow Launcher background daemon")]
struct Args {
    /// Wine prefix to watch. Defaults to Snow Launcher's own prefix.
    #[arg(long, value_name = "PATH")]
    prefix: Option<PathBuf>,

    /// Configuration file. Defaults to ~/.config/snow-launcher/config.toml.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Args::parse();

    // `RUST_LOG=debug snowd` for more detail. Under systemd, stderr goes
    // to the journal.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        // Colors only in a terminal; the journal would store the raw
        // escape codes.
        .with_ansi(io::stderr().is_terminal())
        .init();

    info!(version = env!("CARGO_PKG_VERSION"), "snowd starting");

    // Held until `main` returns; the kernel releases it if we crash.
    let lock_path = default_lock_path()
        .context("XDG_RUNTIME_DIR is not set; snowd must run inside a user session")?;
    let _instance = instance::acquire(&lock_path)?;

    let config_path = args.config.or_else(default_config_path);
    let config = match &config_path {
        Some(path) => load_config(path),
        None => {
            warn!("could not determine the config directory; using defaults");
            Config::default()
        }
    };

    let (prefix_path, source) = choose_prefix(args.prefix, &config)?;
    let prefix = WinePrefix::open(&prefix_path).context("cannot open the prefix")?;
    info!(prefix = %prefix.path().display(), %source, "watching prefix");

    // System bus: logind, NetworkManager. Session bus: desktop portal,
    // notifications.
    let system_bus = zbus::Connection::system()
        .await
        .context("failed to connect to the system D-Bus")?;
    let session_bus = zbus::Connection::session()
        .await
        .context("failed to connect to the session D-Bus")?;

    let mut sleep_monitor = LogindSleepMonitor::connect(&system_bus)
        .await
        .context("failed to subscribe to logind sleep signals")?;
    let mut resume_watcher = ResumeWatcher {
        network: NmNetworkMonitor::new(&system_bus)
            .await
            .context("failed to create the NetworkManager client")?,
        network_timeout: NETWORK_TIMEOUT,
    };

    let game_table = ProcfsProcessTable::new();
    let mut game_guard = GameGuard::new(
        PortalSuspendInhibitor::new(session_bus.clone()),
        config.sleep,
    );
    let mut connections = ConnectionMonitor::new(
        NmNetworkMonitor::new(&system_bus)
            .await
            .context("failed to create the NetworkManager client")?,
        DesktopNotifier::new(session_bus.clone()),
    );

    let mut gamemode = GameModeSession::new(
        DbusGameMode::new(session_bus.clone()),
        config.performance.gamemode,
    );
    // The switch in the window edits config.toml; follow it while running.
    if let Some(path) = config_path.clone() {
        gamemode = gamemode.following(ConfigReload::new(path));
    }

    // D-Bus interface on the session bus, fed by the game watch.
    let (status_tx, status_rx) = tokio::sync::watch::channel(dbus::Status {
        prefix: prefix.path().display().to_string(),
        ..dbus::Status::default()
    });
    dbus::serve(&session_bus, status_rx.borrow().clone()).await?;
    let tray_status = status_rx.clone();

    let mut sigterm = signal(SignalKind::terminate()).context("failed to listen for SIGTERM")?;

    info!("watching suspend, resume and games");
    tokio::select! {
        result = watch_sleep(&mut sleep_monitor, &mut resume_watcher) => result?,
        result = watch_games(&game_table, prefix.path(), &mut game_guard, &mut connections, &mut gamemode, &status_tx, GAME_POLL_INTERVAL) => result?,
        result = dbus::publish(&session_bus, status_rx) => result?,
        () = tray::run(&session_bus, tray_status) => info!("quit from the tray menu, stopping"),
        _ = tokio::signal::ctrl_c() => info!("interrupted, stopping"),
        _ = sigterm.recv() => info!("received SIGTERM, stopping"),
    }
    // Any suspend inhibition ends with our D-Bus connection, when we exit.
    Ok(())
}

/// Reads the config file. Never fails: problems are logged and defaults
/// are used, so a typo never stops the daemon.
fn load_config(path: &Path) -> Config {
    match read_config(path) {
        Ok(Some(loaded)) => {
            for warning in &loaded.warnings {
                warn!(path = %path.display(), "{warning}");
            }
            info!(path = %path.display(), "config loaded");
            loaded.config
        }
        Ok(None) => {
            info!(path = %path.display(), "no config file; using defaults");
            Config::default()
        }
        Err(error) => {
            let error = anyhow::Error::from(error);
            warn!(
                error = format!("{error:#}"),
                "could not load config; using defaults"
            );
            Config::default()
        }
    }
}

/// Reacts to sleep events. Handling is sequential: the next event is only
/// read after the previous one was handled (events are queued meanwhile,
/// so none are lost).
trait SleepHandler {
    fn handle(&mut self, event: SleepEvent) -> impl Future<Output = ()> + Send;
}

/// Passes every sleep event to `handler` until the monitor ends.
async fn watch_sleep<M: SleepMonitor, H: SleepHandler>(
    monitor: &mut M,
    handler: &mut H,
) -> Result<()> {
    while let Some(event) = monitor
        .next_event()
        .await
        .context("failed to read sleep event")?
    {
        handler.handle(event).await;
    }
    warn!("sleep event source closed; no longer watching suspend/resume");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Records the events it receives.
    impl SleepHandler for Vec<SleepEvent> {
        fn handle(&mut self, event: SleepEvent) -> impl Future<Output = ()> + Send {
            self.push(event);
            std::future::ready(())
        }
    }

    /// Plays back a fixed list of results, then ends.
    struct ScriptedMonitor(VecDeque<Result<SleepEvent, std::io::Error>>);

    impl SleepMonitor for ScriptedMonitor {
        type Error = std::io::Error;

        fn next_event(
            &mut self,
        ) -> impl Future<Output = Result<Option<SleepEvent>, Self::Error>> + Send {
            std::future::ready(self.0.pop_front().transpose())
        }
    }

    #[test]
    fn cli_definition_is_valid() {
        use clap::CommandFactory;
        Args::command().debug_assert();
    }

    #[test]
    fn missing_or_broken_config_falls_back_to_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            load_config(&tmp.path().join("none.toml")),
            Config::default()
        );

        let broken = tmp.path().join("broken.toml");
        std::fs::write(&broken, "[sleep\n").unwrap();
        assert_eq!(load_config(&broken), Config::default());

        let valid = tmp.path().join("config.toml");
        std::fs::write(&valid, "[sleep]\nallow_suspend_while_playing = true\n").unwrap();
        assert!(load_config(&valid).sleep.allow_suspend_while_playing);
    }

    #[tokio::test]
    async fn forwards_events_in_order_until_source_ends() {
        let mut monitor = ScriptedMonitor(VecDeque::from([
            Ok(SleepEvent::Suspending),
            Ok(SleepEvent::Resumed),
        ]));
        let mut seen = Vec::new();
        watch_sleep(&mut monitor, &mut seen).await.unwrap();
        assert_eq!(seen, [SleepEvent::Suspending, SleepEvent::Resumed]);
    }

    #[tokio::test]
    async fn stops_on_error_after_forwarding_earlier_events() {
        let mut monitor = ScriptedMonitor(VecDeque::from([
            Ok(SleepEvent::Suspending),
            Err(std::io::Error::other("bus closed")),
            Ok(SleepEvent::Resumed),
        ]));
        let mut seen = Vec::new();
        let result = watch_sleep(&mut monitor, &mut seen).await;
        assert!(result.is_err());
        assert_eq!(seen, [SleepEvent::Suspending]);
    }
}
