//! `snowd`: Snow Launcher's background daemon.
//!
//! For now it only watches suspend/resume, waits for the network after
//! resume and logs it all. Restarting Battle.net comes later, once
//! experiment E4 confirms the approach.

mod logind;
mod networkmanager;
mod resume;

use std::future::Future;
use std::time::Duration;

use anyhow::{Context, Result};
use snowlauncher_core::sleep::{SleepEvent, SleepMonitor};
use tokio::signal::unix::{SignalKind, signal};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::logind::LogindSleepMonitor;
use crate::networkmanager::NmNetworkMonitor;
use crate::resume::ResumeWatcher;

/// How long to wait for the network after resume before giving up.
// TODO(config): make this configurable in config.toml.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(60);

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    // `RUST_LOG=debug snowd` for more detail. Under systemd, stderr goes
    // to the journal.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    info!(version = env!("CARGO_PKG_VERSION"), "snowd starting");

    // One connection to the system bus, shared by logind and NetworkManager.
    let system_bus = zbus::Connection::system()
        .await
        .context("failed to connect to the system D-Bus")?;
    let mut monitor = LogindSleepMonitor::connect(&system_bus)
        .await
        .context("failed to subscribe to logind sleep signals")?;
    let network = NmNetworkMonitor::new(&system_bus)
        .await
        .context("failed to create the NetworkManager client")?;
    let mut handler = ResumeWatcher::new(network, NETWORK_TIMEOUT);
    let mut sigterm = signal(SignalKind::terminate()).context("failed to listen for SIGTERM")?;

    info!("watching suspend and resume");
    tokio::select! {
        result = watch_sleep(&mut monitor, &mut handler) => result?,
        _ = tokio::signal::ctrl_c() => info!("interrupted, stopping"),
        _ = sigterm.recv() => info!("received SIGTERM, stopping"),
    }
    Ok(())
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
