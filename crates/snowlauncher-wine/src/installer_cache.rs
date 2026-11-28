//! Finding downloaded Battle.net installers and keeping the last one that
//! passed the signature check (see `snowlauncher_core::installer`).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use directories::{BaseDirs, UserDirs};
use snowlauncher_core::installer::{CACHED_INSTALLER_NAME, is_installer_name};

use crate::house::write_atomic;

/// `~/.cache/snow-launcher/installer/Battle.net-Setup.exe`.
pub fn kept_installer_path() -> Option<PathBuf> {
    BaseDirs::new().map(|dirs| {
        dirs.cache_dir()
            .join("snow-launcher")
            .join("installer")
            .join(CACHED_INSTALLER_NAME)
    })
}

/// The kept copy, if there is one.
pub fn kept_installer() -> Option<PathBuf> {
    kept_installer_path().filter(|path| path.is_file())
}

/// The user's Downloads folder.
pub fn downloads_dir() -> Option<PathBuf> {
    UserDirs::new().and_then(|dirs| dirs.download_dir().map(Path::to_owned))
}

/// Files in `dir` named like the installer, newest first. Not checked yet.
pub fn downloaded_installers(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(SystemTime, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|entry| is_installer_name(&entry.file_name().to_string_lossy()))
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| {
            let time = entry.metadata().and_then(|meta| meta.modified()).ok()?;
            Some((time, entry.path()))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    found.into_iter().map(|(_, path)| path).collect()
}

/// Keeps a copy of an installer that **already passed** the signature
/// check, replacing the previous one atomically. Returns the kept path.
pub fn keep_verified(verified: &Path, kept: &Path) -> io::Result<PathBuf> {
    let same = fs::canonicalize(verified).ok() == fs::canonicalize(kept).ok();
    if !same {
        if let Some(dir) = kept.parent() {
            fs::create_dir_all(dir)?;
        }
        write_atomic(kept, &fs::read(verified)?)?;
    }
    Ok(kept.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_downloads_newest_first() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("Battle.net-Setup.exe");
        let new = tmp.path().join("Battle.net-Setup (1).exe");
        fs::write(&old, "old").unwrap();
        fs::write(tmp.path().join("notes.txt"), "").unwrap();
        fs::write(tmp.path().join("Battle.net-Setup.exe.part"), "").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(&new, "new").unwrap();
        assert_eq!(downloaded_installers(tmp.path()), [new, old]);
        assert!(downloaded_installers(&tmp.path().join("missing")).is_empty());
    }

    #[test]
    fn keeps_a_copy_and_replaces_it() {
        let tmp = tempfile::tempdir().unwrap();
        let kept = tmp.path().join("cache/installer/Battle.net-Setup.exe");
        let first = tmp.path().join("a.exe");
        let second = tmp.path().join("b.exe");
        fs::write(&first, "v1").unwrap();
        fs::write(&second, "v2").unwrap();
        keep_verified(&first, &kept).unwrap();
        assert_eq!(fs::read_to_string(&kept).unwrap(), "v1");
        keep_verified(&second, &kept).unwrap();
        assert_eq!(fs::read_to_string(&kept).unwrap(), "v2");
        // Keeping the kept copy itself changes nothing.
        keep_verified(&kept, &kept).unwrap();
        assert_eq!(fs::read_to_string(&kept).unwrap(), "v2");
    }
}
