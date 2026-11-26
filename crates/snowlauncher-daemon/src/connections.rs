//! Watches the games' TCP connections while they run, and warns the user
//! when one dies.
//!
//! After a long enough network outage (a suspend, a pulled cable...) the
//! server drops the game's session. When the network is back the game
//! keeps a half-open connection: you can still walk around, but nothing
//! reaches the server, and the game may take ~15 minutes to notice.

use snowlauncher_core::desktop::Notifier;
use snowlauncher_core::network::NetworkMonitor;
use snowlauncher_core::process::Pid;
use snowlauncher_core::tcp::{DeadConnectionDetector, SocketTable, lost_connection_notice};
use tracing::{debug, info, warn};

pub struct ConnectionMonitor<N, M> {
    network: N,
    notifier: M,
    detector: DeadConnectionDetector,
    /// Game processes being watched. When they change (game started,
    /// closed or restarted), everything starts over.
    tracked: Vec<Pid>,
    /// Already warned about the current problem: do not repeat until the
    /// connections look normal again.
    warned: bool,
    /// Avoids repeating the same log line every poll.
    network_down_logged: bool,
    read_failing: bool,
}

impl<N: NetworkMonitor, M: Notifier> ConnectionMonitor<N, M> {
    pub fn new(network: N, notifier: M) -> Self {
        Self {
            network,
            notifier,
            detector: DeadConnectionDetector::default(),
            tracked: Vec::new(),
            warned: false,
            network_down_logged: false,
            read_failing: false,
        }
    }

    /// Takes one sample of the connections of the running games.
    /// Called on every poll, with the games found in the prefix.
    pub async fn update<S: SocketTable>(
        &mut self,
        sockets: &S,
        pids: &[Pid],
        titles: &[&'static str],
    ) {
        if pids != self.tracked.as_slice() {
            self.tracked = pids.to_vec();
            self.detector.reset();
            self.warned = false;
        }
        if pids.is_empty() {
            return;
        }

        // While the local network is down, retransmitting is expected and
        // says nothing about the server: the connection may recover once
        // the network is back. If the state cannot be read, assume online.
        let online = self.network.is_online().await.unwrap_or(true);
        if !online {
            if !self.network_down_logged {
                debug!("network is down; game retransmissions do not count");
                self.network_down_logged = true;
            }
            self.detector.reset();
            return;
        }
        self.network_down_logged = false;

        let mut sample = Vec::new();
        for &pid in pids {
            match sockets.tcp_sockets(pid) {
                Ok(Some(found)) => sample.extend(found),
                // Exited between polls; the next poll drops it.
                Ok(None) => {}
                Err(error) => {
                    if !self.read_failing {
                        warn!(error = %error, "could not read the game's connections");
                        self.read_failing = true;
                    }
                    return;
                }
            }
        }
        self.read_failing = false;

        let looking_dead = sample.iter().filter(|s| s.looks_dead()).count();
        debug!(
            sockets = sample.len(),
            looking_dead,
            max_retransmits = sample.iter().map(|s| s.retransmits).max().unwrap_or(0),
            "game connection sample"
        );
        let dead = self.detector.observe(&sample);

        if self.warned {
            if looking_dead == 0 {
                info!(games = ?titles, "game connections look normal again");
                self.warned = false;
            }
            return;
        }
        if dead {
            info!(games = ?titles, "game connection died; warning the user");
            if let Some(notice) = lost_connection_notice(titles) {
                if let Err(error) = self.notifier.notify(&notice).await {
                    warn!(error = %error, "could not show the notification");
                }
            }
            self.warned = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use snowlauncher_core::desktop::Notification;
    use snowlauncher_core::tcp::TcpSocket;
    use std::collections::VecDeque;
    use std::future::Future;
    use std::sync::Mutex;

    /// Online states reported in order, the last one repeating.
    struct ScriptedNetwork(VecDeque<bool>);

    impl NetworkMonitor for ScriptedNetwork {
        type Error = std::io::Error;

        fn wait_online(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
            std::future::ready(Ok(()))
        }

        fn is_online(&mut self) -> impl Future<Output = Result<bool, Self::Error>> + Send {
            let state = if self.0.len() > 1 {
                self.0.pop_front()
            } else {
                self.0.front().copied()
            };
            std::future::ready(Ok(state.unwrap_or(true)))
        }
    }

    #[derive(Clone)]
    enum Sample {
        Sockets(Vec<TcpSocket>),
        Unreadable,
    }

    /// Socket samples played back in order, the last one repeating.
    struct FakeSockets(Mutex<VecDeque<Sample>>);

    impl SocketTable for FakeSockets {
        type Error = std::io::Error;

        fn tcp_sockets(&self, _pid: Pid) -> Result<Option<Vec<TcpSocket>>, Self::Error> {
            let mut samples = self.0.lock().unwrap();
            let sample = if samples.len() > 1 {
                samples.pop_front()
            } else {
                samples.front().cloned()
            };
            match sample {
                Some(Sample::Sockets(sockets)) => Ok(Some(sockets)),
                Some(Sample::Unreadable) => Err(std::io::Error::other("permission denied")),
                None => Ok(None),
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

    fn healthy() -> Sample {
        Sample::Sockets(vec![TcpSocket {
            inode: 1,
            state: 0x01,
            tx_queue: 0,
            timer: 0,
            retransmits: 0,
        }])
    }

    /// Like the WoW connection seen half-open: port 3724, 10 retransmits.
    fn stuck() -> Sample {
        Sample::Sockets(vec![TcpSocket {
            inode: 1,
            state: 0x01,
            tx_queue: 0x154,
            timer: 1,
            retransmits: 10,
        }])
    }

    const WOW: Pid = Pid(100);
    const TITLES: &[&str] = &["World of Warcraft"];

    /// Runs one `update` per sample (or per network state, whichever is
    /// longer) and returns the notifications shown.
    async fn run(network: &[bool], samples: Vec<Sample>, polls: usize) -> Vec<Notification> {
        let sockets = FakeSockets(Mutex::new(samples.into()));
        let mut monitor = ConnectionMonitor::new(
            ScriptedNetwork(network.iter().copied().collect()),
            RecordingNotifier::default(),
        );
        for _ in 0..polls {
            monitor.update(&sockets, &[WOW], TITLES).await;
        }
        monitor.notifier.0
    }

    #[tokio::test]
    async fn warns_once_when_the_connection_dies() {
        let notices = run(&[true], vec![stuck()], 10).await;
        let [notice] = notices.as_slice() else {
            panic!("expected exactly one notification, got {notices:?}");
        };
        assert_eq!(notice.summary, "World of Warcraft lost its connection");
    }

    #[tokio::test]
    async fn stays_quiet_while_healthy() {
        assert!(run(&[true], vec![healthy()], 10).await.is_empty());
    }

    /// Cable pulled and plugged back: the stuck sample before the outage
    /// and the one right after it must not add up to "dead".
    #[tokio::test]
    async fn outage_does_not_count_as_dead() {
        let notices = run(
            &[true, false, false, true],
            vec![stuck(), stuck(), healthy()],
            10,
        )
        .await;
        assert!(notices.is_empty());
    }

    #[tokio::test]
    async fn warns_again_only_after_recovering() {
        let samples = vec![stuck(), stuck(), healthy(), stuck(), stuck()];
        assert_eq!(run(&[true], samples, 10).await.len(), 2);
    }

    #[tokio::test]
    async fn unreadable_sockets_are_not_a_warning() {
        assert!(run(&[true], vec![Sample::Unreadable], 5).await.is_empty());
    }

    #[tokio::test]
    async fn a_new_game_session_starts_over() {
        let sockets = FakeSockets(Mutex::new(vec![stuck()].into()));
        let mut monitor = ConnectionMonitor::new(
            ScriptedNetwork(VecDeque::from([true])),
            RecordingNotifier::default(),
        );
        monitor.update(&sockets, &[WOW], TITLES).await;
        // Game restarted: a new pid, so the earlier sample does not count.
        monitor.update(&sockets, &[Pid(200)], TITLES).await;
        assert!(monitor.notifier.0.is_empty());
        monitor.update(&sockets, &[Pid(200)], TITLES).await;
        assert_eq!(monitor.notifier.0.len(), 1);
    }
}
