//! Finding games in other launchers' prefixes and copying one into Snow's
//! library (rules in `snowlauncher_core::import`). Other launchers' folders
//! are only read, never changed (`project.md`, section 5).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use directories::BaseDirs;
use snowlauncher_core::import::{
    ImportCheck, ImportRefusal, Tree, TreeDiff, check_import, check_space, compare_trees,
    import_target,
};
use snowlauncher_core::library::parse_build_info;

use crate::house::{HouseError, dir_size, house_in_use};
use crate::prefix::prefixes_root;

/// How deep below `drive_c` a game folder may be
/// (`drive_c/Program Files (x86)/World of Warcraft`).
const GAME_DEPTH: usize = 4;

/// A Blizzard game found in another launcher's prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundGame {
    /// Folder name, e.g. `World of Warcraft`.
    pub name: String,
    pub path: PathBuf,
    /// The product uid from `.build.info` (`wowf`).
    pub product: String,
    /// The prefix it lives in.
    pub prefix: PathBuf,
}

/// Wine prefixes of other launchers: Lutris' (from its game configs),
/// Steam's (`compatdata/*/pfx`) and folders in `~/Games` that are prefixes.
/// Snow's own prefixes are left out.
pub fn other_prefixes() -> Vec<PathBuf> {
    let Some(dirs) = BaseDirs::new() else {
        return Vec::new();
    };
    let home = dirs.home_dir();
    let mut prefixes: Vec<PathBuf> = Vec::new();
    for configs in [
        dirs.config_dir().join("lutris/games"),
        dirs.data_dir().join("lutris/games"),
        home.join(".var/app/net.lutris.Lutris/config/lutris/games"),
    ] {
        for entry in read_dir(&configs) {
            let Ok(text) = fs::read_to_string(&entry) else {
                continue;
            };
            prefixes.extend(lutris_prefixes(&text));
        }
    }
    for compatdata in [
        dirs.data_dir().join("Steam/steamapps/compatdata"),
        home.join(".var/app/com.valvesoftware.Steam/data/Steam/steamapps/compatdata"),
    ] {
        prefixes.extend(read_dir(&compatdata).into_iter().map(|app| app.join("pfx")));
    }
    prefixes.extend(read_dir(&home.join("Games")));

    let ours = prefixes_root().and_then(|root| fs::canonicalize(root).ok());
    let mut found: Vec<PathBuf> = Vec::new();
    for prefix in prefixes {
        let Ok(prefix) = fs::canonicalize(&prefix) else {
            continue;
        };
        let is_ours = ours.as_ref().is_some_and(|root| prefix.starts_with(root));
        if !is_ours && prefix.join("drive_c").is_dir() && !found.contains(&prefix) {
            found.push(prefix);
        }
    }
    found
}

/// `prefix:` lines of a Lutris game config, absolute paths only (not
/// `$GAMEDIR`).
fn lutris_prefixes(yaml: &str) -> Vec<PathBuf> {
    yaml.lines()
        .filter_map(|line| line.trim().strip_prefix("prefix:"))
        .map(|value| value.trim().trim_matches(['"', '\'']))
        .filter(|value| value.starts_with('/'))
        .map(PathBuf::from)
        .collect()
}

fn read_dir(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .map(|entries| entries.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default()
}

/// Blizzard games (folders with a `.build.info`) in the given prefixes.
pub fn find_games(prefixes: &[PathBuf]) -> Vec<FoundGame> {
    fn search(dir: &Path, depth: usize, prefix: &Path, found: &mut Vec<FoundGame>) {
        if let Some(build) = fs::read_to_string(dir.join(".build.info"))
            .ok()
            .and_then(|text| parse_build_info(&text))
        {
            found.push(FoundGame {
                name: dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                path: dir.to_owned(),
                product: build.product,
                prefix: prefix.to_owned(),
            });
            return;
        }
        if depth == 0 {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            // Real folders only: links (like the drive letters) are skipped.
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                search(&entry.path(), depth - 1, prefix, found);
            }
        }
    }
    let mut found = Vec::new();
    for prefix in prefixes {
        search(&prefix.join("drive_c"), GAME_DEPTH, prefix, &mut found);
    }
    found.sort_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));
    found
}

/// Every game Snow could import: [`find_games`] in [`other_prefixes`].
pub fn find_import_sources() -> Vec<FoundGame> {
    find_games(&other_prefixes())
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("not imported: {0}")]
    Refused(ImportRefusal),
    #[error("could not {what} {}", path.display())]
    Io {
        what: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("the copy failed ({0}); the partial copy was removed, the original is untouched")]
    Copy(String),
    #[error("{0}; the copy was removed, the original is untouched")]
    Verify(TreeDiff),
    #[error(transparent)]
    House(#[from] HouseError),
}

fn io_error<'a>(what: &'static str, path: &'a Path) -> impl FnOnce(io::Error) -> ImportError + 'a {
    move |source| ImportError::Io {
        what,
        path: path.to_owned(),
        source,
    }
}

/// The prefix a game folder lives in: the folder above its `drive_c`.
fn prefix_of(game: &Path) -> Option<PathBuf> {
    game.ancestors()
        .find(|dir| dir.file_name().is_some_and(|name| name == "drive_c"))
        .and_then(Path::parent)
        .map(Path::to_owned)
}

/// A file tree as `relative path → size`, links not followed.
pub fn tree_of(root: &Path) -> io::Result<Tree> {
    fn walk(root: &Path, dir: &Path, tree: &mut Tree) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let path = entry.path();
            if kind.is_dir() {
                walk(root, &path, tree)?;
            } else {
                let size = fs::symlink_metadata(&path)?.len();
                let relative = path.strip_prefix(root).unwrap_or(&path).to_owned();
                tree.insert(relative, size);
            }
        }
        Ok(())
    }
    let mut tree = Tree::new();
    walk(root, root, &mut tree)?;
    Ok(tree)
}

/// What [`import_game`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    pub target: PathBuf,
    /// Instant, space-free copy (reflink); else a plain copy.
    pub reflink: bool,
    pub files: usize,
    pub bytes: u64,
}

/// The checks of an import, without copying: `Ok(target)` if it would go.
pub fn check_game_import(source: &Path, library: &Path) -> Result<PathBuf, ImportError> {
    let source = fs::canonicalize(source).map_err(io_error("read", source))?;
    let library = fs::canonicalize(library).map_err(io_error("read", library))?;
    let target = import_target(&source, &library)
        .ok_or_else(|| ImportError::Refused(ImportRefusal::NotAGame(source.clone())))?;
    let source_in_use = match prefix_of(&source) {
        Some(prefix) => house_in_use(&prefix)?,
        None => Vec::new(),
    };
    check_import(&ImportCheck {
        source: &source,
        library: &library,
        is_game: source.join(".build.info").is_file(),
        target_exists: fs::symlink_metadata(&target).is_ok(),
        source_in_use,
    })
    .map_err(ImportError::Refused)?;
    Ok(target)
}

/// Copies a game folder into the library (see the module docs): to a
/// hidden temporary folder first, reflink when the disk allows, else a
/// plain copy after checking the free space; then compared file by file
/// with the original, and only then given its final name. The original is
/// never changed.
pub fn import_game(source: &Path, library: &Path) -> Result<Imported, ImportError> {
    let target = check_game_import(source, library)?;
    let source = fs::canonicalize(source).map_err(io_error("read", source))?;
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = target.with_file_name(format!(".{name}.importing"));
    // A leftover from an import that was cut short: ours, so it goes.
    if fs::symlink_metadata(&temp).is_ok_and(|meta| meta.is_dir()) {
        fs::remove_dir_all(&temp).map_err(io_error("remove", &temp))?;
    }

    let copy = |reflink: &str| -> Result<bool, ImportError> {
        let status = Command::new("cp")
            .args(["-a", reflink, "--"])
            .arg(&source)
            .arg(&temp)
            .status()
            .map_err(io_error("copy", &source))?;
        if !status.success() {
            let _ = fs::remove_dir_all(&temp);
        }
        Ok(status.success())
    };
    let reflink = copy("--reflink=always")?;
    if !reflink {
        let needed = dir_size(&source);
        let available =
            fs4::available_space(library).map_err(io_error("check the space of", library))?;
        check_space(needed, available).map_err(ImportError::Refused)?;
        if !copy("--reflink=never")? {
            return Err(ImportError::Copy("cp exited with an error".to_owned()));
        }
    }

    let original = tree_of(&source).map_err(io_error("read", &source))?;
    let copied = tree_of(&temp).map_err(io_error("read", &temp))?;
    if let Err(diff) = compare_trees(&original, &copied) {
        let _ = fs::remove_dir_all(&temp);
        return Err(ImportError::Verify(diff));
    }
    fs::rename(&temp, &target).map_err(io_error("rename", &temp))?;
    Ok(Imported {
        target,
        reflink,
        files: original.len(),
        bytes: original.values().sum(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_lutris_prefixes() {
        let yaml =
            "game:\n  exe: x\n  prefix: /home/u/Games/battlenet\nwine:\n  prefix: $GAMEDIR\n";
        assert_eq!(
            lutris_prefixes(yaml),
            [PathBuf::from("/home/u/Games/battlenet")]
        );
    }

    /// A prefix with a game in `Program Files (x86)`.
    fn fake_game(prefix: &Path) -> PathBuf {
        let game = prefix.join("drive_c/Program Files (x86)/World of Warcraft");
        fs::create_dir_all(game.join("_retail_")).unwrap();
        fs::write(
            game.join(".build.info"),
            "Active!DEC:1|Version!STRING:0|Product!STRING:0\n1|1.0|wowf\n",
        )
        .unwrap();
        fs::write(game.join("_retail_/Wow.exe"), vec![7u8; 4096]).unwrap();
        std::os::unix::fs::symlink("_retail_/Wow.exe", game.join("link")).unwrap();
        game
    }

    #[test]
    fn finds_games_in_prefixes() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("battlenet");
        let game = fake_game(&prefix);
        let found = find_games(std::slice::from_ref(&prefix));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "World of Warcraft");
        assert_eq!(found[0].product, "wowf");
        assert_eq!(found[0].path, game);
        assert_eq!(prefix_of(&game), Some(prefix));
    }

    #[test]
    fn imports_a_checked_copy_and_leaves_the_original() {
        let tmp = tempfile::tempdir().unwrap();
        let game = fake_game(&tmp.path().join("battlenet"));
        let library = tmp.path().join("library");
        fs::create_dir_all(&library).unwrap();
        let before = tree_of(&game).unwrap();

        let imported = import_game(&game, &library).unwrap();
        assert_eq!(
            imported.target,
            fs::canonicalize(&library)
                .unwrap()
                .join("World of Warcraft")
        );
        assert_eq!(tree_of(&imported.target).unwrap(), before);
        assert_eq!(tree_of(&game).unwrap(), before, "the original is untouched");
        assert!(!library.join(".World of Warcraft.importing").exists());

        // A second import is refused: it is in the library already.
        assert!(matches!(
            import_game(&game, &library),
            Err(ImportError::Refused(ImportRefusal::AlreadyInLibrary(_)))
        ));
    }
}
