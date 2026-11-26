//! Processes and how they relate to Wine prefixes.
//!
//! Snow Launcher must only ever touch processes that belong to its own
//! prefix. A process belongs to a prefix when its `WINEPREFIX` environment
//! variable points to it. Processes are never identified by name, so a
//! Battle.net running under Lutris or Steam is never affected.

use std::path::{Path, PathBuf};

/// A Linux process id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pid(pub u32);

/// A snapshot of one running process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: Pid,
    /// Short process name (`/proc/<pid>/comm`). Only for display and logs,
    /// never for deciding which processes to touch.
    pub name: String,
    /// Value of `WINEPREFIX`, if the process has one and it could be read.
    pub wineprefix: Option<PathBuf>,
}

/// What a process is, as far as its name tells.
///
/// Only for display and status. Which processes Snow Launcher may touch is
/// always decided by prefix (see [`processes_in_prefix`]), never by role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessRole {
    BattleNet,
    Agent,
    /// Wine/Proton infrastructure that runs in every prefix.
    WineService,
    /// Anything else: usually a game, or something not yet known.
    Other,
}

// TODO(v0.4): these lists will come from the quirks repository.
const BATTLENET_NAMES: &[&str] = &["Battle.net.exe", "Battle.net Helper.exe"];
const AGENT_NAMES: &[&str] = &["Agent.exe"];
const WINE_SERVICE_NAMES: &[&str] = &[
    "wineserver",
    "services.exe",
    "winedevice.exe",
    "plugplay.exe",
    "svchost.exe",
    "rpcss.exe",
    "explorer.exe",
    "tabtip.exe",
    "conhost.exe",
    "start.exe",
    "rundll32.exe",
    // Proton's helper that stands in for the Steam client.
    "steam.exe",
];

/// The kernel keeps only the first 15 bytes of a process name in `comm`.
const COMM_MAX_LEN: usize = 15;

impl ProcessRole {
    /// Classifies a process by its `comm` name, accounting for truncation.
    pub fn from_comm(comm: &str) -> Self {
        let in_list = |names: &[&str]| names.iter().any(|name| comm_matches(comm, name));
        if in_list(BATTLENET_NAMES) {
            Self::BattleNet
        } else if in_list(AGENT_NAMES) {
            Self::Agent
        } else if in_list(WINE_SERVICE_NAMES) {
            Self::WineService
        } else {
            Self::Other
        }
    }
}

/// Whether `comm` is `full_name` as the kernel would store it (truncated).
fn comm_matches(comm: &str, full_name: &str) -> bool {
    let full = full_name.as_bytes();
    full.get(..COMM_MAX_LEN).unwrap_or(full) == comm.as_bytes()
}

/// Source of running processes. The real implementation reads `/proc`;
/// tests use fixed lists.
pub trait ProcessTable {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Lists the processes visible right now.
    fn processes(&self) -> Result<Vec<ProcessInfo>, Self::Error>;
}

/// Returns the processes whose `WINEPREFIX` is exactly `prefix`.
///
/// Matching is conservative: relative `WINEPREFIX` values never match,
/// and a subfolder or a sibling with a similar name is not the same prefix.
/// Paths are compared component by component, so `/a/b/` equals `/a/b`.
/// Symlinks are not resolved here (that is IO); callers should pass
/// canonical paths on both sides.
pub fn processes_in_prefix<'a>(
    processes: &'a [ProcessInfo],
    prefix: &'a Path,
) -> impl Iterator<Item = &'a ProcessInfo> {
    processes.iter().filter(move |process| {
        process
            .wineprefix
            .as_deref()
            .is_some_and(|wineprefix| wineprefix.is_absolute() && wineprefix == prefix)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(pid: u32, wineprefix: Option<&str>) -> ProcessInfo {
        ProcessInfo {
            pid: Pid(pid),
            name: format!("proc{pid}"),
            wineprefix: wineprefix.map(PathBuf::from),
        }
    }

    fn matching_pids(processes: &[ProcessInfo], prefix: &str) -> Vec<u32> {
        processes_in_prefix(processes, Path::new(prefix))
            .map(|p| p.pid.0)
            .collect()
    }

    const PREFIX: &str = "/home/user/.local/share/snow-launcher/prefixes/battlenet";

    #[test]
    fn matches_exact_prefix() {
        let processes = [process(1, Some(PREFIX))];
        assert_eq!(matching_pids(&processes, PREFIX), [1]);
    }

    #[test]
    fn ignores_trailing_slash() {
        let processes = [process(1, Some(&format!("{PREFIX}/")))];
        assert_eq!(matching_pids(&processes, PREFIX), [1]);
    }

    #[test]
    fn ignores_processes_without_wineprefix() {
        let processes = [process(1, None)];
        assert!(matching_pids(&processes, PREFIX).is_empty());
    }

    #[test]
    fn ignores_other_launchers() {
        let processes = [
            process(1, Some("/home/user/Games/battlenet")), // Lutris
            process(
                2,
                Some("/home/user/.steam/steam/steamapps/compatdata/123/pfx"),
            ),
            process(3, Some(PREFIX)),
        ];
        assert_eq!(matching_pids(&processes, PREFIX), [3]);
    }

    #[test]
    fn similar_names_and_subfolders_do_not_match() {
        let processes = [
            process(1, Some(&format!("{PREFIX}-old"))),
            process(2, Some(&format!("{PREFIX}/drive_c"))),
        ];
        assert!(matching_pids(&processes, PREFIX).is_empty());
    }

    #[test]
    fn classifies_known_processes() {
        assert_eq!(
            ProcessRole::from_comm("Battle.net.exe"),
            ProcessRole::BattleNet
        );
        assert_eq!(ProcessRole::from_comm("Agent.exe"), ProcessRole::Agent);
        assert_eq!(
            ProcessRole::from_comm("wineserver"),
            ProcessRole::WineService
        );
        assert_eq!(ProcessRole::from_comm("Wow.exe"), ProcessRole::Other);
    }

    #[test]
    fn classifies_truncated_comm() {
        // "Battle.net Helper.exe" is stored by the kernel as its first 15 bytes.
        assert_eq!(
            ProcessRole::from_comm("Battle.net Help"),
            ProcessRole::BattleNet
        );
        // A prefix shorter than 15 bytes is not a truncation.
        assert_eq!(ProcessRole::from_comm("Battle.net"), ProcessRole::Other);
    }

    #[test]
    fn relative_wineprefix_never_matches() {
        let processes = [process(1, Some("battlenet"))];
        assert!(matching_pids(&processes, "battlenet").is_empty());
    }
}
