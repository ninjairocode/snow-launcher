//! Snow's menu entry and icon in the user's folders, for an AppImage or a
//! development build (a system package ships its own; see
//! `snowlauncher_core::packaging`). Only what changed is written, so it
//! can run at every start: if the AppImage moved, the entry follows it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::BaseDirs;
use snowlauncher_core::packaging::{APP_ID, InstallKind, desktop_entry_for};

use crate::house::write_atomic;

const DESKTOP_TEMPLATE: &str =
    include_str!("../../../data/io.github.ninjairocode.SnowLauncher.desktop");
const ICON_SVG: &[u8] =
    include_bytes!("../../../data/icons/io.github.ninjairocode.SnowLauncher.svg");
const LAUNCHER_BINARY: &str = "snow-launcher";

/// What [`ensure_desktop_entry`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryUpdate {
    /// A system package owns the entry: nothing done.
    NotOurs,
    Unchanged,
    Written,
}

/// The program the entry should run: the `.AppImage` file, or the
/// `snow-launcher` next to the running program.
fn launcher_program(kind: &InstallKind) -> Option<PathBuf> {
    match kind {
        InstallKind::AppImage(image) => Some(image.clone()),
        InstallKind::Development => std::env::current_exe()
            .ok()
            .map(|exe| exe.with_file_name(LAUNCHER_BINARY))
            .filter(|path| path.is_file()),
        InstallKind::SystemPackage => None,
    }
}

/// Makes sure the menu entry and the icon are in place for this install.
pub fn ensure_desktop_entry(kind: &InstallKind) -> io::Result<EntryUpdate> {
    if !kind.writes_desktop_entry() {
        return Ok(EntryUpdate::NotOurs);
    }
    let (Some(program), Some(dirs)) = (launcher_program(kind), BaseDirs::new()) else {
        return Ok(EntryUpdate::Unchanged);
    };
    ensure_in(dirs.data_dir(), &program)
}

/// [`ensure_desktop_entry`] under a given data folder (`~/.local/share`).
fn ensure_in(data_dir: &Path, program: &Path) -> io::Result<EntryUpdate> {
    let entry = data_dir
        .join("applications")
        .join(format!("{APP_ID}.desktop"));
    let icon = data_dir
        .join("icons/hicolor/scalable/apps")
        .join(format!("{APP_ID}.svg"));
    let mut written = false;
    for (path, content) in [
        (
            &entry,
            desktop_entry_for(DESKTOP_TEMPLATE, program).into_bytes(),
        ),
        (&icon, ICON_SVG.to_vec()),
    ] {
        if fs::read(path).is_ok_and(|current| current == content) {
            continue;
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        write_atomic(path, &content)?;
        written = true;
    }
    Ok(if written {
        EntryUpdate::Written
    } else {
        EntryUpdate::Unchanged
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_once_and_follows_a_moved_appimage() {
        let tmp = tempfile::tempdir().unwrap();
        let first = Path::new("/home/u/Downloads/Snow_Launcher-x86_64.AppImage");
        assert_eq!(ensure_in(tmp.path(), first).unwrap(), EntryUpdate::Written);
        assert_eq!(
            ensure_in(tmp.path(), first).unwrap(),
            EntryUpdate::Unchanged
        );

        let entry = tmp
            .path()
            .join("applications/io.github.ninjairocode.SnowLauncher.desktop");
        let text = fs::read_to_string(&entry).unwrap();
        assert!(
            text.contains(&format!("Exec=\"{}\"", first.display())),
            "{text}"
        );
        assert!(
            tmp.path()
                .join("icons/hicolor/scalable/apps/io.github.ninjairocode.SnowLauncher.svg")
                .is_file()
        );

        let moved = Path::new("/home/u/Apps/Snow_Launcher-x86_64.AppImage");
        assert_eq!(ensure_in(tmp.path(), moved).unwrap(), EntryUpdate::Written);
        assert!(
            fs::read_to_string(&entry)
                .unwrap()
                .contains("/home/u/Apps/Snow_Launcher")
        );
    }

    #[test]
    fn a_package_keeps_its_own_entry() {
        assert_eq!(
            ensure_desktop_entry(&InstallKind::SystemPackage).unwrap(),
            EntryUpdate::NotOurs
        );
    }
}
