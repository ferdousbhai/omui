// SPDX-License-Identifier: MIT
//! A dialog drawn inside its window, as libadwaita's are: a card over a backdrop on the window's [`crate::Shell`]
//! overlay. A second toplevel would be tiled beside the window on a tiling desktop. The methods are those of
//! `adw::AlertDialog` and `adw::Dialog`.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use gtk::{gdk, gio, glib, prelude::*};

/// How a response's button is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
    Default,
    /// The action the dialog is there for.
    Suggested,
    /// An action that loses something.
    Destructive,
}

type Handler = Rc<dyn Fn(&Dialog, &str)>;

/// The backdrop's class, which is also how a window's open dialogs are found.
const BACKDROP: &str = "dialog-backdrop";

/// The two properties a dialog takes from the rest of its window, each with the class that marks a widget it was
/// taken from. omui.css fades a title bar by the second.
const HELD: [(&str, &str); 2] = [("can-focus", "om-held-can-focus"), ("can-target", "om-held-can-target")];

/// A heading, a body, any widget of the app's, and a row of responses. Every part is optional: with neither heading nor
/// body the dialog is the app's widget alone, edge to edge.
///
/// A response closes the dialog and then runs the handlers. Escape gives the close response, Enter the default one.
/// A dialog stays alive while it is shown, so the handle may be dropped after [`Dialog::present`].
#[derive(Clone)]
pub struct Dialog {
    inner: Rc<Inner>,
}

struct Inner {
    card: gtk::Box,
    heading: Option<gtk::Label>,
    extra: gtk::Box,
    actions: gtk::Box,
    responses: RefCell<Vec<(String, gtk::Button)>>,
    default_response: RefCell<Option<String>>,
    close_response: RefCell<Option<String>>,
    can_close: Cell<bool>,
    handlers: RefCell<Vec<Handler>>,
    /// The backdrop or the window the card is shown in. Weak: the parent window owns it, and its key handler owns
    /// the dialog.
    surface: glib::WeakRef<gtk::Widget>,
    /// What had the keyboard before an in-window dialog took it.
    focus_before: glib::WeakRef<gtk::Widget>,
}

impl Dialog {
    pub fn new(heading: Option<&str>, body: Option<&str>) -> Self {
        let card = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["dialog"])
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .overflow(gtk::Overflow::Hidden)
            .accessible_role(gtk::AccessibleRole::Dialog)
            .build();
        card.update_property(&[gtk::accessible::Property::Modal(true)]);
        let line = |text: &str, class: &str| {
            let label = gtk::Label::builder()
                .label(text)
                .css_classes([class])
                .xalign(0.0)
                .wrap(true)
                .max_width_chars(48)
                .build();
            card.append(&label);
            label
        };
        let heading = heading.map(|text| line(text, "dialog-heading"));
        let body = body.map(|text| line(text, "dialog-body"));
        if let Some(name) = heading.as_ref().or(body.as_ref()) {
            card.update_relation(&[gtk::accessible::Relation::LabelledBy(&[name.upcast_ref()])]);
        }
        if let (Some(_), Some(body)) = (&heading, &body) {
            card.update_relation(&[gtk::accessible::Relation::DescribedBy(&[body.upcast_ref()])]);
        }
        // Homogeneous, so its one child gets the whole width whether or not it asks to expand.
        let extra = gtk::Box::builder()
            .css_classes(["dialog-extra"])
            .homogeneous(true)
            .visible(false)
            .build();
        let actions = gtk::Box::builder()
            .css_classes(["dialog-actions"])
            .visible(false)
            .build();
        card.append(&extra);
        card.append(&actions);

        let dialog = Self {
            inner: Rc::new(Inner {
                card,
                heading,
                extra,
                actions,
                responses: RefCell::new(Vec::new()),
                default_response: RefCell::new(None),
                close_response: RefCell::new(None),
                can_close: Cell::new(true),
                handlers: RefCell::new(Vec::new()),
                surface: glib::WeakRef::new(),
                focus_before: glib::WeakRef::new(),
            }),
        };
        // An entry that activates the default (`set_activates_default`) asks its nearest "default.activate" action:
        // this one, ahead of the window's.
        let default = gio::SimpleAction::new("activate", None);
        let weak = Rc::downgrade(&dialog.inner);
        default.connect_activate(move |_, _| {
            if let Some(dialog) = Dialog::upgrade(&weak) {
                dialog.activate_default();
            }
        });
        let group = gio::SimpleActionGroup::new();
        group.add_action(&default);
        dialog.inner.card.insert_action_group("default", Some(&group));
        // Once the card is on screen, whether presented before its window was or after.
        let weak = Rc::downgrade(&dialog.inner);
        dialog.inner.card.connect_map(move |_| {
            if let Some(dialog) = Dialog::upgrade(&weak) {
                dialog.take_focus();
            }
        });
        dialog
    }

    /// A button at the end of the row, after those already added. `label` may mark a mnemonic with an underscore.
    pub fn add_response(&self, id: &str, label: &str) {
        let button = gtk::Button::builder().label(label).use_underline(true).build();
        let weak = Rc::downgrade(&self.inner);
        let response = id.to_owned();
        button.connect_clicked(move |_| {
            if let Some(dialog) = Dialog::upgrade(&weak) {
                dialog.respond(&response);
            }
        });
        if self.inner.responses.borrow().is_empty() {
            // The row is as wide as the card and its buttons sit at its end: the first takes the room before it.
            button.set_hexpand(true);
            button.set_halign(gtk::Align::End);
        }
        self.inner.actions.append(&button);
        self.inner.actions.set_visible(true);
        self.inner.responses.borrow_mut().push((id.to_owned(), button));
    }

    /// An `id` that was never added is ignored, here and below.
    pub fn set_response_appearance(&self, id: &str, appearance: Appearance) {
        if let Some(button) = self.button(id) {
            crate::set_class(&button, "suggested-action", appearance == Appearance::Suggested);
            crate::set_class(&button, "destructive-action", appearance == Appearance::Destructive);
        }
    }

    /// A disabled response cannot be given, by its button or by Enter: for input that is not valid yet.
    pub fn set_response_enabled(&self, id: &str, on: bool) {
        if let Some(button) = self.button(id) {
            button.set_sensitive(on);
        }
    }

    /// The response Enter gives, from anywhere but another button or a field that keeps Enter for itself.
    pub fn set_default_response(&self, id: &str) {
        self.inner.default_response.replace(Some(id.to_owned()));
    }

    /// The response Escape and [`Dialog::close`] give. Without one they close the dialog and no handler runs.
    pub fn set_close_response(&self, id: &str) {
        self.inner.close_response.replace(Some(id.to_owned()));
    }

    /// The app's own widget, between the body and the responses.
    pub fn set_extra_child(&self, child: Option<&gtk::Widget>) {
        crate::clear(&self.inner.extra);
        if let Some(child) = child {
            self.inner.extra.append(child);
        }
        self.inner.extra.set_visible(child.is_some());
    }

    /// The least size of the app's widget, in pixels; -1 leaves a dimension to the widget. A dialog larger than its
    /// window scrolls inside it.
    pub fn set_content_size(&self, width: i32, height: i32) {
        self.inner.extra.set_size_request(width, height);
    }

    /// Whether Escape and [`Dialog::close`] close the dialog. Off while work that must finish is running: then only a
    /// response or [`Dialog::force_close`] does.
    pub fn set_can_close(&self, on: bool) {
        self.inner.can_close.set(on);
    }

    /// Runs `f` with the response's id after the dialog has closed. The dialog's widgets are still there to read.
    pub fn connect_response(&self, f: impl Fn(&Dialog, &str) + 'static) {
        self.inner.handlers.borrow_mut().push(Rc::new(f));
    }

    /// Shows the dialog over the window `parent` is in. That window's content, title bar and any dialog under this
    /// one take neither keys nor the pointer until it closes. A window without a [`crate::Shell`] (a compose or
    /// editor window) gets a modal window of the dialog's own instead.
    pub fn present(&self, parent: &impl IsA<gtk::Widget>) {
        if self.inner.surface.upgrade().is_some() {
            return;
        }
        let window = parent.as_ref().root().and_downcast::<gtk::Window>();
        let overlay = window
            .as_ref()
            .and_then(|window| window.child())
            .and_downcast::<gtk::Overlay>()
            .filter(|overlay| overlay.has_css_class("om-shell"));
        let Some(overlay) = overlay else {
            self.present_in_window(window.as_ref());
            return;
        };
        self.inner
            .focus_before
            .set(window.as_ref().and_then(RootExt::focus).as_ref());
        // A scrolled window, so a dialog taller or wider than the window can still be reached.
        let backdrop = gtk::ScrolledWindow::builder()
            .css_classes([BACKDROP])
            .child(&self.inner.card)
            .build();
        backdrop.add_controller(self.keys());
        // GTK drops the focus when Tab runs off the last widget, or when the focused one is disabled or removed. The
        // window would then get the keys, and Escape would not arrive here. Where the focus went is known only
        // after `leave`, hence the idle.
        let left = gtk::EventControllerFocus::new();
        let weak = Rc::downgrade(&self.inner);
        left.connect_leave(move |_| {
            let weak = weak.clone();
            glib::idle_add_local_once(move || {
                if let Some(dialog) = Dialog::upgrade(&weak) {
                    dialog.keep_focus();
                }
            });
        });
        backdrop.add_controller(left);
        self.inner.surface.set(Some(backdrop.upcast_ref()));
        // Held back first: the card takes the focus as it is mapped, and GTK moves the focus only between widgets
        // that can have it.
        hold_back(&overlay, Some(backdrop.upcast_ref()));
        overlay.add_overlay(&backdrop);
    }

    fn present_in_window(&self, parent: Option<&gtk::Window>) {
        let title = self.inner.heading.as_ref().map(|heading| heading.label());
        let window = gtk::Window::builder()
            .modal(true)
            .resizable(false)
            .child(&self.inner.card)
            .build();
        window.set_transient_for(parent);
        window.set_title(title.as_deref().or(parent.and_then(|p| p.title()).as_deref()));
        window.add_controller(self.keys());
        window.connect_close_request({
            let dialog = self.clone();
            move |_| {
                dialog.close();
                glib::Propagation::Stop
            }
        });
        self.inner.surface.set(Some(window.upcast_ref()));
        window.present();
    }

    /// Closes as Escape does: not at all while [`Dialog::set_can_close`] is off, and with the close response when
    /// there is one.
    pub fn close(&self) {
        if !self.inner.can_close.get() || self.inner.surface.upgrade().is_none() {
            return;
        }
        let response = self.inner.close_response.borrow().clone();
        match response {
            Some(id) => self.respond(&id),
            None => self.dismiss(),
        }
    }

    /// Closes whatever [`Dialog::set_can_close`] says, and runs no handler.
    pub fn force_close(&self) {
        self.dismiss();
    }

    /// The dialog's own widgets hold it weakly in their handlers: it owns them.
    fn upgrade(weak: &Weak<Inner>) -> Option<Dialog> {
        weak.upgrade().map(|inner| Dialog { inner })
    }

    fn button(&self, id: &str) -> Option<gtk::Button> {
        let responses = self.inner.responses.borrow();
        responses
            .iter()
            .find(|(response, _)| response == id)
            .map(|(_, button)| button.clone())
    }

    fn default_button(&self) -> Option<gtk::Button> {
        let id = self.inner.default_response.borrow().clone()?;
        self.button(&id).filter(|button| button.is_sensitive())
    }

    fn activate_default(&self) -> bool {
        let Some(button) = self.default_button() else {
            return false;
        };
        button.emit_clicked();
        true
    }

    fn respond(&self, id: &str) {
        // Closed first, so a handler that presents the next dialog finds the window as it was.
        self.dismiss();
        let handlers = self.inner.handlers.borrow().clone();
        for handler in handlers {
            handler(self, id);
        }
    }

    /// Escape, Enter and Tab, for the backdrop or the dialog's window, after the focused widget has had them. The
    /// handler owns the dialog, which is what keeps a presented dialog alive after the app has dropped its handle.
    fn keys(&self) -> gtk::EventControllerKey {
        let keys = gtk::EventControllerKey::new();
        let dialog = self.clone();
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let shift = modifiers.contains(gdk::ModifierType::SHIFT_MASK);
            if modifiers.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK) {
                return glib::Propagation::Proceed;
            }
            match key {
                gdk::Key::Escape => dialog.close(),
                gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter if dialog.activate_default() => {}
                gdk::Key::ISO_Left_Tab => dialog.move_focus(gtk::DirectionType::TabBackward),
                gdk::Key::Tab | gdk::Key::KP_Tab if shift => dialog.move_focus(gtk::DirectionType::TabBackward),
                gdk::Key::Tab | gdk::Key::KP_Tab => dialog.move_focus(gtk::DirectionType::TabForward),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        keys
    }

    /// Tab inside the dialog, round from its last widget to its first. Left to the window, Tab goes on into the title
    /// bar, and with that held back it stops at either end.
    fn move_focus(&self, direction: gtk::DirectionType) {
        let card = &self.inner.card;
        if card.child_focus(direction) {
            return;
        }
        // Past the end. A container goes on from the child that has the focus, so with none it starts over.
        if let Some(root) = card.root() {
            root.set_focus(gtk::Widget::NONE);
        }
        if !card.child_focus(direction) {
            self.focus_first();
        }
    }

    /// The keyboard goes to the app's widget when it has somewhere to type or choose, else to the default response,
    /// else to the first response.
    fn take_focus(&self) {
        if self.inner.extra.child_focus(gtk::DirectionType::TabForward) {
            return;
        }
        match self.default_button() {
            Some(button) => {
                button.grab_focus();
            }
            None => self.focus_first(),
        }
    }

    /// A dialog with nothing to focus takes the keyboard itself, so that Escape still arrives.
    fn focus_first(&self) {
        let card = &self.inner.card;
        if !card.child_focus(gtk::DirectionType::TabForward) {
            card.set_focusable(true);
            card.grab_focus();
        }
    }

    /// Brings the focus back when it has left the dialog, unless it went to a dialog over this one.
    fn keep_focus(&self) {
        let Some(backdrop) = self.inner.surface.upgrade() else {
            return;
        };
        let inside = backdrop
            .root()
            .and_then(|root| root.focus())
            .is_some_and(|focus| focus.is_ancestor(&backdrop));
        if !inside && !backdrop.has_css_class(HELD[0].1) {
            self.focus_first();
        }
    }

    fn dismiss(&self) {
        let Some(surface) = self.inner.surface.upgrade() else {
            return;
        };
        self.inner.surface.set(None);
        // The card leaves with the dialog, so it can be presented again.
        if let Some(window) = surface.downcast_ref::<gtk::Window>() {
            window.set_child(gtk::Widget::NONE);
            window.destroy();
        } else if let Some(backdrop) = surface.downcast_ref::<gtk::ScrolledWindow>() {
            backdrop.set_child(gtk::Widget::NONE);
            if let Some(overlay) = backdrop.parent().and_downcast::<gtk::Overlay>() {
                overlay.remove_overlay(backdrop);
                hold_back(&overlay, None);
            }
            if let Some(widget) = self.inner.focus_before.upgrade() {
                widget.grab_focus();
            }
        }
        self.inner.focus_before.set(None);
    }
}

/// Leaves the keyboard and the pointer to the topmost dialog on `overlay`: `arriving` when one is about to be added,
/// else the last one there. Everything else on the overlay and the window's title bar is held back while there is a
/// dialog, and let go when the last one closes, in whatever order they close.
fn hold_back(overlay: &gtk::Overlay, arriving: Option<&gtk::Widget>) {
    let mut others = Vec::new();
    let mut top = arriving.cloned();
    let mut child = overlay.last_child();
    while let Some(widget) = child {
        child = widget.prev_sibling();
        if top.is_none() && widget.has_css_class(BACKDROP) {
            top = Some(widget);
        } else {
            others.push(widget);
        }
    }
    others.extend(overlay.root().and_downcast::<gtk::Window>().and_then(|w| w.titlebar()));
    for widget in others {
        set_held(&widget, top.is_some());
    }
    if let Some(top) = top {
        set_held(&top, false);
    }
}

/// Takes `can-focus` and `can-target` from a widget and all inside it, or gives them back. A class remembers each
/// one taken, so a widget the app had already made unreachable stays so afterwards.
fn set_held(widget: &gtk::Widget, held: bool) {
    for (property, taken) in HELD {
        if held && widget.property::<bool>(property) {
            widget.set_property(property, false);
            widget.add_css_class(taken);
        } else if !held && widget.has_css_class(taken) {
            widget.set_property(property, true);
            widget.remove_css_class(taken);
        }
    }
}

/// Asks before `action`: a dialog with Cancel and a button labelled `action`, drawn as `appearance`. `yes` runs when
/// that button is chosen. Enter chooses it too, unless it is destructive: then Enter cancels, as Escape does.
pub fn confirm(
    parent: &impl IsA<gtk::Widget>,
    heading: &str,
    body: &str,
    action: &str,
    appearance: Appearance,
    yes: impl FnOnce() + 'static,
) {
    let dialog = Dialog::new(Some(heading), Some(body));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("confirm", action);
    dialog.set_response_appearance("confirm", appearance);
    dialog.set_default_response(if appearance == Appearance::Destructive {
        "cancel"
    } else {
        "confirm"
    });
    dialog.set_close_response("cancel");
    let yes = Cell::new(Some(yes));
    dialog.connect_response(move |_, id| {
        if id == "confirm"
            && let Some(yes) = yes.take()
        {
            yes();
        }
    });
    dialog.present(parent);
}
