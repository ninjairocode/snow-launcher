//! The `snowd` user service, through `systemctl --user` (separate
//! arguments, never a shell string). Installing and removing the unit is
//! `snowctl service`; this only looks at it and turns it on or off.

use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use directories::BaseDirs;

/// How this copy of Snow was installed (see `snowlauncher_core::packaging`),
/// from `$APPIMAGE` and the running executable.
pub fn install_kind() -> snowlauncher_core::packaging::InstallKind {
    let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
    let exe = std::env::current_exe().unwrap_or_default();
    snowlauncher_core::packaging::detect(appimage.as_deref(), &exe)
}

/// The unit `snowctl service install` writes.
pub const UNIT_NAME: &str = "snowd.service";

/// `~/.config/systemd/user/snowd.service`.
pub fn unit_path() -> Option<PathBuf> {
    BaseDirs::new().map(|dirs| dirs.config_dir().join("systemd/user").join(UNIT_NAME))
}

/// Whether the service is installed (its unit file exists).
pub fn is_installed() -> bool {
    unit_path().is_some_and(|path| path.is_file())
}

fn systemctl(args: &[&str]) -> io::Result<bool> {
    Ok(Command::new("systemctl")
        .arg("--user")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?
        .success())
}

/// Whether `snowd` runs as the service now.
pub fn is_active() -> io::Result<bool> {
    systemctl(&["--quiet", "is-active", UNIT_NAME])
}

/// Whether `snowd` starts at login.
pub fn starts_at_login() -> io::Result<bool> {
    systemctl(&["--quiet", "is-enabled", UNIT_NAME])
}

fn checked(args: &[&str]) -> io::Result<()> {
    if systemctl(args)? {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "`systemctl --user {}` failed",
            args.join(" ")
        )))
    }
}

/// Starts `snowd` now.
pub fn start() -> io::Result<()> {
    checked(&["start", UNIT_NAME])
}

/// Stops `snowd` now. It is not restarted: the unit only restarts it after
/// a failure.
pub fn stop() -> io::Result<()> {
    checked(&["stop", UNIT_NAME])
}

/// Turns starting at login on or off (does not start or stop it now).
pub fn set_starts_at_login(on: bool) -> io::Result<()> {
    checked(&[if on { "enable" } else { "disable" }, UNIT_NAME])
}
