//! What `snowd` does around suspend and resume.
//!
//! On resume, if a game was left running, it waits for the network and
//! then watches the game's TCP connections for a while. Only if one of
//! them is clearly dead (retransmitting with no answer) does it warn the
//! user: after a short suspend the game often stays online. Restarting
//! Battle.net comes later.

use std::path::PathBuf;
use std::time::Duration;

use snowlauncher_core::desktop::Notifier;
use snowlauncher_core::network::NetworkMonitor;
use snowlauncher_core::process::{Pid, ProcessTable, game_processes, game_titles};
use snowlauncher_core::sleep::{SleepEvent, lost_connection_notice};
use snowlauncher_core::tcp::{DeadConnectionDetector, SocketTable};
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

/// Outcome of watching the games' connections after resume.
#[derive(Debug)]
pub enum ConnectionCheck {
    /// A connection kept retransmitting with no answer: it is dead.
    Lost,
    /// Nothing looked dead during the whole window.
    Survived,
    /// All game processes exited meanwhile.
    GameClosed,
    /// The connections could not be read.
    Unknown(anyhow::Error),
}

/// Samples the TCP sockets of `pids` every `every`, for up to `window`.
pub async fn check_game_connections<S: SocketTable>(
    sockets: &S,
    pids: &[Pid],
    window: Duration,
    every: Duration,
) -> ConnectionCheck {
    let deadline = Instant::now() + window;
    let mut detector = DeadConnectionDetector::default();
    loop {
        let mut sample = Vec::new();
        let mut alive = 0;
        for &pid in pids {
            match sockets.tcp_sockets(pid) {
                Ok(Some(found)) => {
                    alive += 1;
                    sample.extend(found);
                }
                Ok(None) => {}
                Err(error) => return ConnectionCheck::Unknown(error.into()),
            }
        }
        if alive == 0 {
            return ConnectionCheck::GameClosed;
        }
        if detector.observe(&sample) {
            return ConnectionCheck::Lost;
        }
        if Instant::now() >= deadline {
            return ConnectionCheck::Survived;
        }
        tokio::time::sleep(every).await;
    }
}

/// How long to wait at each step after resume.
#[derive(Debug, Clone, Copy)]
pub struct ResumeTimings {
    /// Maximum wait for the network to come back.
    pub network_timeout: Duration,
    /// How long to watch the games' connections once online.
    pub check_window: Duration,
    /// Interval between connection samples.
    pub check_interval: Duration,
}

/// Handles sleep events for one prefix.
pub struct ResumeWatcher<T, M, N> {
    /// Source of processes and of their TCP sockets.
    pub system: T,
    /// Canonical path of the prefix being watched.
    pub prefix: PathBuf,
    pub notifier: M,
    pub network: N,
    pub timings: ResumeTimings,
}

impl<T, M, N> ResumeWatcher<T, M, N>
where
    T: ProcessTable + SocketTable + Sync,
    M: Notifier,
    N: NetworkMonitor,
{
    /// Games running right now, as (pids, titles).
    ///
    /// Checked after resume, not before suspend: the system may freeze
    /// `snowd` before it gets to look, and a game running right after
    /// resume was necessarily running before it.
    fn running_games(&self) -> (Vec<Pid>, Vec<&'static str>) {
        match ProcessTable::processes(&self.system) {
            Ok(processes) => {
                let games = game_processes(&processes, &self.prefix);
                let pids = games.iter().map(|game| game.pid).collect();
                (pids, game_titles(games))
            }
            Err(error) => {
                warn!(error = %error, "could not list processes after resume");
                (Vec::new(), Vec::new())
            }
        }
    }

    async fn after_resume(&mut self) {
        let (pids, titles) = self.running_games();

        info!("waiting for the network");
        let network = wait_for_network(&mut self.network, self.timings.network_timeout).await;
        match &network {
            NetworkWait::Online { after } => {
                info!(after_ms = after.as_millis(), "network is back");
            }
            NetworkWait::TimedOut => warn!(
                timeout_s = self.timings.network_timeout.as_secs(),
                "network did not come back in time"
            ),
            NetworkWait::Unavailable(error) => {
                warn!(error = format!("{error:#}"), "could not check the network");
            }
        }

        if pids.is_empty() {
            return;
        }
        let check = if matches!(network, NetworkWait::TimedOut) {
            // No network for this long: the game's session is gone.
            ConnectionCheck::Lost
        } else {
            info!(games = ?titles, "checking whether the game connection survived");
            check_game_connections(
                &self.system,
                &pids,
                self.timings.check_window,
                self.timings.check_interval,
            )
            .await
        };

        let warn_user = match check {
            ConnectionCheck::Lost => {
                info!(games = ?titles, "game connection lost during suspend; warning the user");
                true
            }
            ConnectionCheck::Survived => {
                info!(games = ?titles, "game connection survived the suspend");
                false
            }
            ConnectionCheck::GameClosed => {
                info!(games = ?titles, "game closed after resume; nothing to warn about");
                false
            }
            ConnectionCheck::Unknown(error) => {
                warn!(
                    error = format!("{error:#}"),
                    "could not check the game connection; warning the user to be safe"
                );
                true
            }
        };
        if warn_user {
            if let Some(notice) = lost_connection_notice(&titles) {
                if let Err(error) = self.notifier.notify(&notice).await {
                    warn!(error = %error, "could not show the notification");
                }
            }
        }
    }
}

impl<T, M, N> SleepHandler for ResumeWatcher<T, M, N>
where
    T: ProcessTable + SocketTable + Send + Sync,
    M: Notifier + Send,
    N: NetworkMonitor + Send,
{
    async fn handle(&mut self, event: SleepEvent) {
        match event {
            SleepEvent::Suspending => info!("system is going to sleep"),
            SleepEvent::Resumed => {
                info!("system woke up");
                self.after_resume().await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use snowlauncher_core::desktop::Notification;
    use snowlauncher_core::process::ProcessInfo;
    use snowlauncher_core::tcp::TcpSocket;
    use std::collections::VecDeque;
    use std::future::Future;
    use std::sync::Mutex;

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

    /// What the fake game's sockets look like at each sample.
    #[derive(Clone)]
    enum Sample {
        Sockets(Vec<TcpSocket>),
        Gone,
        Unreadable,
    }

    /// Fixed processes; socket samples played back in order, the last one
    /// repeating forever.
    struct FakeSystem {
        processes: Vec<ProcessInfo>,
        samples: Mutex<VecDeque<Sample>>,
    }

    impl ProcessTable for FakeSystem {
        type Error = std::io::Error;

        fn processes(&self) -> Result<Vec<ProcessInfo>, Self::Error> {
            Ok(self.processes.clone())
        }
    }

    impl SocketTable for FakeSystem {
        type Error = std::io::Error;

        fn tcp_sockets(&self, _pid: Pid) -> Result<Option<Vec<TcpSocket>>, Self::Error> {
            let mut samples = self.samples.lock().unwrap();
            let sample = if samples.len() > 1 {
                samples.pop_front()
            } else {
                samples.front().cloned()
            };
            match sample {
                Some(Sample::Sockets(sockets)) => Ok(Some(sockets)),
                Some(Sample::Gone) | None => Ok(None),
                Some(Sample::Unreadable) => Err(std::io::Error::other("permission denied")),
            }
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
    const BNET: &str = r"C:\Program Files (x86)\Battle.net\Battle.net.exe";
    const WOW: &str = r"G:\World of Warcraft\_retail_\Wow.exe";

    fn healthy() -> Sample {
        Sample::Sockets(vec![TcpSocket {
            inode: 1,
            state: 0x01,
            tx_queue: 0,
            timer: 0,
            retransmits: 0,
        }])
    }

    fn stuck() -> Sample {
        Sample::Sockets(vec![TcpSocket {
            inode: 1,
            state: 0x01,
            tx_queue: 0xA0,
            timer: 1,
            retransmits: 6,
        }])
    }

    fn watcher(
        running: &[&str],
        samples: Vec<Sample>,
        network: OnlineAfter,
    ) -> ResumeWatcher<FakeSystem, RecordingNotifier, OnlineAfter> {
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
            system: FakeSystem {
                processes,
                samples: Mutex::new(samples.into()),
            },
            prefix: PathBuf::from(PREFIX),
            notifier: RecordingNotifier::default(),
            network,
            timings: ResumeTimings {
                network_timeout: TIMEOUT,
                check_window: Duration::from_secs(60),
                check_interval: Duration::from_secs(5),
            },
        }
    }

    fn online() -> OnlineAfter {
        OnlineAfter(Duration::from_secs(5))
    }

    async fn resume(
        watcher: &mut ResumeWatcher<FakeSystem, RecordingNotifier, OnlineAfter>,
    ) -> Vec<Notification> {
        watcher.handle(SleepEvent::Resumed).await;
        std::mem::take(&mut watcher.notifier.0)
    }

    #[tokio::test(start_paused = true)]
    async fn warns_when_the_game_connection_is_dead() {
        let mut watcher = watcher(&[BNET, WOW], vec![stuck()], online());
        let notices = resume(&mut watcher).await;
        let [notice] = notices.as_slice() else {
            panic!("expected exactly one notification, got {notices:?}");
        };
        assert_eq!(notice.summary, "World of Warcraft lost its connection");
    }

    #[tokio::test(start_paused = true)]
    async fn stays_quiet_when_the_game_is_still_online() {
        // What happened after the 14 s suspend in experiment O3.
        let mut watcher = watcher(&[BNET, WOW], vec![healthy()], online());
        assert!(resume(&mut watcher).await.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn stays_quiet_when_the_connection_recovers() {
        let mut watcher = watcher(&[WOW], vec![stuck(), healthy()], online());
        assert!(resume(&mut watcher).await.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn stays_quiet_when_the_game_was_closed() {
        let mut watcher = watcher(&[WOW], vec![Sample::Gone], online());
        assert!(resume(&mut watcher).await.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn warns_to_be_safe_when_sockets_are_unreadable() {
        let mut watcher = watcher(&[WOW], vec![Sample::Unreadable], online());
        assert_eq!(resume(&mut watcher).await.len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn warns_when_the_network_never_comes_back() {
        let offline = OnlineAfter(Duration::from_secs(3600));
        let mut watcher = watcher(&[WOW], vec![healthy()], offline);
        assert_eq!(resume(&mut watcher).await.len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn stays_quiet_without_games() {
        let mut watcher = watcher(&[BNET], vec![stuck()], online());
        assert!(resume(&mut watcher).await.is_empty());
    }

    #[tokio::test]
    async fn does_nothing_before_suspending() {
        let mut watcher = watcher(&[WOW], vec![stuck()], online());
        watcher.handle(SleepEvent::Suspending).await;
        assert!(watcher.notifier.0.is_empty());
    }
}
