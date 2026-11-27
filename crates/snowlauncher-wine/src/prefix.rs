//! Wine prefixes.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::BaseDirs;

/// Files every Wine prefix has once it has been initialized.
const REQUIRED_ENTRIES: &[&str] = &["drive_c", "system.reg"];

#[derive(Debug, thiserror::Error)]
pub enum PrefixError {
    #[error("{path} does not exist")]
    NotFound { path: PathBuf },

    #[error("{path} is not a Wine prefix (missing {missing})")]
    NotAPrefix {
        path: PathBuf,
        missing: &'static str,
    },

    #[error("failed to access {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// An existing, initialized Wine prefix. Read-only for now.
///
/// The path is canonical (symlinks resolved), so it can be compared directly
/// with the canonical `WINEPREFIX` of running processes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WinePrefix {
    path: PathBuf,
}

impl WinePrefix {
    /// Opens a prefix without changing anything in it.
    pub fn open(path: &Path) -> Result<Self, PrefixError> {
        let canonical = fs::canonicalize(path).map_err(|source| {
            if source.kind() == io::ErrorKind::NotFound {
                PrefixError::NotFound {
                    path: path.to_owned(),
                }
            } else {
                PrefixError::Io {
                    path: path.to_owned(),
                    source,
                }
            }
        })?;

        for &entry in REQUIRED_ENTRIES {
            let entry_path = canonical.join(entry);
            let exists = entry_path.try_exists().map_err(|source| PrefixError::Io {
                path: entry_path,
                source,
            })?;
            if !exists {
                return Err(PrefixError::NotAPrefix {
                    path: canonical,
                    missing: entry,
                });
            }
        }

        Ok(Self { path: canonical })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Where Snow Launcher keeps its own Battle.net prefix:
/// `$XDG_DATA_HOME/snow-launcher/prefixes/battlenet`.
///
/// Returns `None` if the home directory cannot be determined.
pub fn default_prefix_path() -> Option<PathBuf> {
    prefixes_root().map(|root| root.join("battlenet"))
}

/// `$XDG_DATA_HOME/snow-launcher/prefixes`: the only place where Snow
/// Launcher creates (and may delete) prefixes.
pub fn prefixes_root() -> Option<PathBuf> {
    BaseDirs::new().map(|dirs| dirs.data_dir().join("snow-launcher").join("prefixes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_prefix(root: &Path) -> io::Result<()> {
        fs::create_dir_all(root.join("drive_c"))?;
        fs::write(root.join("system.reg"), "WINE REGISTRY Version 2\n")
    }

    #[test]
    fn opens_valid_prefix_through_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("battlenet");
        make_prefix(&real).unwrap();
        // Same layout as umu/Proton prefixes: `pfx -> .`
        std::os::unix::fs::symlink(".", real.join("pfx")).unwrap();

        let prefix = WinePrefix::open(&real.join("pfx")).unwrap();
        assert_eq!(prefix.path(), fs::canonicalize(&real).unwrap());
    }

    #[test]
    fn missing_path_is_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let err = WinePrefix::open(&tmp.path().join("nope")).unwrap_err();
        assert!(matches!(err, PrefixError::NotFound { .. }));
    }

    #[test]
    fn folder_without_registry_is_not_a_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("drive_c")).unwrap();
        let err = WinePrefix::open(tmp.path()).unwrap_err();
        assert!(matches!(
            err,
            PrefixError::NotAPrefix {
                missing: "system.reg",
                ..
            }
        ));
    }
}
