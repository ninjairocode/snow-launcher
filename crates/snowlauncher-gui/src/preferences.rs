//! The Preferences dialog: the performance switches (experiment E15).
//! Each change is written to `config.toml` right away, keeping the rest of
//! the file, with a backup first. `snowd` follows the GameMode switch by
//! itself; MangoHud applies the next time Battle.net opens.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use relm4::adw;
use relm4::adw::prelude::*;
use snowlauncher_core::config::PerformanceConfig;
use snowlauncher_wine::config_file::{default_config_path, read_config, set_config_bool};
use snowlauncher_wine::house::record_path;

// TODO(i18n): translate (pt-BR) once gettext is in place (v0.6).

/// The MangoHud Vulkan layer, as distributions install it.
const MANGOHUD_LAYERS: &[&str] = &[
    "/usr/share/vulkan/implicit_layer.d/MangoHud.x86_64.json",
    "/usr/share/vulkan/implicit_layer.d/MangoHud.json",
];

/// Whether the user installed each tool. Snow never installs them.
struct Installed {
    mangohud: bool,
    gamemode: bool,
}

fn installed() -> Installed {
    Installed {
        mangohud: MANGOHUD_LAYERS
            .iter()
            .any(|layer| Path::new(layer).is_file()),
        gamemode: in_path("gamemoded"),
    }
}

fn in_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

fn current() -> PerformanceConfig {
    default_config_path()
        .and_then(|path| read_config(&path).ok().flatten())
        .map(|loaded| loaded.config.performance)
        .unwrap_or_default()
}

fn backup_dir() -> Option<PathBuf> {
    record_path().and_then(|path| path.parent().map(|dir| dir.join("backups")))
}

/// Writes one switch; the error text is for a toast.
fn save(key: &str, value: bool) -> Result<(), String> {
    let path = default_config_path().ok_or("could not find the config folder")?;
    let backups = backup_dir().ok_or("could not find the state folder")?;
    set_config_bool(&path, "performance", key, value, &backups)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn switch_row(
    dialog: &adw::PreferencesDialog,
    key: &'static str,
    title: &str,
    subtitle: &str,
    active: bool,
    available: bool,
    missing: &str,
) -> adw::SwitchRow {
    let row = adw::SwitchRow::builder()
        .title(title)
        .subtitle(if available { subtitle } else { missing })
        .active(active)
        .sensitive(available || active)
        .build();
    let toasts = dialog.downgrade();
    // Set while the switch is put back after a failed save, so that
    // change is not saved (and failing) again, forever.
    let reverting = Rc::new(Cell::new(false));
    row.connect_active_notify(move |row| {
        if reverting.get() {
            return;
        }
        let Err(problem) = save(key, row.is_active()) else {
            return;
        };
        reverting.set(true);
        row.set_active(!row.is_active());
        reverting.set(false);
        if let Some(dialog) = toasts.upgrade() {
            let toast = adw::Toast::builder()
                .title(format!("Could not save: {problem}"))
                .use_markup(false)
                .build();
            dialog.add_toast(toast);
        }
    });
    row
}

/// Builds the dialog with the settings as they are now.
pub fn dialog() -> adw::PreferencesDialog {
    let dialog = adw::PreferencesDialog::new();
    dialog.set_title("Preferences");
    let now = current();
    let tools = installed();

    let group = adw::PreferencesGroup::builder()
        .title("Performance")
        .description("Tools you installed yourself. Off by default.")
        .build();
    group.add(&switch_row(
        &dialog,
        "mangohud",
        "MangoHud",
        "Your own MangoHud overlay in the games. Takes effect the next time Battle.net opens.",
        now.mangohud,
        tools.mangohud,
        "MangoHud is not installed.",
    ));
    group.add(&switch_row(
        &dialog,
        "gamemode",
        "GameMode",
        "Feral GameMode while a game runs, only for the game.",
        now.gamemode,
        tools.gamemode,
        "GameMode is not installed.",
    ));

    let page = adw::PreferencesPage::new();
    page.add(&group);
    dialog.add(&page);
    dialog
}
