//! Importing a game from another launcher into Snow's library. Pure: the
//! decisions and checks, without touching the disk.
//!
//! - The game is **copied**, never moved or changed: it keeps working in
//!   the other launcher. With reflink (Btrfs, XFS) the copy is instant and
//!   takes no space (O9).
//! - The copy is checked file by file (path and size) before it gets its
//!   final name.
//! - Battle.net does not find a copied game by itself (O6: WoW only showed
//!   after "Locate the game"); Snow never writes Battle.net's own data to
//!   make it, so the user is guided to "Locate the game".

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::house::LIBRARY_DRIVE;

/// Why a game is not imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportRefusal {
    /// No `.build.info`: not an installed Blizzard game folder.
    NotAGame(PathBuf),
    /// The library already has a folder with that name.
    AlreadyInLibrary(PathBuf),
    /// The folder is inside the library already.
    InsideLibrary(PathBuf),
    /// Programs run in the other launcher's prefix (Battle.net, the game):
    /// files may be changing.
    SourceInUse(Vec<String>),
    /// A plain copy (no reflink) would not fit.
    NotEnoughSpace { needed: u64, available: u64 },
}

impl fmt::Display for ImportRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAGame(path) => write!(
                f,
                "{} is not an installed Blizzard game (no .build.info)",
                path.display()
            ),
            Self::AlreadyInLibrary(path) => {
                write!(f, "the library already has {}", path.display())
            }
            Self::InsideLibrary(path) => {
                write!(f, "{} is already in the library", path.display())
            }
            Self::SourceInUse(names) => write!(
                f,
                "programs run in that launcher's prefix ({}); close Battle.net and the game there first",
                names.join(", ")
            ),
            Self::NotEnoughSpace { needed, available } => write!(
                f,
                "not enough free space: the copy needs {} GB, {} GB are free",
                needed / 1_000_000_000,
                available / 1_000_000_000
            ),
        }
    }
}

/// What the importer knows before copying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportCheck<'a> {
    pub source: &'a Path,
    pub library: &'a Path,
    pub is_game: bool,
    pub target_exists: bool,
    /// Programs running in the source's prefix, if it has one.
    pub source_in_use: Vec<String>,
}

/// Where the game goes: `<library>/<its folder name>`.
pub fn import_target(source: &Path, library: &Path) -> Option<PathBuf> {
    source.file_name().map(|name| library.join(name))
}

/// The checks before any copy.
pub fn check_import(check: &ImportCheck<'_>) -> Result<(), ImportRefusal> {
    if check.source.starts_with(check.library) {
        return Err(ImportRefusal::InsideLibrary(check.source.to_owned()));
    }
    if !check.is_game {
        return Err(ImportRefusal::NotAGame(check.source.to_owned()));
    }
    if check.target_exists {
        let target = import_target(check.source, check.library).unwrap_or_default();
        return Err(ImportRefusal::AlreadyInLibrary(target));
    }
    if !check.source_in_use.is_empty() {
        return Err(ImportRefusal::SourceInUse(check.source_in_use.clone()));
    }
    Ok(())
}

/// Room to leave free on the disk after a plain copy.
const SPACE_MARGIN: u64 = 1_000_000_000;

/// The space check for a plain copy (a reflink copy needs none).
pub fn check_space(needed: u64, available: u64) -> Result<(), ImportRefusal> {
    if needed.saturating_add(SPACE_MARGIN) > available {
        return Err(ImportRefusal::NotEnoughSpace { needed, available });
    }
    Ok(())
}

/// A file tree as `relative path → size` (symlinks counted as links).
pub type Tree = BTreeMap<PathBuf, u64>;

/// How a copy differs from its source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeDiff {
    pub missing: Vec<PathBuf>,
    pub extra: Vec<PathBuf>,
    pub different_size: Vec<PathBuf>,
}

impl fmt::Display for TreeDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the copy differs from the original: {} missing, {} extra, {} of another size",
            self.missing.len(),
            self.extra.len(),
            self.different_size.len()
        )?;
        if let Some(first) = self
            .missing
            .first()
            .or(self.different_size.first())
            .or(self.extra.first())
        {
            write!(f, " (e.g. {})", first.display())?;
        }
        Ok(())
    }
}

/// Whether `copy` has exactly the files of `source`, with the same sizes.
pub fn compare_trees(source: &Tree, copy: &Tree) -> Result<(), TreeDiff> {
    let missing: Vec<PathBuf> = source
        .keys()
        .filter(|path| !copy.contains_key(*path))
        .cloned()
        .collect();
    let extra: Vec<PathBuf> = copy
        .keys()
        .filter(|path| !source.contains_key(*path))
        .cloned()
        .collect();
    let different_size: Vec<PathBuf> = source
        .iter()
        .filter(|(path, size)| copy.get(*path).is_some_and(|other| other != *size))
        .map(|(path, _)| path.clone())
        .collect();
    if missing.is_empty() && extra.is_empty() && different_size.is_empty() {
        Ok(())
    } else {
        Err(TreeDiff {
            missing,
            extra,
            different_size,
        })
    }
}

/// The path to give Battle.net's "Locate the game": `G:\World of Warcraft`.
pub fn locate_path(folder_name: &str) -> String {
    format!("{LIBRARY_DRIVE}:\\{folder_name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check<'a>(source: &'a Path, library: &'a Path) -> ImportCheck<'a> {
        ImportCheck {
            source,
            library,
            is_game: true,
            target_exists: false,
            source_in_use: Vec::new(),
        }
    }

    const LUTRIS_WOW: &str =
        "/home/u/Games/battlenet/drive_c/Program Files (x86)/World of Warcraft";
    const LIBRARY: &str = "/home/u/Games/SnowLauncher";

    #[test]
    fn a_free_game_from_another_launcher_is_imported() {
        let (source, library) = (Path::new(LUTRIS_WOW), Path::new(LIBRARY));
        assert_eq!(check_import(&check(source, library)), Ok(()));
        assert_eq!(
            import_target(source, library),
            Some(PathBuf::from(
                "/home/u/Games/SnowLauncher/World of Warcraft"
            ))
        );
        assert_eq!(locate_path("World of Warcraft"), "G:\\World of Warcraft");
    }

    #[test]
    fn refusals() {
        let (source, library) = (Path::new(LUTRIS_WOW), Path::new(LIBRARY));
        let refusal = |change: fn(&mut ImportCheck<'_>)| {
            let mut c = check(source, library);
            change(&mut c);
            check_import(&c).unwrap_err()
        };
        assert!(matches!(
            refusal(|c| c.is_game = false),
            ImportRefusal::NotAGame(_)
        ));
        assert!(matches!(
            refusal(|c| c.target_exists = true),
            ImportRefusal::AlreadyInLibrary(_)
        ));
        let in_use = refusal(|c| c.source_in_use = vec!["Battle.net.exe".to_owned()]);
        assert!(in_use.to_string().contains("Battle.net.exe"), "{in_use}");

        let inside = Path::new("/home/u/Games/SnowLauncher/World of Warcraft");
        assert!(matches!(
            check_import(&check(inside, library)),
            Err(ImportRefusal::InsideLibrary(_))
        ));
    }

    #[test]
    fn space_for_a_plain_copy_keeps_a_margin() {
        let gb = 1_000_000_000;
        assert_eq!(check_space(49 * gb, 100 * gb), Ok(()));
        assert!(check_space(49 * gb, 49 * gb + gb / 2).is_err());
    }

    #[test]
    fn compares_copies_by_path_and_size() {
        let tree = |entries: &[(&str, u64)]| -> Tree {
            entries
                .iter()
                .map(|(path, size)| (PathBuf::from(path), *size))
                .collect()
        };
        let source = tree(&[(".build.info", 900), ("_retail_/Wow.exe", 80_000_000)]);
        assert_eq!(compare_trees(&source, &source.clone()), Ok(()));

        let short = tree(&[(".build.info", 900), ("_retail_/Wow.exe", 1)]);
        let diff = compare_trees(&source, &short).unwrap_err();
        assert_eq!(diff.different_size, [PathBuf::from("_retail_/Wow.exe")]);

        let partial = tree(&[(".build.info", 900)]);
        let diff = compare_trees(&source, &partial).unwrap_err();
        assert_eq!(diff.missing.len(), 1);
        assert!(diff.to_string().contains("1 missing"), "{diff}");
    }
}
