// SPDX-License-Identifier: MIT
//! Development helpers, behind feature `dev`. An app calls them only under `cfg(debug_assertions)`.

use std::path::Path;

use gtk::{gdk, graphene, prelude::*};

/// Writes `widget` as it is drawn now to a PNG at `path`, for before-and-after pictures of a window without a
/// compositor screenshot. The widget must be realized and laid out. The popovers open in it (a menu, a drop-down's
/// list) are drawn over it where they are, as far as they are inside it: each is a surface of its own, which the
/// widget's own picture leaves out.
/// A spinner that has just come on screen is not in the picture yet: GTK fades it in from nothing over half a second.
pub fn shot(widget: &impl IsA<gtk::Widget>, path: &Path) -> Result<(), String> {
    let widget = widget.as_ref();
    let (width, height) = (widget.width() as f32, widget.height() as f32);
    let native = widget.native().ok_or("the widget is not realized")?;
    let renderer = native.renderer().ok_or("the widget is not realized")?;
    let snapshot = gtk::Snapshot::new();
    gtk::WidgetPaintable::new(Some(widget)).snapshot(&snapshot, width.into(), height.into());
    // Where the widget is on its surface: a popover's place is known from there.
    let (left, top) = native.surface_transform();
    let origin = widget
        .compute_point(&native, &graphene::Point::zero())
        .map_or((left, top), |at| (left + f64::from(at.x()), top + f64::from(at.y())));
    let mut popovers = Vec::new();
    open_popovers(widget, &mut popovers);
    for popover in popovers {
        let Some((x, y)) = place(&popover, native.surface().as_ref()) else {
            continue;
        };
        // A popover has no picture of its own, as a window has: what it shows is its children's.
        let mut child = popover.first_child();
        while let Some(inner) = child {
            if let Some(bounds) = inner.compute_bounds(&popover).filter(|_| inner.is_mapped()) {
                snapshot.save();
                snapshot.translate(&graphene::Point::new(
                    (x - origin.0) as f32 + bounds.x(),
                    (y - origin.1) as f32 + bounds.y(),
                ));
                let (w, h) = (bounds.width(), bounds.height());
                gtk::WidgetPaintable::new(Some(&inner)).snapshot(&snapshot, w.into(), h.into());
                snapshot.restore();
            }
            child = inner.next_sibling();
        }
    }
    let node = snapshot.to_node().ok_or("the widget drew nothing")?;
    let texture: gdk::Texture = renderer.render_texture(&node, Some(&graphene::Rect::new(0.0, 0.0, width, height)));
    texture.save_to_png(path).map_err(|e| e.to_string())
}

/// The popovers on screen in `widget`, those under others first.
fn open_popovers(widget: &gtk::Widget, found: &mut Vec<gtk::Widget>) {
    let mut child = widget.first_child();
    while let Some(inner) = child {
        if inner.is::<gtk::Popover>() && inner.is_mapped() {
            found.push(inner.clone());
        }
        open_popovers(&inner, found);
        child = inner.next_sibling();
    }
}

/// Where a popover's surface is on the surface `on`, through the popovers it opened from.
fn place(popover: &gtk::Widget, on: Option<&gdk::Surface>) -> Option<(f64, f64)> {
    let mut surface = popover.native()?.surface()?;
    let (mut x, mut y) = (0.0, 0.0);
    while Some(&surface) != on {
        let popup = surface.downcast_ref::<gdk::Popup>()?;
        x += f64::from(popup.position_x());
        y += f64::from(popup.position_y());
        surface = popup.parent()?;
    }
    Some((x, y))
}
