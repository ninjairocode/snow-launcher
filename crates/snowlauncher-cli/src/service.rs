//! `snowctl service install|uninstall`: runs `snowd` as a `systemd --user`
//! service, so it starts with the desktop session.
//!
//! Each command first builds a plan (a list of [`Step`]s) with pure
//! functions. `--dry-run` only prints the plan; otherwise it is executed.
//! `uninstall` undoes what `install` did.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};

/// Unit template; `@SNOWD@` is replaced with the command that runs `snowd`.
const TEMPLATE: &str = include_str!("../../../data/systemd/snowd.service.in");
const PLACEHOLDER: &str = "@SNOWD@";
pub const UNIT_NAME: &str = "snowd.service";

/// Fills the template with the command that runs `snowd`: `program`
/// (quoted for systemd) and its `args` (simple words only, like `snowd`
/// for the AppImage, so they never need escaping).
pub fn render_unit(snowd: &Path, args: &[String]) -> Result<String> {
    ensure!(
        args.iter().all(|arg| !arg.is_empty()
            && arg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')),
        "unsupported argument for the snowd service: {args:?}"
    );
    ensure!(
        snowd.is_absolute(),
        "the snowd path must be absolute: {}",
        snowd.display()
    );
    let text = snowd
        .to_str()
        .with_context(|| format!("the snowd path is not valid UTF-8: {}", snowd.display()))?;
    // Quotes, backslashes and line breaks would need systemd escaping;
    // refuse them rather than risk a broken or misread ExecStart line.
    ensure!(
        !text.contains(['"', '\\', '\n']),
        "unsupported character in the snowd path: {text}"
    );
    // `%` starts a systemd specifier (like `%h`); `%%` is a literal `%`.
    let mut command = format!("\"{}\"", text.replace('%', "%%"));
    for arg in args {
        command.push(' ');
        command.push_str(arg);
    }
    Ok(TEMPLATE.replace(PLACEHOLDER, &command))
}

/// One action of an install or uninstall.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Backup {
        from: PathBuf,
        to: PathBuf,
    },
    WriteUnit {
        path: PathBuf,
    },
    RemoveUnit {
        path: PathBuf,
    },
    /// Arguments after `systemctl --user`.
    Systemctl(Vec<String>),
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backup { from, to } => {
                write!(f, "back up {} to {}", from.display(), to.display())
            }
            Self::WriteUnit { path } => write!(f, "write {}", path.display()),
            Self::RemoveUnit { path } => write!(f, "remove {}", path.display()),
            Self::Systemctl(args) => write!(f, "run: systemctl --user {}", args.join(" ")),
        }
    }
}

fn systemctl(args: &[&str]) -> Step {
    Step::Systemctl(args.iter().map(|arg| (*arg).to_owned()).collect())
}

fn backup_path(unit_path: &Path) -> PathBuf {
    unit_path.with_extension("service.bak")
}

/// Steps to install the unit. `existing` is the current content of the
/// unit file, if there is one: identical content is not rewritten, and
/// different content is backed up first.
pub fn plan_install(unit_path: &Path, new_content: &str, existing: Option<&str>) -> Vec<Step> {
    let mut steps = Vec::new();
    let write = existing != Some(new_content);
    if write {
        if existing.is_some() {
            steps.push(Step::Backup {
                from: unit_path.to_owned(),
                to: backup_path(unit_path),
            });
        }
        steps.push(Step::WriteUnit {
            path: unit_path.to_owned(),
        });
        steps.push(systemctl(&["daemon-reload"]));
    }
    steps.push(systemctl(&["enable", UNIT_NAME]));
    // `restart` also starts it if stopped, and picks up a changed unit.
    steps.push(systemctl(&["restart", UNIT_NAME]));
    steps
}

/// Steps to uninstall. Nothing to do if the unit file is not there.
pub fn plan_uninstall(unit_path: &Path, installed: bool) -> Vec<Step> {
    if !installed {
        return Vec::new();
    }
    vec![
        systemctl(&["disable", "--now", UNIT_NAME]),
        Step::RemoveUnit {
            path: unit_path.to_owned(),
        },
        systemctl(&["daemon-reload"]),
    ]
}

/// Access to `systemctl --user`. Faked in tests.
pub trait Systemd {
    fn run(&mut self, args: &[String]) -> Result<()>;
    fn is_active(&mut self, unit: &str) -> Result<bool>;
}

/// The real `systemctl --user`, with separate arguments (no shell).
pub struct Systemctl;

impl Systemd for Systemctl {
    fn run(&mut self, args: &[String]) -> Result<()> {
        let status = Command::new("systemctl")
            .arg("--user")
            .args(args)
            .status()
            .context("could not run systemctl")?;
        ensure!(
            status.success(),
            "`systemctl --user {}` failed ({status})",
            args.join(" ")
        );
        Ok(())
    }

    fn is_active(&mut self, unit: &str) -> Result<bool> {
        // Exit code 0 means active; anything else means not active.
        let status = Command::new("systemctl")
            .args(["--user", "--quiet", "is-active", unit])
            .status()
            .context("could not run systemctl")?;
        Ok(status.success())
    }
}

/// Runs the steps in order, stopping at the first failure.
pub fn execute(steps: &[Step], unit_content: &str, systemd: &mut impl Systemd) -> Result<()> {
    for step in steps {
        match step {
            Step::Backup { from, to } => {
                fs::copy(from, to).with_context(|| format!("could not {step}"))?;
            }
            Step::WriteUnit { path } => {
                write_atomically(path, unit_content)
                    .with_context(|| format!("could not {step}"))?;
            }
            Step::RemoveUnit { path } => match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error).with_context(|| format!("could not {step}")),
            },
            Step::Systemctl(args) => systemd.run(args)?,
        }
    }
    Ok(())
}

/// Writes to a temporary file next to `path`, flushes it to disk, then
/// renames it over `path`: readers see the old file or the new one,
/// never half of it (`project.md`, section 9.2).
fn write_atomically(path: &Path, content: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("service.tmp");
    let result = (|| {
        let mut file = File::create(&tmp)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Reads the current unit file, `None` if it does not exist.
pub fn read_existing(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("could not read {}", path.display())),
    }
}

/// `snowd` next to the running `snowctl` (cargo, `cargo install` and the
/// AppImage all put them side by side).
pub fn sibling_snowd() -> Result<PathBuf> {
    let me = std::env::current_exe().context("could not find the snowctl executable")?;
    let dir = me
        .parent()
        .context("the snowctl executable has no parent directory")?;
    Ok(dir.join("snowd"))
}

/// Refuses to continue if `snowd` is missing.
pub fn check_snowd_exists(snowd: &Path) -> Result<()> {
    if !snowd.is_file() {
        bail!(
            "snowd not found at {} (build it, or pass --snowd PATH)",
            snowd.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNOWD: &str = "/home/user/.cargo/bin/snowd";

    #[derive(Default)]
    struct FakeSystemd {
        calls: Vec<String>,
    }

    impl Systemd for FakeSystemd {
        fn run(&mut self, args: &[String]) -> Result<()> {
            self.calls.push(args.join(" "));
            Ok(())
        }

        fn is_active(&mut self, _unit: &str) -> Result<bool> {
            Ok(false)
        }
    }

    #[test]
    fn an_appimage_service_runs_the_appimage_with_snowd() {
        let unit = render_unit(
            Path::new("/home/user/Apps/Snow_Launcher-x86_64.AppImage"),
            &["snowd".to_owned()],
        )
        .unwrap();
        assert!(
            unit.contains("ExecStart=\"/home/user/Apps/Snow_Launcher-x86_64.AppImage\" snowd\n"),
            "{unit}"
        );
        assert!(render_unit(Path::new(SNOWD), &["a b".to_owned()]).is_err());
        assert!(render_unit(Path::new(SNOWD), &["$(x)".to_owned()]).is_err());
    }

    #[test]
    fn template_has_restart_limits() {
        assert!(TEMPLATE.contains("Restart=on-failure"));
        assert!(TEMPLATE.contains("StartLimitIntervalSec=600"));
        assert!(TEMPLATE.contains("StartLimitBurst=3"));
        assert!(TEMPLATE.contains("ExecStart=@SNOWD@"));
    }

    #[test]
    fn renders_quoted_exec_start() {
        let unit = render_unit(Path::new(SNOWD), &[]).unwrap();
        assert!(unit.contains("\nExecStart=\"/home/user/.cargo/bin/snowd\"\n"));
        assert!(!unit.contains(PLACEHOLDER));
    }

    #[test]
    fn escapes_systemd_specifiers_and_keeps_spaces() {
        let unit = render_unit(Path::new("/home/user/My Apps/100%/snowd"), &[]).unwrap();
        assert!(unit.contains("ExecStart=\"/home/user/My Apps/100%%/snowd\""));
    }

    #[test]
    fn rejects_unsafe_paths() {
        assert!(render_unit(Path::new("snowd"), &[]).is_err());
        assert!(render_unit(Path::new("/home/user/a\"b/snowd"), &[]).is_err());
        assert!(render_unit(Path::new("/home/user/a\\b/snowd"), &[]).is_err());
    }

    fn steps_text(steps: &[Step]) -> Vec<String> {
        steps.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn fresh_install_writes_and_enables() {
        let steps = plan_install(Path::new("/u/snowd.service"), "new", None);
        assert_eq!(
            steps_text(&steps),
            [
                "write /u/snowd.service",
                "run: systemctl --user daemon-reload",
                "run: systemctl --user enable snowd.service",
                "run: systemctl --user restart snowd.service",
            ]
        );
    }

    #[test]
    fn different_existing_unit_is_backed_up_first() {
        let steps = plan_install(Path::new("/u/snowd.service"), "new", Some("old"));
        assert_eq!(
            steps[0],
            Step::Backup {
                from: PathBuf::from("/u/snowd.service"),
                to: PathBuf::from("/u/snowd.service.bak"),
            }
        );
    }

    #[test]
    fn identical_unit_is_not_rewritten() {
        let steps = plan_install(Path::new("/u/snowd.service"), "same", Some("same"));
        assert_eq!(
            steps_text(&steps),
            [
                "run: systemctl --user enable snowd.service",
                "run: systemctl --user restart snowd.service",
            ]
        );
    }

    #[test]
    fn uninstall_undoes_install() {
        let path = Path::new("/u/snowd.service");
        assert!(plan_uninstall(path, false).is_empty());
        assert_eq!(
            steps_text(&plan_uninstall(path, true)),
            [
                "run: systemctl --user disable --now snowd.service",
                "remove /u/snowd.service",
                "run: systemctl --user daemon-reload",
            ]
        );
    }

    #[test]
    fn install_then_uninstall_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let unit_path = tmp.path().join("systemd/user").join(UNIT_NAME);
        let content = render_unit(Path::new(SNOWD), &[]).unwrap();
        let mut systemd = FakeSystemd::default();

        // A different unit was there before: it must be backed up.
        fs::create_dir_all(unit_path.parent().unwrap()).unwrap();
        fs::write(&unit_path, "old unit").unwrap();

        let existing = read_existing(&unit_path).unwrap();
        let steps = plan_install(&unit_path, &content, existing.as_deref());
        execute(&steps, &content, &mut systemd).unwrap();
        assert_eq!(fs::read_to_string(&unit_path).unwrap(), content);
        assert_eq!(
            fs::read_to_string(backup_path(&unit_path)).unwrap(),
            "old unit"
        );
        assert!(!unit_path.with_extension("service.tmp").exists());

        let steps = plan_uninstall(&unit_path, unit_path.exists());
        execute(&steps, &content, &mut systemd).unwrap();
        assert!(!unit_path.exists());
        assert_eq!(
            systemd.calls,
            [
                "daemon-reload",
                "enable snowd.service",
                "restart snowd.service",
                "disable --now snowd.service",
                "daemon-reload",
            ]
        );
    }
}
