//! Network availability.
//!
//! After waking up, restarting Battle.net before the network is back would
//! only make it fail again, so the daemon waits for connectivity first.

use std::future::Future;

/// Source of network state. The real implementation asks NetworkManager;
/// tests use fakes that come online after a delay, never, or fail.
pub trait NetworkMonitor {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Resolves once the system has full (internet) connectivity.
    /// Returns immediately if it already has. Never times out on its own:
    /// callers decide how long to wait.
    fn wait_online(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send;
}
