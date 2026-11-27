//! `snowctl`: command-line tool for Snow Launcher.
//!
//! For now it calls the libraries directly. Once `snowd` exposes its D-Bus
//! interface, commands that depend on daemon state will go through it.

mod service;

use std::fmt::Write as _;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use directories::BaseDirs;
use snowlauncher_core::config::Config;
use snowlauncher_core::process::{ProcessInfo, ProcessRole, ProcessTable, processes_in_prefix};
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

    /// Run snowd as a systemd user service, started with the desktop.
    Service {
        #[command(subcommand)]
        action: ServiceAction,
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
        Command::Service { action } => service_command(action),
    }
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

    // `println!` would panic on a closed pipe (`snowctl status | head`);
    // writing explicitly turns that into a normal error.
    io::stdout()
        .lock()
        .write_all(render_status(prefix.path(), &ours).as_bytes())
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
