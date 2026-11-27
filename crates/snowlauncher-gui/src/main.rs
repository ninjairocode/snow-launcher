//! `snow-launcher`: Snow Launcher's window (v0.3, first step).
//!
//! It shows what `snowd` sees (over D-Bus) and the state of the house,
//! with the doghouse illustrations. Actions (open Battle.net, play) come
//! next. Layout rules from experiment E7: `adw::ToolbarView`, an
//! illustration that fails to load is simply not shown, and `adw::Clamp`
//! falls back to pixels when the desktop publishes no text DPI.

mod state;

use std::time::Duration;

use relm4::adw::prelude::*;
use relm4::{Component, ComponentParts, ComponentSender, RelmApp, RelmWidgetExt, adw, gtk};
use snowlauncher_wine::daemon_client::query_async;

use crate::state::{House, Illustration, StatusRow, house_text, read_house, status_rows};

const APP_ID: &str = "io.github.ninjairocode.SnowLauncher";
const REFRESH_EVERY: Duration = Duration::from_secs(3);
const STATUS_ROWS: usize = 4;

const EMPTY_SVG: &[u8] = include_bytes!("../../../data/illustrations/empty.svg");
const READY_SVG: &[u8] = include_bytes!("../../../data/illustrations/progress-3.svg");

struct App {
    rows: Vec<StatusRow>,
    house: House,
    empty: Option<gtk::gdk::Texture>,
    ready: Option<gtk::gdk::Texture>,
}

impl App {
    fn illustration(&self) -> Option<&gtk::gdk::Texture> {
        match house_text(&self.house).0 {
            Illustration::Empty => self.empty.as_ref(),
            Illustration::Ready => self.ready.as_ref(),
        }
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
}

#[derive(Debug)]
enum CommandOutput {
    Refreshed { rows: Vec<StatusRow>, house: House },
}

#[relm4::component]
impl Component for App {
    type Init = ();
    type Input = ();
    type Output = ();
    type CommandOutput = CommandOutput;

    view! {
        adw::ApplicationWindow {
            set_title: Some("Snow Launcher"),
            set_default_size: (720, 680),

            #[wrap(Some)]
            set_content = &adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {},

                #[wrap(Some)]
                set_content = &gtk::ScrolledWindow {
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
                                set_label: house_text(&model.house).1,
                            },
                            gtk::Label {
                                add_css_class: "dim-label",
                                set_wrap: true,
                                set_justify: gtk::Justification::Center,
                                #[watch]
                                set_label: &house_text(&model.house).2,
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
                        },
                    },
                },
            },
        }
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let model = App {
            rows: Vec::new(),
            house: read_house(),
            empty: texture("empty.svg", EMPTY_SVG),
            ready: texture("progress-3.svg", READY_SVG),
        };
        let widgets = view_output!();

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
                    loop {
                        let rows = status_rows(&query_async().await);
                        let house = read_house();
                        if out.send(CommandOutput::Refreshed { rows, house }).is_err() {
                            break;
                        }
                        tokio::time::sleep(REFRESH_EVERY).await;
                    }
                })
                .drop_on_shutdown()
        });

        ComponentParts { model, widgets }
    }

    fn update_cmd(&mut self, message: CommandOutput, _: ComponentSender<Self>, _: &Self::Root) {
        match message {
            CommandOutput::Refreshed { rows, house } => {
                self.rows = rows;
                self.house = house;
            }
        }
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
