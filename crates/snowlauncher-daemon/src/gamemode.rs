//! Feral GameMode while a game runs (`[performance] gamemode`, off by
//! default). Validated in experiment E15 (O16): GameMode registers a
//! process by its pid over D-Bus, so `snowd`, which already sees the games,
//! registers only the game's processes and removes them when they end.
//! No `gamemoderun`, no `LD_PRELOAD` (a problem inside umu's container),
//! and Battle.net never gets game mode.

use std::future::Future;
use std::path::PathBuf;
use std::time::SystemTime;

use snowlauncher_core::process::Pid;
use tracing::{debug, info, warn};

/// GameMode's D-Bus service, behind a trait for tests.
pub trait GameModeClient {
    type Error: std::fmt::Display;

    fn register(&mut self, pid: Pid) -> impl Future<Output = Result<(), Self::Error>> + Send;
    fn unregister(&mut self, pid: Pid) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// Follows `[performance] gamemode` in `config.toml`, so the switch in
/// the window works without restarting `snowd`: the file is only read
/// again when its modification time changes.
pub struct ConfigReload {
    path: PathBuf,
    seen: Option<SystemTime>,
}

impl ConfigReload {
    pub fn new(path: PathBuf) -> Self {
        let seen = modified(&path);
        Self { path, seen }
    }

    /// The new setting, if the file changed since the last look.
    fn changed(&mut self) -> Option<bool> {
        let now = modified(&self.path);
        if now == self.seen {
            return None;
        }
        self.seen = now;
        // A missing or broken file means the default (off), like at start.
        let enabled = snowlauncher_wine::config_file::read_config(&self.path)
            .ok()
            .flatten()
            .is_some_and(|loaded| loaded.config.performance.gamemode);
        Some(enabled)
    }
}

fn modified(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
}

/// Keeps GameMode's registrations in step with the running games.
pub struct GameModeSession<C> {
    client: C,
    enabled: bool,
    registered: Vec<Pid>,
    /// Warn once, not every poll, while GameMode is unavailable.
    warned: bool,
    reload: Option<ConfigReload>,
}

impl<C: GameModeClient> GameModeSession<C> {
    pub fn new(client: C, enabled: bool) -> Self {
        Self {
            client,
            enabled,
            registered: Vec::new(),
            warned: false,
            reload: None,
        }
    }

    /// Also follow changes to the setting in this config file.
    pub fn following(mut self, reload: ConfigReload) -> Self {
        self.reload = Some(reload);
        self
    }

    /// Turns GameMode use on or off; off lets go of every game at once.
    pub async fn set_enabled(&mut self, enabled: bool) {
        if enabled == self.enabled {
            return;
        }
        info!(enabled, "GameMode setting changed");
        self.enabled = enabled;
        if !enabled {
            for pid in std::mem::take(&mut self.registered) {
                if let Err(error) = self.client.unregister(pid).await {
                    debug!(pid = pid.0, %error, "GameMode: already unregistered");
                }
            }
        }
    }

    /// Registers new game processes and unregisters the ones that ended.
    pub async fn update(&mut self, games: &[Pid]) {
        if let Some(enabled) = self.reload.as_mut().and_then(ConfigReload::changed) {
            self.set_enabled(enabled).await;
        }
        if !self.enabled {
            return;
        }
        let gone: Vec<Pid> = self
            .registered
            .iter()
            .copied()
            .filter(|pid| !games.contains(pid))
            .collect();
        for pid in gone {
            // GameMode drops a process that ended by itself, and then
            // answers -1 here: expected, so only for debugging.
            if let Err(error) = self.client.unregister(pid).await {
                debug!(pid = pid.0, %error, "GameMode: already unregistered");
            }
            self.registered.retain(|registered| *registered != pid);
        }
        for &pid in games {
            if self.registered.contains(&pid) {
                continue;
            }
            match self.client.register(pid).await {
                Ok(()) => {
                    info!(pid = pid.0, "GameMode on for the game");
                    self.registered.push(pid);
                    self.warned = false;
                }
                Err(error) if !self.warned => {
                    warn!(%error, "GameMode is enabled but could not be used (is gamemode installed?)");
                    self.warned = true;
                }
                Err(_) => {}
            }
        }
    }
}

#[zbus::proxy(
    interface = "com.feralinteractive.GameMode",
    default_service = "com.feralinteractive.GameMode",
    default_path = "/com/feralinteractive/GameMode"
)]
trait GameMode {
    #[zbus(name = "RegisterGameByPID")]
    fn register_game_by_pid(&self, caller: i32, game: i32) -> zbus::Result<i32>;
    #[zbus(name = "UnregisterGameByPID")]
    fn unregister_game_by_pid(&self, caller: i32, game: i32) -> zbus::Result<i32>;
}

/// The real GameMode, on the session bus.
pub struct DbusGameMode {
    session_bus: zbus::Connection,
}

impl DbusGameMode {
    pub fn new(session_bus: zbus::Connection) -> Self {
        Self { session_bus }
    }

    async fn call(&self, pid: Pid, register: bool) -> zbus::Result<()> {
        let invalid = || zbus::Error::Failure(format!("pid {} out of range", pid.0));
        let game = i32::try_from(pid.0).map_err(|_| invalid())?;
        let caller = i32::try_from(std::process::id()).map_err(|_| invalid())?;
        let proxy = GameModeProxy::new(&self.session_bus).await?;
        let status = if register {
            proxy.register_game_by_pid(caller, game).await?
        } else {
            proxy.unregister_game_by_pid(caller, game).await?
        };
        // GameMode answers 0 on success and -1 on a refusal.
        if status < 0 {
            return Err(zbus::Error::Failure(format!(
                "GameMode refused pid {} (status {status})",
                pid.0
            )));
        }
        Ok(())
    }
}

impl GameModeClient for DbusGameMode {
    type Error = zbus::Error;

    async fn register(&mut self, pid: Pid) -> zbus::Result<()> {
        self.call(pid, true).await
    }

    async fn unregister(&mut self, pid: Pid) -> zbus::Result<()> {
        self.call(pid, false).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records calls; refuses everything while `broken`.
    #[derive(Default)]
    struct FakeGameMode {
        calls: Vec<String>,
        broken: bool,
    }

    impl GameModeClient for FakeGameMode {
        type Error = String;

        fn register(&mut self, pid: Pid) -> impl Future<Output = Result<(), String>> + Send {
            self.calls.push(format!("register {}", pid.0));
            let result = if self.broken {
                Err("no gamemoded".to_owned())
            } else {
                Ok(())
            };
            std::future::ready(result)
        }

        fn unregister(&mut self, pid: Pid) -> impl Future<Output = Result<(), String>> + Send {
            self.calls.push(format!("unregister {}", pid.0));
            std::future::ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn follows_the_games() {
        let mut session = GameModeSession::new(FakeGameMode::default(), true);
        session.update(&[Pid(10)]).await;
        session.update(&[Pid(10)]).await; // same game: nothing new
        session.update(&[Pid(10), Pid(11)]).await;
        session.update(&[]).await;
        assert_eq!(
            session.client.calls,
            [
                "register 10",
                "register 11",
                "unregister 10",
                "unregister 11"
            ]
        );
    }

    #[tokio::test]
    async fn turning_it_off_lets_go_of_the_games() {
        let mut session = GameModeSession::new(FakeGameMode::default(), true);
        session.update(&[Pid(10)]).await;
        session.set_enabled(false).await;
        session.update(&[Pid(10)]).await; // off: not registered again
        session.set_enabled(true).await;
        session.update(&[Pid(10)]).await;
        assert_eq!(
            session.client.calls,
            ["register 10", "unregister 10", "register 10"]
        );
    }

    #[tokio::test]
    async fn follows_the_config_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        let mut session = GameModeSession::new(FakeGameMode::default(), false)
            .following(ConfigReload::new(path.clone()));
        session.update(&[Pid(10)]).await;
        assert!(session.client.calls.is_empty());

        std::fs::write(&path, "[performance]\ngamemode = true\n").unwrap();
        session.update(&[Pid(10)]).await;
        assert_eq!(session.client.calls, ["register 10"]);
    }

    #[tokio::test]
    async fn off_means_no_calls() {
        let mut session = GameModeSession::new(FakeGameMode::default(), false);
        session.update(&[Pid(10)]).await;
        assert!(session.client.calls.is_empty());
    }

    #[tokio::test]
    async fn unavailable_gamemode_is_retried_quietly() {
        let mut session = GameModeSession::new(
            FakeGameMode {
                broken: true,
                ..FakeGameMode::default()
            },
            true,
        );
        session.update(&[Pid(10)]).await;
        session.update(&[Pid(10)]).await;
        // Not registered, so retried each poll; warned only once.
        assert_eq!(session.client.calls, ["register 10", "register 10"]);
        assert!(session.warned);
        assert!(session.registered.is_empty());
    }
}
