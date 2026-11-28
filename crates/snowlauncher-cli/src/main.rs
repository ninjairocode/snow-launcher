//! `snowctl`: command-line tool for Snow Launcher.
//!
//! For now it calls the libraries directly. Once `snowd` exposes its D-Bus
//! interface, commands that depend on daemon state will go through it.

mod daemon;
mod doctor;
mod house;
mod installer;
mod service;

use std::fmt::Write as _;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use directories::BaseDirs;
use snowlauncher_core::battlenet_log::error_sightings;
use snowlauncher_core::config::Config;
use snowlauncher_core::process::{ProcessInfo, ProcessRole, ProcessTable, processes_in_prefix};
use snowlauncher_wine::battlenet_logs::{client_logs, session_start};
use snowlauncher_wine::config_file::{choose_prefix, default_config_path, read_config};
use snowlauncher_wine::instance::{default_lock_path, running_instance};
use snowlauncher_wine::prefix::WinePrefix;
use snowlauncher_wine::procfs::ProcfsProcessTable;

#[derive(Debug, Parser)]
#[command(
    name = "snowctl",
    version,
    about = "Command-line tool for Snow Launcher"
)]
struct Cli {
    /// Configuration file. Defaults to ~/.config/snow-launcher/config.toml.
    #[arg(long, value_name = "PATH", global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show a Wine prefix and the processes running in it (read-only).
    Status {
        /// Prefix to inspect. Defaults to `prefix` in the config file, then
        /// to Snow Launcher's own prefix.
        #[arg(long, value_name = "PATH")]
        prefix: Option<PathBuf>,
    },

    /// Look for known problems in a prefix (read-only).
    Doctor {
        /// Prefix to check. Defaults to `prefix` in the config file, then
        /// to Snow Launcher's own prefix.
        #[arg(long, value_name = "PATH")]
        prefix: Option<PathBuf>,

        /// How many recent Battle.net sessions to check.
        #[arg(long, value_name = "N", default_value_t = 5)]
        sessions: usize,
    },

    /// Check that a Battle.net installer is Blizzard's, unmodified.
    CheckInstaller {
        /// The installer, e.g. ~/Downloads/Battle.net-Setup.exe.
        #[arg(value_name = "PATH")]
        path: PathBuf,
    },

    /// List the games in the house's library and their launch codes.
    Games,

    /// Open a game through Battle.net in the house.
    Play {
        /// Folder name (`"World of Warcraft"`) or product uid (`wowf`).
        game: String,

        /// Only show what would be done.
        #[arg(long)]
        dry_run: bool,
    },

    /// Snow Launcher's own Battle.net prefix (the "house").
    House {
        #[command(subcommand)]
        action: HouseAction,
    },

    /// Run snowd as a systemd user service, started with the desktop.
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
}

#[derive(Debug, Subcommand)]
enum InstallerAction {
    /// Show the kept copy and the installers in Downloads, checked.
    Status,
    /// Open Blizzard's download page in the browser (Snow itself
    /// downloads nothing).
    Download,
}

#[derive(Debug, Subcommand)]
enum RuntimeAction {
    /// List the Proton builds found (GE-Proton first), marking the house's.
    List,
    /// Switch the house to another Proton. Only with the house closed.
    Set {
        /// Folder name (`GE-Proton11-14-x86_64`, `Proton-GE Latest`) or
        /// version (`UMU-Proton-10.0-4`).
        name: String,

        /// Only show what would be done.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
enum HouseAction {
    /// Create the house: prefix, Windows 11, UI scale and the G: library.
    Create {
        /// Only show what would be done.
        #[arg(long)]
        dry_run: bool,

        /// Prefix to create. Defaults to Snow Launcher's own prefix.
        #[arg(long, value_name = "PATH")]
        prefix: Option<PathBuf>,

        /// Game library, seen as G:. Defaults to [house] library, then
        /// ~/Games/SnowLauncher.
        #[arg(long, value_name = "PATH")]
        library: Option<PathBuf>,

        /// Proton build folder. Defaults to [house] runtime, then the
        /// newest GE-Proton found.
        #[arg(long, value_name = "PATH")]
        runtime: Option<PathBuf>,
    },
    /// Install Battle.net in the house from Blizzard's installer, verified
    /// first, and watch the installation until Battle.net opens.
    InstallBattlenet {
        /// The official installer, e.g. ~/Downloads/Battle.net-Setup.exe.
        /// Without it: the newest one in Downloads, then the last verified
        /// copy Snow kept. Every one is checked by Blizzard's signature.
        #[arg(long, value_name = "PATH")]
        installer: Option<PathBuf>,

        /// Only check and show what would be done.
        #[arg(long)]
        dry_run: bool,
    },
    /// Open Battle.net in the house (with the house's environment), or
    /// bring its window back if it is already open, even from the tray.
    Open {
        /// Only show what would be done.
        #[arg(long)]
        dry_run: bool,
    },
    /// Close Battle.net in the house the clean way. Refuses while a game
    /// runs there.
    Quit {
        /// Only show what would be done.
        #[arg(long)]
        dry_run: bool,

        /// For a stuck house: close request, then SIGTERM, then SIGKILL
        /// (through the house's wineserver), until it is empty. Still never
        /// while a game runs.
        #[arg(long)]
        force: bool,
    },
    /// Hide or show Battle.net's icon in the desktop tray (Snow's own item
    /// replaces it), or show whether it is hidden.
    BattlenetIcon {
        #[arg(value_enum)]
        action: house::IconAction,

        /// Only show what would be done.
        #[arg(long)]
        dry_run: bool,
    },
    /// List the Proton builds Snow can use, or switch the house to one.
    Runtime {
        #[command(subcommand)]
        action: RuntimeAction,
    },
    /// List the prefix backups, or put one back in place of the house's
    /// prefix (the current one is kept aside). Only with the house closed.
    RestorePrefix {
        /// Backup name (`prefix-1795833065`) or path. Without it, list them.
        backup: Option<PathBuf>,

        /// Only show what would be done.
        #[arg(long)]
        dry_run: bool,
    },
    /// The Battle.net installer: what Snow has, or open the official
    /// download page in the browser.
    Installer {
        #[command(subcommand)]
        action: InstallerAction,
    },
    /// Remove the house. Never deletes games.
    Remove {
        /// Only show what would be done.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ServiceAction {
    /// Install, enable and start the service.
    Install {
        /// Only show what would be done.
        #[arg(long)]
        dry_run: bool,

        /// snowd binary to run. Defaults to the one next to snowctl.
        #[arg(long, value_name = "PATH")]
        snowd: Option<PathBuf>,
    },
    /// Stop, disable and remove the service.
    Uninstall {
        /// Only show what would be done.
        #[arg(long)]
        dry_run: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = load_config(cli.config);
    match cli.command {
        Command::Status { prefix } => status(prefix, &config),
        Command::Doctor { prefix, sessions } => doctor_command(prefix, sessions, &config),
        Command::Games => house::games(&mut io::stdout().lock()),
        Command::Play { game, dry_run } => house::play(&game, dry_run, &mut io::stdout().lock()),
        Command::CheckInstaller { path } => {
            let report = installer::check(&path)?;
            io::stdout()
                .lock()
                .write_all(report.as_bytes())
                .context("failed to write to stdout")
        }
        Command::House {
            action:
                HouseAction::Create {
                    dry_run,
                    prefix,
                    library,
                    runtime,
                },
        } => {
            let options = house::CreateOptions {
                dry_run,
                prefix,
                library,
                runtime,
            };
            house::create(options, &config, &mut io::stdout().lock())
        }
        Command::House {
            action: HouseAction::InstallBattlenet { installer, dry_run },
        } => house::install_battlenet(installer.as_deref(), dry_run, &mut io::stdout().lock()),
        Command::House {
            action: HouseAction::Open { dry_run },
        } => house::open(dry_run, &mut io::stdout().lock()),
        Command::House {
            action: HouseAction::BattlenetIcon { action, dry_run },
        } => house::battlenet_icon(action, dry_run, &mut io::stdout().lock()),
        Command::House {
            action:
                HouseAction::Runtime {
                    action: RuntimeAction::List,
                },
        } => house::runtime_list(&mut io::stdout().lock()),
        Command::House {
            action:
                HouseAction::Runtime {
                    action: RuntimeAction::Set { name, dry_run },
                },
        } => house::runtime_set(&name, dry_run, &mut io::stdout().lock()),
        Command::House {
            action: HouseAction::RestorePrefix { backup, dry_run },
        } => house::restore_prefix(backup.as_deref(), dry_run, &mut io::stdout().lock()),
        Command::House {
            action:
                HouseAction::Installer {
                    action: InstallerAction::Status,
                },
        } => installer::status(&mut io::stdout().lock()),
        Command::House {
            action:
                HouseAction::Installer {
                    action: InstallerAction::Download,
                },
        } => installer::open_download_page(&mut io::stdout().lock()),
        Command::House {
            action: HouseAction::Quit { dry_run, force },
        } => house::quit(dry_run, force, &mut io::stdout().lock()),
        Command::House {
            action: HouseAction::Remove { dry_run },
        } => house::remove(dry_run, &mut io::stdout().lock()),
        Command::Service { action } => service_command(action),
    }
}

fn doctor_command(prefix: Option<PathBuf>, sessions: usize, config: &Config) -> Result<()> {
    use service::Systemd as _;

    let (path, _source) = choose_prefix(prefix, config)?;
    let prefix = WinePrefix::open(&path).context("cannot open the prefix")?;
    let service_active = service::Systemctl.is_active(service::UNIT_NAME).ok();

    let logs: Vec<PathBuf> = client_logs(&prefix)
        .context("could not list the Battle.net logs")?
        .into_iter()
        .take(sessions)
        .collect();
    let mut per_session = Vec::new();
    for log in &logs {
        let bytes =
            std::fs::read(log).with_context(|| format!("could not read {}", log.display()))?;
        per_session.push(error_sightings(&String::from_utf8_lossy(&bytes)));
    }
    // `logs` is newest first.
    let period = match (logs.last(), logs.first()) {
        (Some(oldest), Some(newest)) => session_start(oldest).zip(session_start(newest)),
        _ => None,
    };

    let report = doctor::Report {
        prefix: prefix.path().to_owned(),
        service_active,
        sessions_checked: logs.len(),
        period,
        errors: doctor::summarize(&per_session),
    };
    io::stdout()
        .lock()
        .write_all(doctor::render(&report).as_bytes())
        .context("failed to write to stdout")
}

fn service_command(action: ServiceAction) -> Result<()> {
    let unit_path = BaseDirs::new()
        .context("could not determine the config directory")?
        .config_dir()
        .join("systemd")
        .join("user")
        .join(service::UNIT_NAME);
    let mut systemd = service::Systemctl;
    let mut out = io::stdout().lock();

    match action {
        ServiceAction::Install { dry_run, snowd } => {
            let snowd = match snowd {
                Some(path) => path,
                None => service::sibling_snowd()?,
            };
            service::check_snowd_exists(&snowd)?;
            if snowd.components().any(|part| part.as_os_str() == "target") {
                warn(
                    "snowd is inside a cargo `target` directory: `cargo clean` would \
                     break the service. Consider `cargo install --path \
                     crates/snowlauncher-daemon` (and the same for snowlauncher-cli).",
                );
            }
            refuse_if_started_by_hand(&mut systemd)?;

            let content = service::render_unit(&snowd)?;
            let existing = service::read_existing(&unit_path)?;
            let steps = service::plan_install(&unit_path, &content, existing.as_deref());
            run_plan(&mut out, &steps, &content, dry_run, &mut systemd)?;
            if !dry_run {
                writeln!(
                    out,
                    "snowd is running as a service. Logs: journalctl --user -u snowd -f"
                )?;
            }
        }
        ServiceAction::Uninstall { dry_run } => {
            let steps = service::plan_uninstall(&unit_path, unit_path.exists());
            if steps.is_empty() {
                writeln!(out, "The service is not installed; nothing to do.")?;
                return Ok(());
            }
            run_plan(&mut out, &steps, "", dry_run, &mut systemd)?;
        }
    }
    Ok(())
}

/// A snowd started by hand holds the single-instance lock: the service
/// would fail to start and systemd would give up after 3 tries.
fn refuse_if_started_by_hand(systemd: &mut impl service::Systemd) -> Result<()> {
    let Some(lock) = default_lock_path() else {
        return Ok(());
    };
    let Some(running) = running_instance(&lock)? else {
        return Ok(());
    };
    if systemd.is_active(service::UNIT_NAME)? {
        // It is the service itself: reinstalling is fine.
        return Ok(());
    }
    let pid = running
        .pid
        .map(|pid| format!(" (pid {pid})"))
        .unwrap_or_default();
    bail!(
        "a snowd is already running{pid}, probably started by hand. \
         Stop it (Ctrl+C in its terminal) and run this again."
    );
}

fn run_plan(
    out: &mut impl io::Write,
    steps: &[service::Step],
    unit_content: &str,
    dry_run: bool,
    systemd: &mut impl service::Systemd,
) -> Result<()> {
    for step in steps {
        if dry_run {
            writeln!(out, "would {step}")?;
        } else {
            writeln!(out, "- {step}")?;
        }
    }
    if dry_run {
        writeln!(out, "(dry run: nothing was changed)")?;
        return Ok(());
    }
    out.flush()?;
    service::execute(steps, unit_content, systemd)
}

/// Like `snowd`: problems with the config file are warnings, and the
/// defaults are used.
fn load_config(path: Option<PathBuf>) -> Config {
    let Some(path) = path.or_else(default_config_path) else {
        return Config::default();
    };
    match read_config(&path) {
        Ok(Some(loaded)) => {
            for warning in &loaded.warnings {
                warn(&format!("{}: {warning}", path.display()));
            }
            loaded.config
        }
        Ok(None) => Config::default(),
        Err(error) => {
            warn(&format!("{:#}; using defaults", anyhow::Error::from(error)));
            Config::default()
        }
    }
}

fn warn(message: &str) {
    // Nothing useful to do if stderr itself is gone.
    let _ = writeln!(io::stderr(), "warning: {message}");
}

fn status(prefix: Option<PathBuf>, config: &Config) -> Result<()> {
    let (path, _source) = choose_prefix(prefix, config)?;
    let prefix = WinePrefix::open(&path).context("cannot open the prefix")?;
    let processes = ProcfsProcessTable::new()
        .processes()
        .context("failed to list processes")?;
    let ours: Vec<&ProcessInfo> = processes_in_prefix(&processes, prefix.path()).collect();

    let mut text = render_status(prefix.path(), &ours);
    text.push_str(&daemon::render(&daemon::query(), prefix.path()));

    // `println!` would panic on a closed pipe (`snowctl status | head`);
    // writing explicitly turns that into a normal error.
    io::stdout()
        .lock()
        .write_all(text.as_bytes())
        .context("failed to write to stdout")
}

fn render_status(prefix: &Path, processes: &[&ProcessInfo]) -> String {
    let running = |role: ProcessRole| -> String {
        let pids: Vec<String> = processes
            .iter()
            .filter(|p| ProcessRole::of(p) == role)
            .map(|p| p.pid.0.to_string())
            .collect();
        if pids.is_empty() {
            "not running".to_owned()
        } else {
            format!("running (pid {})", pids.join(", "))
        }
    };

    let mut out = String::new();
    // Writing to a String cannot fail, so the results are ignored.
    let _ = writeln!(out, "Prefix:     {}", prefix.display());
    let _ = writeln!(out, "Battle.net: {}", running(ProcessRole::BattleNet));
    let _ = writeln!(out, "Agent:      {}", running(ProcessRole::Agent));
    let _ = writeln!(out, "Processes:  {}", processes.len());
    for process in processes {
        let role = match ProcessRole::of(process) {
            ProcessRole::BattleNet => "Battle.net",
            ProcessRole::BattleNetHelper => "Battle.net (helper)",
            ProcessRole::Agent => "Agent",
            ProcessRole::WineService => "Wine",
            ProcessRole::Launcher => "launcher",
            ProcessRole::Game => "game",
            ProcessRole::Other => "other",
        };
        let _ = writeln!(
            out,
            "  {:>7}  {:<22} {role}",
            process.pid.0,
            process.display_name()
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use snowlauncher_core::process::Pid;

    fn process(pid: u32, comm: &str, cmdline: &[&str]) -> ProcessInfo {
        ProcessInfo {
            pid: Pid(pid),
            name: comm.to_owned(),
            cmdline: cmdline.iter().map(|arg| (*arg).to_owned()).collect(),
            wineprefix: None,
        }
    }

    #[test]
    fn cli_definition_is_valid() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn renders_running_battlenet() {
        let exe = r"C:\Program Files (x86)\Battle.net\Battle.net.exe";
        let server = process(100, "wineserver", &[]);
        let bnet = process(200, "CrBrowserMain", &[exe]);
        let gpu = process(201, "CrGpuMain", &[exe, "--type=gpu-process"]);
        let out = render_status(Path::new("/pfx"), &[&server, &bnet, &gpu]);
        assert_eq!(
            out,
            "Prefix:     /pfx\n\
             Battle.net: running (pid 200)\n\
             Agent:      not running\n\
             Processes:  3\n\
             \x20     100  wineserver             Wine\n\
             \x20     200  Battle.net.exe         Battle.net\n\
             \x20     201  Battle.net.exe         Battle.net (helper)\n"
        );
    }

    #[test]
    fn renders_idle_prefix() {
        let out = render_status(Path::new("/pfx"), &[]);
        assert!(out.contains("Battle.net: not running"));
        assert!(out.contains("Processes:  0"));
    }
}
