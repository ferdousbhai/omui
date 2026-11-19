// SPDX-License-Identifier: MIT
//! What surrounds a window's content: the overlay that carries toasts, the header bar, and its title.

use std::{cell::RefCell, rc::Rc};

use gtk::{glib, prelude::*};

use crate::icons;

/// What a toast's button runs.
type Action = Rc<dyn Fn()>;

/// How long a plain toast stays, as libadwaita's does.
const TOAST_SECONDS: u32 = 5;

/// A window's content under an overlay (class `om-shell`) that shows toasts above it.
pub struct Shell {
    pub overlay: gtk::Overlay,
    toast: gtk::Box,
    toast_label: gtk::Label,
    toast_button: gtk::Button,
    toast_action: RefCell<Option<Action>>,
    toast_timer: RefCell<Option<glib::SourceId>>,
}

impl Shell {
    /// Makes `content` the child of `window`, under the overlay.
    pub fn new(window: &impl IsA<gtk::Window>, content: &impl IsA<gtk::Widget>) -> Rc<Shell> {
        let overlay = gtk::Overlay::new();
        overlay.add_css_class("om-shell");
        overlay.set_child(Some(content));

        let toast_label = gtk::Label::builder().wrap(true).xalign(0.0).max_width_chars(60).build();
        let toast_button = gtk::Button::new();
        toast_button.add_css_class("flat");
        toast_button.add_css_class("accent");
        let close = icons::icon_button(icons::name::CLOSE, "Dismiss");
        // An alert, so a screen reader announces the text when the toast appears.
        let toast = gtk::Box::builder()
            .accessible_role(gtk::AccessibleRole::Alert)
            .css_classes(["toast"])
            .halign(gtk::Align::Center)
            .valign(gtk::Align::End)
            .visible(false)
            .build();
        toast.append(&toast_label);
        toast.append(&toast_button);
        toast.append(&close);
        overlay.add_overlay(&toast);
        window.set_child(Some(&overlay));

        let shell = Rc::new(Shell {
            overlay,
            toast,
            toast_label,
            toast_button,
            toast_action: RefCell::new(None),
            toast_timer: RefCell::new(None),
        });
        let weak = Rc::downgrade(&shell);
        close.connect_clicked(move |_| {
            if let Some(shell) = weak.upgrade() {
                shell.dismiss();
            }
        });
        let weak = Rc::downgrade(&shell);
        shell.toast_button.connect_clicked(move |_| {
            let Some(shell) = weak.upgrade() else { return };
            let action = shell.toast_action.borrow().clone();
            shell.dismiss();
            if let Some(action) = action {
                action();
            }
        });
        shell
    }

    /// Shows `text` (plain, not markup) at the bottom of the window for a few seconds, in place of any toast showing.
    pub fn toast(self: &Rc<Self>, text: &str) {
        self.show(text, None, TOAST_SECONDS);
    }

    /// A toast with a button labelled `action`, for `seconds`: an undo, a retry. `f` runs when the button is pressed.
    pub fn toast_action(self: &Rc<Self>, text: &str, action: &str, seconds: u32, f: impl Fn() + 'static) {
        self.show(text, Some((action, Rc::new(f))), seconds);
    }

    fn show(self: &Rc<Self>, text: &str, action: Option<(&str, Action)>, seconds: u32) {
        self.dismiss();
        self.toast_label.set_label(text);
        self.toast_button.set_visible(action.is_some());
        if let Some((label, f)) = action {
            self.toast_button.set_label(label);
            self.toast_action.replace(Some(f));
        }
        self.toast.set_visible(true);
        let weak = Rc::downgrade(self);
        let timer = glib::timeout_add_seconds_local_once(seconds, move || {
            if let Some(shell) = weak.upgrade() {
                // This timer has run, so there is nothing left of it to remove.
                shell.toast_timer.take();
                shell.dismiss();
            }
        });
        self.toast_timer.replace(Some(timer));
    }

    fn dismiss(&self) {
        self.toast.set_visible(false);
        self.toast_action.take();
        if let Some(timer) = self.toast_timer.take() {
            timer.remove();
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
/// `title` it shows the window's own. It serves as a window's title bar and, with [`claim_titlebar`], inside content.
pub fn header(title: Option<&gtk::Widget>) -> gtk::HeaderBar {
    let header = gtk::HeaderBar::new();
    header.set_show_title_buttons(false);
    header.set_title_widget(title);
    header
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
