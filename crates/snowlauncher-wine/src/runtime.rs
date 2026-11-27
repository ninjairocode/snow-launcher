//! Finding the Proton runtime and `umu-run`. Read-only: other launchers'
//! folders are only looked at, never changed (project.md, section 5).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use directories::BaseDirs;

/// A GE-Proton build found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runtime {
    /// Folder name, e.g. `GE-Proton11-14-x86_64`.
    pub name: String,
    pub path: PathBuf,
    /// `(11, 14)` for GE-Proton11-14.
    pub version: (u32, u32),
}

/// Folders where GE-Proton builds may live: Snow Launcher's own, then the
/// Steam ones used by ProtonPlus and ProtonUp-Qt (native and Flatpak).
pub fn runtime_dirs() -> Vec<PathBuf> {
    let Some(dirs) = BaseDirs::new() else {
        return Vec::new();
    };
    let data = dirs.data_dir();
    let home = dirs.home_dir();
    vec![
        data.join("snow-launcher/runtimes"),
        data.join("Steam/compatibilitytools.d"),
        home.join(".steam/root/compatibilitytools.d"),
        home.join(".var/app/com.valvesoftware.Steam/data/Steam/compatibilitytools.d"),
    ]
}

/// All GE-Proton builds in `dirs`, newest first, without duplicates.
pub fn find_runtimes(dirs: &[PathBuf]) -> Vec<Runtime> {
    let mut found: Vec<Runtime> = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(version) = ge_proton_version(&name) else {
                continue;
            };
            // The same build may be reachable through two Steam paths.
            let path = fs::canonicalize(entry.path()).unwrap_or_else(|_| entry.path());
            if !path.join("proton").is_file() || found.iter().any(|r| r.path == path) {
                continue;
            }
            found.push(Runtime {
                name,
                path,
                version,
            });
        }
    }
    found.sort_by(|a, b| b.version.cmp(&a.version).then(a.name.cmp(&b.name)));
    found
}

/// `GE-Proton11-14` or `GE-Proton11-14-x86_64` → `(11, 14)`.
fn ge_proton_version(name: &str) -> Option<(u32, u32)> {
    let rest = name.strip_prefix("GE-Proton")?;
    let rest = rest.strip_suffix("-x86_64").unwrap_or(rest);
    let (major, minor) = rest.split_once('-')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// `umu-run` from `PATH`, else the copy bundled with Lutris.
// TODO(v0.2): Snow Launcher should provide its own umu-launcher.
pub fn find_umu_run() -> Option<PathBuf> {
    let from_path = env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|dir| dir.join("umu-run"))
            .find(|candidate| is_executable(candidate))
    });
    from_path.or_else(|| {
        let lutris = BaseDirs::new()?
            .data_dir()
            .join("lutris/runtime/umu/umu-run");
        is_executable(&lutris).then_some(lutris)
    })
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_ge_proton_versions() {
        assert_eq!(ge_proton_version("GE-Proton11-14-x86_64"), Some((11, 14)));
        assert_eq!(ge_proton_version("GE-Proton9-27"), Some((9, 27)));
        assert_eq!(ge_proton_version("Proton-GE Latest"), None);
        assert_eq!(ge_proton_version("UMU-Proton-10.0-4"), None);
        assert_eq!(ge_proton_version("GE-ProtonX-1"), None);
    }

    fn fake_runtime(dir: &Path, name: &str) {
        let path = dir.join(name);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("proton"), "").unwrap();
    }

    #[test]
    fn newest_first_without_duplicates_or_broken_builds() {
        let tmp = tempfile::tempdir().unwrap();
        let snow = tmp.path().join("snow");
        let steam = tmp.path().join("steam");
        fake_runtime(&steam, "GE-Proton11-12-x86_64");
        fake_runtime(&steam, "GE-Proton11-14-x86_64");
        fake_runtime(&steam, "GE-Proton9-27");
        // Not a GE-Proton, and a folder without the `proton` script.
        fake_runtime(&steam, "UMU-Proton-10.0-4");
        fs::create_dir_all(steam.join("GE-Proton12-1")).unwrap();
        // The same build seen from a second folder (symlink).
        fs::create_dir_all(&snow).unwrap();
        std::os::unix::fs::symlink(
            steam.join("GE-Proton11-14-x86_64"),
            snow.join("GE-Proton11-14-x86_64"),
        )
        .unwrap();

        let names: Vec<String> = find_runtimes(&[snow, steam, tmp.path().join("missing")])
            .into_iter()
            .map(|runtime| runtime.name)
            .collect();
        assert_eq!(
            names,
            [
                "GE-Proton11-14-x86_64",
                "GE-Proton11-12-x86_64",
                "GE-Proton9-27"
            ]
        );
    }
}
