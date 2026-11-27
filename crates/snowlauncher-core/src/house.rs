//! The "house": Snow Launcher's own Wine prefix for Battle.net
//! (project.md, section 5). This module only builds the plan; carrying it
//! out is up to other crates.
//!
//! What goes in comes from the experiments: a prefix created with umu-run
//! and GE-Proton (E6), Windows 11 so the in-game shop opens (E3, strong
//! indication), the desktop's DPI (E8) and the game library outside the
//! prefix, mapped as `G:` (E6).

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::display::DpiChoice;

/// Organization in the certificate that signs the Battle.net installer.
pub const BATTLENET_PUBLISHER: &str = "Blizzard Entertainment, Inc.";

/// Windows version the prefix reports (`winecfg -v`).
pub const WINDOWS_VERSION: &str = "win11";
/// Drive letter of the game library inside the prefix.
pub const LIBRARY_DRIVE: char = 'G';
/// Registry values that hold the DPI; the same three Lutris sets.
pub const DPI_REGISTRY_KEYS: [&str; 3] = [
    r"HKEY_CURRENT_USER\Control Panel\Desktop",
    r"HKEY_CURRENT_USER\Software\Wine\Fonts",
    r"HKEY_LOCAL_MACHINE\System\CurrentControlSet\Hardware Profiles\Current\Software\Fonts",
];

/// Where the DPI values land in the prefix's files, to verify them:
/// (file, section as written in the file, value name). Wine stores
/// `CurrentControlSet` as `ControlSet001`.
pub const DPI_REGISTRY_FILES: [(&str, &str, &str); 3] = [
    ("user.reg", r"Control Panel\\Desktop", "LogPixels"),
    ("user.reg", r"Software\\Wine\\Fonts", "LogPixels"),
    (
        "system.reg",
        r"System\\ControlSet001\\Hardware Profiles\\Current\\Software\\Fonts",
        "LogPixels",
    ),
];

/// Where `winecfg -v win11` leaves its mark, to verify it.
pub const WINDOWS_PRODUCT_FILE: (&str, &str, &str) = (
    "system.reg",
    r"Software\\Microsoft\\Windows NT\\CurrentVersion",
    "ProductName",
);
pub const WINDOWS_PRODUCT_NAME: &str = "Microsoft Windows 11";

/// Everything the plan needs, already resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HouseInputs {
    pub prefix: PathBuf,
    /// Folder name of the runtime, e.g. `GE-Proton11-14-x86_64`.
    pub runtime_name: String,
    pub library: PathBuf,
    pub dpi: DpiChoice,
}

/// What already exists on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Existing {
    pub prefix: bool,
    pub library: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HouseStep {
    CreatePrefix {
        prefix: PathBuf,
        runtime_name: String,
    },
    SetWindowsVersion(&'static str),
    SetDpi {
        dpi: u32,
    },
    CreateLibrary {
        path: PathBuf,
    },
    MapDrive {
        letter: char,
        target: PathBuf,
    },
}

impl fmt::Display for HouseStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreatePrefix {
                prefix,
                runtime_name,
            } => write!(
                f,
                "create the Wine prefix {} with umu-run and {runtime_name}",
                prefix.display()
            ),
            Self::SetWindowsVersion(version) => {
                write!(
                    f,
                    "set the Windows version to {version} (winecfg -v {version})"
                )
            }
            Self::SetDpi { dpi } => write!(
                f,
                "set the UI scale to {dpi} DPI ({} registry values)",
                DPI_REGISTRY_KEYS.len()
            ),
            Self::CreateLibrary { path } => {
                write!(f, "create the game library folder {}", path.display())
            }
            Self::MapDrive { letter, target } => {
                write!(f, "map {letter}: to {}", target.display())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    #[error("{} already exists; Snow Launcher never overwrites a prefix", .0.display())]
    PrefixExists(PathBuf),

    #[error(
        "the game library ({}) must be outside the prefix: the prefix can be \
         thrown away and recreated, the library cannot",
        .0.display()
    )]
    LibraryInsidePrefix(PathBuf),
}

/// What `snowctl house create` built, kept in Snow Launcher's state so it
/// can be undone exactly (`snowctl house remove`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HouseRecord {
    pub schema: u32,
    pub prefix: PathBuf,
    pub runtime_name: String,
    pub runtime_path: PathBuf,
    pub library: PathBuf,
    /// The library folder was created by Snow Launcher (not pre-existing).
    pub library_created: bool,
    pub dpi: u32,
    /// Unix time of creation.
    pub created_at: u64,
}

pub const HOUSE_RECORD_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoveStep {
    DeletePrefix {
        path: PathBuf,
    },
    DeleteEmptyLibrary {
        path: PathBuf,
    },
    /// The library stays: not created by Snow Launcher, or not empty.
    KeepLibrary {
        path: PathBuf,
        reason: &'static str,
    },
    ForgetRecord,
}

impl fmt::Display for RemoveStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeletePrefix { path } => write!(f, "delete the prefix {}", path.display()),
            Self::DeleteEmptyLibrary { path } => {
                write!(f, "delete the empty library folder {}", path.display())
            }
            Self::KeepLibrary { path, reason } => {
                write!(f, "keep the library {} ({reason})", path.display())
            }
            Self::ForgetRecord => f.write_str("forget the house record"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RemoveError {
    #[error("programs are still running in the house; close Battle.net and the games first")]
    StillRunning,
}

/// Steps to undo a house. Games are never deleted: the library goes only
/// if Snow Launcher created it and it is empty.
pub fn plan_removal(
    record: &HouseRecord,
    processes_running: bool,
    library_empty: bool,
) -> Result<Vec<RemoveStep>, RemoveError> {
    if processes_running {
        return Err(RemoveError::StillRunning);
    }
    let library = match (record.library_created, library_empty) {
        (true, true) => RemoveStep::DeleteEmptyLibrary {
            path: record.library.clone(),
        },
        (true, false) => RemoveStep::KeepLibrary {
            path: record.library.clone(),
            reason: "it has files in it",
        },
        (false, _) => RemoveStep::KeepLibrary {
            path: record.library.clone(),
            reason: "it existed before the house",
        },
    };
    Ok(vec![
        RemoveStep::DeletePrefix {
            path: record.prefix.clone(),
        },
        library,
        RemoveStep::ForgetRecord,
    ])
}

/// The steps to build a new house. Refuses to touch an existing prefix.
pub fn plan_house(inputs: &HouseInputs, existing: Existing) -> Result<Vec<HouseStep>, PlanError> {
    if existing.prefix {
        return Err(PlanError::PrefixExists(inputs.prefix.clone()));
    }
    if inputs.library.starts_with(&inputs.prefix) {
        return Err(PlanError::LibraryInsidePrefix(inputs.library.clone()));
    }
    let mut steps = vec![
        HouseStep::CreatePrefix {
            prefix: inputs.prefix.clone(),
            runtime_name: inputs.runtime_name.clone(),
        },
        HouseStep::SetWindowsVersion(WINDOWS_VERSION),
        // Before the installer runs, so it is readable too.
        HouseStep::SetDpi {
            dpi: inputs.dpi.dpi,
        },
    ];
    if !existing.library {
        steps.push(HouseStep::CreateLibrary {
            path: inputs.library.clone(),
        });
    }
    steps.push(HouseStep::MapDrive {
        letter: LIBRARY_DRIVE,
        target: inputs.library.clone(),
    });
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::DpiSource;

    fn inputs() -> HouseInputs {
        HouseInputs {
            prefix: PathBuf::from("/home/u/.local/share/snow-launcher/prefixes/battlenet"),
            runtime_name: "GE-Proton11-14-x86_64".to_owned(),
            library: PathBuf::from("/home/u/Games/SnowLauncher"),
            dpi: DpiChoice {
                dpi: 192,
                source: DpiSource::Desktop,
            },
        }
    }

    const NEW: Existing = Existing {
        prefix: false,
        library: false,
    };

    #[test]
    fn plans_a_new_house_in_order() {
        let steps: Vec<String> = plan_house(&inputs(), NEW)
            .unwrap()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            steps,
            [
                "create the Wine prefix /home/u/.local/share/snow-launcher/prefixes/battlenet with umu-run and GE-Proton11-14-x86_64",
                "set the Windows version to win11 (winecfg -v win11)",
                "set the UI scale to 192 DPI (3 registry values)",
                "create the game library folder /home/u/Games/SnowLauncher",
                "map G: to /home/u/Games/SnowLauncher",
            ]
        );
    }

    #[test]
    fn existing_library_is_reused() {
        let existing = Existing {
            prefix: false,
            library: true,
        };
        let steps = plan_house(&inputs(), existing).unwrap();
        assert!(
            !steps
                .iter()
                .any(|step| matches!(step, HouseStep::CreateLibrary { .. }))
        );
    }

    #[test]
    fn never_touches_an_existing_prefix() {
        let existing = Existing {
            prefix: true,
            library: true,
        };
        assert!(matches!(
            plan_house(&inputs(), existing),
            Err(PlanError::PrefixExists(_))
        ));
    }

    fn record(library_created: bool) -> HouseRecord {
        HouseRecord {
            schema: HOUSE_RECORD_SCHEMA,
            prefix: PathBuf::from("/home/u/.local/share/snow-launcher/prefixes/battlenet"),
            runtime_name: "GE-Proton11-14-x86_64".to_owned(),
            runtime_path: PathBuf::from("/x/GE-Proton11-14-x86_64"),
            library: PathBuf::from("/home/u/Games/SnowLauncher"),
            library_created,
            dpi: 192,
            created_at: 0,
        }
    }

    #[test]
    fn removal_never_deletes_games() {
        let text = |steps: Vec<RemoveStep>| -> Vec<String> {
            steps.iter().map(ToString::to_string).collect()
        };
        assert_eq!(
            text(plan_removal(&record(true), false, true).unwrap()),
            [
                "delete the prefix /home/u/.local/share/snow-launcher/prefixes/battlenet",
                "delete the empty library folder /home/u/Games/SnowLauncher",
                "forget the house record",
            ]
        );
        assert!(
            text(plan_removal(&record(true), false, false).unwrap())[1]
                .contains("keep the library /home/u/Games/SnowLauncher (it has files in it)")
        );
        assert!(
            text(plan_removal(&record(false), false, true).unwrap())[1]
                .contains("(it existed before the house)")
        );
    }

    #[test]
    fn removal_waits_for_running_programs() {
        assert_eq!(
            plan_removal(&record(true), true, true),
            Err(RemoveError::StillRunning)
        );
    }

    #[test]
    fn record_round_trips_through_toml() {
        let text = toml::to_string(&record(true)).unwrap();
        assert_eq!(toml::from_str::<HouseRecord>(&text).unwrap(), record(true));
    }

    #[test]
    fn library_must_be_outside_the_prefix() {
        let mut inside = inputs();
        inside.library = inside.prefix.join("drive_c/Games");
        assert!(matches!(
            plan_house(&inside, NEW),
            Err(PlanError::LibraryInsidePrefix(_))
        ));
    }
}
