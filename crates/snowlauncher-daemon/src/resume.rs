//! What `snowd` does around suspend and resume.
//!
//! For now it logs the events and, on resume, how long the network took
//! to come back. Dead game connections after a long suspend are caught by
//! the continuous connection watch (`connections.rs`), like any other
//! outage. Restarting Battle.net comes later.

use std::time::Duration;

use snowlauncher_core::network::NetworkMonitor;
use snowlauncher_core::sleep::SleepEvent;
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

/// Handles sleep events: logs them and, on resume, waits for the network.
pub struct ResumeWatcher<N> {
    pub network: N,
    pub network_timeout: Duration,
}

impl<N: NetworkMonitor + Send> SleepHandler for ResumeWatcher<N> {
    async fn handle(&mut self, event: SleepEvent) {
        match event {
            SleepEvent::Suspending => info!("system is going to sleep"),
            SleepEvent::Resumed => {
                info!("system woke up, waiting for the network");
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

        fn is_online(&mut self) -> impl Future<Output = Result<bool, Self::Error>> + Send {
            std::future::ready(Ok(true))
        }
    }

    struct Broken;

    impl NetworkMonitor for Broken {
        type Error = std::io::Error;

        fn wait_online(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
            std::future::ready(Err(std::io::Error::other("no NetworkManager")))
        }

        fn is_online(&mut self) -> impl Future<Output = Result<bool, Self::Error>> + Send {
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
}
