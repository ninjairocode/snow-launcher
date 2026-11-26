//! [`SuspendInhibitor`] backed by the XDG desktop portal (session D-Bus).
//!
//! The portal works on GNOME, KDE and others, and also from inside a
//! Flatpak sandbox later on.

use std::collections::HashMap;

use snowlauncher_core::desktop::SuspendInhibitor;
use zbus::zvariant::{OwnedObjectPath, Value};

/// `Suspend` flag of `org.freedesktop.portal.Inhibit`. `Idle` (8) is left
/// out on purpose: the screen may still turn off while a game runs.
const INHIBIT_SUSPEND: u32 = 4;

#[zbus::proxy(
    interface = "org.freedesktop.portal.Inhibit",
    default_service = "org.freedesktop.portal.Desktop",
    default_path = "/org/freedesktop/portal/desktop"
)]
trait Inhibit {
    /// Returns a request handle; the inhibition lasts until it is closed
    /// (or until our D-Bus connection goes away, e.g. if `snowd` exits).
    fn inhibit(
        &self,
        window: &str,
        flags: u32,
        options: HashMap<&str, Value<'_>>,
    ) -> zbus::Result<OwnedObjectPath>;
}

#[zbus::proxy(
    interface = "org.freedesktop.portal.Request",
    default_service = "org.freedesktop.portal.Desktop"
)]
trait Request {
    fn close(&self) -> zbus::Result<()>;
}

pub struct PortalSuspendInhibitor {
    session_bus: zbus::Connection,
    /// Handle of the active inhibition, if any.
    active: Option<OwnedObjectPath>,
}

impl PortalSuspendInhibitor {
    pub fn new(session_bus: zbus::Connection) -> Self {
        Self {
            session_bus,
            active: None,
        }
    }
}

impl SuspendInhibitor for PortalSuspendInhibitor {
    type Error = zbus::Error;

    async fn inhibit(&mut self, reason: &str) -> zbus::Result<()> {
        if self.active.is_some() {
            return Ok(());
        }
        let proxy = InhibitProxy::new(&self.session_bus).await?;
        let options = HashMap::from([("reason", Value::from(reason))]);
        // No parent window: `snowd` has none.
        let handle = proxy.inhibit("", INHIBIT_SUSPEND, options).await?;
        self.active = Some(handle);
        Ok(())
    }

    async fn release(&mut self) -> zbus::Result<()> {
        let Some(handle) = self.active.take() else {
            return Ok(());
        };
        let request = RequestProxy::builder(&self.session_bus)
            .path(handle)?
            .build()
            .await?;
        request.close().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GNOME's session manager, to check the inhibition took effect.
    #[zbus::proxy(
        interface = "org.gnome.SessionManager",
        default_service = "org.gnome.SessionManager",
        default_path = "/org/gnome/SessionManager"
    )]
    trait SessionManager {
        fn is_inhibited(&self, flags: u32) -> zbus::Result<bool>;
    }

    /// The portal forwards requests asynchronously, so poll for up to 2 s.
    async fn suspend_inhibited_becomes(session: &SessionManagerProxy<'_>, expected: bool) -> bool {
        for _ in 0..20 {
            if session.is_inhibited(INHIBIT_SUSPEND).await.unwrap() == expected {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        false
    }

    /// Run with `cargo test -p snowlauncher-daemon -- --ignored` inside a
    /// GNOME session. Assumes nothing else is inhibiting suspend.
    #[tokio::test]
    #[ignore = "needs a GNOME session with xdg-desktop-portal"]
    async fn real_portal_inhibits_and_releases_suspend_on_gnome() {
        let bus = zbus::Connection::session().await.unwrap();
        let session = SessionManagerProxy::new(&bus).await.unwrap();
        let mut inhibitor = PortalSuspendInhibitor::new(bus.clone());

        inhibitor.inhibit("Snow Launcher test").await.unwrap();
        assert!(suspend_inhibited_becomes(&session, true).await);
        // Only Suspend: the screen may still blank (Idle = 8).
        assert!(!session.is_inhibited(8).await.unwrap());

        inhibitor.release().await.unwrap();
        assert!(suspend_inhibited_becomes(&session, false).await);
    }
}
