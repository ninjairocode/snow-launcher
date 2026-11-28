//! Finding the Proton runtime and `umu-run`. Read-only: other launchers'
//! folders are only looked at, never changed (project.md, section 5).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use directories::BaseDirs;

/// A Proton build found on disk (GE-Proton, UMU-Proton, …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runtime {
    /// Folder name, e.g. `GE-Proton11-14-x86_64` or `Proton-GE Latest`.
    pub name: String,
    pub path: PathBuf,
    /// What it really is, from its `version` file (`GE-Proton11-14`);
    /// the folder name if that file is missing.
    pub label: String,
    /// `(11, 14)` for a GE-Proton 11-14; `None` for other builds.
    pub version: Option<(u32, u32)>,
}

/// Folders where Proton builds may live: Snow Launcher's own, then the
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

/// All Proton builds in `dirs` (folders with a `proton` script), without
/// duplicates: GE-Proton first, newest first (at the same version, a fixed
/// name before one that updates itself, like ProtonPlus' "Proton-GE
/// Latest"), then the others by name.
pub fn find_runtimes(dirs: &[PathBuf]) -> Vec<Runtime> {
    let mut found: Vec<Runtime> = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            // The same build may be reachable through two Steam paths.
            let path = fs::canonicalize(entry.path()).unwrap_or_else(|_| entry.path());
            if !path.join("proton").is_file() || found.iter().any(|r| r.path == path) {
                continue;
            }
            let label = fs::read_to_string(path.join("version"))
                .ok()
                .and_then(|text| version_label(&text))
                .unwrap_or_else(|| name.clone());
            let version = ge_proton_version(&label).or_else(|| ge_proton_version(&name));
            found.push(Runtime {
                name,
                path,
                label,
                version,
            });
        }
    }
    // `Some` sorts after `None`, so comparing b to a puts GE-Proton first.
    found.sort_by(|a, b| b.version.cmp(&a.version).then(a.name.cmp(&b.name)));
    found
}

/// The default runtime: the newest GE-Proton.
pub fn default_runtime(runtimes: &[Runtime]) -> Option<&Runtime> {
    runtimes.iter().find(|runtime| runtime.version.is_some())
}

/// The runtime called `wanted`: its folder name first, else what it is
/// (its label), ignoring case. A label shared by two folders (a fixed
/// GE-Proton and ProtonPlus' "Latest" copy of it) is refused: the user
/// must pick the folder.
pub fn choose_runtime<'a>(runtimes: &'a [Runtime], wanted: &str) -> Result<&'a Runtime, String> {
    if let Some(runtime) = runtimes
        .iter()
        .find(|r| r.name.eq_ignore_ascii_case(wanted))
    {
        return Ok(runtime);
    }
    let by_label: Vec<&Runtime> = runtimes
        .iter()
        .filter(|r| r.label.eq_ignore_ascii_case(wanted))
        .collect();
    match by_label.as_slice() {
        [one] => Ok(one),
        [] => {
            let names: Vec<&str> = runtimes.iter().map(|r| r.name.as_str()).collect();
            Err(format!(
                "no Proton called {wanted:?} (found: {})",
                names.join(", ")
            ))
        }
        several => {
            let names: Vec<&str> = several.iter().map(|r| r.name.as_str()).collect();
            Err(format!(
                "{wanted:?} matches several folders ({}); use the folder name",
                names.join(", ")
            ))
        }
    }
}

/// A Proton `version` file holds `<timestamp> <name>`, e.g.
/// `1795427215 GE-Proton11-14`.
fn version_label(text: &str) -> Option<String> {
    let label = text.split_whitespace().nth(1)?;
    Some(label.to_owned())
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

    fn runtime(name: &str, label: &str) -> Runtime {
        Runtime {
            name: name.to_owned(),
            path: PathBuf::from("/c").join(name),
            label: label.to_owned(),
            version: ge_proton_version(label),
        }
    }

    #[test]
    fn chooses_by_folder_then_by_label() {
        let runtimes = [
            runtime("GE-Proton11-14-x86_64", "GE-Proton11-14"),
            runtime("Proton-GE Latest", "GE-Proton11-14"),
            runtime("UMU-Proton-10.0-4", "UMU-Proton-10.0-4"),
        ];
        let name = |wanted| choose_runtime(&runtimes, wanted).map(|r| r.name.as_str());
        assert_eq!(name("proton-ge latest"), Ok("Proton-GE Latest"));
        assert_eq!(name("UMU-Proton-10.0-4"), Ok("UMU-Proton-10.0-4"));
        assert!(name("GE-Proton11-14").unwrap_err().contains("several"));
        assert!(
            name("GE-Proton9-1")
                .unwrap_err()
                .contains("found: GE-Proton11-14-x86_64")
        );
    }

    #[test]
    fn reads_the_version_file() {
        assert_eq!(
            version_label("1795427215 GE-Proton11-14\n").as_deref(),
            Some("GE-Proton11-14")
        );
        assert_eq!(version_label("1795427215"), None);
    }

    #[test]
    fn lists_every_proton_ge_first() {
        let tmp = tempfile::tempdir().unwrap();
        let steam = tmp.path().join("steam");
        fake_runtime(&steam, "UMU-Proton-10.0-4");
        fake_runtime(&steam, "GE-Proton11-14-x86_64");
        fake_runtime(&steam, "GE-Proton11-12-x86_64");
        // ProtonPlus' self-updating copy: its version file says what it is.
        fake_runtime(&steam, "Proton-GE Latest");
        fs::write(
            steam.join("Proton-GE Latest/version"),
            "1795427215 GE-Proton11-14\n",
        )
        .unwrap();

        let runtimes = find_runtimes(&[steam]);
        let names: Vec<&str> = runtimes.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "GE-Proton11-14-x86_64",
                "Proton-GE Latest",
                "GE-Proton11-12-x86_64",
                "UMU-Proton-10.0-4"
            ]
        );
        assert_eq!(runtimes[1].label, "GE-Proton11-14");
        assert_eq!(runtimes[1].version, Some((11, 14)));
        assert_eq!(runtimes[3].version, None);
        assert_eq!(
            default_runtime(&runtimes).map(|r| r.name.as_str()),
            Some("GE-Proton11-14-x86_64")
        );
        assert_eq!(default_runtime(&runtimes[3..]), None);
    }

    #[test]
    fn newest_first_without_duplicates_or_broken_builds() {
        let tmp = tempfile::tempdir().unwrap();
        let snow = tmp.path().join("snow");
        let steam = tmp.path().join("steam");
        fake_runtime(&steam, "GE-Proton11-12-x86_64");
        fake_runtime(&steam, "GE-Proton11-14-x86_64");
        fake_runtime(&steam, "GE-Proton9-27");
        // Not a GE-Proton (listed last), and a folder without the `proton`
        // script (not listed).
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
                "GE-Proton9-27",
                "UMU-Proton-10.0-4"
            ]
        );
    }
}
