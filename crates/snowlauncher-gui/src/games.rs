//! One row per game of the library, with its "Play" button.

use relm4::adw::prelude::*;
use relm4::factory::{DynamicIndex, FactoryComponent, FactorySender};
use relm4::{adw, gtk};

use crate::images::file_texture;
use crate::state::GameCard;

/// Icon size in the row, in logical pixels.
const ICON_SIZE: i32 = 32;
/// Rendered for up to a scale factor of 2, so it stays sharp.
const ICON_PIXELS: i32 = ICON_SIZE * 2;

#[derive(Debug)]
pub struct GameRow {
    card: GameCard,
    icon: Option<gtk::gdk::Texture>,
}

#[derive(Debug)]
pub enum GameRowInput {
    Play,
}

#[relm4::factory(pub)]
impl FactoryComponent for GameRow {
    type Init = GameCard;
    type Input = GameRowInput;
    /// "Play" was clicked: the window runs it.
    type Output = GameCard;
    type CommandOutput = ();
    type ParentWidget = gtk::ListBox;

    view! {
        adw::ActionRow {
            set_use_markup: false,
            set_title: &self.card.name,
            set_subtitle: &self.card.detail,

            add_prefix = &gtk::Image {
                set_pixel_size: ICON_SIZE,
                set_paintable: self.icon.as_ref(),
                set_visible: self.icon.is_some(),
            },

            add_suffix = &gtk::Button {
                set_label: "Play",
                set_valign: gtk::Align::Center,
                add_css_class: "suggested-action",
                set_sensitive: self.card.launch_code.is_some(),
                connect_clicked => GameRowInput::Play,
            },
        }
    }

    fn init_model(card: GameCard, _: &DynamicIndex, _: FactorySender<Self>) -> Self {
        let icon = card
            .icon
            .as_deref()
            .and_then(|path| file_texture(path, ICON_PIXELS));
        Self { card, icon }
    }

    fn update(&mut self, message: GameRowInput, sender: FactorySender<Self>) {
        match message {
            // Fails only if the window is gone; nothing to do then.
            GameRowInput::Play => drop(sender.output(self.card.clone())),
        }
    }
}
