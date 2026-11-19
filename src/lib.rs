// SPDX-License-Identifier: MIT
//! The shared look and widgets of a family of Omarchy desktop apps: plain GTK 4, themed live from Omarchy's current
//! theme, without libadwaita. See README.md.

#[cfg(feature = "dev")]
pub mod dev;
mod dialog;
pub mod icons;
mod shell;
pub mod theme;
mod widgets;

pub use gtk;
use gtk::{gdk, gio, glib, prelude::*};

pub use dialog::{Appearance, Dialog, WeakDialog, confirm};
pub use icons::{data_uri, icon, icon_button};
pub use shell::{Shell, Title, claim_titlebar, header};
pub use theme::{Theme, Tokens, blend, omarchy_state_dir};
pub use widgets::{Banner, EmptyState, badge, clear, label, set_class, sidebar_row};

/// The application `id`, named `name` as its command line is (logs, X11 WM_CLASS) and shown as `title`. Its windows
/// are styled by omui's stylesheet and then `app_css`. It has one window: `open` builds and presents it on the first
/// activation, and a later launch presents it again.
///
/// Call it first thing in `main`: it takes `GTK_THEME` out of the environment.
pub fn application(
    id: &str,
    name: &str,
    title: &str,
    app_css: &'static str,
    open: impl Fn(&gtk::Application) + 'static,
) -> gtk::Application {
    // GTK_THEME loads that theme whatever the settings say, so the empty theme `Theme::install` pins would not hold
    // (Strata drops it the same way). GTK reads it once it starts, in `run`.
    // SAFETY: the caller is at the start of `main`, before any thread that could be reading the environment.
    unsafe { std::env::remove_var("GTK_THEME") };
    glib::set_prgname(Some(name));
    glib::set_application_name(title);
    let app = gtk::Application::builder().application_id(id).build();
    app.connect_startup(move |_| {
        if let Some(display) = gdk::Display::default() {
            Theme::install(&display, app_css);
        }
    });
    app.connect_activate(move |app| match app.active_window() {
        Some(window) => window.present(),
        None => open(app),
    });
    app
}

/// `main` of a binary that is both a window and a command line: with no arguments it runs the application `app`
/// builds, and with any it runs `cli`, whose result is the exit status.
pub fn main(cli: fn() -> u8, app: impl FnOnce() -> gtk::Application) -> ! {
    if std::env::args_os().len() > 1 {
        std::process::exit(cli().into());
    }
    std::process::exit(app().run().get().into());
}

/// Runs `work` on a worker thread and hands its result to `done` on the main loop. Network, file and database calls
/// go through here, never the main loop. `done` gets an error when `work` panicked.
pub fn background<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(Result<T, String>) + 'static,
) {
    glib::spawn_future_local(async move {
        done(
            gio::spawn_blocking(work)
                .await
                .map_err(|_| "the background task crashed".to_owned()),
        );
    });
}
