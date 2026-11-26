//! [`ProcessTable`] implementation that reads Linux `/proc`.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use snowlauncher_core::process::{Pid, ProcessInfo, ProcessTable};
use snowlauncher_core::tcp::{SocketTable, TcpSocket, parse_proc_net_tcp};

#[derive(Debug, thiserror::Error)]
pub enum ProcfsError {
    #[error("failed to list processes in {path}")]
    ReadDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to read {path}")]
    Read {
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

impl SocketTable for ProcfsProcessTable {
    type Error = ProcfsError;

    /// Matches the socket inodes in `/proc/<pid>/fd` against the process's
    /// own view of `net/tcp` and `net/tcp6` (its network namespace).
    fn tcp_sockets(&self, pid: Pid) -> Result<Option<Vec<TcpSocket>>, ProcfsError> {
        let dir = self.root.join(pid.0.to_string());
        let fd_dir = dir.join("fd");
        let entries = match fs::read_dir(&fd_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(ProcfsError::Read {
                    path: fd_dir,
                    source,
                });
            }
        };
        // File descriptors may close while we look; skip those.
        let inodes: HashSet<u64> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| fs::read_link(entry.path()).ok())
            .filter_map(|target| socket_inode(&target))
            .collect();

        let mut sockets = Vec::new();
        for table in ["tcp", "tcp6"] {
            let path = dir.join("net").join(table);
            match fs::read_to_string(&path) {
                Ok(text) => sockets.extend(
                    parse_proc_net_tcp(&text)
                        .into_iter()
                        .filter(|socket| inodes.contains(&socket.inode)),
                ),
                // tcp6 is missing when IPv6 is disabled; both vanish if the
                // process just exited.
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(source) => return Err(ProcfsError::Read { path, source }),
            }
        }
        Ok(Some(sockets))
    }
}

/// `socket:[12345]` (an `fd` symlink target) → `12345`.
fn socket_inode(target: &Path) -> Option<u64> {
    target
        .to_str()?
        .strip_prefix("socket:[")?
        .strip_suffix(']')?
        .parse()
        .ok()
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

    // Kernel threads and zombies have an empty cmdline; that is fine.
    let cmdline = fs::read(dir.join("cmdline"))
        .map(|raw| parse_cmdline(&raw))
        .unwrap_or_default();

    Some(ProcessInfo {
        pid,
        name,
        cmdline,
        wineprefix,
    })
}

/// Splits a NUL-separated `/proc/<pid>/cmdline`. Empty entries are dropped:
/// Wine rewrites the command line in place and pads it with NULs.
fn parse_cmdline(raw: &[u8]) -> Vec<String> {
    raw.split(|byte| *byte == 0)
        .filter(|arg| !arg.is_empty())
        .map(|arg| String::from_utf8_lossy(arg).into_owned())
        .collect()
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
    fn cmdline_drops_wine_padding() {
        // Captured from a real Battle.net process under GE-Proton.
        let raw =
            b"C:\\Program Files (x86)\\Battle.net\\Battle.net.exe\0--type=gpu-process\0\0\0\0";
        assert_eq!(
            parse_cmdline(raw),
            [
                r"C:\Program Files (x86)\Battle.net\Battle.net.exe",
                "--type=gpu-process"
            ]
        );
        assert!(parse_cmdline(b"").is_empty());
    }

    #[test]
    fn socket_inode_from_fd_link() {
        assert_eq!(socket_inode(Path::new("socket:[774411]")), Some(774411));
        assert_eq!(socket_inode(Path::new("pipe:[774411]")), None);
        assert_eq!(socket_inode(Path::new("/dev/null")), None);
    }

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
