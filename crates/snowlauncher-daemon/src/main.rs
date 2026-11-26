//! `snowd`: Snow Launcher's background daemon.
//!
//! For now it only watches suspend/resume and logs it. Restarting
//! Battle.net after resume comes next, once experiment E4 confirms the
//! approach.

mod logind;

use anyhow::{Context, Result};
use snowlauncher_core::sleep::{SleepEvent, SleepMonitor};
use tokio::signal::unix::{SignalKind, signal};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::logind::LogindSleepMonitor;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    // `RUST_LOG=debug snowd` for more detail. Under systemd, stderr goes
    // to the journal.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    info!(version = env!("CARGO_PKG_VERSION"), "snowd starting");

    let mut monitor = LogindSleepMonitor::connect()
        .await
        .context("failed to subscribe to logind sleep signals")?;
    let mut sigterm = signal(SignalKind::terminate()).context("failed to listen for SIGTERM")?;

    info!("watching suspend and resume");
    tokio::select! {
        result = watch_sleep(&mut monitor, log_sleep_event) => result?,
        _ = tokio::signal::ctrl_c() => info!("interrupted, stopping"),
        _ = sigterm.recv() => info!("received SIGTERM, stopping"),
    }
    Ok(())
}

/// Forwards every sleep event to `on_event` until the monitor ends.
async fn watch_sleep<M: SleepMonitor>(
    monitor: &mut M,
    mut on_event: impl FnMut(SleepEvent),
) -> Result<()> {
    while let Some(event) = monitor
        .next_event()
        .await
        .context("failed to read sleep event")?
    {
        on_event(event);
    }
    warn!("sleep event source closed; no longer watching suspend/resume");
    Ok(())
}

fn log_sleep_event(event: SleepEvent) {
    match event {
        SleepEvent::Suspending => info!("system is going to sleep"),
        SleepEvent::Resumed => info!("system woke up"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::future::Future;

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
        watch_sleep(&mut monitor, |event| seen.push(event))
            .await
            .unwrap();
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
        let result = watch_sleep(&mut monitor, |event| seen.push(event)).await;
        assert!(result.is_err());
        assert_eq!(seen, [SleepEvent::Suspending]);
    }
}
