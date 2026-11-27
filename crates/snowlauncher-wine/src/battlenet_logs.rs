//! Finding the Battle.net app's log files inside a prefix. Read-only.
//!
//! They live in `drive_c/users/<user>/AppData/Local/Battle.net/Logs/`,
//! one `battle.net-<YYYYMMDDTHHMMSS.micros>.log` per start of the app.
//! Proton prefixes link `users/<you>` to `users/steamuser`, so the same
//! folder is reachable twice; it is only counted once.
//!
//! A second `Battle.net.exe` that only hands its request to the running
//! one (open the window, launch a game) also leaves a tiny log; those are
//! not sessions and are skipped.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use snowlauncher_core::battlenet_log::is_handoff;

use crate::prefix::WinePrefix;

const LOG_PREFIX: &str = "battle.net-";
const LOG_SUFFIX: &str = ".log";
/// Hand-off logs are ~1.5 KiB; real sessions are far bigger, so only
/// small logs are read to check.
const HANDOFF_MAX_BYTES: u64 = 16 * 1024;

/// Client session logs of the prefix, newest first (hand-off runs
/// excluded).
pub fn client_logs(prefix: &WinePrefix) -> io::Result<Vec<PathBuf>> {
    client_logs_in(prefix.path())
}

fn client_logs_in(prefix: &Path) -> io::Result<Vec<PathBuf>> {
    let users = prefix.join("drive_c").join("users");
    let entries = match fs::read_dir(&users) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };

    // Canonical paths dedupe `users/<you>` and `users/steamuser`.
    let log_dirs: BTreeSet<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|user| user.path().join("AppData/Local/Battle.net/Logs"))
        .filter_map(|dir| fs::canonicalize(dir).ok())
        .collect();

    let mut logs = Vec::new();
    for dir in log_dirs {
        for entry in fs::read_dir(&dir)?.filter_map(Result::ok) {
            let name = entry.file_name();
            let is_client_log = name
                .to_str()
                .is_some_and(|name| name.starts_with(LOG_PREFIX) && name.ends_with(LOG_SUFFIX));
            if is_client_log && !is_handoff_log(&entry.path()) {
                logs.push(entry.path());
            }
        }
    }
    // The timestamp in the name sorts chronologically as text.
    logs.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    Ok(logs)
}

fn is_handoff_log(path: &Path) -> bool {
    let small = fs::metadata(path).is_ok_and(|meta| meta.len() <= HANDOFF_MAX_BYTES);
    small && fs::read(path).is_ok_and(|bytes| is_handoff(&String::from_utf8_lossy(&bytes)))
}

/// Start time of a session from its log name:
/// `battle.net-20261127T010151.942635.log` → `2026-11-27 01:01`.
pub fn session_start(log: &Path) -> Option<String> {
    let name = log.file_name()?.to_str()?;
    let stamp = name.strip_prefix(LOG_PREFIX)?.get(..13)?; // YYYYMMDDTHHMM
    let (date, time) = stamp.split_once('T')?;
    if date.len() != 8
        || time.len() != 4
        || !(date.chars().chain(time.chars())).all(|c| c.is_ascii_digit())
    {
        return None;
    }
    Some(format!(
        "{}-{}-{} {}:{}",
        &date[..4],
        &date[4..6],
        &date[6..],
        &time[..2],
        &time[2..]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_logs_once_newest_first() {
        let tmp = tempfile::tempdir().unwrap();
        let users = tmp.path().join("drive_c/users");
        let logs = users.join("steamuser/AppData/Local/Battle.net/Logs");
        fs::create_dir_all(&logs).unwrap();
        // Proton layout: users/<you> -> steamuser.
        std::os::unix::fs::symlink("steamuser", users.join("someone")).unwrap();
        fs::create_dir_all(users.join("Public")).unwrap();

        for name in [
            "battle.net-20261126T134027.115323.log",
            "battle.net-20261127T010151.942635.log",
            "libcef-20261127T010154.393314.log",
        ] {
            fs::write(logs.join(name), "").unwrap();
        }
        // A newer hand-off run: not a session.
        fs::write(
            logs.join("battle.net-20261127T220427.464717.log"),
            "I 2026-11-27 22:04:27.468432 [Main] {Main} Leaving because another instance of battle.net is running\n",
        )
        .unwrap();

        let found: Vec<String> = client_logs_in(tmp.path())
            .unwrap()
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            found,
            [
                "battle.net-20261127T010151.942635.log",
                "battle.net-20261126T134027.115323.log",
            ]
        );
    }

    #[test]
    fn prefix_without_battlenet_has_no_logs() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(client_logs_in(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn session_start_from_file_name() {
        let log = Path::new("/x/battle.net-20261127T010151.942635.log");
        assert_eq!(session_start(log).as_deref(), Some("2026-11-27 01:01"));
        assert_eq!(session_start(Path::new("/x/battle.net-weird.log")), None);
    }
}
