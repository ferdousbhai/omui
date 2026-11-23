// SPDX-License-Identifier: MIT
//! The shared look and widgets of a family of Omarchy desktop apps: plain GTK 4, themed live from Omarchy's current
//! theme, without libadwaita. See README.md.

#[cfg(feature = "dev")]
pub mod dev;
mod dialog;
pub mod i18n;
pub mod icons;
mod json;
mod keys;
mod layout;
pub mod picker;
#[cfg(feature = "qa")]
pub mod qa;
mod rounding;
mod rows;
mod shell;
#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;
pub mod theme;
mod widgets;

pub use gtk;
use gtk::{gdk, gio, glib, prelude::*};

pub use dialog::{Appearance, Dialog, WeakDialog, about, confirm, confirmf, dialog_open};
pub use i18n::{Translator, tr, trf, trn, trnf};
pub use icons::{data_uri, icon, icon_button, icon_menu_button};
pub use keys::{Keys, ShortcutOverlay, ShortcutSection, on_key, quit_keys, text_has_focus};
pub use layout::{Nav, Split, WeakNav, WeakSplit, clamp, on_narrow, page, tabs};
pub use picker::{
    FakeFilePicker, FileFilter, FilePicker, PickDetail, PickError, PickOptions, Picked, PortalFilePicker,
};
pub use rounding::Rounding;
pub use rows::{ExpanderRow, Group, Row, combo_row, entry_row, password_row, switch_row};
pub use shell::{Shell, Title, Toast, claim_titlebar, header, header_without_title};
pub use theme::{APP_PRIORITY, Colors, Theme, Tokens, blend, omarchy_state_dir};
pub use widgets::{Banner, EmptyState, avatar, badge, clear, label, set_class, sidebar_row, single_tab_stop, spinner};

/// The application `id`, named `name` as its command line is (logs, X11 WM_CLASS) and shown as `title`. Its windows
/// are styled by omui's stylesheet and then `app_css`. It has one window: `open` builds and presents it on the first
/// activation, and a later launch presents it again.
///
/// `id` is the dotted name the application has on D-Bus (`com.example.Notes`); `name` is the one short lowercase
/// name Omarchy knows the app by (`notes`), which is also its binary's and its `notes.desktop`'s.
///
/// **`name` becomes the Wayland app id of every window of the application**, so that Hyprland and the launcher match
/// it to the desktop entry: GTK would give it the dotted id. A window made with `.application(app)`,
/// `set_application` or `add_window` is named as it joins, by [`name_window`]; a window the app keeps out of the
/// application is named by calling [`name_window`] on it. An app needs no `gdk4-wayland` of its own for this.
///
/// Call it first thing in `main`, after registering the app's catalogs ([`i18n::register_all`]): it takes
/// `GTK_THEME` out of the environment. The application follows Omarchy's theme, Hyprland's rounding and Omarchy's
/// language from when it starts.
pub fn application(
    id: &str,
    name: &str,
    title: &str,
    app_css: &'static str,
    open: impl Fn(&gtk::Application) + 'static,
) -> gtk::Application {
    let app = build(id, name, title, app_css, gio::ApplicationFlags::empty());
    app.connect_activate(move |app| match app.active_window() {
        Some(window) => window.present(),
        None => open(app),
    });
    app
}

/// [`application`] for an app that is also handed files or URIs to open (GApplication's `HANDLES_OPEN`): a mail app
/// given a `mailto:` link, a viewer given a file from the file manager or `xdg-open`. `open` builds and presents the
/// window on an activation, as [`application`]'s does, and `open_files` runs when the app is started, or started
/// again, with files or URIs: what was asked to be opened is in `files` (a URI's `gio::File` gives it back by
/// `uri()`), and no activation comes with it, so it makes the window too when there is none yet. Everything else
/// is [`application`]'s: `name` as every window's Wayland app id, the theme, the language, scripted QA.
///
/// ```ignore
/// let app = omui::application_opening(APP_ID, "mail", "Mail", CSS, |app| window(app).present(), |app, files| {
///     let window = window(app);
///     window.present();
///     for file in files {
///         if file.uri().starts_with("mailto:") {
///             compose(&window, &file.uri());
///         }
///     }
/// });
/// ```
pub fn application_opening(
    id: &str,
    name: &str,
    title: &str,
    app_css: &'static str,
    open: impl Fn(&gtk::Application) + 'static,
    open_files: impl Fn(&gtk::Application, &[gio::File]) + 'static,
) -> gtk::Application {
    let app = build(id, name, title, app_css, gio::ApplicationFlags::HANDLES_OPEN);
    app.connect_activate(move |app| match app.active_window() {
        Some(window) => window.present(),
        None => open(app),
    });
    app.connect_open(move |app, files, _hint| open_files(app, files));
    app
}

/// What [`application`] and [`application_opening`] share: the application with `flags`, without its handlers.
fn build(id: &str, name: &str, title: &str, app_css: &'static str, flags: gio::ApplicationFlags) -> gtk::Application {
    // GTK_THEME loads that theme whatever the settings say, so the empty theme `Theme::install` pins would not hold
    // (Strata drops it the same way). GTK reads it once it starts, in `run`.
    // SAFETY: the caller is at the start of `main`, before any thread that could be reading the environment.
    unsafe { std::env::remove_var("GTK_THEME") };
    glib::set_prgname(Some(name));
    glib::set_application_name(title);
    let app = gtk::Application::builder().application_id(id).flags(flags).build();
    name_windows(&app);
    // Before GTK starts, so that what it logs from the start is counted.
    #[cfg(feature = "qa")]
    qa::attach(&app);
    app.connect_startup(move |_| {
        if let Some(display) = gdk::Display::default() {
            Theme::install(&display, app_css);
            // A selectable label (a fingerprint, a status line) is often the first focus stop; GTK would select all of
            // it as a window opens, which reads as a stray highlight. Selecting stays the reader's choice.
            gtk::Settings::for_display(&display).set_gtk_label_select_on_focus(false);
        }
        // After GTK has started: the language decides which way the windows read.
        Translator::install();
    });
    app
}

/// Gives `window` the program's name ([`glib::prgname`], which [`application`] sets to the app's short name) as its
/// Wayland app id each time it is mapped, from now on. [`application`] does this for every window added to the
/// application, and omui for the windows it makes; an app calls it for a window of its own that it keeps out of the
/// application. On another backend (X11, broadway) it does nothing.
///
/// GTK chooses the app id itself as it maps a window: the GApplication's id for a window of the application, which
/// must have a dot in it, and the program's name only for a window outside it. It does so on every map, since the
/// compositor's toplevel is made anew each time, and after `realize`; the `map` handlers run after it, so the name
/// set here is the one the compositor keeps. Calling it twice for a window is harmless.
pub fn name_window(window: &impl IsA<gtk::Window>) {
    let window = window.as_ref();
    // SAFETY: the key is this function's alone, and only ever holds `()`.
    if unsafe { window.data::<()>(NAMED) }.is_some() {
        return;
    }
    unsafe { window.set_data(NAMED, ()) };
    window.connect_map(set_app_id);
    // A window handed over already on screen is named now, as well as on its next map.
    if window.is_mapped() {
        set_app_id(window);
    }
}

fn set_app_id(window: &gtk::Window) {
    if let Some(name) = glib::prgname()
        && let Some(toplevel) = window.surface().and_downcast::<gdk_wayland::WaylandToplevel>()
    {
        toplevel.set_application_id(&name);
    }
}

/// [`name_window`] for every window added to `app`. A window joins the application before it is first mapped,
/// whichever way it was made (a builder's `application`, `set_application`, `add_window`).
pub(crate) fn name_windows(app: &gtk::Application) {
    app.connect_window_added(|_, window| name_window(window));
}

/// The mark [`name_window`] leaves on a window it has seen to.
const NAMED: &str = "omui-name-window";

/// Whether [`name_window`] has seen to `window`.
#[cfg(test)]
pub(crate) fn is_named(window: &impl IsA<gtk::Window>) -> bool {
    // SAFETY: as in `name_window`.
    unsafe { window.as_ref().data::<()>(NAMED) }.is_some()
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
///
/// The application is held ([`hold`]) until `done` has run: closing the last window while a save or an upload is on
/// its way does not end the process under it. The app stays, windowless, until the work is done, and then goes.
pub fn background<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(Result<T, String>) + 'static,
) {
    let held = hold();
    glib::spawn_future_local(async move {
        done(
            gio::spawn_blocking(work)
                .await
                .map_err(|_| i18n::tr("the background task crashed")),
        );
        drop(held);
    });
}

/// The application kept running until this is dropped, with or without a window: what work that must finish holds
/// on to. Made by [`hold`].
#[must_use = "the application is only held until this is dropped"]
pub struct Hold(#[allow(dead_code)] Option<gio::ApplicationHoldGuard>);

/// Keeps the application running until the [`Hold`] returned is dropped, on the main thread. GTK ends an
/// application when its last window closes, whatever is still running; [`background`] holds it by itself, and an
/// app that starts work another way (`gio::spawn_blocking`, a `gio::Subprocess`, a future on the main loop) takes a
/// hold as the work starts and drops it when the work has ended:
///
/// ```ignore
/// let held = omui::hold();
/// glib::spawn_future_local(async move {
///     let result = gio::spawn_blocking(move || export(&file)).await;
///     report(result);
///     drop(held);
/// });
/// ```
///
/// With no application (a command line, a test) there is nothing to hold and the hold does nothing.
pub fn hold() -> Hold {
    Hold(gio::Application::default().map(|application| application.hold()))
}

#[cfg(test)]
mod background_tests {
    use std::{cell::Cell, rc::Rc, time::Duration};

    use super::*;

    /// An application with no window ends as soon as it has been activated, unless something holds it.
    #[test]
    fn work_in_the_background_keeps_the_application_until_it_is_done() {
        let _main = testing::main_loop();
        let application = gio::Application::new(None, gio::ApplicationFlags::NON_UNIQUE);
        application.set_default();
        let (done, held) = (Rc::new(Cell::new(None)), Rc::new(Cell::new(None)));
        application.connect_activate({
            let (done, held) = (done.clone(), held.clone());
            move |_| {
                let done = done.clone();
                background(
                    || {
                        std::thread::sleep(Duration::from_millis(300));
                        42
                    },
                    move |result| done.set(Some(result)),
                );
                // A hold of the app's own, for work it started another way, dropped a while after.
                let hold = hold();
                let held = held.clone();
                glib::timeout_add_local_once(Duration::from_millis(600), move || {
                    held.set(Some(hold.0.is_some()));
                    drop(hold);
                });
            }
        });
        let started = std::time::Instant::now();
        application.run_with_args::<&str>(&[]);
        assert_eq!(
            done.take(),
            Some(Ok(42)),
            "the work was let finish, and its result delivered"
        );
        assert_eq!(held.take(), Some(true), "and so was what the app held on for");
        assert!(started.elapsed() >= Duration::from_millis(600));
        // Without an application there is nothing to hold, and work runs all the same.
        drop(application);
        assert!(hold().0.is_none());
        let done = Rc::new(Cell::new(None));
        background(|| 7, {
            let done = done.clone();
            move |result| done.set(Some(result))
        });
        assert!(testing::spin_until(5000, || {
            let result = done.take();
            done.set(result.clone());
            result.is_some()
        }));
        assert_eq!(done.take(), Some(Ok(7)));
    }
}
