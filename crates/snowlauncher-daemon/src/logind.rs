//! [`SleepMonitor`] backed by systemd-logind on the system D-Bus.

use futures_util::StreamExt;
use snowlauncher_core::sleep::{SleepEvent, SleepMonitor};

/// The part of `org.freedesktop.login1.Manager` that Snow Launcher uses.
/// The `proxy` macro generates `ManagerProxy` from this trait.
#[zbus::proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait Manager {
    /// Emitted with `start = true` before sleeping and `false` after waking.
    #[zbus(signal)]
    fn prepare_for_sleep(&self, start: bool) -> zbus::Result<()>;
}

/// Listens to logind's `PrepareForSleep` signal.
///
/// Only observes: it takes no inhibitor lock, so it never delays the
/// system going to sleep.
pub struct LogindSleepMonitor {
    signals: PrepareForSleepStream,
}

impl LogindSleepMonitor {
    /// Subscribes to the signal on an existing system bus connection.
    pub async fn connect(system_bus: &zbus::Connection) -> zbus::Result<Self> {
        let manager = ManagerProxy::new(system_bus).await?;
        let signals = manager.receive_prepare_for_sleep().await?;
        Ok(Self { signals })
    }
}

impl SleepMonitor for LogindSleepMonitor {
    type Error = zbus::Error;

    async fn next_event(&mut self) -> Result<Option<SleepEvent>, zbus::Error> {
        let Some(signal) = self.signals.next().await else {
            return Ok(None);
        };
        let args = signal.args()?;
        Ok(Some(SleepEvent::from_prepare_for_sleep(args.start)))
    }
}
