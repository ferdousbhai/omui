// SPDX-License-Identifier: MIT
//! Development helpers, behind feature `dev`. An app calls them only under `cfg(debug_assertions)`.

use std::path::Path;

use gtk::{gdk, graphene, prelude::*};

/// Writes `widget` as it is drawn now to a PNG at `path`, for before-and-after pictures of a window without a
/// compositor screenshot. The widget must be realized and laid out.
pub fn shot(widget: &impl IsA<gtk::Widget>, path: &Path) -> Result<(), String> {
    let widget = widget.as_ref();
    let (width, height) = (widget.width() as f32, widget.height() as f32);
    let renderer = widget
        .native()
        .and_then(|native| native.renderer())
        .ok_or("the widget is not realized")?;
    let snapshot = gtk::Snapshot::new();
    gtk::WidgetPaintable::new(Some(widget)).snapshot(&snapshot, width.into(), height.into());
    let node = snapshot.to_node().ok_or("the widget drew nothing")?;
    let texture: gdk::Texture = renderer.render_texture(&node, Some(&graphene::Rect::new(0.0, 0.0, width, height)));
    texture.save_to_png(path).map_err(|e| e.to_string())
}
