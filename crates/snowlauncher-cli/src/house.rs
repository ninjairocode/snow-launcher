//! `snowctl house create|remove`: Snow Launcher's own Battle.net prefix.
//!
//! Both first resolve everything and show the plan; `--dry-run` stops
//! there. Creating is verified step by step and undone if a step fails;
//! removing never deletes games (see `snowlauncher_wine::house`).

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use directories::BaseDirs;
use snowlauncher_core::config::{Config, expand_home};
use snowlauncher_core::display::{DpiChoice, choose_dpi};
use snowlauncher_core::house::plan_removal;
use snowlauncher_core::house::{
    Existing, HouseInputs, HouseStep, LIBRARY_DRIVE, WINDOWS_VERSION, plan_house,
};
use snowlauncher_core::process::{ProcessTable, processes_in_prefix};
use snowlauncher_wine::desktop::desktop_dpi;
use snowlauncher_wine::house::{
    self as executor, UmuRunner, create_log_path, load_record, record_path, save_record,
};
use snowlauncher_wine::prefix::{default_prefix_path, prefixes_root};
use snowlauncher_wine::procfs::ProcfsProcessTable;
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

pub fn create(options: CreateOptions, config: &Config, out: &mut impl io::Write) -> Result<()> {
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
    out.write_all(render(&summary, &steps, options.dry_run).as_bytes())?;
    if options.dry_run {
        return Ok(());
    }

    let root = prefixes_root().context("could not determine the data directory")?;
    let record_file = record_path().context("could not determine the state directory")?;
    let umu_run = check_can_create(
        &summary.inputs.prefix,
        &root,
        summary.umu_run.clone(),
        load_record(&record_file)?.is_some(),
        &record_file,
    )?;
    let log = create_log_path().context("could not determine the state directory")?;
    let mut runner = UmuRunner {
        umu_run,
        prefix: summary.inputs.prefix.clone(),
        runtime: summary.runtime_path.clone(),
        log: log.clone(),
    };

    writeln!(out, "\nCreating (Wine tools log: {}):", log.display())?;
    let record = executor::create(
        &steps,
        &summary.inputs,
        &summary.runtime_path,
        &root,
        &mut runner,
        |step| {
            // Best effort: progress output must not stop the work.
            let _ = writeln!(out, "  - {step}");
            let _ = out.flush();
        },
    )
    .context("the house was not created; everything this run created was removed")?;
    save_record(&record_file, &record)?;
    writeln!(
        out,
        "\nDone. The house is ready; its record is in {}.\n\
         Undo it with `snowctl house remove`.",
        record_file.display()
    )?;
    Ok(())
}

/// Guards before changing anything. Returns the umu-run to use.
fn check_can_create(
    prefix: &Path,
    prefixes_root: &Path,
    umu_run: Option<PathBuf>,
    house_recorded: bool,
    record_file: &Path,
) -> Result<PathBuf> {
    ensure!(
        !house_recorded,
        "a house already exists (record in {}); remove it first with `snowctl house remove`",
        record_file.display()
    );
    ensure!(
        prefix.starts_with(prefixes_root) && prefix != prefixes_root,
        "Snow Launcher only creates prefixes inside {} (got {})",
        prefixes_root.display(),
        prefix.display()
    );
    umu_run.context("umu-run not found: install umu-launcher (or Lutris, which bundles it)")
}

pub fn remove(dry_run: bool, out: &mut impl io::Write) -> Result<()> {
    let record_file = record_path().context("could not determine the state directory")?;
    let Some(record) = load_record(&record_file)? else {
        writeln!(out, "No house recorded; nothing to remove.")?;
        return Ok(());
    };
    let processes = ProcfsProcessTable::new()
        .processes()
        .context("failed to list processes")?;
    let prefix = fs::canonicalize(&record.prefix).unwrap_or_else(|_| record.prefix.clone());
    let running = processes_in_prefix(&processes, &prefix).next().is_some();
    let library_empty = match fs::read_dir(&record.library) {
        Ok(mut entries) => entries.next().is_none(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(error) => {
            return Err(error)
                .with_context(|| format!("could not read {}", record.library.display()));
        }
    };
    let steps = plan_removal(&record, running, library_empty)?;

    let mode = if dry_run {
        " (dry run: nothing is changed)"
    } else {
        ""
    };
    writeln!(out, "Removing Snow Launcher's Battle.net house{mode}:")?;
    if dry_run {
        for step in &steps {
            writeln!(out, "  - would {step}")?;
        }
        return Ok(());
    }
    let root = prefixes_root().context("could not determine the data directory")?;
    executor::remove(&steps, &root, &record_file, |step| {
        let _ = writeln!(out, "  - {step}");
    })?;
    writeln!(out, "Done.")?;
    Ok(())
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

pub fn render(summary: &Summary, steps: &[HouseStep], dry_run: bool) -> String {
    let inputs = &summary.inputs;
    let mut out = String::new();
    // Writing to a String cannot fail, so the results are ignored.
    let mode = if dry_run {
        " (dry run: nothing is changed)"
    } else {
        ""
    };
    let _ = writeln!(out, "Plan for Snow Launcher's Battle.net house{mode}\n");
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
    if dry_run {
        let _ = writeln!(
            out,
            "\nAfter the house: install Battle.net and locate the games (not yet automated)."
        );
    }
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
        let out = render(&summary, &steps, true);
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
        assert!(render(&summary, &steps, false).contains("umu-run:   NOT FOUND"));
    }

    #[test]
    fn guards_before_changing_anything() {
        let root = Path::new("/home/u/.local/share/snow-launcher/prefixes");
        let record = Path::new("/home/u/.local/state/snow-launcher/house.toml");
        let umu = Some(PathBuf::from("/usr/bin/umu-run"));
        let inside = root.join("battlenet");

        assert!(check_can_create(&inside, root, umu.clone(), false, record).is_ok());
        let only_one = check_can_create(&inside, root, umu.clone(), true, record).unwrap_err();
        assert!(only_one.to_string().contains("already exists"));
        let outside = check_can_create(
            Path::new("/home/u/Games/battlenet"),
            root,
            umu.clone(),
            false,
            record,
        )
        .unwrap_err();
        assert!(outside.to_string().contains("only creates prefixes inside"));
        assert!(check_can_create(root, root, umu, false, record).is_err());
        let no_umu = check_can_create(&inside, root, None, false, record).unwrap_err();
        assert!(no_umu.to_string().contains("umu-run not found"));
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
