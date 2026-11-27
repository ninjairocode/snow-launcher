//! Asking the running `snowd` what it sees, over its D-Bus interface
//! (names in `snowlauncher_core::dbus`). Shared by `snowctl` and the GUI.

use snowlauncher_core::dbus::{BUS_NAME, INTERFACE, OBJECT_PATH, property};

use crate::instance::{default_lock_path, running_instance};

/// What `snowd` reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonView {
    pub version: String,
    pub prefix: String,
    pub running_games: Vec<String>,
    pub suspend_inhibited: bool,
    pub game_connection: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("could not connect to the session D-Bus")]
    Connect(#[source] zbus::Error),
    #[error("could not read snowd's status")]
    Read(#[source] zbus::Error),
    #[error("could not start an async runtime")]
    Runtime(#[source] std::io::Error),
}

#[derive(Debug)]
pub enum DaemonState {
    Running(DaemonView),
    /// Holds the single-instance lock but has no D-Bus interface: an
    /// older snowd, from before the interface existed.
    RunningWithoutInterface {
        pid: Option<u32>,
    },
    NotRunning,
    /// Could not ask (no session bus, unexpected error...).
    Unknown(ClientError),
}

/// Queries `snowd` from synchronous code. Never fails: problems become
/// [`DaemonState::Unknown`].
pub fn query() -> DaemonState {
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(query_async()),
        Err(error) => DaemonState::Unknown(ClientError::Runtime(error)),
    }
}

/// Queries `snowd` from inside a Tokio runtime (the GUI).
pub async fn query_async() -> DaemonState {
    match read_view().await {
        Ok(Some(view)) => DaemonState::Running(view),
        Ok(None) => match default_lock_path().map(|path| running_instance(&path)) {
            Some(Ok(Some(instance))) => DaemonState::RunningWithoutInterface { pid: instance.pid },
            _ => DaemonState::NotRunning,
        },
        Err(error) => DaemonState::Unknown(error),
    }
}

async fn read_view() -> Result<Option<DaemonView>, ClientError> {
    let bus = zbus::Connection::session()
        .await
        .map_err(ClientError::Connect)?;
    let proxy = zbus::proxy::Builder::<zbus::Proxy<'_>>::new(&bus)
        .destination(BUS_NAME)
        .and_then(|builder| builder.path(OBJECT_PATH))
        .and_then(|builder| builder.interface(INTERFACE))
        .map_err(ClientError::Read)?
        // Read fresh values; do not subscribe to changes.
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .map_err(ClientError::Read)?;

    // The first read tells whether anyone owns the name.
    let version: String = match proxy.get_property(property::VERSION).await {
        Ok(version) => version,
        Err(error) if nobody_owns_the_name(&error) => return Ok(None),
        Err(error) => return Err(ClientError::Read(error)),
    };
    let read = ClientError::Read;
    Ok(Some(DaemonView {
        version,
        prefix: proxy.get_property(property::PREFIX).await.map_err(read)?,
        running_games: proxy
            .get_property(property::RUNNING_GAMES)
            .await
            .map_err(read)?,
        suspend_inhibited: proxy
            .get_property(property::SUSPEND_INHIBITED)
            .await
            .map_err(read)?,
        game_connection: proxy
            .get_property(property::GAME_CONNECTION)
            .await
            .map_err(read)?,
    }))
}

/// The bus answers "no such name" when snowd is not running, either as a
/// standard D-Bus error or as a raw method error with the same name.
fn nobody_owns_the_name(error: &zbus::Error) -> bool {
    const NOT_RUNNING: [&str; 2] = [
        "org.freedesktop.DBus.Error.ServiceUnknown",
        "org.freedesktop.DBus.Error.NameHasNoOwner",
    ];
    match error {
        zbus::Error::FDO(fdo) => matches!(
            **fdo,
            zbus::fdo::Error::ServiceUnknown(_) | zbus::fdo::Error::NameHasNoOwner(_)
        ),
        zbus::Error::MethodError(name, _, _) => NOT_RUNNING.contains(&name.as_str()),
        _ => false,
    }
}
