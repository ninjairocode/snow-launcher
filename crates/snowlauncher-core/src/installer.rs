//! Where the Battle.net installer comes from. Pure.
//!
//! Snow never ships Blizzard's installer (that would be redistribution)
//! and never downloads it itself (it does not talk to Blizzard's servers).
//! The user downloads it from the official page in the browser; Snow finds
//! it, checks Blizzard's signature, and keeps the last copy that passed as
//! the default for later installs.

use std::path::{Path, PathBuf};

/// The official download page, opened in the user's browser.
pub const OFFICIAL_DOWNLOAD_PAGE: &str = "https://download.battle.net/?product=bnetdesk";

/// File name of the kept copy.
pub const CACHED_INSTALLER_NAME: &str = "Battle.net-Setup.exe";

/// Whether a downloaded file looks like the installer by its name:
/// `Battle.net-Setup.exe`, or a browser's `Battle.net-Setup (1).exe`.
/// The name is only a hint: the signature decides.
pub fn is_installer_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("battle.net-setup") && lower.ends_with(".exe")
}

/// The installers to try, in order: the one the user named (alone: if it
/// fails, nothing else is tried behind their back); otherwise the
/// downloaded ones, newest first, then the kept copy.
pub fn installer_candidates(
    named: Option<&Path>,
    downloads_newest_first: &[PathBuf],
    kept: Option<&Path>,
) -> Vec<PathBuf> {
    if let Some(named) = named {
        return vec![named.to_owned()];
    }
    let mut candidates: Vec<PathBuf> = downloads_newest_first.to_vec();
    if let Some(kept) = kept {
        candidates.push(kept.to_owned());
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_installer_names() {
        assert!(is_installer_name("Battle.net-Setup.exe"));
        assert!(is_installer_name("Battle.net-Setup (1).exe"));
        assert!(is_installer_name("battle.net-setup-enUS.exe"));
        assert!(!is_installer_name("Battle.net-Setup.exe.part"));
        assert!(!is_installer_name("WoW-Setup.exe"));
    }

    #[test]
    fn named_installer_is_the_only_candidate() {
        let named = Path::new("/x/mine.exe");
        let downloads = [PathBuf::from("/d/Battle.net-Setup.exe")];
        assert_eq!(
            installer_candidates(Some(named), &downloads, Some(Path::new("/c/kept.exe"))),
            [PathBuf::from("/x/mine.exe")]
        );
    }

    #[test]
    fn otherwise_downloads_then_the_kept_copy() {
        let downloads = [
            PathBuf::from("/d/Battle.net-Setup (1).exe"),
            PathBuf::from("/d/Battle.net-Setup.exe"),
        ];
        assert_eq!(
            installer_candidates(None, &downloads, Some(Path::new("/c/kept.exe"))),
            [
                PathBuf::from("/d/Battle.net-Setup (1).exe"),
                PathBuf::from("/d/Battle.net-Setup.exe"),
                PathBuf::from("/c/kept.exe"),
            ]
        );
        assert!(installer_candidates(None, &[], None).is_empty());
    }
}
