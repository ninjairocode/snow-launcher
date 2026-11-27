//! `snowctl house create`: Snow Launcher's own Battle.net prefix.
//!
//! For now only `--dry-run` exists: it resolves everything and shows the
//! plan, changing nothing. Carrying it out comes in a later version.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use directories::BaseDirs;
use snowlauncher_core::config::{Config, expand_home};
use snowlauncher_core::display::{DpiChoice, choose_dpi};
use snowlauncher_core::house::{
    Existing, HouseInputs, HouseStep, LIBRARY_DRIVE, WINDOWS_VERSION, plan_house,
};
use snowlauncher_wine::desktop::desktop_dpi;
use snowlauncher_wine::prefix::default_prefix_path;
use snowlauncher_wine::runtime::{Runtime, find_runtimes, find_umu_run, runtime_dirs};

/// Default game library, under the user's home.
const DEFAULT_LIBRARY: &str = "Games/SnowLauncher";

pub struct CreateOptions {
    pub dry_run: bool,
    pub prefix: Option<PathBuf>,
    pub library: Option<PathBuf>,
    pub runtime: Option<PathBuf>,
}

/// Everything resolved, shown above the steps.
pub struct Summary {
    pub inputs: HouseInputs,
    pub runtime_path: PathBuf,
    pub umu_run: Option<PathBuf>,
    pub library_exists: bool,
}

pub fn create(options: CreateOptions, config: &Config) -> Result<String> {
    if !options.dry_run {
        bail!(
            "Snow Launcher cannot create the house yet (v0.2 in progress). \
             Use --dry-run to see what it would do."
        );
    }
    let home = BaseDirs::new()
        .context("could not determine the home directory")?
        .home_dir()
        .to_owned();

    // Always Snow Launcher's own prefix: `prefix` in config.toml is the
    // prefix to *watch* (maybe Lutris's), not where to build the house.
    let prefix = match options.prefix {
        Some(path) => path,
        None => default_prefix_path().context("could not determine the data directory")?,
    };
    let library = absolute(
        options
            .library
            .or_else(|| config.house.library.clone())
            .unwrap_or_else(|| home.join(DEFAULT_LIBRARY)),
        &home,
        "library",
    )?;
    let runtime = pick_runtime(
        options.runtime.or_else(|| config.house.runtime.clone()),
        &home,
    )?;

    let inputs = HouseInputs {
        prefix: prefix.clone(),
        runtime_name: runtime.name.clone(),
        library: library.clone(),
        dpi: choose_dpi(config.display.ui_scale, desktop_dpi()),
    };
    let existing = Existing {
        prefix: prefix.exists(),
        library: library.is_dir(),
    };
    let steps = plan_house(&inputs, existing)?;
    let summary = Summary {
        inputs,
        runtime_path: runtime.path,
        umu_run: find_umu_run(),
        library_exists: existing.library,
    };
    Ok(render(&summary, &steps))
}

/// Expands `~/` and requires an absolute path.
fn absolute(path: PathBuf, home: &Path, what: &str) -> Result<PathBuf> {
    let expanded = expand_home(&path, home);
    ensure!(
        expanded.is_absolute(),
        "the {what} path must be absolute or start with ~/ (got {})",
        path.display()
    );
    Ok(expanded)
}

/// The configured runtime, else the newest GE-Proton found.
fn pick_runtime(configured: Option<PathBuf>, home: &Path) -> Result<Runtime> {
    if let Some(path) = configured {
        let path = absolute(path, home, "runtime")?;
        ensure!(
            path.join("proton").is_file(),
            "{} is not a Proton build (no `proton` script inside)",
            path.display()
        );
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        return Ok(Runtime {
            name,
            path,
            version: (0, 0),
        });
    }
    find_runtimes(&runtime_dirs()).into_iter().next().context(
        "no GE-Proton found; install one (for example with ProtonPlus or \
         ProtonUp-Qt) or set `runtime` under [house] in config.toml",
    )
}

pub fn render(summary: &Summary, steps: &[HouseStep]) -> String {
    let inputs = &summary.inputs;
    let mut out = String::new();
    // Writing to a String cannot fail, so the results are ignored.
    let _ = writeln!(
        out,
        "Plan for Snow Launcher's Battle.net house (dry run: nothing is changed)\n"
    );
    let _ = writeln!(out, "Prefix:    {} (new)", inputs.prefix.display());
    let _ = writeln!(
        out,
        "Runtime:   {} ({})",
        inputs.runtime_name,
        summary.runtime_path.display()
    );
    let umu = summary.umu_run.as_ref().map_or_else(
        || "NOT FOUND: needed to create the prefix (install umu-launcher)".to_owned(),
        |path| path.display().to_string(),
    );
    let _ = writeln!(out, "umu-run:   {umu}");
    let library_state = if summary.library_exists {
        "exists"
    } else {
        "will be created"
    };
    let _ = writeln!(
        out,
        "Library:   {} ({library_state}), seen as {LIBRARY_DRIVE}:",
        inputs.library.display()
    );
    let DpiChoice { dpi, source } = inputs.dpi;
    let _ = writeln!(
        out,
        "UI scale:  {}% ({dpi} DPI, from {source})",
        inputs.dpi.percent()
    );
    let _ = writeln!(out, "Windows:   {WINDOWS_VERSION}");

    let _ = writeln!(out, "\nSteps:");
    for (number, step) in steps.iter().enumerate() {
        let _ = writeln!(out, "  {}. {step}", number + 1);
    }
    let _ = writeln!(
        out,
        "\nNext, not part of this command yet: install Battle.net and locate the games."
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use snowlauncher_core::display::DpiSource;

    fn summary(umu: bool) -> (Summary, Vec<HouseStep>) {
        let inputs = HouseInputs {
            prefix: PathBuf::from("/home/u/.local/share/snow-launcher/prefixes/battlenet"),
            runtime_name: "GE-Proton11-14-x86_64".to_owned(),
            library: PathBuf::from("/home/u/Games/SnowLauncher"),
            dpi: DpiChoice {
                dpi: 192,
                source: DpiSource::Desktop,
            },
        };
        let steps = plan_house(
            &inputs,
            Existing {
                prefix: false,
                library: false,
            },
        )
        .unwrap();
        let summary = Summary {
            inputs,
            runtime_path: PathBuf::from(
                "/home/u/.local/share/Steam/compatibilitytools.d/GE-Proton11-14-x86_64",
            ),
            umu_run: umu.then(|| PathBuf::from("/usr/bin/umu-run")),
            library_exists: false,
        };
        (summary, steps)
    }

    #[test]
    fn shows_inputs_and_numbered_steps() {
        let (summary, steps) = summary(true);
        let out = render(&summary, &steps);
        assert!(out.contains("dry run: nothing is changed"));
        assert!(out.contains("UI scale:  200% (192 DPI, from the desktop)"));
        assert!(
            out.contains("Library:   /home/u/Games/SnowLauncher (will be created), seen as G:")
        );
        assert!(out.contains("  1. create the Wine prefix"));
        assert!(out.contains("  5. map G: to /home/u/Games/SnowLauncher"));
    }

    #[test]
    fn flags_a_missing_umu_run() {
        let (summary, steps) = summary(false);
        assert!(render(&summary, &steps).contains("umu-run:   NOT FOUND"));
    }

    #[test]
    fn refuses_to_run_without_dry_run() {
        let options = CreateOptions {
            dry_run: false,
            prefix: None,
            library: None,
            runtime: None,
        };
        let error = create(options, &Config::default()).unwrap_err();
        assert!(error.to_string().contains("--dry-run"));
    }

    #[test]
    fn relative_paths_are_rejected() {
        let home = Path::new("/home/u");
        assert!(absolute(PathBuf::from("Games/Snow"), home, "library").is_err());
        assert_eq!(
            absolute(PathBuf::from("~/Games/Snow"), home, "library").unwrap(),
            Path::new("/home/u/Games/Snow")
        );
    }
}
