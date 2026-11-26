//! Single instance: only one `snowd` per user may run at a time.
//!
//! Two daemons would block suspend twice and show every warning twice.
//! The guard is an exclusive `flock` on a file in `$XDG_RUNTIME_DIR`
//! (`project.md`, section 9.2). The kernel releases the lock when the
//! process exits, even if it crashes, so a stale lock file is harmless.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};

use directories::BaseDirs;

const LOCK_FILE_NAME: &str = "snow-launcher.lock";

#[derive(Debug, thiserror::Error)]
pub enum InstanceError {
    #[error("another snowd is already running{}", pid_suffix(*.pid))]
    AlreadyRunning { pid: Option<u32> },

    #[error("could not use the lock file {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

fn pid_suffix(pid: Option<u32>) -> String {
    pid.map(|pid| format!(" (pid {pid})")).unwrap_or_default()
}

/// Holds the lock while alive. Keep it until the daemon exits.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

/// `$XDG_RUNTIME_DIR/snow-launcher.lock`, or `None` if there is no
/// runtime directory (no user session).
pub fn default_lock_path() -> Option<PathBuf> {
    BaseDirs::new()?
        .runtime_dir()
        .map(|dir| dir.join(LOCK_FILE_NAME))
}

/// Takes the lock at `path`, without waiting. On success the file holds
/// our pid, so a second instance can say which process is running.
pub fn acquire(path: &Path) -> Result<InstanceLock, InstanceError> {
    let io_error = |source| InstanceError::Io {
        path: path.to_owned(),
        source,
    };

    // Do not truncate yet: the file may belong to the running instance.
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(io_error)?;

    // Called through the trait on purpose: since Rust 1.89, `File` has an
    // inherent `try_lock` with a different error type, which would win.
    match fs4::FileExt::try_lock(&file) {
        Ok(()) => {}
        Err(fs4::TryLockError::WouldBlock) => {
            return Err(InstanceError::AlreadyRunning {
                pid: read_pid(&mut file),
            });
        }
        Err(fs4::TryLockError::Error(source)) => return Err(io_error(source)),
    }

    // The lock is ours: replace whatever pid was left there.
    file.set_len(0).map_err(io_error)?;
    file.rewind().map_err(io_error)?;
    writeln!(file, "{}", std::process::id()).map_err(io_error)?;
    file.flush().map_err(io_error)?;

    Ok(InstanceLock { _file: file })
}

fn read_pid(file: &mut File) -> Option<u32> {
    let mut text = String::new();
    file.read_to_string(&mut text).ok()?;
    text.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_instance_is_refused_with_the_running_pid() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(LOCK_FILE_NAME);

        let _first = acquire(&path).unwrap();
        let second = acquire(&path).unwrap_err();
        assert!(matches!(
            second,
            InstanceError::AlreadyRunning { pid: Some(pid) } if pid == std::process::id()
        ));
        assert_eq!(
            second.to_string(),
            format!(
                "another snowd is already running (pid {})",
                std::process::id()
            )
        );
    }

    #[test]
    fn lock_is_free_again_after_the_first_instance_ends() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(LOCK_FILE_NAME);

        drop(acquire(&path).unwrap());
        assert!(acquire(&path).is_ok());
    }

    #[test]
    fn stale_file_from_a_crash_does_not_block() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(LOCK_FILE_NAME);
        // Left behind by a process that died: nobody holds the lock.
        std::fs::write(&path, "99999\n").unwrap();

        let _lock = acquire(&path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content.trim(), std::process::id().to_string());
    }

    #[test]
    fn missing_directory_is_an_io_error() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("missing").join(LOCK_FILE_NAME);
        assert!(matches!(acquire(&path), Err(InstanceError::Io { .. })));
    }
}
