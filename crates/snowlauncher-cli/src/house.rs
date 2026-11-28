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
use snowlauncher_core::house::{BATTLENET_EXE, plan_removal};
use snowlauncher_core::house::{
    Existing, HouseInputs, HouseStep, LIBRARY_DRIVE, WINDOWS_VERSION, plan_house,
};
use snowlauncher_core::install::InstallState;
use snowlauncher_core::process::{ProcessTable, processes_in_prefix};
use snowlauncher_wine::desktop::desktop_dpi;
use snowlauncher_wine::house::{
    self as executor, UmuRunner, create_log_path, launch_argument, load_record, record_path,
    save_record,
};
use snowlauncher_wine::library::{installed_games, known_product_codes};
use snowlauncher_wine::prefix::{default_prefix_path, prefixes_root};
use snowlauncher_wine::procfs::ProcfsProcessTable;
use snowlauncher_wine::runtime::{
    Runtime, choose_runtime, default_runtime, find_runtimes, find_umu_run, runtime_dirs,
};

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
        // Creating the prefix: no game, so no performance options.
        extra_env: Vec::new(),
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

/// How often the installer is looked at, and progress printed.
const WATCH_EVERY: std::time::Duration = std::time::Duration::from_secs(2);
const PRINT_EVERY: std::time::Duration = std::time::Duration::from_secs(10);

/// `snowctl house install-battlenet`: runs the verified official installer
/// in the house and watches it until Battle.net opens.
pub fn install_battlenet(
    installer: Option<&Path>,
    dry_run: bool,
    out: &mut impl io::Write,
) -> Result<()> {
    let record_file = record_path().context("could not determine the state directory")?;
    let record = load_record(&record_file)?
        .context("there is no house yet; create it first with `snowctl house create`")?;
    let prefix = fs::canonicalize(&record.prefix)
        .with_context(|| format!("the house's prefix {} is missing", record.prefix.display()))?;
    ensure!(
        !prefix.join(BATTLENET_EXE).exists(),
        "Battle.net is already installed in the house ({})",
        prefix.join(BATTLENET_EXE).display()
    );
    let processes = ProcfsProcessTable::new()
        .processes()
        .context("failed to list processes")?;
    ensure!(
        processes_in_prefix(&processes, &prefix).next().is_none(),
        "programs are running in the house; close them first"
    );
    // Only Blizzard's installer, unmodified (project.md, section 5).
    let (installer, report) = crate::installer::choose(installer)?;
    let installer = installer.as_path();
    // Checked early, so a dry run already says so.
    find_umu_run()
        .context("umu-run not found: install umu-launcher (or Lutris, which bundles it)")?;

    let mode = if dry_run {
        " (dry run: nothing is changed)"
    } else {
        ""
    };
    writeln!(out, "Install Battle.net into the house{mode}\n")?;
    writeln!(out, "House:     {}", prefix.display())?;
    out.write_all(report.as_bytes())?;
    writeln!(
        out,
        "\nThe installer window will open: follow it. Snow Launcher watches until \
         Battle.net opens (stuck after 5 minutes without progress, gives up after 30)."
    )?;
    if dry_run {
        return Ok(());
    }

    writeln!(out)?;
    let mut last_print = None::<std::time::Instant>;
    let mut printed = Ok(());
    let state = executor::install_battlenet(installer, |state, elapsed| {
        if let InstallState::Installing { installed_bytes } = state {
            if last_print.is_none_or(|at| at.elapsed() >= PRINT_EVERY) {
                // Stop writing after the first failure; reported below.
                if printed.is_ok() {
                    printed = writeln!(
                        out,
                        "  {}  installing… {} MB",
                        clock(elapsed),
                        installed_bytes / 1_000_000
                    )
                    .and_then(|()| out.flush());
                }
                last_print = Some(std::time::Instant::now());
            }
        }
    })?;
    printed?;
    match state {
        InstallState::Done => {
            writeln!(
                out,
                "  Battle.net is installed and open.\n\n\
                 Log in, then use \"Locate the game\" for games already in the \
                 library (G:)."
            )?;
            Ok(())
        }
        InstallState::Stalled => {
            bail_left_open("the installer made no progress for 5 minutes and seems stuck")
        }
        InstallState::ExitedWithoutBattlenet => {
            bail_left_open("the installer closed but Battle.net did not open")
        }
        InstallState::TimedOut => bail_left_open("the installation took more than 30 minutes"),
        InstallState::Installing { .. } => Ok(()),
    }
}

/// What `house open` and `play` will do, depending on the house's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenAction {
    /// Battle.net is closed: start it.
    Start,
    /// Battle.net is open (maybe only in the tray): pass the request to it.
    PassToRunning,
}

fn open_action(installed: bool, running: bool) -> Result<OpenAction> {
    ensure!(
        installed,
        "Battle.net is not installed in the house yet; use \
         `snowctl house install-battlenet --installer PATH`"
    );
    Ok(if running {
        OpenAction::PassToRunning
    } else {
        OpenAction::Start
    })
}

/// How long to wait for Battle.net to show up after starting it.
const START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// The house's record, its canonical prefix and a runner for it.
fn house_runner() -> Result<(executor::HouseRecordView, UmuRunner)> {
    executor::open_house().map_err(|error| match error {
        executor::OpenHouseError::NoHouse => {
            anyhow::anyhow!("there is no house yet; create it first with `snowctl house create`")
        }
        other => other.into(),
    })
}

/// Runs Battle.net with `args` and, if it had to start, waits until its
/// process shows up.
fn run_and_report(
    runner: &UmuRunner,
    args: &[&str],
    what: &str,
    dry_run: bool,
    out: &mut impl io::Write,
) -> Result<()> {
    let installed = runner.prefix.join(BATTLENET_EXE).is_file();
    let running = executor::battlenet_pid(&runner.prefix)?.is_some();
    let action = open_action(installed, running)?;
    let how = match action {
        OpenAction::Start => "start Battle.net",
        OpenAction::PassToRunning => "Battle.net is already open: pass it the request",
    };
    let mode = if dry_run {
        " (dry run: nothing is changed)"
    } else {
        ""
    };
    writeln!(out, "{what}{mode}")?;
    writeln!(out, "  {how}")?;
    if dry_run {
        return Ok(());
    }
    match executor::run_battlenet(runner, args)? {
        executor::BattlenetRun::PassedToRunning => {
            writeln!(out, "Done.")?;
        }
        executor::BattlenetRun::Started => {
            let started = std::time::Instant::now();
            while started.elapsed() < START_TIMEOUT {
                std::thread::sleep(WATCH_EVERY);
                if let Some(pid) = executor::battlenet_pid(&runner.prefix)? {
                    writeln!(out, "Battle.net is starting (pid {pid}).")?;
                    return Ok(());
                }
            }
            anyhow::bail!(
                "Battle.net did not show up within {} s (Wine log: {})",
                START_TIMEOUT.as_secs(),
                runner.log.display()
            );
        }
    }
    Ok(())
}

/// `snowctl house runtime list`: the Proton builds found, the house's
/// marked with `*` and the default with "(default)".
pub fn runtime_list(out: &mut impl io::Write) -> Result<()> {
    let runtimes = find_runtimes(&runtime_dirs());
    if runtimes.is_empty() {
        writeln!(
            out,
            "No Proton found; install GE-Proton with ProtonPlus or ProtonUp-Qt."
        )?;
        return Ok(());
    }
    let current = record_path()
        .and_then(|path| load_record(&path).ok().flatten())
        .and_then(|record| fs::canonicalize(record.runtime_path).ok());
    let default = default_runtime(&runtimes).map(|runtime| runtime.path.clone());
    for runtime in &runtimes {
        let mark = if current.as_ref() == Some(&runtime.path) {
            '*'
        } else {
            ' '
        };
        let mut line = format!("{mark} {}", runtime.name);
        if runtime.label != runtime.name {
            line.push_str(&format!("  ({})", runtime.label));
        }
        if default.as_ref() == Some(&runtime.path) {
            line.push_str("  (default)");
        }
        writeln!(out, "{line}")?;
    }
    Ok(())
}

/// `snowctl house runtime set`: switches the house to another Proton,
/// with the house closed, backing the record up first.
pub fn runtime_set(name: &str, dry_run: bool, out: &mut impl io::Write) -> Result<()> {
    let runtimes = find_runtimes(&runtime_dirs());
    let chosen = choose_runtime(&runtimes, name).map_err(anyhow::Error::msg)?;
    let record_file = record_path().context("could not determine the state directory")?;
    let record = load_record(&record_file)?
        .context("there is no house yet; create it first with `snowctl house create`")?;
    let current_path =
        fs::canonicalize(&record.runtime_path).unwrap_or(record.runtime_path.clone());
    if current_path == chosen.path {
        writeln!(out, "The house already uses {}.", chosen.name)?;
        return Ok(());
    }
    let mode = if dry_run {
        " (dry run: nothing is changed)"
    } else {
        ""
    };
    writeln!(
        out,
        "Switch the house from {} to {}{mode}",
        record.runtime_name, chosen.name
    )?;
    let current_version = runtimes
        .iter()
        .find(|runtime| runtime.path == current_path)
        .and_then(|runtime| runtime.version);
    if let (Some(from), Some(to)) = (current_version, chosen.version) {
        if to < from {
            writeln!(
                out,
                "  warning: {} is older than the current one; Proton may not like a \
                 prefix made by a newer version",
                chosen.label
            )?;
        }
    }
    let running = executor::house_in_use(&record.prefix)?;
    if !running.is_empty() {
        anyhow::bail!(
            "the house is in use ({}); close Battle.net (`snowctl house quit`) and wait for it to stop",
            running.join(", ")
        );
    }
    // Next to `prefixes/`, on the prefix's disk, so the copy can reflink.
    let prefix_backups = prefix_backup_dir(&record.prefix)?;
    if dry_run {
        writeln!(out, "  back up {}", record_file.display())?;
        writeln!(
            out,
            "  copy the prefix to {}/prefix-<time> (reflink when the disk allows)",
            prefix_backups.display()
        )?;
        writeln!(
            out,
            "  record {} as the house's Proton",
            chosen.path.display()
        )?;
        return Ok(());
    }
    let backups = record_file
        .parent()
        .map(|dir| dir.join("backups"))
        .context("could not determine the state directory")?;
    let switch = executor::set_runtime(&record_file, chosen, &backups, &prefix_backups)?;
    writeln!(out, "Done.")?;
    writeln!(
        out,
        "  backup of the old record: {}",
        switch.record_backup.display()
    )?;
    writeln!(
        out,
        "  backup of the prefix:     {}",
        switch.prefix_backup.display()
    )?;
    writeln!(
        out,
        "To go back: snowctl house runtime set \"{}\"",
        record.runtime_name
    )?;
    Ok(())
}

/// Where prefix backups live (see [`executor::prefix_backup_dir`]).
fn prefix_backup_dir(prefix: &Path) -> Result<PathBuf> {
    executor::prefix_backup_dir(prefix).context("the house's prefix has no parent folder")
}

/// `snowctl house restore-prefix`: lists the prefix backups, or puts one
/// back in place of the house's prefix, keeping the current one aside.
pub fn restore_prefix(
    backup: Option<&Path>,
    dry_run: bool,
    out: &mut impl io::Write,
) -> Result<()> {
    let record_file = record_path().context("could not determine the state directory")?;
    let record = load_record(&record_file)?
        .context("there is no house yet; create it first with `snowctl house create`")?;
    let dir = prefix_backup_dir(&record.prefix)?;
    let Some(backup) = backup else {
        let backups = executor::prefix_backups(&dir);
        if backups.is_empty() {
            writeln!(out, "No prefix backups in {}.", dir.display())?;
            return Ok(());
        }
        writeln!(out, "Prefix backups, newest first ({}):", dir.display())?;
        for path in &backups {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            writeln!(out, "  {name}")?;
        }
        writeln!(out, "Restore one with: snowctl house restore-prefix <name>")?;
        return Ok(());
    };
    // A bare name is looked up in the backups folder.
    let backup = if backup.components().count() == 1 {
        dir.join(backup)
    } else {
        backup.to_owned()
    };
    ensure!(
        backup.is_dir(),
        "no backup at {} (list them with `snowctl house restore-prefix`)",
        backup.display()
    );
    let mode = if dry_run {
        " (dry run: nothing is changed)"
    } else {
        ""
    };
    writeln!(
        out,
        "Restore the house's prefix from {}{mode}",
        backup.display()
    )?;
    if dry_run {
        let running = executor::house_in_use(&record.prefix)?;
        if !running.is_empty() {
            writeln!(
                out,
                "  refused now: the house is in use ({})",
                running.join(", ")
            )?;
        }
        writeln!(
            out,
            "  move the current prefix aside to {}/prefix-replaced-<time>",
            dir.display()
        )?;
        writeln!(
            out,
            "  copy the backup in its place (reflink when the disk allows)"
        )?;
        return Ok(());
    }
    let aside = executor::restore_prefix(&record.prefix, &backup, &dir)?;
    writeln!(
        out,
        "Done. The prefix that was there is kept at {}",
        aside.display()
    )?;
    writeln!(
        out,
        "The house still uses {}; switch with `snowctl house runtime set` if this backup was made under another Proton.",
        record.runtime_name
    )?;
    Ok(())
}

/// What `snowctl house battlenet-icon` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum IconAction {
    /// Hide it (the house's programs put no icon in the tray).
    Hide,
    /// Show it again (back to the default).
    Show,
    /// Say whether it is hidden.
    Status,
}

/// `snowctl house battlenet-icon`: hides Battle.net's blurry tray icon
/// with a Windows policy that Wine honours (E14), or shows it again.
pub fn battlenet_icon(action: IconAction, dry_run: bool, out: &mut impl io::Write) -> Result<()> {
    use snowlauncher_core::house::tray_policy_args;
    let (_, runner) = house_runner()?;
    let hidden = executor::battlenet_icon_hidden(&runner)?;
    let hide = match action {
        IconAction::Status => {
            let state = if hidden { "hidden" } else { "shown" };
            writeln!(out, "Battle.net's tray icon: {state}")?;
            return Ok(());
        }
        IconAction::Hide => true,
        IconAction::Show => false,
    };
    if hidden == hide {
        let state = if hidden { "hidden" } else { "shown" };
        writeln!(out, "Battle.net's tray icon is already {state}.")?;
        return Ok(());
    }
    let what = if hide { "Hide" } else { "Show" };
    let mode = if dry_run {
        " (dry run: nothing is changed)"
    } else {
        ""
    };
    writeln!(out, "{what} Battle.net's tray icon{mode}")?;
    if dry_run {
        writeln!(out, "  back up the policy key (reg export)")?;
        writeln!(out, "  {}", tray_policy_args(hide).join(" "))?;
        writeln!(out, "  check it (reg query)")?;
        return Ok(());
    }
    let backups = record_path()
        .and_then(|path| path.parent().map(|dir| dir.join("backups")))
        .context("could not determine the state directory")?;
    let change = executor::set_battlenet_icon(&runner, hide, &backups)?;
    match &change.backup {
        Some(path) => writeln!(out, "Backup of the policy key: {}", path.display())?,
        None => writeln!(out, "No backup needed: the policy key did not exist yet.")?,
    }
    writeln!(
        out,
        "Done. It takes effect the next time Battle.net starts with the house closed."
    )?;
    if hide {
        writeln!(
            out,
            "To bring Battle.net's window back, use Snow's tray menu or `snowctl house open`."
        )?;
    }
    Ok(())
}

/// `snowctl house quit`: closes Battle.net the clean way (E10), never
/// while a game runs in the house.
pub fn quit(dry_run: bool, force: bool, out: &mut impl io::Write) -> Result<()> {
    if force {
        return force_quit(dry_run, out);
    }
    use snowlauncher_core::process::QuitDecision;
    let (house, runner) = house_runner()?;
    let mode = if dry_run {
        " (dry run: nothing is changed)"
    } else {
        ""
    };
    writeln!(out, "Close Battle.net in the house{mode}")?;
    let outcome = if dry_run {
        match executor::quit_check(&house.prefix)? {
            QuitDecision::NotRunning => executor::QuitOutcome::NotRunning,
            QuitDecision::ProgramsRunning(names) => executor::QuitOutcome::ProgramsRunning(names),
            QuitDecision::Quit => {
                writeln!(out, "  ask Battle.net to close (taskkill without /f)")?;
                writeln!(
                    out,
                    "  wait up to {} s for it to close",
                    executor::QUIT_TIMEOUT.as_secs()
                )?;
                return Ok(());
            }
        }
    } else {
        executor::quit_battlenet(&runner)?
    };
    match outcome {
        executor::QuitOutcome::NotRunning => writeln!(out, "Battle.net is not running.")?,
        executor::QuitOutcome::Closed => writeln!(out, "Battle.net closed.")?,
        executor::QuitOutcome::ProgramsRunning(names) => anyhow::bail!(
            "not closing Battle.net while {} runs in the house; close it first",
            names.join(", ")
        ),
        executor::QuitOutcome::StillOpen => anyhow::bail!(
            "Battle.net was asked to close but is still open after {} s; if it is stuck, \
             `snowctl house quit --force` closes the house step by step",
            executor::QUIT_TIMEOUT.as_secs()
        ),
    }
    Ok(())
}

/// `snowctl house quit --force`: empties a stuck house step by step (O15),
/// never while a game runs there.
fn force_quit(dry_run: bool, out: &mut impl io::Write) -> Result<()> {
    use snowlauncher_core::shutdown::{FORCE_STEPS, ForceOutcome, ForceStep};
    let (house, runner) = house_runner()?;
    let mode = if dry_run {
        " (dry run: nothing is changed)"
    } else {
        ""
    };
    writeln!(out, "Close everything in the house, step by step{mode}")?;
    if dry_run {
        let running = executor::house_in_use(&house.prefix)?;
        if running.is_empty() {
            writeln!(out, "  nothing runs in the house")?;
            return Ok(());
        }
        writeln!(out, "  running: {}", running.join(", "))?;
        for (step, wait) in FORCE_STEPS {
            let what = match step {
                ForceStep::Request => "ask Battle.net to close (taskkill without /f)",
                ForceStep::Terminate => "SIGTERM through wineserver -k15",
                ForceStep::Kill => "SIGKILL through wineserver -k",
            };
            writeln!(out, "  {what}, then wait up to {} s", wait.as_secs())?;
        }
        writeln!(out, "  (stops before any step if a game runs in the house)")?;
        return Ok(());
    }
    match executor::force_quit_house(&runner)? {
        ForceOutcome::AlreadyEmpty => writeln!(out, "Nothing runs in the house.")?,
        ForceOutcome::Emptied(step) => {
            let how = match step {
                ForceStep::Request => "after the normal close request",
                ForceStep::Terminate => "after SIGTERM",
                ForceStep::Kill => "after SIGKILL",
            };
            writeln!(out, "The house is empty ({how}).")?;
        }
        ForceOutcome::ProgramsRunning(names) => anyhow::bail!(
            "stopped: {} runs in the house; close it first",
            names.join(", ")
        ),
        ForceOutcome::StillRunning(names) => {
            anyhow::bail!("still running after SIGKILL: {}", names.join(", "))
        }
    }
    Ok(())
}

/// `snowctl house open`: opens Battle.net in the house, with the house's
/// environment (the Qt fix, E11), or brings its window back.
pub fn open(dry_run: bool, out: &mut impl io::Write) -> Result<()> {
    let (_, runner) = house_runner()?;
    run_and_report(&runner, &[], "Open Battle.net in the house", dry_run, out)
}

/// `snowctl games`: the games in the house's library and their launch
/// codes.
pub fn games(out: &mut impl io::Write) -> Result<()> {
    let (house, _) = house_runner()?;
    let codes = known_product_codes(&house.prefix);
    let games = installed_games(&house.library, &codes);
    if games.is_empty() {
        writeln!(
            out,
            "No games in the library ({}).",
            house.library.display()
        )?;
        return Ok(());
    }
    for game in &games {
        let version = game.build.version.as_deref().unwrap_or("?");
        let code = game
            .launch_code
            .as_deref()
            .unwrap_or("unknown yet (open Battle.net once, logged in, so it shows up in its log)");
        writeln!(out, "{}", game.name)?;
        writeln!(
            out,
            "  product: {}   version: {version}",
            game.build.product
        )?;
        writeln!(out, "  launch code: {code}")?;
    }
    Ok(())
}

/// `snowctl play <game>`: opens a game through Battle.net. `<game>` is
/// the folder name or the product uid, ignoring case.
pub fn play(game: &str, dry_run: bool, out: &mut impl io::Write) -> Result<()> {
    let (house, runner) = house_runner()?;
    let games = installed_games(&house.library, &known_product_codes(&house.prefix));
    let found = games
        .iter()
        .find(|g| g.name.eq_ignore_ascii_case(game) || g.build.product.eq_ignore_ascii_case(game))
        .with_context(|| {
            let names: Vec<&str> = games.iter().map(|g| g.name.as_str()).collect();
            format!(
                "no game called {game:?} in the library (found: {})",
                names.join(", ")
            )
        })?;
    let code = found.launch_code.as_deref().with_context(|| {
        format!(
            "the launch code of {} is not known yet: open Battle.net once, logged in \
             (`snowctl house open`), then try again",
            found.name
        )
    })?;
    let argument = launch_argument(code);
    let mode = if dry_run {
        " (dry run: nothing is changed)"
    } else {
        ""
    };
    writeln!(out, "Play {} ({argument}){mode}", found.name)?;
    if dry_run {
        if executor::battlenet_pid(&runner.prefix)?.is_none() {
            writeln!(out, "  start Battle.net")?;
        }
        writeln!(
            out,
            "  wait until Battle.net is logged in (up to {} s)",
            executor::LOGIN_TIMEOUT.as_secs()
        )?;
        writeln!(out, "  ask it for the game")?;
        return Ok(());
    }
    match executor::play_game(&runner, code)? {
        executor::PlayRun::Requested => writeln!(out, "Asked Battle.net for the game.")?,
        executor::PlayRun::RequestedAfterLogin => {
            writeln!(out, "Battle.net is logged in; asked it for the game.")?
        }
    }
    Ok(())
}

/// Stops with an explanation, leaving whatever is open for the user to see.
fn bail_left_open(what: &str) -> Result<()> {
    let log = create_log_path()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    anyhow::bail!(
        "{what}. Nothing was closed: check the installer window, \
         close it if needed and run this command again (Wine log: {log})."
    )
}

/// `78 s` → `1:18`.
fn clock(elapsed: std::time::Duration) -> String {
    let secs = elapsed.as_secs();
    format!("{}:{:02}", secs / 60, secs % 60)
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
            label: name.clone(),
            name,
            path,
            version: None,
        });
    }
    let runtimes = find_runtimes(&runtime_dirs());
    default_runtime(&runtimes).cloned().context(
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
        assert!(out.contains("  4. hide Battle.net's tray icon"));
        assert!(out.contains("  6. map G: to /home/u/Games/SnowLauncher"));
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
    fn open_starts_or_brings_back() {
        assert_eq!(open_action(true, false).unwrap(), OpenAction::Start);
        assert_eq!(open_action(true, true).unwrap(), OpenAction::PassToRunning);
        let error = open_action(false, false).unwrap_err().to_string();
        assert!(error.contains("install-battlenet"), "{error}");
    }

    #[test]
    fn clock_is_minutes_and_seconds() {
        assert_eq!(clock(std::time::Duration::from_secs(78)), "1:18");
        assert_eq!(clock(std::time::Duration::from_secs(5)), "0:05");
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
