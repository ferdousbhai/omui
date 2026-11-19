// SPDX-License-Identifier: MIT
//! The bundled icons: Lucide's geometry with each stroke outlined as a fill (dev/outline), so GTK recolours them from
//! the CSS `color` like any symbolic icon. The `om-` prefix keeps the desktop's icon theme from shadowing one.

use std::sync::Once;

use gtk::{gio, glib, prelude::*};

/// Where the gresource holds the icons, in the layout an icon theme expects.
pub(crate) const RESOURCE_PATH: &str = "/io/github/ferdousbhai/omui/icons";

/// Icon names, for [`icon`], [`icon_button`], [`data_uri`] and `gtk::Image::from_icon_name`.
pub mod name {
    pub const ADD: &str = "om-plus-symbolic";
    pub const BACK: &str = "om-chevron-left-symbolic";
    pub const BOOK: &str = "om-book-open-symbolic";
    pub const CALENDAR: &str = "om-calendar-symbolic";
    pub const CHECK: &str = "om-check-symbolic";
    pub const CHECKED: &str = "om-square-check-symbolic";
    pub const CLOCK: &str = "om-clock-symbolic";
    pub const CLOSE: &str = "om-x-symbolic";
    pub const CLOUD: &str = "om-cloud-symbolic";
    pub const COPY: &str = "om-copy-symbolic";
    pub const DELETE: &str = "om-trash-2-symbolic";
    pub const DOCUMENT: &str = "om-file-text-symbolic";
    pub const DOWN: &str = "om-chevron-down-symbolic";
    pub const EDIT: &str = "om-pencil-symbolic";
    pub const EXTERNAL_LINK: &str = "om-external-link-symbolic";
    pub const FIT: &str = "om-maximize-symbolic";
    pub const FOLDER: &str = "om-folder-symbolic";
    pub const FORWARD: &str = "om-chevron-right-symbolic";
    pub const IMAGE: &str = "om-image-symbolic";
    pub const INFO: &str = "om-info-symbolic";
    pub const LOCATION: &str = "om-map-pin-symbolic";
    pub const MAIL: &str = "om-mail-symbolic";
    pub const OFFLINE: &str = "om-cloud-off-symbolic";
    pub const OK: &str = "om-circle-check-symbolic";
    pub const REFRESH: &str = "om-refresh-cw-symbolic";
    pub const REMOVE: &str = "om-minus-symbolic";
    pub const SEARCH: &str = "om-search-symbolic";
    pub const SETTINGS: &str = "om-settings-symbolic";
    pub const SIDEBAR: &str = "om-panel-left-symbolic";
    pub const STAR: &str = "om-star-symbolic";
    pub const STARRED: &str = "om-star-filled-symbolic";
    pub const UP: &str = "om-chevron-up-symbolic";
    pub const USER: &str = "om-user-symbolic";
    pub const VIDEO: &str = "om-video-symbolic";
    pub const WARNING: &str = "om-triangle-alert-symbolic";
    pub const ZOOM_IN: &str = "om-zoom-in-symbolic";
    pub const ZOOM_OUT: &str = "om-zoom-out-symbolic";

    /// Every bundled icon.
    pub const ALL: &[&str] = &[
        ADD,
        BACK,
        BOOK,
        CALENDAR,
        CHECK,
        CHECKED,
        CLOCK,
        CLOSE,
        CLOUD,
        COPY,
        DELETE,
        DOCUMENT,
        DOWN,
        EDIT,
        EXTERNAL_LINK,
        FIT,
        FOLDER,
        FORWARD,
        IMAGE,
        INFO,
        LOCATION,
        MAIL,
        OFFLINE,
        OK,
        REFRESH,
        REMOVE,
        SEARCH,
        SETTINGS,
        SIDEBAR,
        STAR,
        STARRED,
        UP,
        USER,
        VIDEO,
        WARNING,
        ZOOM_IN,
        ZOOM_OUT,
    ];
}

/// Makes the gresource compiled into this crate (build.rs) known to GIO.
pub(crate) fn register() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| gio::resources_register_include!("omui.gresource").expect("omui's bundled resources"));
}

/// A decorative icon, drawn in the CSS `color` of where it sits. One that carries meaning alone belongs in an
/// [`icon_button`] or next to a label.
pub fn icon(name: &str) -> gtk::Image {
    gtk::Image::builder()
        .icon_name(name)
        .accessible_role(gtk::AccessibleRole::Presentation)
        .build()
}

/// A borderless icon button, as in a header bar. `tooltip` is also its accessible name.
pub fn icon_button(name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(name);
    button.add_css_class("header-action");
    button.set_tooltip_text(Some(tooltip));
    button.update_property(&[gtk::accessible::Property::Label(tooltip)]);
    button
}

/// A bundled icon as a `data:` URI drawn in the colour `hex`, for a WebKit page whose policy allows `img-src data:`.
///
/// # Panics
/// When `name` is not a bundled icon.
pub fn data_uri(name: &str, hex: &str) -> String {
    register();
    let path = format!("{RESOURCE_PATH}/scalable/actions/{name}.svg");
    let bytes = gio::resources_lookup_data(&path, gio::ResourceLookupFlags::NONE)
        .unwrap_or_else(|_| panic!("omui bundles no icon named {name}"));
    let svg = String::from_utf8_lossy(&bytes).replace(SYMBOLIC_FILL, hex);
    format!("data:image/svg+xml;base64,{}", glib::base64_encode(svg.as_bytes()))
}

/// The foreground colour the bundled files are written in, which GTK replaces when it recolours a symbolic icon.
const SYMBOLIC_FILL: &str = "#2e3436";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_named_icon_is_bundled_and_recolours() {
        for icon in name::ALL {
            let uri = data_uri(icon, "#abcdef");
            let svg = glib::base64_decode(uri.strip_prefix("data:image/svg+xml;base64,").unwrap());
            let svg = String::from_utf8(svg).unwrap();
            assert!(
                svg.contains("fill=\"#abcdef\"") && !svg.contains(SYMBOLIC_FILL),
                "{icon}"
            );
            assert!(
                !svg.contains("stroke="),
                "{icon} must be filled outlines: GTK recolours only fills"
            );
        }
    }
}
