//! The menu of Snow's tray item (experiment E12). Pure: `snowd` turns it
//! into a `com.canonical.dbusmenu` and runs the chosen [`TrayAction`].
//!
//! In GNOME's AppIndicator extension any click opens the menu, so every
//! action lives here, "Open Snow Launcher" first.

use crate::dbus::GameConnection;

// TODO(i18n): translate (pt-BR) once gettext is in place (v0.6).

/// What a menu item does when clicked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayAction {
    OpenLauncher,
    OpenBattlenet,
    Play {
        name: String,
        code: String,
    },
    /// Close Battle.net the clean way; never while a game runs.
    QuitBattlenet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuKind {
    /// `None`: a line of text, e.g. the status.
    Action(Option<TrayAction>),
    Separator,
    Submenu(Vec<MenuItem>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    /// dbusmenu id; 0 is the root. Fixed items keep fixed ids.
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub kind: MenuKind,
    /// PNG shown next to the label (a game's icon).
    pub icon_png: Option<Vec<u8>>,
}

/// A library game, for the "Play" submenu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayGame {
    pub name: String,
    /// `None` until Battle.net's log reveals it.
    pub launch_code: Option<String>,
    /// The game's icon as PNG, read from its own executable.
    pub icon_png: Option<Vec<u8>>,
}

const STATUS_ID: i32 = 1;
const OPEN_LAUNCHER_ID: i32 = 2;
const OPEN_BATTLENET_ID: i32 = 3;
const PLAY_ID: i32 = 4;
const SEPARATOR_ID: i32 = 5;
const NO_GAMES_ID: i32 = 6;
const QUIT_SEPARATOR_ID: i32 = 7;
const QUIT_ID: i32 = 8;
/// Games are numbered from here, in the order given.
const FIRST_GAME_ID: i32 = 100;

/// One line with what `snowd` sees, e.g. "World of Warcraft running ·
/// suspend blocked".
pub fn status_line(
    running_games: &[String],
    suspend_inhibited: bool,
    connection: GameConnection,
) -> String {
    if running_games.is_empty() {
        return "No game running".to_owned();
    }
    let mut line = format!("{} running", running_games.join(", "));
    if suspend_inhibited {
        line.push_str(" · suspend blocked");
    }
    match connection {
        GameConnection::Lost => line.push_str(" · connection lost"),
        GameConnection::NetworkDown => line.push_str(" · network down"),
        GameConnection::NoGame | GameConnection::Ok => {}
    }
    line
}

/// What the menu depends on, besides the games.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuState<'a> {
    /// The status line (see [`status_line`]).
    pub status: &'a str,
    /// A house with Battle.net installed: Battle.net and games can open.
    pub house_ready: bool,
    pub game_running: bool,
    pub battlenet_running: bool,
}

/// The whole menu. "Quit Battle.net" only shows while Battle.net runs,
/// and is off while a game runs.
pub fn tray_menu(state: MenuState<'_>, games: &[TrayGame]) -> Vec<MenuItem> {
    let MenuState {
        status,
        house_ready,
        game_running,
        battlenet_running,
    } = state;
    let item = |id, label: &str, enabled, action| MenuItem {
        id,
        label: label.to_owned(),
        enabled,
        kind: MenuKind::Action(action),
        icon_png: None,
    };
    let mut play: Vec<MenuItem> = games
        .iter()
        .zip(FIRST_GAME_ID..)
        .map(|(game, id)| {
            let action = game.launch_code.as_ref().map(|code| TrayAction::Play {
                name: game.name.clone(),
                code: code.clone(),
            });
            MenuItem {
                icon_png: game.icon_png.clone(),
                ..item(id, &game.name, house_ready && action.is_some(), action)
            }
        })
        .collect();
    if play.is_empty() {
        play.push(item(NO_GAMES_ID, "No games in the library", false, None));
    }
    let mut menu = vec![
        item(STATUS_ID, status, false, None),
        MenuItem {
            id: SEPARATOR_ID,
            label: String::new(),
            enabled: true,
            kind: MenuKind::Separator,
            icon_png: None,
        },
        item(
            OPEN_LAUNCHER_ID,
            "Open Snow Launcher",
            true,
            Some(TrayAction::OpenLauncher),
        ),
        item(
            OPEN_BATTLENET_ID,
            "Open Battle.net",
            house_ready,
            Some(TrayAction::OpenBattlenet),
        ),
        MenuItem {
            id: PLAY_ID,
            label: "Play".to_owned(),
            enabled: house_ready,
            kind: MenuKind::Submenu(play),
            icon_png: None,
        },
    ];
    if battlenet_running {
        menu.extend([
            MenuItem {
                id: QUIT_SEPARATOR_ID,
                label: String::new(),
                enabled: true,
                kind: MenuKind::Separator,
                icon_png: None,
            },
            item(
                QUIT_ID,
                "Quit Battle.net",
                house_ready && !game_running,
                Some(TrayAction::QuitBattlenet),
            ),
        ]);
    }
    menu
}

/// The item with `id`, at any depth.
pub fn find(items: &[MenuItem], id: i32) -> Option<&MenuItem> {
    items.iter().find_map(|item| {
        if item.id == id {
            Some(item)
        } else if let MenuKind::Submenu(children) = &item.kind {
            find(children, id)
        } else {
            None
        }
    })
}

/// What clicking the item `id` does: nothing for disabled items, text
/// lines, separators or unknown ids.
pub fn action_for(items: &[MenuItem], id: i32) -> Option<&TrayAction> {
    match find(items, id) {
        Some(MenuItem {
            enabled: true,
            kind: MenuKind::Action(Some(action)),
            ..
        }) => Some(action),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(
        status: &str,
        house_ready: bool,
        game_running: bool,
        battlenet_running: bool,
    ) -> MenuState<'_> {
        MenuState {
            status,
            house_ready,
            game_running,
            battlenet_running,
        }
    }

    #[test]
    fn quit_only_shows_while_battlenet_runs() {
        let menu = tray_menu(state("No game running", true, false, false), &[]);
        assert!(find(&menu, QUIT_ID).is_none());
        assert!(find(&menu, QUIT_SEPARATOR_ID).is_none());
        let menu = tray_menu(state("No game running", true, false, true), &[]);
        assert_eq!(action_for(&menu, QUIT_ID), Some(&TrayAction::QuitBattlenet));
    }

    fn wow(code: Option<&str>) -> TrayGame {
        TrayGame {
            name: "World of Warcraft".to_owned(),
            launch_code: code.map(str::to_owned),
            icon_png: Some(b"png".to_vec()),
        }
    }

    #[test]
    fn status_lines() {
        assert_eq!(
            status_line(&[], false, GameConnection::NoGame),
            "No game running"
        );
        let games = ["World of Warcraft".to_owned()];
        assert_eq!(
            status_line(&games, true, GameConnection::Lost),
            "World of Warcraft running · suspend blocked · connection lost"
        );
        assert_eq!(
            status_line(&games, false, GameConnection::Ok),
            "World of Warcraft running"
        );
    }

    #[test]
    fn menu_with_a_playable_game() {
        let menu = tray_menu(
            state("No game running", true, false, true),
            &[wow(Some("WoWF"))],
        );
        let labels: Vec<&str> = menu.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "No game running",
                "",
                "Open Snow Launcher",
                "Open Battle.net",
                "Play",
                "",
                "Quit Battle.net"
            ]
        );
        assert_eq!(action_for(&menu, QUIT_ID), Some(&TrayAction::QuitBattlenet));
        let game = find(&menu, FIRST_GAME_ID).unwrap();
        assert_eq!(game.icon_png.as_deref(), Some(&b"png"[..]));
        assert_eq!(action_for(&menu, STATUS_ID), None);
        assert_eq!(
            action_for(&menu, OPEN_LAUNCHER_ID),
            Some(&TrayAction::OpenLauncher)
        );
        assert_eq!(
            action_for(&menu, FIRST_GAME_ID),
            Some(&TrayAction::Play {
                name: "World of Warcraft".to_owned(),
                code: "WoWF".to_owned()
            })
        );
        assert_eq!(action_for(&menu, 9999), None);
    }

    #[test]
    fn without_a_house_only_the_launcher_opens() {
        let menu = tray_menu(
            state("No game running", false, false, true),
            &[wow(Some("WoWF"))],
        );
        assert!(action_for(&menu, OPEN_LAUNCHER_ID).is_some());
        assert_eq!(action_for(&menu, OPEN_BATTLENET_ID), None);
        assert_eq!(action_for(&menu, FIRST_GAME_ID), None);
    }

    #[test]
    fn battlenet_cannot_be_quit_while_a_game_runs() {
        let menu = tray_menu(state("World of Warcraft running", true, true, true), &[]);
        assert_eq!(action_for(&menu, QUIT_ID), None);
        assert!(action_for(&menu, OPEN_BATTLENET_ID).is_some());
        let without_house = tray_menu(state("No game running", false, false, true), &[]);
        assert_eq!(action_for(&without_house, QUIT_ID), None);
    }

    #[test]
    fn games_without_code_or_an_empty_library_cannot_be_played() {
        let menu = tray_menu(state("", true, false, true), &[wow(None)]);
        assert_eq!(action_for(&menu, FIRST_GAME_ID), None);
        assert!(!find(&menu, FIRST_GAME_ID).unwrap().enabled);

        let menu = tray_menu(state("", true, false, true), &[]);
        let empty = find(&menu, NO_GAMES_ID).unwrap();
        assert_eq!(empty.label, "No games in the library");
        assert!(!empty.enabled);
    }
}
