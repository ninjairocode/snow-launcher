//! Finding downloaded Battle.net installers, checking Blizzard's
//! signature on them, choosing one, and keeping the last one that passed
//! (see `snowlauncher_core::installer`).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use directories::{BaseDirs, UserDirs};
use snowlauncher_core::house::BATTLENET_PUBLISHER;
use snowlauncher_core::installer::{
    CACHED_INSTALLER_NAME, OFFICIAL_DOWNLOAD_PAGE, installer_candidates, is_installer_name,
};

use crate::house::write_atomic;
use crate::signature::{
    Publisher, SignatureError, system_roots, verify_executable, verify_publisher,
};

/// Whether `path` is really Blizzard's installer, unmodified: valid
/// Authenticode signature, trusted chain, signed by Blizzard.
pub fn verify_installer(path: &Path) -> Result<Publisher, SignatureError> {
    let bytes = fs::read(path).map_err(|source| SignatureError::Io {
        path: path.to_owned(),
        source,
    })?;
    let publisher = verify_executable(&bytes, &system_roots()?)?;
    verify_publisher(&publisher, BATTLENET_PUBLISHER)?;
    Ok(publisher)
}

/// Why no installer could be chosen.
#[derive(Debug, thiserror::Error)]
pub enum ChooseError {
    #[error("{}: do not run it: {source}", path.display())]
    Refused {
        path: PathBuf,
        #[source]
        source: SignatureError,
    },
    #[error(
        "no Battle.net installer found: download it from {OFFICIAL_DOWNLOAD_PAGE} (Snow finds it in Downloads)"
    )]
    NoneFound,
    #[error("no installer passed the check ({}); download it again from {OFFICIAL_DOWNLOAD_PAGE}", .0.join("; "))]
    AllRefused(Vec<String>),
}

/// The installer to run: the one named, or else the newest download that
/// passes the check, or else the kept copy. A verified one becomes the
/// kept copy. A named file that fails is not replaced by another.
pub fn choose_installer(named: Option<&Path>) -> Result<(PathBuf, Publisher), ChooseError> {
    let downloads = downloads_dir()
        .map(|dir| downloaded_installers(&dir))
        .unwrap_or_default();
    choose_from(
        named,
        &downloads,
        kept_installer().as_deref(),
        kept_installer_path().as_deref(),
        verify_installer,
    )
}

/// [`choose_installer`] with its inputs and the check given, for tests.
fn choose_from(
    named: Option<&Path>,
    downloads: &[PathBuf],
    kept: Option<&Path>,
    keep_to: Option<&Path>,
    check: impl Fn(&Path) -> Result<Publisher, SignatureError>,
) -> Result<(PathBuf, Publisher), ChooseError> {
    let mut refused = Vec::new();
    for candidate in installer_candidates(named, downloads, kept) {
        match check(&candidate) {
            Ok(publisher) => {
                if let Some(keep_to) = keep_to {
                    // Keeping a copy is a convenience: never a reason to fail.
                    let _ = keep_verified(&candidate, keep_to);
                }
                return Ok((candidate, publisher));
            }
            Err(source) if named.is_some() => {
                return Err(ChooseError::Refused {
                    path: candidate,
                    source,
                });
            }
            Err(source) => refused.push(format!("{}: {source}", candidate.display())),
        }
    }
    if refused.is_empty() {
        Err(ChooseError::NoneFound)
    } else {
        Err(ChooseError::AllRefused(refused))
    }
}

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

    /// Accepts files whose content is "good".
    fn fake_check(path: &Path) -> Result<Publisher, SignatureError> {
        if fs::read_to_string(path).unwrap_or_default() == "good" {
            Ok(Publisher {
                organization: Some(BATTLENET_PUBLISHER.to_owned()),
                common_name: None,
                chain: Vec::new(),
            })
        } else {
            Err(SignatureError::FileModified)
        }
    }

    #[test]
    fn the_first_good_download_wins_and_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let bad = tmp.path().join("Battle.net-Setup (1).exe");
        let good = tmp.path().join("Battle.net-Setup.exe");
        fs::write(&bad, "tampered").unwrap();
        fs::write(&good, "good").unwrap();
        let keep = tmp.path().join("cache/Battle.net-Setup.exe");
        let downloads = [bad, good.clone()];

        let (chosen, _) = choose_from(None, &downloads, None, Some(&keep), fake_check).unwrap();
        assert_eq!(chosen, good);
        assert_eq!(fs::read_to_string(&keep).unwrap(), "good");

        // Later, with nothing in Downloads, the kept copy is used.
        let (chosen, _) = choose_from(None, &[], Some(&keep), Some(&keep), fake_check).unwrap();
        assert_eq!(chosen, keep);
    }

    #[test]
    fn a_named_file_that_fails_is_not_replaced_by_another() {
        let tmp = tempfile::tempdir().unwrap();
        let named = tmp.path().join("mine.exe");
        let good = tmp.path().join("Battle.net-Setup.exe");
        fs::write(&named, "tampered").unwrap();
        fs::write(&good, "good").unwrap();
        let keep = tmp.path().join("cache/Battle.net-Setup.exe");
        let error = choose_from(Some(&named), &[good], None, Some(&keep), fake_check).unwrap_err();
        assert!(matches!(error, ChooseError::Refused { .. }), "{error}");
        assert!(!keep.exists(), "nothing kept from a refused file");
    }

    #[test]
    fn nothing_found_points_to_the_download_page() {
        let error = choose_from(None, &[], None, None, fake_check).unwrap_err();
        assert!(
            error.to_string().contains(OFFICIAL_DOWNLOAD_PAGE),
            "{error}"
        );
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
