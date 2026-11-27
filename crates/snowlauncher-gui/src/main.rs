//! `snow-launcher`: Snow Launcher's window (v0.3, first step).
//!
//! It shows what `snowd` sees (over D-Bus), the house and how far
//! Battle.net got in starting (the doghouse's lights, one per stage, from
//! its log), and the library's games. "Open Battle.net"
//! and "Play" run off the window's thread and report with a toast. Layout rules from experiment E7: `adw::ToolbarView`, an
//! illustration that fails to load is simply not shown, and `adw::Clamp`
//! falls back to pixels when the desktop publishes no text DPI.

mod games;
mod state;

use std::time::{Duration, Instant};

use relm4::adw::prelude::*;
use relm4::factory::FactoryVecDeque;
use relm4::{Component, ComponentParts, ComponentSender, RelmApp, RelmWidgetExt, adw, gtk};
use snowlauncher_wine::battlenet_logs::{newest_session_log, session_stage};
use snowlauncher_wine::daemon_client::query_async;
use snowlauncher_wine::house::{battlenet_pid, launch_argument, open_house, run_battlenet};
use snowlauncher_wine::library::{installed_games, known_product_codes};

use crate::games::GameRow;
use crate::state::{
    BattlenetView, GameCard, House, Illustration, SessionTracker, StatusRow, action_message,
    game_cards, house_text, read_house, status_rows,
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
    empty: Option<gtk::gdk::Texture>,
    lights: Vec<Option<gtk::gdk::Texture>>,
    battlenet: BattlenetView,
    cards: Vec<GameCard>,
    games: FactoryVecDeque<GameRow>,
    /// An action is running: buttons are disabled until it ends.
    busy: bool,
}

impl App {
    fn illustration(&self) -> Option<&gtk::gdk::Texture> {
        match self.text().0 {
            Illustration::Empty => self.empty.as_ref(),
            Illustration::Lights(on) => self.lights.get(on)?.as_ref(),
        }
    }

    fn text(&self) -> (Illustration, &'static str, String) {
        house_text(&self.house, self.battlenet)
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

    /// Runs Battle.net with `args` on a worker thread; the result comes
    /// back as [`CommandOutput::ActionDone`].
    fn act(&mut self, what: String, args: Vec<String>, sender: &ComponentSender<Self>) {
        if !self.can_act() {
            return;
        }
        self.busy = true;
        sender.spawn_oneshot_command(move || {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let result = open_house()
                .map_err(|error| error.to_string())
                .and_then(|(_, runner)| {
                    run_battlenet(&runner, &args).map_err(|error| error.to_string())
                });
            CommandOutput::ActionDone(action_message(&what, &result))
        });
    }
}

#[derive(Debug)]
enum Msg {
    OpenBattlenet,
    Play(GameCard),
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
                                set_height_request: 240,
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
                            gtk::Label {
                                add_css_class: "heading",
                                set_halign: gtk::Align::Start,
                                set_margin_top: 18,
                                set_label: "Games",
                                #[watch]
                                set_visible: model.house.is_ready(),
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
            empty: texture("empty.svg", EMPTY_SVG),
            lights: LIGHTS_SVG
                .iter()
                .enumerate()
                .map(|(on, bytes)| texture(&format!("progress-{on}.svg"), bytes))
                .collect(),
            battlenet: BattlenetView::Closed,
            cards: Vec::new(),
            games,
            busy: false,
        };
        let games_list = model.games.widget();
        let widgets = view_output!();
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
                        let cards = games_due.then(|| {
                            games_read = Some(Instant::now());
                            read_games(&house)
                        });
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

    fn update(&mut self, message: Msg, sender: ComponentSender<Self>, _: &Self::Root) {
        match message {
            Msg::OpenBattlenet => self.act("Battle.net".to_owned(), Vec::new(), &sender),
            Msg::Play(card) => {
                if let Some(code) = card.launch_code {
                    self.act(card.name, vec![launch_argument(&code)], &sender);
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
                if let Some(cards) = cards.filter(|cards| *cards != self.cards) {
                    let mut guard = self.games.guard();
                    guard.clear();
                    for card in &cards {
                        guard.push_back(card.clone());
                    }
                    drop(guard);
                    self.cards = cards;
                }
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
            game_cards(&installed_games(library, &known_product_codes(prefix)))
        }
        _ => Vec::new(),
    }
}

/// Loads an embedded SVG. A missing SVG loader (librsvg) must not take the
/// window down: the illustration is then just not shown.
fn texture(name: &str, bytes: &'static [u8]) -> Option<gtk::gdk::Texture> {
    let bytes = gtk::glib::Bytes::from_static(bytes);
    match gtk::gdk::Texture::from_bytes(&bytes) {
        Ok(texture) => Some(texture),
        Err(error) => {
            eprintln!("snow-launcher: could not load the {name} illustration: {error}");
            None
        }
    }
}

fn main() {
    RelmApp::new(APP_ID).run::<App>(());
}
