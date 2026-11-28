//! The "Import a game" dialog: games found in other launchers, or a folder
//! the user picks, copied into Snow's library by `snowlauncher_wine::import`
//! (the original is never changed). Battle.net does not find a copied game
//! by itself (O17), so a successful import ends with the "Locate the game"
//! path. Finding and copying run off the window's thread.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use relm4::adw::prelude::*;
use relm4::gtk::{gio, glib};
use relm4::{adw, gtk};
use snowlauncher_core::import::locate_path;
use snowlauncher_wine::house::dir_size;
use snowlauncher_wine::import::{FoundGame, find_import_sources, import_game};

// TODO(i18n): translate (pt-BR) once gettext is in place (v0.6).

/// What the dialog shares with its callbacks.
struct Context {
    library: PathBuf,
    list: gtk::ListBox,
    toasts: adw::ToastOverlay,
    on_imported: Box<dyn Fn()>,
}

impl Context {
    fn toast(&self, text: String, stays: bool) {
        let toast = adw::Toast::builder()
            .title(text)
            .use_markup(false)
            .timeout(if stays { 0 } else { 8 })
            .build();
        self.toasts.add_toast(toast);
    }

    /// Copies `source` on a worker thread; the list is off meanwhile.
    fn import(self: &Rc<Self>, source: PathBuf, done: Option<adw::ActionRow>) {
        self.list.set_sensitive(false);
        self.toast(format!("Importing {}…", source.display()), false);
        let context = Rc::clone(self);
        glib::spawn_future_local(async move {
            let library = context.library.clone();
            let result = gio::spawn_blocking(move || {
                import_game(&source, &library).map_err(|error| error.to_string())
            })
            .await
            .unwrap_or_else(|_| Err("the import stopped unexpectedly".to_owned()));
            context.list.set_sensitive(true);
            match result {
                Ok(imported) => {
                    let name = imported
                        .target
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let how = if imported.reflink {
                        "instant, no extra space"
                    } else {
                        "copied"
                    };
                    context.toast(
                        format!(
                            "{name} imported ({how}). In Battle.net, open it and use \
                             \"Locate the game\" with {}",
                            locate_path(&name)
                        ),
                        true,
                    );
                    if let Some(row) = done {
                        row.set_subtitle("Imported into Snow's library");
                        row.set_activatable(false);
                        if let Some(button) = row.activatable_widget() {
                            button.set_visible(false);
                        }
                    }
                    (context.on_imported)();
                }
                Err(problem) => context.toast(problem, true),
            }
        });
    }
}

fn game_row(context: &Rc<Context>, game: &FoundGame, gb: u64) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(game.name.as_str())
        .subtitle(format!("{gb} GB · {}", game.path.display()))
        .use_markup(false)
        .build();
    let button = gtk::Button::builder()
        .label("Import")
        .valign(gtk::Align::Center)
        .css_classes(["suggested-action"])
        .build();
    row.add_suffix(&button);
    row.set_activatable_widget(Some(&button));
    let (context, path, row_ref) = (Rc::clone(context), game.path.clone(), row.clone());
    button.connect_clicked(move |_| context.import(path.clone(), Some(row_ref.clone())));
    row
}

fn message_row(text: &str, spinning: bool) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(text)
        .use_markup(false)
        .build();
    if spinning {
        let spinner = gtk::Spinner::new();
        spinner.start();
        row.add_prefix(&spinner);
    }
    row
}

/// Looks for games in other launchers and fills the list.
fn fill(context: &Rc<Context>) {
    let loading = message_row("Looking for games in other launchers…", true);
    context.list.append(&loading);
    let context = Rc::clone(context);
    glib::spawn_future_local(async move {
        let found = gio::spawn_blocking(|| {
            find_import_sources()
                .into_iter()
                .map(|game| {
                    let gb = dir_size(&game.path) / 1_000_000_000;
                    (game, gb)
                })
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        context.list.remove(&loading);
        if found.is_empty() {
            context.list.append(&message_row(
                "No Blizzard games found in other launchers. Choose a folder instead.",
                false,
            ));
        }
        for (game, gb) in &found {
            context.list.append(&game_row(&context, game, *gb));
        }
    });
}

/// Builds the dialog for the house's `library`. `on_imported` runs after
/// each successful import (to refresh the window's game list).
pub fn dialog(library: &Path, on_imported: impl Fn() + 'static) -> adw::Dialog {
    let list = gtk::ListBox::builder()
        .css_classes(["boxed-list"])
        .selection_mode(gtk::SelectionMode::None)
        .build();
    let intro = gtk::Label::builder()
        .label(
            "Copies a game from another launcher into Snow's library. \
             The original is never changed and keeps working there.",
        )
        .wrap(true)
        .xalign(0.0)
        .css_classes(["dim-label"])
        .build();
    let choose = gtk::Button::builder()
        .label("Choose a folder…")
        .halign(gtk::Align::Center)
        .css_classes(["pill"])
        .build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    content.append(&intro);
    content.append(&list);
    content.append(&choose);
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .child(&content)
        .build();
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&scrolled));
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&toasts));
    let dialog = adw::Dialog::builder()
        .title("Import a game")
        .content_width(560)
        .child(&toolbar)
        .build();

    let context = Rc::new(Context {
        library: library.to_owned(),
        list,
        toasts,
        on_imported: Box::new(on_imported),
    });
    fill(&context);

    let picker_context = Rc::clone(&context);
    let dialog_ref = dialog.downgrade();
    choose.connect_clicked(move |_| {
        let picker = gtk::FileDialog::builder()
            .title("Choose the game folder")
            .modal(true)
            .build();
        let parent = dialog_ref
            .upgrade()
            .and_then(|dialog| dialog.root())
            .and_then(|root| root.downcast::<gtk::Window>().ok());
        let context = Rc::clone(&picker_context);
        picker.select_folder(parent.as_ref(), gio::Cancellable::NONE, move |picked| {
            if let Some(path) = picked.ok().and_then(|folder| folder.path()) {
                context.import(path, None);
            }
        });
    });
    dialog
}
