//! The Preferences dialog.
//!
//! - Performance (experiment E15): each switch is written to `config.toml`
//!   right away, keeping the rest of the file, with a backup first. `snowd`
//!   follows the GameMode switch by itself; MangoHud applies the next time
//!   Battle.net opens.
//! - House: the Proton the house uses (switched with a backup of the
//!   prefix, only with the house empty, O15) and Battle.net's tray icon
//!   (E14). Both run off the window's thread.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use relm4::adw::prelude::*;
use relm4::gtk::{gio, glib};
use relm4::{adw, gtk};
use snowlauncher_core::config::PerformanceConfig;
use snowlauncher_wine::config_file::{default_config_path, read_config, set_config_bool};
use snowlauncher_wine::house::{
    battlenet_icon_hidden, load_record, open_house, prefix_backup_dir, record_path,
    set_battlenet_icon, set_runtime,
};
use snowlauncher_wine::runtime::{Runtime, default_runtime, find_runtimes, runtime_dirs};

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
fn save(section: &str, key: &str, value: bool) -> Result<(), String> {
    let path = default_config_path().ok_or("could not find the config folder")?;
    let backups = backup_dir().ok_or("could not find the state folder")?;
    set_config_bool(&path, section, key, value, &backups)
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
        let Err(problem) = save("performance", key, row.is_active()) else {
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

fn toast(dialog: &glib::WeakRef<adw::PreferencesDialog>, text: String) {
    if let Some(dialog) = dialog.upgrade() {
        let toast = adw::Toast::builder()
            .title(text)
            .use_markup(false)
            .timeout(6)
            .build();
        dialog.add_toast(toast);
    }
}

/// How a Proton shows in the list: its folder, what it really is when the
/// name does not say it (ProtonPlus' "Latest"), and the default.
/// Short, so the list's selected value fits in its row:
/// `GE-Proton11-14-x86_64` shows as `GE-Proton11-14`.
pub fn runtime_choice_label(runtime: &Runtime, is_default: bool) -> String {
    let mut label = if runtime.name.starts_with(&runtime.label) {
        runtime.label.clone()
    } else {
        format!("{} ({})", runtime.name, runtime.label)
    };
    if is_default {
        label.push_str(" (default)");
    }
    label
}

/// The Proton list. Choosing another one switches the house, with a
/// backup of the prefix; a refusal (house in use) puts the list back.
fn proton_row(dialog: &adw::PreferencesDialog) -> Option<adw::ComboRow> {
    let record_file = record_path()?;
    let record = load_record(&record_file).ok().flatten()?;
    let runtimes = find_runtimes(&runtime_dirs());
    let default = default_runtime(&runtimes).map(|runtime| runtime.path.clone());
    let current_path =
        std::fs::canonicalize(&record.runtime_path).unwrap_or(record.runtime_path.clone());
    let labels: Vec<String> = runtimes
        .iter()
        .map(|runtime| runtime_choice_label(runtime, default.as_ref() == Some(&runtime.path)))
        .collect();
    let current = runtimes
        .iter()
        .position(|runtime| runtime.path == current_path)?;
    let model = gtk::StringList::new(&labels.iter().map(String::as_str).collect::<Vec<_>>());
    let row = adw::ComboRow::builder()
        .title("Proton")
        // The chosen Proton shows as the subtitle: room for its whole name.
        .use_subtitle(true)
        .model(&model)
        .selected(u32::try_from(current).ok()?)
        .build();

    let shown = Rc::new(Cell::new(current));
    let reverting = Rc::new(Cell::new(false));
    let prefix_backups = prefix_backup_dir(&record.prefix)?;
    let backups = backup_dir()?;
    let toasts = dialog.downgrade();
    row.connect_selected_notify(move |row| {
        if reverting.get() {
            return;
        }
        let Some(index) = usize::try_from(row.selected()).ok() else {
            return;
        };
        let Some(chosen) = runtimes.get(index).cloned() else {
            return;
        };
        if index == shown.get() {
            return;
        }
        row.set_sensitive(false);
        let (record_file, backups, prefix_backups) =
            (record_file.clone(), backups.clone(), prefix_backups.clone());
        let (row, shown, reverting, toasts) = (
            row.clone(),
            shown.clone(),
            reverting.clone(),
            toasts.clone(),
        );
        glib::spawn_future_local(async move {
            let name = chosen.name.clone();
            let result = gio::spawn_blocking(move || {
                set_runtime(&record_file, &chosen, &backups, &prefix_backups)
                    .map_err(|error| error.to_string())
            })
            .await
            .unwrap_or_else(|_| Err("the switch stopped unexpectedly".to_owned()));
            match result {
                Ok(switch) => {
                    shown.set(index);
                    toast(
                        &toasts,
                        format!(
                            "The house now uses {name}. Prefix backed up to {}",
                            switch.prefix_backup.display()
                        ),
                    );
                }
                Err(problem) => {
                    reverting.set(true);
                    row.set_selected(u32::try_from(shown.get()).unwrap_or(0));
                    reverting.set(false);
                    toast(&toasts, format!("Proton not switched: {problem}"));
                }
            }
            row.set_sensitive(true);
        });
    });
    Some(row)
}

/// The "hide Battle.net's tray icon" switch. Its real state is asked to
/// Wine (about 2 s), so it starts disabled.
fn icon_row(dialog: &adw::PreferencesDialog) -> adw::SwitchRow {
    let row = adw::SwitchRow::builder()
        .title("Hide Battle.net's tray icon")
        .subtitle("Snow's own icon replaces it. Takes effect the next time Battle.net starts.")
        .sensitive(false)
        .build();
    let reverting = Rc::new(Cell::new(false));

    {
        let (row, reverting) = (row.clone(), reverting.clone());
        glib::spawn_future_local(async move {
            let hidden = gio::spawn_blocking(|| {
                let (_, runner) = open_house().ok()?;
                battlenet_icon_hidden(&runner).ok()
            })
            .await
            .ok()
            .flatten();
            if let Some(hidden) = hidden {
                reverting.set(true);
                row.set_active(hidden);
                reverting.set(false);
                row.set_sensitive(true);
            }
        });
    }

    let toasts = dialog.downgrade();
    row.connect_active_notify(move |row| {
        if reverting.get() {
            return;
        }
        let hide = row.is_active();
        row.set_sensitive(false);
        let (row, reverting, toasts) = (row.clone(), reverting.clone(), toasts.clone());
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let backups = backup_dir().ok_or("could not find the state folder")?;
                let (_, runner) = open_house().map_err(|error| error.to_string())?;
                set_battlenet_icon(&runner, hide, &backups)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            })
            .await
            .unwrap_or_else(|_| Err("the change stopped unexpectedly".to_owned()));
            match result {
                Ok(()) => toast(
                    &toasts,
                    "Saved. It takes effect the next time Battle.net starts with the house closed."
                        .to_owned(),
                ),
                Err(problem) => {
                    reverting.set(true);
                    row.set_active(!hide);
                    reverting.set(false);
                    toast(&toasts, format!("Could not change it: {problem}"));
                }
            }
            row.set_sensitive(true);
        });
    });
    row
}

/// "Start Snow at login": turns the `snowd` service on or off at login.
/// Off, nothing of Snow runs until its window is opened (which starts it).
fn startup_group(dialog: &adw::PreferencesDialog) -> adw::PreferencesGroup {
    use snowlauncher_wine::user_service;
    let installed = user_service::is_installed();
    let row = adw::SwitchRow::builder()
        .title("Start Snow at login")
        .subtitle(if installed {
            "The tray icon and the protections while you play (no suspend, lost \
             connection warning) start with your session. Off, Snow starts when you \
             open this window."
        } else {
            "The background service is not installed: run `snowctl service install`."
        })
        .active(installed && user_service::starts_at_login().unwrap_or(false))
        .sensitive(installed)
        .build();
    let reverting = Rc::new(Cell::new(false));
    let toasts = dialog.downgrade();
    row.connect_active_notify(move |row| {
        if reverting.get() {
            return;
        }
        let Err(error) = user_service::set_starts_at_login(row.is_active()) else {
            return;
        };
        reverting.set(true);
        row.set_active(!row.is_active());
        reverting.set(false);
        toast(&toasts, format!("Could not change it: {error}"));
    });
    let group = adw::PreferencesGroup::builder().title("Startup").build();
    group.add(&row);
    group.add(&open_battlenet_row(dialog));
    group
}

/// "Open Battle.net when Snow starts" (`[startup] open_battlenet`).
fn open_battlenet_row(dialog: &adw::PreferencesDialog) -> adw::SwitchRow {
    let on = default_config_path()
        .and_then(|path| read_config(&path).ok().flatten())
        .is_some_and(|loaded| loaded.config.startup.open_battlenet);
    let row = adw::SwitchRow::builder()
        .title("Open Battle.net when Snow starts")
        .subtitle("Once per session: at login, or when this window starts Snow.")
        .active(on)
        .build();
    let reverting = Rc::new(Cell::new(false));
    let toasts = dialog.downgrade();
    row.connect_active_notify(move |row| {
        if reverting.get() {
            return;
        }
        let Err(problem) = save("startup", "open_battlenet", row.is_active()) else {
            return;
        };
        reverting.set(true);
        row.set_active(!row.is_active());
        reverting.set(false);
        toast(&toasts, format!("Could not save: {problem}"));
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
    page.add(&startup_group(&dialog));
    page.add(&group);

    // Only with a house: there is nothing to set up without one.
    if let Some(proton) = proton_row(&dialog) {
        let house = adw::PreferencesGroup::builder()
            .title("House")
            .description("Switching Proton backs up the house first, and needs Battle.net closed.")
            .build();
        house.add(&proton);
        house.add(&icon_row(&dialog));
        page.add(&house);
    }
    dialog.add(&page);
    dialog
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime(name: &str, label: &str) -> Runtime {
        Runtime {
            name: name.to_owned(),
            path: PathBuf::from("/c").join(name),
            label: label.to_owned(),
            version: None,
        }
    }

    #[test]
    fn proton_labels() {
        assert_eq!(
            runtime_choice_label(&runtime("GE-Proton11-14-x86_64", "GE-Proton11-14"), true),
            "GE-Proton11-14 (default)"
        );
        assert_eq!(
            runtime_choice_label(&runtime("Proton-GE Latest", "GE-Proton11-14"), false),
            "Proton-GE Latest (GE-Proton11-14)"
        );
        assert_eq!(
            runtime_choice_label(&runtime("UMU-Proton-10.0-4", "UMU-Proton-10.0-4"), false),
            "UMU-Proton-10.0-4"
        );
    }
}
