//! How Snow Launcher was installed, and what follows from it. Pure.
//!
//! | | AppImage | System package (RPM…) | Development (`cargo install`) |
//! |---|---|---|---|
//! | `snowd` service runs | the AppImage, with `snowd` | the packaged `snowd` | `snowd` next to `snowctl` |
//! | Menu entry and icon | Snow writes them | the package ships them | Snow writes them |
//! | Updates | Snow offers them | the system's package manager | the developer rebuilds |
//!
//! Inside an AppImage the programs run from a temporary mount
//! (`/tmp/.mount_…`) that disappears when they exit, so nothing outside
//! may point at them there: always at the AppImage file itself.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallKind {
    /// Running from this AppImage file (its `$APPIMAGE`).
    AppImage(PathBuf),
    /// Installed by the system (`/usr/…`), e.g. from an RPM.
    SystemPackage,
    /// Built by hand: `cargo install`, `cargo run`.
    Development,
}

/// From the `$APPIMAGE` variable (set by the AppImage runtime) and the
/// running executable's path.
pub fn detect(appimage: Option<&Path>, exe: &Path) -> InstallKind {
    if let Some(image) = appimage.filter(|image| image.is_absolute()) {
        return InstallKind::AppImage(image.to_owned());
    }
    if exe.starts_with("/usr") {
        return InstallKind::SystemPackage;
    }
    InstallKind::Development
}

impl InstallKind {
    /// The command the `snowd` service runs: the program and its
    /// arguments. `snowd_beside` is `snowd` next to the running program.
    pub fn snowd_command(&self, snowd_beside: &Path) -> (PathBuf, Vec<String>) {
        match self {
            InstallKind::AppImage(image) => (image.clone(), vec!["snowd".to_owned()]),
            InstallKind::SystemPackage | InstallKind::Development => {
                (snowd_beside.to_owned(), Vec::new())
            }
        }
    }

    /// Whether Snow writes its own menu entry and icon (a package ships
    /// them instead).
    pub fn writes_desktop_entry(&self) -> bool {
        !matches!(self, InstallKind::SystemPackage)
    }

    /// Whether Snow looks for and offers its own updates: only as an
    /// AppImage. A package is updated by the system; a program must not
    /// replace files a package manager owns.
    pub fn offers_updates(&self) -> bool {
        matches!(self, InstallKind::AppImage(_))
    }
}

/// The menu entry and the icon, as shipped in `data/` (the entry has
/// `Exec=snow-launcher`, right for a package).
pub const APP_ID: &str = "io.github.ninjairocode.SnowLauncher";

/// The menu entry with `Exec` set to `program` (an absolute path: the
/// `snow-launcher` of a development build, or the `.AppImage` file),
/// quoted as the Desktop Entry specification asks.
pub fn desktop_entry_for(template: &str, program: &Path) -> String {
    let exec = format!("Exec={}", desktop_exec_quote(&program.to_string_lossy()));
    let mut out = String::with_capacity(template.len() + 64);
    for line in template.lines() {
        if line.starts_with("Exec=") {
            out.push_str(&exec);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// One `Exec` argument: in double quotes, with `"`, `` ` ``, `$` and `\`
/// escaped by a backslash, and `%` doubled (it starts a field code).
fn desktop_exec_quote(argument: &str) -> String {
    let mut quoted = String::from('"');
    for c in argument.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(c);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMAGE: &str = "/home/u/Apps/Snow_Launcher-x86_64.AppImage";

    #[test]
    fn detects_how_it_was_installed() {
        let mount = Path::new("/tmp/.mount_SnowLaX1y2/usr/bin/snowctl");
        assert_eq!(
            detect(Some(Path::new(IMAGE)), mount),
            InstallKind::AppImage(PathBuf::from(IMAGE))
        );
        assert_eq!(
            detect(None, Path::new("/usr/bin/snowctl")),
            InstallKind::SystemPackage
        );
        assert_eq!(
            detect(None, Path::new("/home/u/.cargo/bin/snowctl")),
            InstallKind::Development
        );
        // A relative $APPIMAGE is not trusted.
        assert_eq!(
            detect(
                Some(Path::new("Snow.AppImage")),
                Path::new("/home/u/.cargo/bin/snowctl")
            ),
            InstallKind::Development
        );
    }

    #[test]
    fn the_service_runs_the_appimage_file_never_its_mount() {
        let beside = Path::new("/tmp/.mount_SnowLaX1y2/usr/bin/snowd");
        let (program, args) = InstallKind::AppImage(PathBuf::from(IMAGE)).snowd_command(beside);
        assert_eq!(program, PathBuf::from(IMAGE));
        assert_eq!(args, ["snowd"]);

        let cargo = Path::new("/home/u/.cargo/bin/snowd");
        assert_eq!(
            InstallKind::Development.snowd_command(cargo),
            (cargo.to_owned(), Vec::new())
        );
    }

    #[test]
    fn updates_and_menu_entries_per_kind() {
        let image = InstallKind::AppImage(PathBuf::from(IMAGE));
        assert!(image.offers_updates() && image.writes_desktop_entry());
        assert!(!InstallKind::SystemPackage.offers_updates());
        assert!(!InstallKind::SystemPackage.writes_desktop_entry());
        assert!(!InstallKind::Development.offers_updates());
        assert!(InstallKind::Development.writes_desktop_entry());
    }

    const TEMPLATE: &str = "[Desktop Entry]\nName=Snow Launcher\nExec=snow-launcher\nIcon=io.github.ninjairocode.SnowLauncher\n";

    #[test]
    fn the_entry_runs_the_absolute_program() {
        let entry = desktop_entry_for(TEMPLATE, Path::new(IMAGE));
        assert!(entry.contains(&format!("\nExec=\"{IMAGE}\"\n")), "{entry}");
        assert!(entry.contains("Icon=io.github.ninjairocode.SnowLauncher\n"));
        assert_eq!(entry.matches("Exec=").count(), 1);
    }

    #[test]
    fn hard_paths_are_quoted_by_the_spec() {
        let entry = desktop_entry_for(
            TEMPLATE,
            Path::new("/home/u/My \"Apps\"/100% $x/Snow.AppImage"),
        );
        assert!(
            entry.contains(r#"Exec="/home/u/My \"Apps\"/100%% \$x/Snow.AppImage""#),
            "{entry}"
        );
    }
}
