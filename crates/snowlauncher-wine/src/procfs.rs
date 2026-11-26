//! [`ProcessTable`] implementation that reads Linux `/proc`.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use snowlauncher_core::process::{Pid, ProcessInfo, ProcessTable};

#[derive(Debug, thiserror::Error)]
pub enum ProcfsError {
    #[error("failed to list processes in {path}")]
    ReadDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Reads processes from `/proc` (or another root, for tests).
///
/// `WINEPREFIX` values are canonicalized when possible, because launchers
/// such as umu/Proton may reach the same prefix through a symlink.
/// Processes we cannot read (they exited, or belong to another user) are
/// skipped or reported without `WINEPREFIX`; that is never an error.
#[derive(Debug, Clone)]
pub struct ProcfsProcessTable {
    root: PathBuf,
}

impl ProcfsProcessTable {
    pub fn new() -> Self {
        Self::with_root("/proc")
    }

    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl Default for ProcfsProcessTable {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessTable for ProcfsProcessTable {
    type Error = ProcfsError;

    fn processes(&self) -> Result<Vec<ProcessInfo>, ProcfsError> {
        let entries = fs::read_dir(&self.root).map_err(|source| ProcfsError::ReadDir {
            path: self.root.clone(),
            source,
        })?;

        let mut processes: Vec<ProcessInfo> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let pid = entry.file_name().to_str()?.parse::<u32>().ok()?;
                read_process(&entry.path(), Pid(pid))
            })
            .collect();
        processes.sort_by_key(|process| process.pid);
        Ok(processes)
    }
}

/// Reads one `/proc/<pid>` directory. Returns `None` if the process is gone.
fn read_process(dir: &Path, pid: Pid) -> Option<ProcessInfo> {
    let name = fs::read_to_string(dir.join("comm"))
        .ok()?
        .trim_end_matches('\n')
        .to_owned();

    let wineprefix = fs::read(dir.join("environ"))
        .ok()
        .and_then(|environ| env_var(&environ, b"WINEPREFIX").map(<[u8]>::to_vec))
        .map(|value| canonicalize_if_absolute(PathBuf::from(OsStr::from_bytes(&value))));

    Some(ProcessInfo {
        pid,
        name,
        wineprefix,
    })
}

/// Finds `key` in a NUL-separated `KEY=value` list, as in `/proc/<pid>/environ`.
/// Empty values count as unset.
fn env_var<'a>(environ: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    environ
        .split(|byte| *byte == 0)
        .find_map(|entry| entry.strip_prefix(key)?.strip_prefix(b"="))
        .filter(|value| !value.is_empty())
}

/// Resolves symlinks of absolute paths. Relative paths are kept as they are,
/// because they are relative to the *other* process's working directory.
fn canonicalize_if_absolute(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        fs::canonicalize(&path).unwrap_or(path)
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_var_finds_exact_key() {
        let environ = b"HOME=/home/u\0WINEPREFIXX=/wrong\0WINEPREFIX=/right\0";
        assert_eq!(env_var(environ, b"WINEPREFIX"), Some(&b"/right"[..]));
    }

    #[test]
    fn env_var_missing_or_empty_is_none() {
        assert_eq!(env_var(b"HOME=/home/u\0", b"WINEPREFIX"), None);
        assert_eq!(env_var(b"WINEPREFIX=\0", b"WINEPREFIX"), None);
        assert_eq!(env_var(b"", b"WINEPREFIX"), None);
    }

    #[test]
    fn env_var_accepts_non_utf8_values() {
        let environ = b"WINEPREFIX=/home/\xff/pfx\0";
        assert_eq!(
            env_var(environ, b"WINEPREFIX"),
            Some(&b"/home/\xff/pfx"[..])
        );
    }
}
