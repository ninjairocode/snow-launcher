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

use crate::display::DpiChoice;

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
