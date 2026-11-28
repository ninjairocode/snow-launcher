//! Turning images into textures at the size they are shown, so nothing
//! is stretched (a stretched bitmap is what looks blurry).

use std::path::Path;

use relm4::gtk::prelude::*;
use relm4::gtk::{gdk, gdk_pixbuf, gio, glib};

/// Rasterizes an embedded SVG to `height` device pixels (width follows).
pub fn svg_texture(name: &str, bytes: &'static [u8], height: i32) -> Option<gdk::Texture> {
    let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_static(bytes));
    match gdk_pixbuf::Pixbuf::from_stream_at_scale(
        &stream,
        -1,
        height,
        true,
        gio::Cancellable::NONE,
    ) {
        Ok(pixbuf) => texture(&pixbuf),
        Err(error) => {
            // A missing SVG loader (librsvg) must not take the window down.
            eprintln!("snow-launcher: could not load the {name} illustration: {error}");
            None
        }
    }
}

/// Loads an image file (a game icon: PNG or ICO) scaled to fit `size`
/// device pixels. `None` if it cannot be read: the row just has no icon.
pub fn file_texture(path: &Path, size: i32) -> Option<gdk::Texture> {
    let pixbuf = gdk_pixbuf::Pixbuf::from_file_at_scale(path, size, size, true).ok()?;
    texture(&pixbuf)
}

fn texture(pixbuf: &gdk_pixbuf::Pixbuf) -> Option<gdk::Texture> {
    let format = if pixbuf.has_alpha() {
        gdk::MemoryFormat::R8g8b8a8
    } else {
        gdk::MemoryFormat::R8g8b8
    };
    let stride = usize::try_from(pixbuf.rowstride()).ok()?;
    let texture = gdk::MemoryTexture::new(
        pixbuf.width(),
        pixbuf.height(),
        format,
        &pixbuf.read_pixel_bytes(),
        stride,
    );
    Some(texture.upcast())
}
