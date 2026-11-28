//! `snowd`'s D-Bus interface on the session bus (names in
//! `snowlauncher_core::dbus`). Read-only properties; every change is
//! announced with `PropertiesChanged`, so clients can follow it live.
//!
//! The watchers do not know about D-Bus: they publish a [`Status`] on a
//! `tokio::sync::watch` channel, and [`publish`] forwards what changed.

use anyhow::{Context, Result};
use snowlauncher_core::dbus::{BUS_NAME, GameConnection, OBJECT_PATH};
use tokio::sync::watch;

/// What `snowd` is seeing right now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub prefix: String,
    pub running_games: Vec<String>,
    pub suspend_inhibited: bool,
    pub game_connection: GameConnection,
    /// Battle.net's main process runs in the prefix. Internal for now
    /// (the tray uses it); not on D-Bus.
    pub battlenet_running: bool,
}

pub struct DaemonInterface {
    status: Status,
}

// The name must be a literal here; a test checks it matches
// `snowlauncher_core::dbus::INTERFACE`.
#[zbus::interface(name = "io.github.ninjairocode.SnowLauncher1")]
impl DaemonInterface {
    #[zbus(property)]
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_owned()
    }

    /// Canonical path of the watched Wine prefix.
    #[zbus(property)]
    fn prefix(&self) -> String {
        self.status.prefix.clone()
    }

    /// Titles of the known games running in the prefix.
    #[zbus(property)]
    fn running_games(&self) -> Vec<String> {
        self.status.running_games.clone()
    }

    /// Whether automatic suspend is being blocked because of a game.
    #[zbus(property)]
    fn suspend_inhibited(&self) -> bool {
        self.status.suspend_inhibited
    }

    /// `no-game`, `ok`, `network-down` or `lost`.
    #[zbus(property)]
    fn game_connection(&self) -> String {
        self.status.game_connection.as_str().to_owned()
    }
}

/// Registers the interface at [`OBJECT_PATH`] on `bus`, without taking
/// the well-known name (see [`serve`]).
async fn register(bus: &zbus::Connection, initial: Status) -> zbus::Result<()> {
    bus.object_server()
        .at(OBJECT_PATH, DaemonInterface { status: initial })
        .await?;
    Ok(())
}

/// Registers the interface and takes [`BUS_NAME`].
pub async fn serve(session_bus: &zbus::Connection, initial: Status) -> Result<()> {
    register(session_bus, initial)
        .await
        .context("could not register the D-Bus interface")?;
    session_bus
        .request_name(BUS_NAME)
        .await
        .with_context(|| format!("could not take the D-Bus name {BUS_NAME}"))?;
    Ok(())
}

/// Forwards status updates to the interface, emitting `PropertiesChanged`
/// for the properties that changed. Returns when the sender is dropped.
pub async fn publish(bus: &zbus::Connection, mut updates: watch::Receiver<Status>) -> Result<()> {
    let iface = bus
        .object_server()
        .interface::<_, DaemonInterface>(OBJECT_PATH)
        .await
        .context("the D-Bus interface is not registered")?;
    while updates.changed().await.is_ok() {
        let new = updates.borrow_and_update().clone();
        let mut current = iface.get_mut().await;
        let old = std::mem::replace(&mut current.status, new);
        let emitter = iface.signal_emitter();
        let status = &current.status;
        if old.prefix != status.prefix {
            current.prefix_changed(emitter).await?;
        }
        if old.running_games != status.running_games {
            current.running_games_changed(emitter).await?;
        }
        if old.suspend_inhibited != status.suspend_inhibited {
            current.suspend_inhibited_changed(emitter).await?;
        }
        if old.game_connection != status.game_connection {
            current.game_connection_changed(emitter).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use snowlauncher_core::dbus::{INTERFACE, property};

    #[test]
    fn interface_name_matches_core() {
        assert_eq!(
            <DaemonInterface as zbus::object_server::Interface>::name().as_str(),
            INTERFACE
        );
    }

    /// Serves on a private connection (no well-known name, so it does not
    /// clash with a running snowd) and reads it from another connection.
    #[tokio::test]
    #[ignore = "needs a session D-Bus"]
    async fn properties_are_readable_and_follow_updates() {
        let server = zbus::Connection::session().await.unwrap();
        let initial = Status {
            prefix: "/home/user/Games/battlenet".to_owned(),
            ..Status::default()
        };
        register(&server, initial.clone()).await.unwrap();
        let (tx, rx) = watch::channel(initial);
        let server_for_task = server.clone();
        let task = tokio::spawn(async move { publish(&server_for_task, rx).await });

        let client = zbus::Connection::session().await.unwrap();
        let destination = server.unique_name().unwrap().to_owned();
        let proxy = zbus::proxy::Builder::<zbus::Proxy<'_>>::new(&client)
            .destination(destination)
            .unwrap()
            .path(OBJECT_PATH)
            .unwrap()
            .interface(INTERFACE)
            .unwrap()
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .unwrap();

        let prefix: String = proxy.get_property(property::PREFIX).await.unwrap();
        assert_eq!(prefix, "/home/user/Games/battlenet");
        let connection: String = proxy.get_property(property::GAME_CONNECTION).await.unwrap();
        assert_eq!(connection, "no-game");

        tx.send_modify(|status| {
            status.running_games = vec!["World of Warcraft".to_owned()];
            status.suspend_inhibited = true;
            status.game_connection = GameConnection::Ok;
        });
        // Let the publish task apply it.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let games: Vec<String> = proxy.get_property(property::RUNNING_GAMES).await.unwrap();
        assert_eq!(games, ["World of Warcraft"]);
        let inhibited: bool = proxy
            .get_property(property::SUSPEND_INHIBITED)
            .await
            .unwrap();
        assert!(inhibited);
        let version: String = proxy.get_property(property::VERSION).await.unwrap();
        assert_eq!(version, env!("CARGO_PKG_VERSION"));

        drop(tx);
        task.await.unwrap().unwrap();
    }
}
