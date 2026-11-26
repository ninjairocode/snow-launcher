//! Desktop integration: notifications and suspend inhibition.
//!
//! Real implementations talk to the desktop over the session D-Bus;
//! tests use fakes that record what was asked.

use std::future::Future;

/// A desktop notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub summary: String,
    pub body: String,
    /// Urgent notifications stay on screen until the user dismisses them.
    pub urgent: bool,
}

pub trait Notifier {
    type Error: std::error::Error + Send + Sync + 'static;

    fn notify(
        &mut self,
        notification: &Notification,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// Keeps the desktop from suspending *automatically* (on idle).
/// Manual suspend and the screen turning off are not affected.
pub trait SuspendInhibitor {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Starts inhibiting. `reason` is shown to the user by the desktop.
    fn inhibit(&mut self, reason: &str) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Stops inhibiting. Does nothing if not inhibiting.
    fn release(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send;
}
