//! [`Notifier`] backed by the freedesktop notification service.
//!
//! Uses `org.freedesktop.Notifications` directly, which works for apps
//! outside a sandbox (AppImage, RPM). A Flatpak build will need the
//! notification portal instead.

use std::collections::HashMap;

use snowlauncher_core::desktop::{Notification, Notifier};
use zbus::zvariant::Value;

const APP_NAME: &str = "Snow Launcher";
const ICON: &str = "network-offline-symbolic";
/// `urgency` hint: 2 = critical, stays until dismissed.
const URGENCY_CRITICAL: u8 = 2;
const URGENCY_NORMAL: u8 = 1;
/// `expire_timeout`: -1 lets the notification server decide.
const DEFAULT_TIMEOUT: i32 = -1;

#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)] // signature fixed by the spec
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;
}

pub struct DesktopNotifier {
    session_bus: zbus::Connection,
    /// Id of our last notification, so a new one replaces it instead of
    /// piling up (e.g. after several suspends in a row).
    last_id: u32,
}

impl DesktopNotifier {
    pub fn new(session_bus: zbus::Connection) -> Self {
        Self {
            session_bus,
            last_id: 0,
        }
    }
}

impl Notifier for DesktopNotifier {
    type Error = zbus::Error;

    async fn notify(&mut self, notification: &Notification) -> zbus::Result<()> {
        let proxy = NotificationsProxy::new(&self.session_bus).await?;
        let urgency = if notification.urgent {
            URGENCY_CRITICAL
        } else {
            URGENCY_NORMAL
        };
        let hints = HashMap::from([("urgency", Value::from(urgency))]);
        self.last_id = proxy
            .notify(
                APP_NAME,
                self.last_id,
                ICON,
                &notification.summary,
                &notification.body,
                &[],
                hints,
                DEFAULT_TIMEOUT,
            )
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shows a real (harmless) notification on the desktop.
    #[tokio::test]
    #[ignore = "needs a desktop session; shows a notification"]
    async fn real_notification_is_accepted() {
        let bus = zbus::Connection::session().await.unwrap();
        let mut notifier = DesktopNotifier::new(bus);
        let test = Notification {
            summary: "Snow Launcher test".to_owned(),
            body: "If you can read this, notifications work.".to_owned(),
            urgent: false,
        };
        notifier.notify(&test).await.unwrap();
        assert_ne!(notifier.last_id, 0);
    }
}
