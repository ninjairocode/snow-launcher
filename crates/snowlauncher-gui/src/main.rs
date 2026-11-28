//! `snow-launcher`: Snow Launcher's window (v0.3, first step).
//!
//! It shows what `snowd` sees (over D-Bus), the house and how far
//! Battle.net got in starting (the doghouse's lights, one per stage, from
//! its log), and the library's games. "Open Battle.net"
//! and "Play" run off the window's thread and report with a toast. Layout rules from experiment E7: `adw::ToolbarView`, an
//! illustration that fails to load is simply not shown, and `adw::Clamp`
//! falls back to pixels when the desktop publishes no text DPI.

mod games;
mod images;
mod import_dialog;
mod preferences;
mod state;

use std::time::{Duration, Instant};

use relm4::adw::prelude::*;
use relm4::factory::FactoryVecDeque;
use relm4::{Component, ComponentParts, ComponentSender, RelmApp, RelmWidgetExt, adw, gtk};
use snowlauncher_wine::battlenet_logs::{newest_session_log, session_stage};
use snowlauncher_wine::daemon_client::query_async;
use snowlauncher_wine::game_icon::game_icon;
use snowlauncher_wine::house::{battlenet_pid, open_house, play_game, run_battlenet};
use snowlauncher_wine::library::{installed_games, known_product_codes};

use crate::games::GameRow;
use snowlauncher_core::install::InstallState;
use snowlauncher_core::installer::OFFICIAL_DOWNLOAD_PAGE;
use snowlauncher_wine::house::install_battlenet;
use snowlauncher_wine::installer_cache::choose_installer;

use crate::state::{
    BattlenetView, GameCard, House, Illustration, SessionTracker, StatusRow, game_cards,
    house_text, install_text, open_message, play_message, read_house, status_rows,
};

const APP_ID: &str = "io.github.ninjairocode.SnowLauncher";
const REFRESH_EVERY: Duration = Duration::from_secs(3);
/// Faster while Battle.net starts, so each light is seen.
const REFRESH_WHILE_STARTING: Duration = Duration::from_secs(1);
const STATUS_ROWS: usize = 4;
/// Games are read less often: it means reading Battle.net's logs.
const GAMES_EVERY: Duration = Duration::from_secs(30);

const EMPTY_SVG: &[u8] = include_bytes!("../../../data/illustrations/empty.svg");
/// The doghouse with 0 to 3 lights on.
const LIGHTS_SVG: [&[u8]; 4] = [
    include_bytes!("../../../data/illustrations/progress-0.svg"),
    include_bytes!("../../../data/illustrations/progress-1.svg"),
    include_bytes!("../../../data/illustrations/progress-2.svg"),
    include_bytes!("../../../data/illustrations/progress-3.svg"),
];

struct App {
    rows: Vec<StatusRow>,
    house: House,
    art: Illustrations,
    battlenet: BattlenetView,
    cards: Vec<GameCard>,
    games: FactoryVecDeque<GameRow>,
    /// An action is running: buttons are disabled until it ends.
    busy: bool,
    /// Battle.net being installed from the window: last state and time.
    install: Option<(InstallState, Duration)>,
}

impl App {
    fn illustration(&self) -> Option<&gtk::gdk::Texture> {
        match self.text().0 {
            Illustration::Empty => self.art.empty.as_ref(),
            Illustration::Lights(on) => self.art.lights.get(on)?.as_ref(),
        }
    }

    fn text(&self) -> (Illustration, &'static str, String) {
        match (&self.house, &self.install) {
            (House::NoBattlenet, Some((state, elapsed))) => install_text(state, *elapsed),
            _ => house_text(&self.house, self.battlenet),
        }
    }

    /// Shows `cards` in the games list, rebuilding it only if it changed.
    fn set_cards(&mut self, cards: Vec<GameCard>) {
        if cards == self.cards {
            return;
        }
        let mut guard = self.games.guard();
        guard.clear();
        for card in &cards {
            guard.push_back(card.clone());
        }
        drop(guard);
        self.cards = cards;
    }

    /// "Install Battle.net" and the download page: only for a house
    /// without Battle.net, and not while installing.
    fn offers_install(&self) -> bool {
        self.house == House::NoBattlenet && !self.busy
    }

    /// Chooses a verified installer and installs Battle.net on a worker
    /// thread, sending each look at the installation back.
    fn install(&mut self, sender: &ComponentSender<Self>) {
        if !self.offers_install() {
            return;
        }
        self.busy = true;
        self.install = Some((
            InstallState::Installing { installed_bytes: 0 },
            Duration::ZERO,
        ));
        sender.spawn_command(|out| {
            let installer = match choose_installer(None) {
                Ok((path, _)) => path,
                Err(error) => {
                    out.emit(CommandOutput::InstallFailed(error.to_string()));
                    return;
                }
            };
            let result = install_battlenet(&installer, |state, elapsed| {
                out.emit(CommandOutput::Installing(*state, elapsed));
            });
            if let Err(error) = result {
                out.emit(CommandOutput::InstallFailed(error.to_string()));
            }
        });
    }

    fn row_title(&self, index: usize) -> &str {
        self.rows.get(index).map_or("", |row| row.title)
    }

    fn row_value(&self, index: usize) -> &str {
        self.rows.get(index).map_or("", |row| row.value.as_str())
    }

    fn row_visible(&self, index: usize) -> bool {
        index < self.rows.len()
    }

    fn can_act(&self) -> bool {
        self.house.is_ready() && !self.busy
    }

    /// Runs `job` on a worker thread (playing may wait for Battle.net to
    /// log in); the result comes back as [`CommandOutput::ActionDone`].
    fn act(&mut self, job: Job, sender: &ComponentSender<Self>) {
        if !self.can_act() {
            return;
        }
        self.busy = true;
        sender.spawn_oneshot_command(move || {
            let house = open_house().map_err(|error| error.to_string());
            let message = match job {
                Job::OpenBattlenet => open_message(&house.and_then(|(_, runner)| {
                    run_battlenet(&runner, &[]).map_err(|error| error.to_string())
                })),
                Job::Play { name, code } => play_message(
                    &name,
                    &house.and_then(|(_, runner)| {
                        play_game(&runner, &code).map_err(|error| error.to_string())
                    }),
                ),
            };
            CommandOutput::ActionDone(message)
        });
    }
}

/// What a button asks for.
#[derive(Debug)]
enum Job {
    OpenBattlenet,
    Play { name: String, code: String },
}

#[derive(Debug)]
enum Msg {
    OpenBattlenet,
    Play(GameCard),
    /// The window's scale factor changed (e.g. moved to another monitor).
    Scale(i32),
    Preferences,
    InstallBattlenet,
    OpenDownloadPage,
    ImportGame,
    /// A game was imported: read the library again.
    RefreshGames,
}

#[derive(Debug)]
enum CommandOutput {
    Refreshed {
        rows: Vec<StatusRow>,
        house: House,
        battlenet: BattlenetView,
        /// `None`: not read this time, keep the current list.
        cards: Option<Vec<GameCard>>,
    },
    ActionDone(String),
    Installing(InstallState, Duration),
    InstallFailed(String),
    Games(Vec<GameCard>),
}

#[relm4::component]
impl Component for App {
    type Init = ();
    type Input = Msg;
    type Output = ();
    type CommandOutput = CommandOutput;

    view! {
        adw::ApplicationWindow {
            set_title: Some("Snow Launcher"),
            set_default_size: (720, 680),

            #[wrap(Some)]
            set_content = &adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {
                    pack_start = &gtk::Button {
                        set_label: "Open Battle.net",
                        set_tooltip_text: Some("Open Battle.net in the house, or bring its window back"),
                        #[watch]
                        set_sensitive: model.can_act(),
                        connect_clicked => Msg::OpenBattlenet,
                    },
                    pack_end = &gtk::Button {
                        set_icon_name: "preferences-system-symbolic",
                        set_tooltip_text: Some("Preferences"),
                        connect_clicked => Msg::Preferences,
                    },
                },

                #[wrap(Some)]
                #[name = "toasts"]
                set_content = &adw::ToastOverlay {
                #[wrap(Some)]
                set_child = &gtk::ScrolledWindow {
                    set_hscrollbar_policy: gtk::PolicyType::Never,

                    #[name = "clamp"]
                    adw::Clamp {
                        set_maximum_size: 640,

                        gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_spacing: 12,
                            set_margin_all: 24,

                            gtk::Picture {
                                set_height_request: ILLUSTRATION_HEIGHT,
                                set_can_shrink: true,
                                #[watch]
                                set_paintable: model.illustration(),
                                #[watch]
                                set_visible: model.illustration().is_some(),
                            },
                            gtk::Label {
                                add_css_class: "title-1",
                                set_wrap: true,
                                set_justify: gtk::Justification::Center,
                                #[watch]
                                set_label: model.text().1,
                            },
                            gtk::Label {
                                add_css_class: "dim-label",
                                set_wrap: true,
                                set_justify: gtk::Justification::Center,
                                #[watch]
                                set_label: &model.text().2,
                            },
                            gtk::Box {
                                set_orientation: gtk::Orientation::Horizontal,
                                set_spacing: 12,
                                set_halign: gtk::Align::Center,
                                set_margin_top: 6,
                                #[watch]
                                set_visible: model.offers_install(),

                                gtk::Button {
                                    set_label: "Install Battle.net",
                                    add_css_class: "suggested-action",
                                    add_css_class: "pill",
                                    set_tooltip_text: Some("Uses Blizzard's installer from Downloads, or the verified copy Snow kept"),
                                    connect_clicked => Msg::InstallBattlenet,
                                },
                                gtk::Button {
                                    set_label: "Open download page",
                                    add_css_class: "pill",
                                    set_tooltip_text: Some("Download the installer in the browser; Snow finds it in Downloads"),
                                    connect_clicked => Msg::OpenDownloadPage,
                                },
                            },
                            gtk::ListBox {
                                add_css_class: "boxed-list",
                                set_selection_mode: gtk::SelectionMode::None,
                                set_margin_top: 18,

                                adw::ActionRow {
                                    set_use_markup: false,
                                    #[watch]
                                    set_title: model.row_title(0),
                                    #[watch]
                                    set_subtitle: model.row_value(0),
                                },
                                adw::ActionRow {
                                    set_use_markup: false,
                                    #[watch]
                                    set_title: model.row_title(1),
                                    #[watch]
                                    set_subtitle: model.row_value(1),
                                    #[watch]
                                    set_visible: model.row_visible(1),
                                },
                                adw::ActionRow {
                                    set_use_markup: false,
                                    #[watch]
                                    set_title: model.row_title(2),
                                    #[watch]
                                    set_subtitle: model.row_value(2),
                                    #[watch]
                                    set_visible: model.row_visible(2),
                                },
                                adw::ActionRow {
                                    set_use_markup: false,
                                    #[watch]
                                    set_title: model.row_title(STATUS_ROWS - 1),
                                    #[watch]
                                    set_subtitle: model.row_value(STATUS_ROWS - 1),
                                    #[watch]
                                    set_visible: model.row_visible(STATUS_ROWS - 1),
                                },
                            },
                            gtk::Box {
                                set_orientation: gtk::Orientation::Horizontal,
                                set_margin_top: 18,
                                #[watch]
                                set_visible: model.house.is_ready(),

                                gtk::Label {
                                    add_css_class: "heading",
                                    set_halign: gtk::Align::Start,
                                    set_hexpand: true,
                                    set_label: "Games",
                                },
                                gtk::Button {
                                    set_label: "Import a game…",
                                    add_css_class: "flat",
                                    set_tooltip_text: Some("Copy a game from another launcher (Lutris, Steam…) into Snow's library"),
                                    connect_clicked => Msg::ImportGame,
                                },
                            },
                            #[local_ref]
                            games_list -> gtk::ListBox {
                                add_css_class: "boxed-list",
                                set_selection_mode: gtk::SelectionMode::None,
                                #[watch]
                                set_visible: model.house.is_ready(),
                                #[watch]
                                set_sensitive: model.can_act(),
                            },
                        },
                    },
                },
                },
            },
        }
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let games = FactoryVecDeque::builder()
            .launch(gtk::ListBox::default())
            .forward(sender.input_sender(), Msg::Play);
        let model = App {
            rows: Vec::new(),
            house: read_house(),
            art: Illustrations::render(root.scale_factor()),
            battlenet: BattlenetView::Closed,
            cards: Vec::new(),
            games,
            busy: false,
            install: None,
        };
        let games_list = model.games.widget();
        let widgets = view_output!();
        // The real scale is known once the window is on a monitor.
        let scale_sender = sender.input_sender().clone();
        root.connect_scale_factor_notify(move |window| {
            scale_sender.emit(Msg::Scale(window.scale_factor()));
        });
        let placeholder = gtk::Label::builder()
            .label("No games in the library yet")
            .margin_top(12)
            .margin_bottom(12)
            .css_classes(["dim-label"])
            .build();
        widgets.games_list.set_placeholder(Some(&placeholder));

        // `adw::Clamp` measures in "sp" (follows the text size), converted
        // with GTK's text DPI. Without a settings service (minimal window
        // managers, containers) that DPI is unset (-1), the conversion
        // breaks and the whole column collapses to a word's width (E7,
        // O10). Then use plain pixels.
        if gtk::Settings::default().is_none_or(|settings| settings.gtk_xft_dpi() <= 0) {
            widgets.clamp.set_unit(adw::LengthUnit::Px);
        }

        // Refresh in the background until the window closes.
        sender.command(|out, shutdown| {
            shutdown
                .register(async move {
                    let mut tracker = SessionTracker::default();
                    let mut games_read: Option<Instant> = None;
                    let mut was_ready = false;
                    loop {
                        let rows = status_rows(&query_async().await);
                        let house = read_house();
                        let battlenet = battlenet_view(&house, &mut tracker);
                        let is_ready = battlenet == BattlenetView::Ready;
                        // Launch codes show up in the log at login: reread
                        // the games then, besides every 30 s.
                        let games_due = games_read.is_none_or(|at| at.elapsed() >= GAMES_EVERY)
                            || (is_ready && !was_ready);
                        was_ready = is_ready;
                        let cards = if games_due {
                            games_read = Some(Instant::now());
                            // The first icon extraction reads the game's
                            // executable: keep it off this thread.
                            let for_games = house.clone();
                            let read = tokio::task::spawn_blocking(move || read_games(&for_games));
                            Some(read.await.unwrap_or_default())
                        } else {
                            None
                        };
                        let starting = matches!(
                            battlenet,
                            BattlenetView::Starting | BattlenetView::LoggingIn
                        );
                        let message = CommandOutput::Refreshed {
                            rows,
                            house,
                            battlenet,
                            cards,
                        };
                        if out.send(message).is_err() {
                            break;
                        }
                        let pause = if starting {
                            REFRESH_WHILE_STARTING
                        } else {
                            REFRESH_EVERY
                        };
                        tokio::time::sleep(pause).await;
                    }
                })
                .drop_on_shutdown()
        });

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Msg, sender: ComponentSender<Self>, root: &Self::Root) {
        match message {
            Msg::Preferences => preferences::dialog().present(Some(root)),
            Msg::InstallBattlenet => self.install(&sender),
            Msg::ImportGame => {
                if let House::Ready { library, .. } = &self.house {
                    let refresh = sender.input_sender().clone();
                    import_dialog::dialog(library, move || refresh.emit(Msg::RefreshGames))
                        .present(Some(root));
                }
            }
            Msg::RefreshGames => {
                let house = self.house.clone();
                sender.spawn_oneshot_command(move || CommandOutput::Games(read_games(&house)));
            }
            Msg::OpenDownloadPage => {
                // The browser downloads it, not Snow.
                let opened = gtk::gio::AppInfo::launch_default_for_uri(
                    OFFICIAL_DOWNLOAD_PAGE,
                    gtk::gio::AppLaunchContext::NONE,
                );
                if let Err(error) = opened {
                    eprintln!("snow-launcher: could not open {OFFICIAL_DOWNLOAD_PAGE}: {error}");
                }
            }
            Msg::OpenBattlenet => self.act(Job::OpenBattlenet, &sender),
            Msg::Scale(scale) => {
                if scale != self.art.scale {
                    self.art = Illustrations::render(scale);
                }
            }
            Msg::Play(card) => {
                if let Some(code) = card.launch_code {
                    let job = Job::Play {
                        name: card.name,
                        code,
                    };
                    self.act(job, &sender);
                }
            }
        }
    }

    fn update_cmd_with_view(
        &mut self,
        widgets: &mut Self::Widgets,
        message: CommandOutput,
        sender: ComponentSender<Self>,
        _: &Self::Root,
    ) {
        match message {
            CommandOutput::Refreshed {
                rows,
                house,
                battlenet,
                cards,
            } => {
                self.rows = rows;
                self.house = house;
                self.battlenet = battlenet;
                if let Some(cards) = cards {
                    self.set_cards(cards);
                }
            }
            CommandOutput::Installing(state, elapsed) => {
                self.install = Some((state, elapsed));
                if !matches!(state, InstallState::Installing { .. }) {
                    self.busy = false;
                    if state == InstallState::Done {
                        self.install = None;
                        self.house = read_house();
                    }
                }
            }
            CommandOutput::Games(cards) => self.set_cards(cards),
            CommandOutput::InstallFailed(text) => {
                self.busy = false;
                self.install = None;
                let toast = adw::Toast::builder()
                    .title(text)
                    .use_markup(false)
                    .timeout(10)
                    .build();
                widgets.toasts.add_toast(toast);
            }
            CommandOutput::ActionDone(text) => {
                self.busy = false;
                let toast = adw::Toast::builder().title(text).use_markup(false).build();
                widgets.toasts.add_toast(toast);
            }
        }
        self.update_view(widgets, sender);
    }
}

/// Whether Battle.net runs in the house and how far it got, from its
/// processes and its session log.
fn battlenet_view(house: &House, tracker: &mut SessionTracker) -> BattlenetView {
    let House::Ready { prefix, .. } = house else {
        return BattlenetView::Closed;
    };
    // Processes are matched by their canonical WINEPREFIX.
    let prefix = std::fs::canonicalize(prefix).unwrap_or_else(|_| prefix.clone());
    let running = battlenet_pid(&prefix).ok().flatten().is_some();
    tracker.update(
        running,
        newest_session_log(&prefix),
        Instant::now(),
        |log| session_stage(log).ok(),
    )
}

/// The library's games, with launch codes from Battle.net's logs.
fn read_games(house: &House) -> Vec<GameCard> {
    match house {
        House::Ready { prefix, library } => {
            let games = installed_games(library, &known_product_codes(prefix));
            game_cards(&games, game_icon)
        }
        _ => Vec::new(),
    }
}

/// Illustration height in the window, in logical pixels.
const ILLUSTRATION_HEIGHT: i32 = 240;

/// The illustrations, rasterized for one scale factor.
struct Illustrations {
    scale: i32,
    empty: Option<gtk::gdk::Texture>,
    /// The doghouse with 0 to 3 lights on.
    lights: Vec<Option<gtk::gdk::Texture>>,
}

impl Illustrations {
    fn render(scale: i32) -> Self {
        let scale = scale.max(1);
        Self {
            scale,
            empty: texture("empty.svg", EMPTY_SVG, scale),
            lights: LIGHTS_SVG
                .iter()
                .enumerate()
                .map(|(on, bytes)| texture(&format!("progress-{on}.svg"), bytes, scale))
                .collect(),
        }
    }
}

/// Rasterizes an embedded SVG at the height it is shown, in device pixels
/// (`ILLUSTRATION_HEIGHT` × `scale`). Loading the SVG at its own small
/// size (128 px) and stretching it was what made it blurry.
fn texture(name: &str, bytes: &'static [u8], scale: i32) -> Option<gtk::gdk::Texture> {
    images::svg_texture(name, bytes, ILLUSTRATION_HEIGHT * scale)
}

fn main() {
    RelmApp::new(APP_ID).run::<App>(());
}
