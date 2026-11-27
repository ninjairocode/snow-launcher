//! Watches for running games: blocks automatic suspend while they run and
//! has their connections watched (see `connections.rs`).

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use snowlauncher_core::config::SleepConfig;
use snowlauncher_core::desktop::{Notifier, SuspendInhibitor};
use snowlauncher_core::network::NetworkMonitor;
use snowlauncher_core::process::{Pid, ProcessTable, game_processes, game_titles};
use snowlauncher_core::sleep::should_inhibit_suspend;
use snowlauncher_core::tcp::SocketTable;
use tokio::time::MissedTickBehavior;
use tracing::{info, warn};

use crate::connections::ConnectionMonitor;
use crate::dbus::Status;

/// Keeps the inhibition in sync with the set of running games.
pub struct GameGuard<I> {
    inhibitor: I,
    config: SleepConfig,
    inhibiting: bool,
    games: Vec<&'static str>,
    /// Avoids repeating the same warning every poll while the desktop
    /// refuses to inhibit.
    failing: bool,
}

impl<I: SuspendInhibitor> GameGuard<I> {
    pub fn new(inhibitor: I, config: SleepConfig) -> Self {
        Self {
            inhibitor,
            config,
            inhibiting: false,
            games: Vec::new(),
            failing: false,
        }
    }

    /// Whether automatic suspend is currently being blocked.
    pub fn is_inhibiting(&self) -> bool {
        self.inhibiting
    }

    /// Applies the current set of running games.
    pub async fn update(&mut self, games: Vec<&'static str>) {
        if games != self.games {
            if games.is_empty() {
                info!("no game running");
            } else if self.config.allow_suspend_while_playing {
                info!(?games, "game running; automatic suspend allowed by config");
            } else {
                info!(?games, "game running");
            }
            self.games = games;
        }

        let wanted = should_inhibit_suspend(!self.games.is_empty(), &self.config);
        if wanted == self.inhibiting {
            return;
        }
        let result = if wanted {
            let reason = format!(
                "{} is running. Suspending would disconnect it from the server.",
                self.games.join(", ")
            );
            self.inhibitor.inhibit(&reason).await
        } else {
            self.inhibitor.release().await
        };
        match result {
            Ok(()) => {
                self.inhibiting = wanted;
                self.failing = false;
                if wanted {
                    info!("automatic suspend blocked");
                } else {
                    info!("automatic suspend allowed again");
                }
            }
            Err(error) => {
                if !self.failing {
                    warn!(error = %error, wanted, "could not change suspend inhibition; will retry");
                }
                self.failing = true;
            }
        }
    }
}

/// Polls the processes of `prefix` every `every`, then updates the
/// suspend inhibition and samples the games' connections.
/// Only returns on a fatal error (e.g. `/proc` unreadable).
pub async fn watch_games<T, I, N, M>(
    system: &T,
    prefix: &Path,
    guard: &mut GameGuard<I>,
    connections: &mut ConnectionMonitor<N, M>,
    status: &tokio::sync::watch::Sender<Status>,
    every: Duration,
) -> Result<()>
where
    T: ProcessTable + SocketTable,
    I: SuspendInhibitor,
    N: NetworkMonitor,
    M: Notifier,
{
    let mut ticker = tokio::time::interval(every);
    // After a suspend, do not fire a burst of missed ticks.
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        let processes = ProcessTable::processes(system).context("failed to list processes")?;
        let games = game_processes(&processes, prefix);
        let pids: Vec<Pid> = games.iter().map(|game| game.pid).collect();
        let titles = game_titles(games);
        guard.update(titles.clone()).await;
        connections.update(system, &pids, &titles).await;

        // Publish only real changes, so D-Bus clients get no noise.
        status.send_if_modified(|current| {
            let games: Vec<String> = titles.iter().map(|&title| title.to_owned()).collect();
            let changed = current.running_games != games
                || current.suspend_inhibited != guard.is_inhibiting()
                || current.game_connection != connections.state();
            if changed {
                current.running_games = games;
                current.suspend_inhibited = guard.is_inhibiting();
                current.game_connection = connections.state();
            }
            changed
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;

    /// Records calls; fails while `broken` is true.
    #[derive(Default)]
    struct FakeInhibitor {
        calls: Vec<String>,
        broken: bool,
    }

    impl SuspendInhibitor for FakeInhibitor {
        type Error = std::io::Error;

        fn inhibit(
            &mut self,
            reason: &str,
        ) -> impl Future<Output = Result<(), Self::Error>> + Send {
            let result = if self.broken {
                Err(std::io::Error::other("portal unavailable"))
            } else {
                self.calls.push(format!("inhibit: {reason}"));
                Ok(())
            };
            std::future::ready(result)
        }

        fn release(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
            self.calls.push("release".to_owned());
            std::future::ready(Ok(()))
        }
    }

    const WOW: &str = "World of Warcraft";

    #[tokio::test]
    async fn inhibits_while_a_game_runs_then_releases() {
        let mut guard = GameGuard::new(FakeInhibitor::default(), SleepConfig::default());
        guard.update(vec![]).await;
        guard.update(vec![WOW]).await;
        guard.update(vec![WOW]).await; // no duplicate call
        guard.update(vec![]).await;
        assert_eq!(
            guard.inhibitor.calls,
            [
                "inhibit: World of Warcraft is running. Suspending would disconnect it from the server.",
                "release",
            ]
        );
    }

    #[tokio::test]
    async fn config_can_allow_suspend_while_playing() {
        let config = SleepConfig {
            allow_suspend_while_playing: true,
        };
        let mut guard = GameGuard::new(FakeInhibitor::default(), config);
        guard.update(vec![WOW]).await;
        assert!(guard.inhibitor.calls.is_empty());
    }

    #[tokio::test]
    async fn retries_after_a_failure() {
        let broken = FakeInhibitor {
            broken: true,
            ..FakeInhibitor::default()
        };
        let mut guard = GameGuard::new(broken, SleepConfig::default());
        guard.update(vec![WOW]).await;
        assert!(!guard.inhibiting);

        guard.inhibitor.broken = false;
        guard.update(vec![WOW]).await;
        assert!(guard.inhibiting);
        assert_eq!(guard.inhibitor.calls.len(), 1);
    }
}
