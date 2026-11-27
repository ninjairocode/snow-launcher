//! Snow's item in the desktop tray: a StatusNotifierItem with a
//! `com.canonical.dbusmenu` menu, as validated in experiment E12.
//!
//! - The icon goes by name, with the SVG loose in our own
//!   `IconThemePath` (a `hicolor/` tree without `index.theme` shows as
//!   three dots): crisp at any scale.
//! - The menu (built in `snowlauncher_core::tray`) follows `snowd`'s
//!   status and the house's games.
//! - Clicks become [`TrayAction`]s, run off the D-Bus handlers.
//! - No tray (GNOME without the AppIndicator extension) is not an error:
//!   the item registers whenever a `StatusNotifierWatcher` shows up.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::StreamExt;
use snowlauncher_core::desktop::{Notification, Notifier};
use snowlauncher_core::house::BATTLENET_EXE;
use snowlauncher_core::tray::{
    MenuItem, MenuKind, TrayAction, TrayGame, action_for, find, status_line, tray_menu,
};
use snowlauncher_wine::house::{
    QUIT_TIMEOUT, QuitOutcome, launch_argument, load_record, open_house, quit_battlenet,
    record_path, run_battlenet, write_atomic,
};
use snowlauncher_wine::library::{installed_games, known_product_codes};
use tokio::sync::{mpsc, watch};
use tracing::{info, warn};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

use crate::dbus::Status;
use crate::notifications::DesktopNotifier;

const ITEM_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";
const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const ICON_NAME: &str = "io.github.ninjairocode.SnowLauncher";
const ICON_SVG: &[u8] =
    include_bytes!("../../../data/icons/io.github.ninjairocode.SnowLauncher.svg");
/// Games are reread this often: it means reading Battle.net's logs.
const GAMES_EVERY: Duration = Duration::from_secs(30);
const LAUNCHER_BINARY: &str = "snow-launcher";

struct Item {
    theme_path: String,
    tooltip: String,
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    #[zbus(property)]
    fn category(&self) -> &str {
        "ApplicationStatus"
    }

    #[zbus(property)]
    fn id(&self) -> &str {
        "snow-launcher"
    }

    #[zbus(property)]
    fn title(&self) -> &str {
        "Snow Launcher"
    }

    #[zbus(property)]
    fn status(&self) -> &str {
        "Active"
    }

    #[zbus(property)]
    fn icon_name(&self) -> &str {
        ICON_NAME
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> &str {
        &self.theme_path
    }

    /// We have a menu, but also answer clicks (see `activate`).
    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn menu(&self) -> ObjectPath<'static> {
        ObjectPath::from_static_str_unchecked(MENU_PATH)
    }

    #[zbus(property)]
    fn tool_tip(&self) -> ToolTip {
        (
            String::new(),
            Vec::new(),
            "Snow Launcher".to_owned(),
            self.tooltip.clone(),
        )
    }

    // GNOME's AppIndicator opens the menu on any click (E12); hosts that
    // do send these get nothing surprising.
    fn activate(&self, _x: i32, _y: i32) {}

    fn secondary_activate(&self, _x: i32, _y: i32) {}

    fn context_menu(&self, _x: i32, _y: i32) {}

    fn scroll(&self, _delta: i32, _orientation: &str) {}

    #[zbus(signal)]
    async fn new_tool_tip(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// A StatusNotifierItem pixmap: width, height, ARGB32 bytes.
type Pixmap = (i32, i32, Vec<u8>);
/// StatusNotifierItem's `ToolTip`: icon name, pixmaps, title, text.
type ToolTip = (String, Vec<Pixmap>, String, String);

/// dbusmenu's `(ia{sv}av)`: id, properties, children.
type Layout = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

struct Menu {
    items: Vec<MenuItem>,
    revision: u32,
    actions: mpsc::UnboundedSender<TrayAction>,
}

impl Menu {
    fn click(&self, id: i32) {
        let Some(action) = action_for(&self.items, id) else {
            return;
        };
        if self.actions.send(action.clone()).is_err() {
            warn!("tray action dropped: the action runner stopped");
        }
    }

    fn layout_of(&self, parent: i32) -> Option<Layout> {
        if parent == 0 {
            let root = props([("children-display", Value::from("submenu"))]);
            return Some((0, root, children(&self.items)));
        }
        let item = find(&self.items, parent)?;
        let kids = match &item.kind {
            MenuKind::Submenu(kids) => children(kids),
            _ => Vec::new(),
        };
        Some((item.id, properties(item), kids))
    }

    fn all_ids(&self) -> Vec<i32> {
        fn walk(items: &[MenuItem], ids: &mut Vec<i32>) {
            for item in items {
                ids.push(item.id);
                if let MenuKind::Submenu(kids) = &item.kind {
                    walk(kids, ids);
                }
            }
        }
        let mut ids = Vec::new();
        walk(&self.items, &mut ids);
        ids
    }
}

fn props<'a>(pairs: impl IntoIterator<Item = (&'a str, Value<'a>)>) -> HashMap<String, OwnedValue> {
    pairs
        .into_iter()
        .filter_map(|(key, value)| Some((key.to_owned(), value.try_to_owned().ok()?)))
        .collect()
}

fn properties(item: &MenuItem) -> HashMap<String, OwnedValue> {
    let mut pairs = Vec::new();
    match &item.kind {
        MenuKind::Separator => pairs.push(("type", Value::from("separator"))),
        MenuKind::Submenu(_) => {
            pairs.push(("label", Value::from(item.label.as_str())));
            pairs.push(("children-display", Value::from("submenu")));
        }
        MenuKind::Action(_) => pairs.push(("label", Value::from(item.label.as_str()))),
    }
    if !item.enabled {
        pairs.push(("enabled", Value::from(false)));
    }
    props(pairs)
}

fn children(items: &[MenuItem]) -> Vec<OwnedValue> {
    items
        .iter()
        .filter_map(|item| {
            let kids = match &item.kind {
                MenuKind::Submenu(kids) => children(kids),
                _ => Vec::new(),
            };
            let layout: Layout = (item.id, properties(item), kids);
            Value::from(layout).try_to_owned().ok()
        })
        .collect()
}

#[zbus::interface(name = "com.canonical.dbusmenu")]
impl Menu {
    #[zbus(property)]
    fn version(&self) -> u32 {
        3
    }

    #[zbus(property)]
    fn status(&self) -> &str {
        "normal"
    }

    #[zbus(property)]
    fn text_direction(&self) -> &str {
        "ltr"
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> Vec<String> {
        Vec::new()
    }

    fn get_layout(
        &self,
        parent_id: i32,
        _recursion_depth: i32,
        _property_names: Vec<String>,
    ) -> zbus::fdo::Result<(u32, Layout)> {
        let layout = self
            .layout_of(parent_id)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("no menu item {parent_id}")))?;
        Ok((self.revision, layout))
    }

    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        _property_names: Vec<String>,
    ) -> Vec<(i32, HashMap<String, OwnedValue>)> {
        let ids = if ids.is_empty() { self.all_ids() } else { ids };
        ids.into_iter()
            .filter_map(|id| Some((id, properties(find(&self.items, id)?))))
            .collect()
    }

    fn get_property(&self, id: i32, name: &str) -> zbus::fdo::Result<OwnedValue> {
        find(&self.items, id)
            .and_then(|item| properties(item).remove(name))
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("no {name} on item {id}")))
    }

    fn event(&self, id: i32, event_id: &str, _data: Value<'_>, _timestamp: u32) {
        if event_id == "clicked" {
            self.click(id);
        }
    }

    /// Returns the ids that were not found.
    fn event_group(&self, events: Vec<(i32, String, OwnedValue, u32)>) -> Vec<i32> {
        let mut missing = Vec::new();
        for (id, event_id, _, _) in events {
            if find(&self.items, id).is_none() {
                missing.push(id);
            } else if event_id == "clicked" {
                self.click(id);
            }
        }
        missing
    }

    fn about_to_show(&self, _id: i32) -> bool {
        false
    }

    fn about_to_show_group(&self, _ids: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
        (Vec::new(), Vec::new())
    }

    #[zbus(signal)]
    async fn layout_updated(
        emitter: &SignalEmitter<'_>,
        revision: u32,
        parent: i32,
    ) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "org.kde.StatusNotifierWatcher",
    default_service = "org.kde.StatusNotifierWatcher",
    default_path = "/StatusNotifierWatcher"
)]
trait Watcher {
    fn register_status_notifier_item(&self, service: &str) -> zbus::Result<()>;
}

/// The house as the menu needs it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct HouseGames {
    ready: bool,
    games: Vec<TrayGame>,
}

/// Shows the tray item until `snowd` stops. Never fails: without a tray
/// `snowd` still does everything else, so problems are only logged.
pub async fn run(session_bus: &zbus::Connection, status: watch::Receiver<Status>) {
    if let Err(error) = serve(session_bus, status).await {
        warn!(error = format!("{error:#}"), "no tray item");
    }
    std::future::pending::<()>().await;
}

async fn serve(bus: &zbus::Connection, mut status: watch::Receiver<Status>) -> Result<()> {
    let theme_path = install_icon().context("could not write the tray icon")?;
    let house = read_house_games().await;
    let current = status.borrow_and_update().clone();
    let line = line_for(&current);
    let (actions, requests) = mpsc::unbounded_channel();
    let server = bus.object_server();
    server
        .at(
            ITEM_PATH,
            Item {
                theme_path: theme_path.display().to_string(),
                tooltip: line.clone(),
            },
        )
        .await
        .context("could not serve the tray item")?;
    server
        .at(
            MENU_PATH,
            Menu {
                items: tray_menu(
                    &line,
                    house.ready,
                    !current.running_games.is_empty(),
                    &house.games,
                ),
                revision: 1,
                actions,
            },
        )
        .await
        .context("could not serve the tray menu")?;
    let name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
    bus.request_name(name.as_str())
        .await
        .with_context(|| format!("could not take the D-Bus name {name}"))?;

    tokio::select! {
        result = keep_registered(bus, &name) => result,
        result = follow(bus, status, house) => result,
        () = run_actions(bus, requests) => Ok(()),
    }
}

fn line_for(status: &Status) -> String {
    status_line(
        &status.running_games,
        status.suspend_inhibited,
        status.game_connection,
    )
}

/// Writes the icon where only our item looks: loose in a folder of its
/// own under `$XDG_RUNTIME_DIR` (E12).
fn install_icon() -> std::io::Result<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .ok_or_else(|| std::io::Error::other("XDG_RUNTIME_DIR is not set"))?;
    let dir = PathBuf::from(runtime).join("snow-launcher").join("icons");
    std::fs::create_dir_all(&dir)?;
    write_atomic(&dir.join(format!("{ICON_NAME}.svg")), ICON_SVG)?;
    Ok(dir)
}

/// Registers with the tray host now and whenever one (re)appears, e.g.
/// after GNOME Shell restarts or the extension is turned on.
async fn keep_registered(bus: &zbus::Connection, name: &str) -> Result<()> {
    let dbus = zbus::fdo::DBusProxy::new(bus)
        .await
        .context("could not reach the D-Bus daemon")?;
    let mut owners = dbus
        .receive_name_owner_changed_with_args(&[(0, WATCHER)])
        .await
        .context("could not follow the tray host")?;
    register(bus, name).await;
    while let Some(change) = owners.next().await {
        if change.args().is_ok_and(|args| args.new_owner().is_some()) {
            register(bus, name).await;
        }
    }
    Ok(())
}

async fn register(bus: &zbus::Connection, name: &str) {
    let result = async {
        WatcherProxy::new(bus)
            .await?
            .register_status_notifier_item(name)
            .await
    }
    .await;
    match result {
        Ok(()) => info!("tray item registered"),
        // GNOME without the AppIndicator extension, for instance.
        Err(error) => info!(%error, "no tray host yet; the item will show when one appears"),
    }
}

/// Keeps the menu and tooltip in step with `snowd`'s status and the
/// house's games.
async fn follow(
    bus: &zbus::Connection,
    mut status: watch::Receiver<Status>,
    mut house: HouseGames,
) -> Result<()> {
    let menu = bus
        .object_server()
        .interface::<_, Menu>(MENU_PATH)
        .await
        .context("the tray menu is not served")?;
    let item = bus
        .object_server()
        .interface::<_, Item>(ITEM_PATH)
        .await
        .context("the tray item is not served")?;
    let mut games_timer = tokio::time::interval(GAMES_EVERY);
    loop {
        tokio::select! {
            changed = status.changed() => if changed.is_err() { return Ok(()) },
            _ = games_timer.tick() => house = read_house_games().await,
        }
        let current = status.borrow_and_update().clone();
        let line = line_for(&current);
        let items = tray_menu(
            &line,
            house.ready,
            !current.running_games.is_empty(),
            &house.games,
        );
        {
            let mut menu_ref = menu.get_mut().await;
            if menu_ref.items != items {
                menu_ref.items = items;
                menu_ref.revision = menu_ref.revision.wrapping_add(1);
                let revision = menu_ref.revision;
                Menu::layout_updated(menu.signal_emitter(), revision, 0).await?;
            }
        }
        let mut item_ref = item.get_mut().await;
        if item_ref.tooltip != line {
            item_ref.tooltip = line;
            Item::new_tool_tip(item.signal_emitter()).await?;
        }
    }
}

async fn read_house_games() -> HouseGames {
    tokio::task::spawn_blocking(|| {
        let Some(record) = record_path().and_then(|path| load_record(&path).ok().flatten()) else {
            return HouseGames::default();
        };
        let games = installed_games(&record.library, &known_product_codes(&record.prefix))
            .into_iter()
            .map(|game| TrayGame {
                name: game.name,
                launch_code: game.launch_code,
            })
            .collect();
        HouseGames {
            ready: record.prefix.join(BATTLENET_EXE).is_file(),
            games,
        }
    })
    .await
    .unwrap_or_default()
}

/// Runs the clicked actions one at a time; failures become a desktop
/// notification.
async fn run_actions(bus: &zbus::Connection, mut requests: mpsc::UnboundedReceiver<TrayAction>) {
    let mut notifier = DesktopNotifier::new(bus.clone());
    while let Some(action) = requests.recv().await {
        info!(?action, "tray action");
        let result = match &action {
            TrayAction::OpenLauncher => open_launcher().await,
            TrayAction::OpenBattlenet => battlenet(Vec::new()).await,
            TrayAction::Play { code, .. } => battlenet(vec![launch_argument(code)]).await,
            TrayAction::QuitBattlenet => quit().await,
        };
        if let Err(problem) = result {
            warn!(?action, %problem, "tray action failed");
            let notification = Notification {
                summary: "Snow Launcher".to_owned(),
                body: problem,
                urgent: false,
            };
            if let Err(error) = notifier.notify(&notification).await {
                warn!(%error, "could not show a notification");
            }
        }
    }
}

/// Opens or shows the window (GTK keeps a single instance). Prefers the
/// `snow-launcher` next to `snowd`, then the one in `PATH`.
async fn open_launcher() -> Result<(), String> {
    let beside = std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name(LAUNCHER_BINARY))
        .filter(|path| path.is_file());
    let program = beside.unwrap_or_else(|| PathBuf::from(LAUNCHER_BINARY));
    let mut child = tokio::process::Command::new(&program)
        .process_group(0)
        .spawn()
        .map_err(|error| {
            format!(
                "Could not open Snow Launcher ({}): {error}",
                program.display()
            )
        })?;
    // Reaped when it exits, so it never lingers as a zombie.
    tokio::spawn(async move { child.wait().await });
    Ok(())
}

async fn battlenet(args: Vec<String>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let (_, runner) = open_house().map_err(|error| error.to_string())?;
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        run_battlenet(&runner, &args).map_err(|error| error.to_string())?;
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|problem: String| format!("Could not open Battle.net: {problem}"))
}

/// Closes Battle.net (E10), re-checking for games right before.
async fn quit() -> Result<(), String> {
    let outcome = tokio::task::spawn_blocking(|| {
        let (_, runner) = open_house().map_err(|error| error.to_string())?;
        quit_battlenet(&runner).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|problem| format!("Could not close Battle.net: {problem}"))?;
    match outcome {
        QuitOutcome::Closed => {
            info!("Battle.net closed");
            Ok(())
        }
        QuitOutcome::NotRunning => Ok(()),
        QuitOutcome::ProgramsRunning(names) => Err(format!(
            "Battle.net stays open while {} runs. Close it first.",
            names.join(", ")
        )),
        QuitOutcome::StillOpen => Err(format!(
            "Battle.net was asked to close but is still open after {} s.",
            QUIT_TIMEOUT.as_secs()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> (Menu, mpsc::UnboundedReceiver<TrayAction>) {
        let (actions, requests) = mpsc::unbounded_channel();
        let games = [TrayGame {
            name: "World of Warcraft".to_owned(),
            launch_code: Some("WoWF".to_owned()),
        }];
        let menu = Menu {
            items: tray_menu("No game running", true, false, &games),
            revision: 1,
            actions,
        };
        (menu, requests)
    }

    fn label(props: &HashMap<String, OwnedValue>) -> Option<String> {
        props
            .get("label")
            .and_then(|value| String::try_from(value.clone()).ok())
    }

    #[test]
    fn root_layout_lists_the_items() {
        let (menu, _) = menu();
        let (id, _, kids) = menu.layout_of(0).unwrap();
        assert_eq!(id, 0);
        assert_eq!(kids.len(), menu.items.len());
    }

    #[test]
    fn properties_follow_the_item_kind() {
        let (menu, _) = menu();
        let status = properties(&menu.items[0]);
        assert_eq!(label(&status).as_deref(), Some("No game running"));
        assert_eq!(
            status
                .get("enabled")
                .and_then(|v| bool::try_from(v.clone()).ok()),
            Some(false)
        );
        let separator = properties(&menu.items[1]);
        assert_eq!(
            separator
                .get("type")
                .and_then(|v| String::try_from(v.clone()).ok())
                .as_deref(),
            Some("separator")
        );
        let play = menu.items.iter().find(|item| item.label == "Play").unwrap();
        let (_, play_props, games) = menu.layout_of(play.id).unwrap();
        assert!(play_props.contains_key("children-display"));
        assert_eq!(games.len(), 1);
    }

    #[test]
    fn clicks_become_actions() {
        let (menu, mut requests) = menu();
        let open = menu
            .items
            .iter()
            .find(|item| item.label == "Open Battle.net")
            .unwrap();
        menu.click(open.id);
        menu.click(menu.items[0].id); // the status line does nothing
        assert_eq!(requests.try_recv().unwrap(), TrayAction::OpenBattlenet);
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn group_properties_cover_every_item() {
        let (menu, _) = menu();
        let all = menu.get_group_properties(Vec::new(), Vec::new());
        assert_eq!(all.len(), menu.all_ids().len());
        assert!(menu.layout_of(12345).is_none());
    }
}
