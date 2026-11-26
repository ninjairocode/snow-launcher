//! What `snowd` does around suspend and resume.
//!
//! On resume it warns the user if a game was left running (its connection
//! did not survive the suspend), then waits for the network and logs how
//! long that took. Restarting Battle.net comes later.

use std::path::PathBuf;
use std::time::Duration;

use snowlauncher_core::desktop::Notifier;
use snowlauncher_core::network::NetworkMonitor;
use snowlauncher_core::process::{ProcessTable, running_games};
use snowlauncher_core::sleep::{SleepEvent, lost_connection_notice};
use tokio::time::Instant;
use tracing::{info, warn};

use crate::SleepHandler;

/// Outcome of waiting for the network after resume.
#[derive(Debug)]
pub enum NetworkWait {
    Online {
        after: Duration,
    },
    TimedOut,
    /// The network state could not be checked (e.g. no NetworkManager).
    Unavailable(anyhow::Error),
}

/// Waits up to `timeout` for the network to come back.
pub async fn wait_for_network<N: NetworkMonitor>(
    network: &mut N,
    timeout: Duration,
) -> NetworkWait {
    let start = Instant::now();
    match tokio::time::timeout(timeout, network.wait_online()).await {
        Ok(Ok(())) => NetworkWait::Online {
            after: start.elapsed(),
        },
        Ok(Err(error)) => NetworkWait::Unavailable(error.into()),
        Err(_elapsed) => NetworkWait::TimedOut,
    }
}

/// Handles sleep events for one prefix.
pub struct ResumeWatcher<T, M, N> {
    pub processes: T,
    /// Canonical path of the prefix being watched.
    pub prefix: PathBuf,
    pub notifier: M,
    pub network: N,
    pub network_timeout: Duration,
}

impl<T, M, N> ResumeWatcher<T, M, N>
where
    T: ProcessTable,
    M: Notifier,
{
    /// Tells the user about games that were running through the suspend.
    ///
    /// Checked after resume, not before suspend: the system may freeze
    /// `snowd` before it gets to look, and a game running right after
    /// resume was necessarily running before it.
    async fn warn_about_running_games(&mut self) {
        let processes = match self.processes.processes() {
            Ok(processes) => processes,
            Err(error) => {
                warn!(error = %error, "could not list processes after resume");
                return;
            }
        };
        let games = running_games(&processes, &self.prefix);
        let Some(notice) = lost_connection_notice(&games) else {
            return;
        };
        info!(
            ?games,
            "games were running during suspend; warning the user"
        );
        if let Err(error) = self.notifier.notify(&notice).await {
            warn!(error = %error, "could not show the notification");
        }
    }
}

impl<T, M, N> SleepHandler for ResumeWatcher<T, M, N>
where
    T: ProcessTable + Send,
    M: Notifier + Send,
    N: NetworkMonitor + Send,
{
    async fn handle(&mut self, event: SleepEvent) {
        match event {
            SleepEvent::Suspending => info!("system is going to sleep"),
            SleepEvent::Resumed => {
                info!("system woke up");
                self.warn_about_running_games().await;
                info!("waiting for the network");
                match wait_for_network(&mut self.network, self.network_timeout).await {
                    NetworkWait::Online { after } => {
                        info!(after_ms = after.as_millis(), "network is back");
                    }
                    NetworkWait::TimedOut => warn!(
                        timeout_s = self.network_timeout.as_secs(),
                        "network did not come back in time"
                    ),
                    NetworkWait::Unavailable(error) => {
                        warn!(error = format!("{error:#}"), "could not check the network");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use snowlauncher_core::desktop::Notification;
    use snowlauncher_core::process::{Pid, ProcessInfo};
    use std::future::Future;

    /// Comes online after a fixed delay (in Tokio's virtual time).
    struct OnlineAfter(Duration);

    impl NetworkMonitor for OnlineAfter {
        type Error = std::io::Error;

        fn wait_online(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
            let delay = self.0;
            async move {
                tokio::time::sleep(delay).await;
                Ok(())
            }
        }
    }

    struct Broken;

    impl NetworkMonitor for Broken {
        type Error = std::io::Error;

        fn wait_online(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
            std::future::ready(Err(std::io::Error::other("no NetworkManager")))
        }
    }

    const TIMEOUT: Duration = Duration::from_secs(60);

    // `start_paused` runs on virtual time: sleeps finish instantly, but
    // durations are measured as if they had really passed.

    #[tokio::test(start_paused = true)]
    async fn reports_how_long_the_network_took() {
        let mut network = OnlineAfter(Duration::from_secs(5));
        let result = wait_for_network(&mut network, TIMEOUT).await;
        assert!(matches!(result, NetworkWait::Online { after } if after == Duration::from_secs(5)));
    }

    #[tokio::test(start_paused = true)]
    async fn already_online_returns_immediately() {
        let mut network = OnlineAfter(Duration::ZERO);
        let result = wait_for_network(&mut network, TIMEOUT).await;
        assert!(matches!(result, NetworkWait::Online { after } if after == Duration::ZERO));
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_timeout() {
        let mut network = OnlineAfter(Duration::from_secs(3600));
        let result = wait_for_network(&mut network, TIMEOUT).await;
        assert!(matches!(result, NetworkWait::TimedOut));
    }

    #[tokio::test]
    async fn reports_monitor_errors() {
        let result = wait_for_network(&mut Broken, TIMEOUT).await;
        assert!(matches!(result, NetworkWait::Unavailable(_)));
    }

    struct FixedTable(Vec<ProcessInfo>);

    impl ProcessTable for FixedTable {
        type Error = std::io::Error;

        fn processes(&self) -> Result<Vec<ProcessInfo>, Self::Error> {
            Ok(self.0.clone())
        }
    }

    #[derive(Default)]
    struct RecordingNotifier(Vec<Notification>);

    impl Notifier for RecordingNotifier {
        type Error = std::io::Error;

        fn notify(
            &mut self,
            notification: &Notification,
        ) -> impl Future<Output = Result<(), Self::Error>> + Send {
            self.0.push(notification.clone());
            std::future::ready(Ok(()))
        }
    }

    const PREFIX: &str = "/home/user/.local/share/snow-launcher/prefixes/battlenet";

    fn watcher(running: &[&str]) -> ResumeWatcher<FixedTable, RecordingNotifier, OnlineAfter> {
        let processes = running
            .iter()
            .zip(1..)
            .map(|(exe, pid)| ProcessInfo {
                pid: Pid(pid),
                name: String::new(),
                cmdline: vec![(*exe).to_owned()],
                wineprefix: Some(PathBuf::from(PREFIX)),
            })
            .collect();
        ResumeWatcher {
            processes: FixedTable(processes),
            prefix: PathBuf::from(PREFIX),
            notifier: RecordingNotifier::default(),
            network: OnlineAfter(Duration::ZERO),
            network_timeout: TIMEOUT,
        }
    }

    #[tokio::test]
    async fn warns_on_resume_when_a_game_is_running() {
        let mut watcher = watcher(&[
            r"C:\Program Files (x86)\Battle.net\Battle.net.exe",
            r"G:\World of Warcraft\_retail_\Wow.exe",
        ]);
        watcher.handle(SleepEvent::Resumed).await;
        let [notice] = watcher.notifier.0.as_slice() else {
            panic!("expected exactly one notification");
        };
        assert_eq!(notice.summary, "World of Warcraft lost its connection");
    }

    #[tokio::test]
    async fn stays_quiet_without_games() {
        let mut watcher = watcher(&[r"C:\Program Files (x86)\Battle.net\Battle.net.exe"]);
        watcher.handle(SleepEvent::Resumed).await;
        assert!(watcher.notifier.0.is_empty());
    }

    #[tokio::test]
    async fn does_not_warn_before_suspending() {
        let mut watcher = watcher(&[r"G:\World of Warcraft\_retail_\Wow.exe"]);
        watcher.handle(SleepEvent::Suspending).await;
        assert!(watcher.notifier.0.is_empty());
    }
}
