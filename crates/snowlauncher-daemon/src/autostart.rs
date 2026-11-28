//! "Open Battle.net when Snow starts" (`[startup] open_battlenet`, off by
//! default): once per session, when `snowd` starts. The "done this
//! session" mark lives in `$XDG_RUNTIME_DIR`, which the system empties at
//! logout, so a `snowd` restarted mid-session does not open Battle.net again.

use std::path::{Path, PathBuf};

use snowlauncher_core::house::{BATTLENET_EXE, opens_battlenet_at_start};
use snowlauncher_wine::house::{
    battlenet_pid, load_record, open_house, record_path, run_battlenet, write_atomic,
};
use tracing::{info, warn};

/// `$XDG_RUNTIME_DIR/snow-launcher/battlenet-opened-at-start`.
fn session_mark() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|dir| {
        PathBuf::from(dir)
            .join("snow-launcher")
            .join("battlenet-opened-at-start")
    })
}

/// What happened, for the log.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Skipped,
    Opened,
    Failed(String),
}

fn open_if_asked(enabled: bool, mark: Option<&Path>) -> Outcome {
    let house = record_path().and_then(|path| load_record(&path).ok().flatten());
    let house_ready = house
        .as_ref()
        .is_some_and(|record| record.prefix.join(BATTLENET_EXE).is_file());
    let running = house
        .as_ref()
        .and_then(|record| std::fs::canonicalize(&record.prefix).ok())
        .is_some_and(|prefix| battlenet_pid(&prefix).ok().flatten().is_some());
    let opened = mark.is_some_and(Path::exists);
    if !opens_battlenet_at_start(enabled, house_ready, running, opened) {
        return Outcome::Skipped;
    }
    // Marked first: even a failed try is not repeated this session.
    if let Some(mark) = mark {
        if let Some(dir) = mark.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = write_atomic(mark, b"");
    }
    match open_house() {
        Ok((_, runner)) => match run_battlenet(&runner, &[]) {
            Ok(_) => Outcome::Opened,
            Err(error) => Outcome::Failed(error.to_string()),
        },
        Err(error) => Outcome::Failed(error.to_string()),
    }
}

/// Opens Battle.net if the option asks for it (see the module docs).
pub async fn run(enabled: bool) {
    if !enabled {
        return;
    }
    let outcome =
        tokio::task::spawn_blocking(move || open_if_asked(true, session_mark().as_deref()))
            .await
            .unwrap_or_else(|error| Outcome::Failed(error.to_string()));
    match outcome {
        Outcome::Opened => info!("opened Battle.net at start, as configured"),
        Outcome::Skipped => {}
        Outcome::Failed(problem) => warn!(%problem, "could not open Battle.net at start"),
    }
}
