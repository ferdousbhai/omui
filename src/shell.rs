// SPDX-License-Identifier: MIT
//! What surrounds a window's content: the overlay that carries toasts, the header bar, and its title.

use std::{cell::RefCell, rc::Rc};

use gtk::{glib, prelude::*};

use crate::icons;

/// What a toast runs: its button's action, or what follows its going.
type Action = Rc<dyn Fn()>;

/// How long a toast stays unless it says otherwise, as libadwaita's do.
const TOAST_SECONDS: u32 = 5;

/// A line at the bottom of the window for a few seconds (`adw::Toast`), with an optional button: an undo, a retry,
/// a way to the file just saved. Shown by [`Shell::add_toast`].
pub struct Toast {
    text: String,
    seconds: u32,
    action: Option<(String, Action)>,
    dismissed: Option<Action>,
}

impl Toast {
    /// `text` is plain, not markup.
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            seconds: TOAST_SECONDS,
            action: None,
            dismissed: None,
        }
    }

    /// How long it stays, unless it is dismissed first.
    pub fn seconds(mut self, seconds: u32) -> Self {
        self.seconds = seconds;
        self
    }

    /// A button labelled `label` that runs `f` and then dismisses the toast.
    pub fn action(mut self, label: &str, f: impl Fn() + 'static) -> Self {
        self.action = Some((label.to_owned(), Rc::new(f)));
        self
    }

    /// Runs `f` once the toast has gone, however it went: its time ran out, it was closed, a later toast took its
    /// place, or its button was pressed (after the button's action).
    pub fn on_dismissed(mut self, f: impl Fn() + 'static) -> Self {
        self.dismissed = Some(Rc::new(f));
        self
    }
}

/// A toast on screen.
struct Shown {
    widget: gtk::Box,
    /// Without a button: the next toast takes its place.
    plain: bool,
    timer: Option<glib::SourceId>,
    dismissed: Option<Action>,
}

/// A window's content under an overlay (class `om-shell`) that shows toasts above it, and dialogs ([`crate::Dialog`]).
pub struct Shell {
    pub overlay: gtk::Overlay,
    /// The toasts on screen, oldest first and at the top: a column at the bottom of the window.
    column: gtk::Box,
    shown: RefCell<Vec<Shown>>,
}

impl Shell {
    /// Makes `content` the child of `window`, under the overlay.
    pub fn new(window: &impl IsA<gtk::Window>, content: &impl IsA<gtk::Widget>) -> Rc<Shell> {
        let overlay = gtk::Overlay::new();
        overlay.add_css_class("om-shell");
        overlay.set_child(Some(content));
        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["toasts"])
            .halign(gtk::Align::Center)
            .valign(gtk::Align::End)
            .visible(false)
            .build();
        overlay.add_overlay(&column);
        window.set_child(Some(&overlay));
        Rc::new(Shell {
            overlay,
            column,
            shown: RefCell::default(),
        })
    }

    /// Shows `text` at the bottom of the window for five seconds, in place of any other toast without a button.
    pub fn toast(self: &Rc<Self>, text: &str) {
        self.add_toast(Toast::new(text));
    }

    /// A toast with a button labelled `action`, for `seconds`: an undo, a retry. `f` runs when the button is pressed.
    pub fn toast_action(self: &Rc<Self>, text: &str, action: &str, seconds: u32, f: impl Fn() + 'static) {
        self.add_toast(Toast::new(text).seconds(seconds).action(action, f));
    }

    /// Shows `toast` under those on screen. A toast without a button takes the place of another without one; one
    /// with a button is never put away by a later toast, so its undo stays within reach for as long as it said.
    pub fn add_toast(self: &Rc<Self>, toast: Toast) {
        let label = gtk::Label::builder()
            .label(&toast.text)
            .wrap(true)
            .xalign(0.0)
            .max_width_chars(60)
            .build();
        // An alert, so a screen reader announces the text when the toast appears.
        let widget = gtk::Box::builder()
            .accessible_role(gtk::AccessibleRole::Alert)
            .css_classes(["toast"])
            .halign(gtk::Align::Center)
            .build();
        widget.append(&label);
        let plain = toast.action.is_none();
        if let Some((text, action)) = toast.action {
            let button = gtk::Button::builder()
                .label(text)
                .css_classes(["flat", "accent"])
                .build();
            let (shell, toast) = (Rc::downgrade(self), widget.downgrade());
            button.connect_clicked(move |_| {
                action();
                if let (Some(shell), Some(toast)) = (shell.upgrade(), toast.upgrade()) {
                    shell.dismiss(&toast);
                }
            });
            widget.append(&button);
        }
        let close = icons::icon_button(icons::name::CLOSE, "Dismiss");
        let (shell, toast_widget) = (Rc::downgrade(self), widget.downgrade());
        close.connect_clicked(move |_| {
            if let (Some(shell), Some(toast)) = (shell.upgrade(), toast_widget.upgrade()) {
                shell.dismiss(&toast);
            }
        });
        widget.append(&close);

        if plain {
            let replaced: Vec<gtk::Box> = self
                .shown
                .borrow()
                .iter()
                .filter(|shown| shown.plain)
                .map(|shown| shown.widget.clone())
                .collect();
            for old in replaced {
                self.dismiss(&old);
            }
        }
        let (shell, toast_widget) = (Rc::downgrade(self), widget.downgrade());
        let timer = glib::timeout_add_seconds_local_once(toast.seconds, move || {
            let (Some(shell), Some(toast)) = (shell.upgrade(), toast_widget.upgrade()) else {
                return;
            };
            // This timer has run, so there is nothing left of it to remove.
            if let Some(shown) = shell.shown.borrow_mut().iter_mut().find(|shown| shown.widget == toast) {
                shown.timer = None;
            }
            shell.dismiss(&toast);
        });
        self.column.append(&widget);
        self.column.set_visible(true);
        self.shown.borrow_mut().push(Shown {
            widget,
            plain,
            timer: Some(timer),
            dismissed: toast.dismissed,
        });
    }

    /// Whether a [`crate::Dialog`] is open over this window's content.
    pub fn dialog_open(&self) -> bool {
        crate::dialog::over(&self.overlay)
    }

    /// Takes `toast` off the screen and runs what follows its going. A toast already gone is left alone.
    fn dismiss(&self, toast: &gtk::Box) {
        let shown = {
            let mut shown = self.shown.borrow_mut();
            let Some(index) = shown.iter().position(|shown| shown.widget == *toast) else {
                return;
            };
            shown.remove(index)
        };
        if let Some(timer) = shown.timer {
            timer.remove();
        }
        self.column.remove(toast);
        self.column.set_visible(!self.shown.borrow().is_empty());
        if let Some(dismissed) = shown.dismissed {
            dismissed();
        }
    }
}

/// Registers a hidden title bar, for a window whose header bars sit inside its content (a split view with one per
/// pane). GTK then treats the window as client-decorated and adds no title bar of its own.
pub fn claim_titlebar(window: &impl IsA<gtk::Window>) {
    let placeholder = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    placeholder.set_visible(false);
    window.set_titlebar(Some(&placeholder));
}

/// A header bar without window buttons: on a tiling desktop the compositor closes and arranges windows. With no
/// `title` it shows the window's own; [`header_without_title`] shows none. It serves as a window's title bar and,
/// with [`claim_titlebar`], inside content.
pub fn header(title: Option<&gtk::Widget>) -> gtk::HeaderBar {
    let header = gtk::HeaderBar::new();
    header.set_show_title_buttons(false);
    header.set_title_widget(title);
    header
}

/// A [`header`] with buttons and no title, for the second pane of a split whose first one names the window.
pub fn header_without_title() -> gtk::HeaderBar {
    // GTK shows the window's title in a header bar with no title widget, so it is given an empty one.
    header(Some(gtk::Box::new(gtk::Orientation::Horizontal, 0).upcast_ref()))
}

/// A header bar's title with an optional line under it. An empty subtitle takes no space.
pub struct Title {
    pub widget: gtk::Box,
    title: gtk::Label,
    subtitle: gtk::Label,
}

impl Title {
    pub fn new(title: &str, subtitle: &str) -> Self {
        let line = |class: &str| {
            gtk::Label::builder()
                .css_classes([class])
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .single_line_mode(true)
                .build()
        };
        let this = Self {
            widget: gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .valign(gtk::Align::Center)
                .build(),
            title: line("title"),
            subtitle: line("subtitle"),
        };
        this.widget.append(&this.title);
        this.widget.append(&this.subtitle);
        this.set_title(title);
        this.set_subtitle(subtitle);
        this
    }

    pub fn set_title(&self, title: &str) {
        self.title.set_label(title);
    }

    pub fn set_subtitle(&self, subtitle: &str) {
        self.subtitle.set_label(subtitle);
        self.subtitle.set_visible(!subtitle.is_empty());
    }
}
