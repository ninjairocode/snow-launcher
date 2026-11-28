//! Game icons, read at runtime from the user's own game executables (the
//! `RT_GROUP_ICON` resources of the `.exe`). No game art ships with Snow
//! Launcher. Read-only on the game: the icon is copied to Snow's cache.
//!
//! The executable is read into memory instead of memory-mapped: a game
//! update rewriting the file while it is mapped would crash us (SIGBUS).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use directories::BaseDirs;
use pelite::PeFile;
use snowlauncher_core::process::KNOWN_GAMES;

use crate::house::write_atomic;
use crate::library::InstalledGame;

/// Executables bigger than this are not read (WoW's is ~80 MB).
const MAX_EXE_BYTES: u64 = 512 * 1024 * 1024;
/// How deep below the game folder its executable may be
/// (`World of Warcraft/_retail_/Wow.exe`).
const EXE_DEPTH: usize = 2;
const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

#[derive(Debug, thiserror::Error)]
pub enum IconError {
    #[error("could not read {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{path} is too big to read ({size} bytes)")]
    TooBig { path: PathBuf, size: u64 },
    #[error("{path} is not a readable Windows executable: {problem}")]
    Pe { path: PathBuf, problem: String },
}

/// An icon image as stored in the executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IconImage {
    /// Modern executables store the large sizes as PNG.
    Png(Vec<u8>),
    /// Older ones only have bitmaps; kept as a whole `.ico` file, which
    /// GTK reads.
    Ico(Vec<u8>),
}

impl IconImage {
    fn extension(&self) -> &'static str {
        match self {
            IconImage::Png(_) => "png",
            IconImage::Ico(_) => "ico",
        }
    }

    fn bytes(&self) -> &[u8] {
        match self {
            IconImage::Png(bytes) | IconImage::Ico(bytes) => bytes,
        }
    }
}

/// One size of an icon group: width in pixels (0 in the file means 256),
/// bits per pixel and resource id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Entry {
    width: u32,
    bit_count: u16,
    id: u16,
}

/// The largest, then deepest-colored, size.
fn best_entry(entries: &[Entry]) -> Option<Entry> {
    entries
        .iter()
        .copied()
        .max_by_key(|entry| (entry.width, entry.bit_count))
}

/// The game's executable: a known game's file name (`Wow.exe`) in the
/// game folder or up to [`EXE_DEPTH`] levels below it.
pub fn game_executable(game_dir: &Path) -> Option<PathBuf> {
    fn search(dir: &Path, depth: usize) -> Option<PathBuf> {
        let mut subdirs = Vec::new();
        for entry in fs::read_dir(dir).ok()?.filter_map(Result::ok) {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_file() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if KNOWN_GAMES
                    .iter()
                    .any(|(exe, _)| exe.eq_ignore_ascii_case(&name))
                {
                    return Some(path);
                }
            } else if kind.is_dir() && depth > 0 {
                subdirs.push(path);
            }
        }
        subdirs.sort();
        subdirs.iter().find_map(|sub| search(sub, depth - 1))
    }
    search(game_dir, EXE_DEPTH)
}

/// The best icon of an executable, or `None` if it has none.
pub fn extract_icon(exe: &Path) -> Result<Option<IconImage>, IconError> {
    let read_error = |source| IconError::Read {
        path: exe.to_owned(),
        source,
    };
    let size = fs::metadata(exe).map_err(read_error)?.len();
    if size > MAX_EXE_BYTES {
        return Err(IconError::TooBig {
            path: exe.to_owned(),
            size,
        });
    }
    let bytes = fs::read(exe).map_err(read_error)?;
    let pe_error = |problem: String| IconError::Pe {
        path: exe.to_owned(),
        problem,
    };
    let pe = PeFile::from_bytes(&bytes).map_err(|error| pe_error(error.to_string()))?;
    let Ok(resources) = pe.resources() else {
        return Ok(None); // no resource section: no icon
    };
    // The first icon group is the one Windows shows for the file.
    let Some(Ok((_, group))) = resources.icons().next() else {
        return Ok(None);
    };
    let entries: Vec<Entry> = group
        .entries()
        .iter()
        .map(|entry| Entry {
            width: if entry.bWidth == 0 {
                256
            } else {
                u32::from(entry.bWidth)
            },
            bit_count: entry.wBitCount,
            id: entry.nId,
        })
        .collect();
    let Some(best) = best_entry(&entries) else {
        return Ok(None);
    };
    let image = group
        .image(best.id)
        .map_err(|error| pe_error(error.to_string()))?;
    if image.starts_with(PNG_MAGIC) {
        return Ok(Some(IconImage::Png(image.to_vec())));
    }
    let mut ico = Vec::new();
    group
        .write(&mut ico)
        .map_err(|error| pe_error(error.to_string()))?;
    Ok(Some(IconImage::Ico(ico)))
}

/// `~/.cache/snow-launcher/icons`.
pub fn icon_cache_dir() -> Option<PathBuf> {
    BaseDirs::new().map(|dirs| dirs.cache_dir().join("snow-launcher").join("icons"))
}

/// The cached icon of a game, extracted again when its executable is newer
/// than the cache. `None` when there is no executable, no icon or any
/// error: a game without an icon is fine.
pub fn game_icon(game: &InstalledGame) -> Option<PathBuf> {
    cached_icon(game, &icon_cache_dir()?)
}

fn cached_icon(game: &InstalledGame, cache: &Path) -> Option<PathBuf> {
    let exe = game_executable(&game.path)?;
    let exe_time = modified(&exe)?;
    // The product uid (`wowf`) names the cache file: it is unique per
    // game and safe as a file name.
    let uid: String = game
        .build
        .product
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if uid.is_empty() {
        return None;
    }
    for extension in ["png", "ico"] {
        let cached = cache.join(format!("{uid}.{extension}"));
        if modified(&cached).is_some_and(|time| time >= exe_time) {
            return Some(cached);
        }
    }
    let icon = extract_icon(&exe).ok().flatten()?;
    fs::create_dir_all(cache).ok()?;
    let path = cache.join(format!("{uid}.{}", icon.extension()));
    write_atomic(&path, icon.bytes()).ok()?;
    Some(path)
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|meta| meta.modified()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use snowlauncher_core::library::BuildInfo;

    #[test]
    fn picks_the_largest_then_deepest_icon() {
        let entries = [
            Entry {
                width: 32,
                bit_count: 32,
                id: 1,
            },
            Entry {
                width: 256,
                bit_count: 8,
                id: 2,
            },
            Entry {
                width: 256,
                bit_count: 32,
                id: 3,
            },
            Entry {
                width: 48,
                bit_count: 32,
                id: 4,
            },
        ];
        assert_eq!(best_entry(&entries).map(|e| e.id), Some(3));
        assert_eq!(best_entry(&[]), None);
    }

    #[test]
    fn finds_the_game_executable_below_the_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("World of Warcraft");
        fs::create_dir_all(game.join("_forever_")).unwrap();
        fs::write(game.join("World of Warcraft Launcher.exe"), "").unwrap();
        fs::write(game.join("_forever_/BlizzardError.exe"), "").unwrap();
        fs::write(game.join("_forever_/wow.exe"), "").unwrap(); // any case
        let exe = game_executable(&game).unwrap();
        assert!(exe.ends_with("_forever_/wow.exe"), "{}", exe.display());

        let empty = tmp.path().join("Empty");
        fs::create_dir_all(&empty).unwrap();
        assert_eq!(game_executable(&empty), None);
    }

    #[test]
    fn a_broken_executable_has_no_icon_and_no_panic() {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("World of Warcraft");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("Wow.exe"), b"MZ not really a PE").unwrap();
        assert!(matches!(
            extract_icon(&game.join("Wow.exe")),
            Err(IconError::Pe { .. })
        ));
        let installed = InstalledGame {
            name: "World of Warcraft".to_owned(),
            path: game,
            build: BuildInfo {
                product: "wowf".to_owned(),
                version: None,
            },
            launch_code: None,
        };
        assert_eq!(cached_icon(&installed, &tmp.path().join("cache")), None);
    }

    /// Reads the icon of a real game, if `SNOW_TEST_EXE` points to one.
    #[test]
    #[ignore = "needs a real game executable in SNOW_TEST_EXE"]
    fn extracts_a_real_icon() {
        let exe = PathBuf::from(std::env::var_os("SNOW_TEST_EXE").unwrap());
        let icon = extract_icon(&exe).unwrap().unwrap();
        let bytes = icon.bytes();
        println!("{} icon, {} bytes", icon.extension(), bytes.len());
        if let IconImage::Png(png) = &icon {
            // IHDR: width and height, big-endian, right after the signature.
            let side = |at: usize| u32::from_be_bytes(png[at..at + 4].try_into().unwrap());
            println!("{} x {} px", side(16), side(20));
        }
    }
}
