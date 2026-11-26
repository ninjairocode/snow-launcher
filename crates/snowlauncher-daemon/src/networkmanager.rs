//! [`NetworkMonitor`] backed by NetworkManager on the system D-Bus.

use futures_util::StreamExt;
use snowlauncher_core::network::NetworkMonitor;

/// `NM_STATE_CONNECTED_GLOBAL`: connected, with internet access.
/// Lower values (e.g. 60, `CONNECTED_SITE`) mean local network only.
const NM_STATE_CONNECTED_GLOBAL: u32 = 70;

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager"
)]
trait NetworkManager {
    /// Overall networking state (`NMState`).
    #[zbus(property)]
    fn state(&self) -> zbus::Result<u32>;
}

/// Waits for NetworkManager to report internet connectivity.
pub struct NmNetworkMonitor {
    proxy: NetworkManagerProxy<'static>,
}

impl NmNetworkMonitor {
    /// Creates the proxy. Does not fail if NetworkManager is missing;
    /// that shows up later as an error from [`NetworkMonitor::wait_online`].
    pub async fn new(system_bus: &zbus::Connection) -> zbus::Result<Self> {
        let proxy = NetworkManagerProxy::new(system_bus).await?;
        Ok(Self { proxy })
    }
}

impl NetworkMonitor for NmNetworkMonitor {
    type Error = zbus::Error;

    async fn wait_online(&mut self) -> Result<(), zbus::Error> {
        // Subscribe *before* reading the current value, so a change that
        // happens between the two is not missed.
        let mut changes = self.proxy.receive_state_changed().await;
        if is_online(self.proxy.state().await?) {
            return Ok(());
        }
        while let Some(change) = changes.next().await {
            if is_online(change.get().await?) {
                return Ok(());
            }
        }
        Err(zbus::Error::Failure(
            "NetworkManager state updates stopped".to_owned(),
        ))
    }
}

fn is_online(state: u32) -> bool {
    state >= NM_STATE_CONNECTED_GLOBAL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_global_connectivity_counts_as_online() {
        assert!(is_online(70)); // CONNECTED_GLOBAL
        assert!(!is_online(60)); // CONNECTED_SITE: local network, no internet
        assert!(!is_online(40)); // CONNECTING
        assert!(!is_online(20)); // DISCONNECTED
        assert!(!is_online(10)); // ASLEEP (right after resume)
        assert!(!is_online(0)); // UNKNOWN
    }

    /// Run with `cargo test -p snowlauncher-daemon -- --ignored` on a
    /// machine that is online and uses NetworkManager.
    #[tokio::test]
    #[ignore = "needs NetworkManager on the system bus and an internet connection"]
    async fn real_networkmanager_reports_online() {
        let bus = zbus::Connection::system().await.unwrap();
        let mut monitor = NmNetworkMonitor::new(&bus).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), monitor.wait_online())
            .await
            .expect("NetworkManager did not report online within 5 s")
            .unwrap();
    }
}
