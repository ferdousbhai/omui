// SPDX-License-Identifier: MIT
//! A dialog drawn inside its window, as libadwaita's are: a card over a backdrop on the window's [`crate::Shell`]
//! overlay. A second toplevel would be tiled beside the window on a tiling desktop. The methods are those of
//! `adw::AlertDialog` and `adw::Dialog`.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use gtk::{gdk, gio, glib, prelude::*};

use crate::layout::PreferredLayout;

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

/// How wide a dialog with a heading or a body is where the window has room, when the app gives its widget no width:
/// a card of 372 pixels, as libadwaita's alert dialogs are, and the card's margins. Text then wraps at that width,
/// rather than making a card as wide as its longest paragraph.
pub(crate) const DIALOG_WIDTH: i32 = 372 + 2 * 24;

/// The shortcut controllers GTK puts on every window that a dialog leaves alone: the window's own keys (Tab between
/// widgets, Enter), and the two that run the managed shortcuts of whatever is in the window, the dialog's included.
/// The shortcuts of what the dialog covers are held back one by one.
const WINDOW_KEYS: [&str; 3] = [
    "gtk-widget-class-shortcuts",
    "gtk-shortcut-manager-capture",
    "gtk-shortcut-manager-bubble",
];

thread_local! {
    /// The shortcut controllers held back on each window that has a dialog open, with the phase each is given back.
    static HELD_SHORTCUTS: RefCell<Vec<(glib::WeakRef<gtk::Window>, Vec<HeldShortcut>)>> = RefCell::default();
}

type HeldShortcut = (gtk::ShortcutController, gtk::PropagationPhase);

/// The two properties a dialog takes from the rest of its window, each with the class that marks a widget it was
/// taken from. omui.css fades a title bar by the second.
const HELD: [(&str, &str); 2] = [("can-focus", "om-held-can-focus"), ("can-target", "om-held-can-target")];

/// A heading, a body, any widget of the app's, and a row of responses. Every part is optional: with neither heading nor
/// body the dialog is the app's widget alone, edge to edge.
///
/// A response closes the dialog and then runs the handlers, unless [`Dialog::set_response_closes`] says it stays open.
/// Escape gives the close response, Enter the default one.
/// A dialog stays alive while it is shown, so the handle may be dropped after [`Dialog::present`].
#[derive(Clone)]
pub struct Dialog {
    inner: Rc<Inner>,
}

/// A handle that does not keep its dialog alive: what a handler on one of the dialog's own widgets holds, since the
/// dialog owns the widget. `glib::clone!` makes one for `#[weak] dialog`.
#[derive(Clone)]
pub struct WeakDialog(Weak<Inner>);

impl WeakDialog {
    /// The dialog, while a handle to it is held or it is shown.
    pub fn upgrade(&self) -> Option<Dialog> {
        self.0.upgrade().map(|inner| Dialog { inner })
    }
}

impl glib::clone::Downgrade for Dialog {
    type Weak = WeakDialog;

    fn downgrade(&self) -> WeakDialog {
        Dialog::downgrade(self)
    }
}

impl glib::clone::Upgrade for WeakDialog {
    type Strong = Dialog;

    fn upgrade(&self) -> Option<Dialog> {
        WeakDialog::upgrade(self)
    }
}

struct Inner {
    card: gtk::Box,
    /// What the card is shown in, and what sizes it: [`DIALOG_WIDTH`] for a dialog that has a heading or a body,
    /// until the app sizes its widget.
    frame: gtk::Box,
    frame_size: PreferredLayout,
    heading: Option<gtk::Label>,
    /// The close button of [`Dialog::set_close_button`], once asked for.
    close_button: RefCell<Option<gtk::Button>>,
    title: RefCell<Option<String>>,
    /// Where the app's widget scrolls, and inside it the box that holds the widget.
    extra: gtk::ScrolledWindow,
    content: gtk::Box,
    content_size: PreferredLayout,
    actions: gtk::Box,
    responses: RefCell<Vec<(String, gtk::Button)>>,
    /// The responses that leave the dialog open (`set_response_closes`).
    staying: RefCell<Vec<String>>,
    default_response: RefCell<Option<String>>,
    close_response: RefCell<Option<String>>,
    can_close: Cell<bool>,
    handlers: RefCell<Vec<Handler>>,
    /// The backdrop or the window the card is shown in. Weak: the parent window owns it, and its key handler owns
    /// the dialog.
    surface: glib::WeakRef<gtk::Widget>,
    /// What keeps mnemonics to an in-window dialog, on the window.
    mnemonics: RefCell<Option<gtk::EventControllerKey>>,
    /// What had the keyboard before an in-window dialog took it.
    focus_before: glib::WeakRef<gtk::Widget>,
}

impl Dialog {
    pub fn new(heading: Option<&str>, body: Option<&str>) -> Self {
        let card = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["dialog"])
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
        // A box for its `append` alone: the layout is the dialog's, and gives the app's widget the whole of it.
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let content_size: PreferredLayout = glib::Object::new();
        content.set_layout_manager(Some(content_size.clone()));
        // In a window too low for the dialog the app's widget scrolls, and the heading and the responses stay. It
        // never scrolls sideways: in a narrow window it is narrower.
        // As any scrolled window, it takes the focus itself when nothing inside does, for the keys that scroll.
        let extra = gtk::ScrolledWindow::builder()
            .css_classes(["dialog-extra"])
            .child(&content)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_width(true)
            .propagate_natural_height(true)
            .visible(false)
            .build();
        let actions = gtk::Box::builder()
            .css_classes(["dialog-actions"])
            .visible(false)
            .build();
        card.append(&extra);
        card.append(&actions);
        // A box for its `append` alone, as the content's is.
        let frame = gtk::Box::builder()
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        let frame_size: PreferredLayout = glib::Object::new();
        if heading.is_some() || body.is_some() {
            frame_size.set_size(DIALOG_WIDTH, -1);
        }
        frame.set_layout_manager(Some(frame_size.clone()));
        frame.append(&card);

        let dialog = Self {
            inner: Rc::new(Inner {
                card,
                frame,
                frame_size,
                heading,
                close_button: RefCell::new(None),
                title: RefCell::new(None),
                extra,
                content,
                content_size,
                actions,
                responses: RefCell::new(Vec::new()),
                staying: RefCell::new(Vec::new()),
                default_response: RefCell::new(None),
                close_response: RefCell::new(None),
                can_close: Cell::new(true),
                handlers: RefCell::new(Vec::new()),
                surface: glib::WeakRef::new(),
                mnemonics: RefCell::new(None),
                focus_before: glib::WeakRef::new(),
            }),
        };
        // An entry that activates the default (`set_activates_default`) asks its nearest "default.activate" action:
        // this one, ahead of the window's.
        let default = gio::SimpleAction::new("activate", None);
        let weak = dialog.downgrade();
        default.connect_activate(move |_, _| {
            if let Some(dialog) = weak.upgrade() {
                dialog.activate_default();
            }
        });
        let group = gio::SimpleActionGroup::new();
        group.add_action(&default);
        dialog.inner.card.insert_action_group("default", Some(&group));
        // Once the card is on screen, whether presented before its window was or after.
        let weak = dialog.downgrade();
        dialog.inner.card.connect_map(move |_| {
            if let Some(dialog) = weak.upgrade() {
                dialog.take_focus();
            }
        });
        dialog
    }

    /// For a handler on a widget the dialog owns.
    pub fn downgrade(&self) -> WeakDialog {
        WeakDialog(Rc::downgrade(&self.inner))
    }

    /// Names a dialog that has no heading, as the title of an `adw::Dialog` does: for a screen reader, and for the
    /// window when the dialog is one.
    pub fn set_title(&self, title: &str) {
        self.inner
            .card
            .update_property(&[gtk::accessible::Property::Label(title)]);
        self.inner.title.replace(Some(title.to_owned()));
        if let Some(window) = self.inner.surface.upgrade().and_downcast::<gtk::Window>() {
            window.set_title(Some(title));
        }
    }

    /// A button at the end of the row, after those already added. `label` may mark a mnemonic with an underscore.
    pub fn add_response(&self, id: &str, label: &str) {
        let button = gtk::Button::builder().label(label).use_underline(true).build();
        let weak = self.downgrade();
        let response = id.to_owned();
        button.connect_clicked(move |_| {
            if let Some(dialog) = weak.upgrade() {
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

    /// Whether giving the response closes the dialog; it does unless this turns it off. One that does not runs the
    /// handlers with the dialog still open, for an action on what the dialog shows: Clear in a log, Copy beside a
    /// code. The close response always closes, from Escape or the close button.
    pub fn set_response_closes(&self, id: &str, on: bool) {
        let mut staying = self.inner.staying.borrow_mut();
        staying.retain(|response| response != id);
        if !on {
            staying.push(id.to_owned());
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
        crate::clear(&self.inner.content);
        if let Some(child) = child {
            self.inner.content.append(child);
        }
        self.inner.extra.set_visible(child.is_some());
    }

    /// The size of the app's widget where the window has room for it, in pixels, as the content size of an
    /// `adw::Dialog` is; -1 leaves a dimension to the widget. In a narrower window the widget is narrower, down to
    /// the least it can be, and in a lower one it scrolls.
    ///
    /// A dialog with a heading or a body is 420 pixels wide, margins included, until a width is given here; one with
    /// neither is as wide as its widget.
    pub fn set_content_size(&self, width: i32, height: i32) {
        self.inner.content_size.set_size(width, height);
        if width >= 0 {
            self.inner.frame_size.set_size(-1, -1);
        }
    }

    /// Whether Escape, [`Dialog::close`] and the close button close the dialog. Off while work that must finish is
    /// running: then only a response or [`Dialog::force_close`] does, and the close button is hidden.
    pub fn set_can_close(&self, on: bool) {
        self.inner.can_close.set(on);
        if let Some(button) = self.inner.close_button.borrow().as_ref() {
            button.set_visible(on);
        }
    }

    /// A close button at the end of the heading, as a preferences or an about dialog has (`adw::Dialog`): it closes
    /// as Escape does. For a dialog that has no responses, or none that only closes it.
    pub fn set_close_button(&self, on: bool) {
        if let Some(button) = self.inner.close_button.borrow().as_ref() {
            button.set_visible(on && self.inner.can_close.get());
            return;
        }
        if !on {
            return;
        }
        let button = crate::icon_button(crate::icons::name::CLOSE, "Close");
        button.add_css_class("dialog-close");
        button.set_halign(gtk::Align::End);
        button.set_valign(gtk::Align::Start);
        button.set_hexpand(true);
        button.set_visible(self.inner.can_close.get());
        let weak = self.downgrade();
        button.connect_clicked(move |_| {
            if let Some(dialog) = weak.upgrade() {
                dialog.close();
            }
        });
        // The heading and the button side by side, on the first line of the card.
        let line = gtk::Box::builder().css_classes(["dialog-header"]).build();
        if let Some(heading) = &self.inner.heading {
            self.inner.card.remove(heading);
            heading.set_hexpand(true);
            line.append(heading);
        }
        line.append(&button);
        self.inner.card.prepend(&line);
        self.inner.close_button.replace(Some(button));
    }

    /// Runs `f` with the response's id after the dialog has closed, or while it stays open for one that does not
    /// close it. The dialog's widgets are still there to read.
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
        let shell = window.as_ref().and_then(|window| {
            let overlay = window.child().and_downcast::<gtk::Overlay>()?;
            overlay.has_css_class("om-shell").then_some((window, overlay))
        });
        let Some((window, overlay)) = shell else {
            self.present_in_window(window.as_ref());
            return;
        };
        self.inner.focus_before.set(RootExt::focus(window).as_ref());
        let mnemonics = self.mnemonics();
        window.add_controller(mnemonics.clone());
        self.inner.mnemonics.replace(Some(mnemonics));
        // Homogeneous, so the card has the whole of it to centre itself in, and no more than that to grow to.
        let backdrop = gtk::Box::builder().css_classes([BACKDROP]).homogeneous(true).build();
        backdrop.append(&self.inner.frame);
        backdrop.add_controller(self.keys());
        // GTK drops the focus when Tab runs off the last widget, or when the focused one is disabled or removed. The
        // window would then get the keys, and Escape would not arrive here. Where the focus went is known only
        // after `leave`, hence the idle.
        let left = gtk::EventControllerFocus::new();
        let weak = self.downgrade();
        left.connect_leave(move |_| {
            let weak = weak.clone();
            glib::idle_add_local_once(move || {
                if let Some(dialog) = weak.upgrade() {
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
        let title = self.inner.title.borrow().clone();
        let title = title
            .or_else(|| self.inner.heading.as_ref().map(|heading| heading.label().into()))
            .or_else(|| parent.and_then(|parent| parent.title()).map(Into::into));
        // A question about a window that has gone has no one to answer to.
        let window = gtk::Window::builder()
            .modal(true)
            .destroy_with_parent(true)
            .resizable(false)
            .child(&self.inner.frame)
            .build();
        window.set_transient_for(parent);
        window.set_title(title.as_deref());
        window.add_controller(self.keys());
        window.connect_destroy({
            let weak = self.downgrade();
            move |_| {
                if let Some(dialog) = weak.upgrade() {
                    dialog.inner.surface.set(None);
                }
            }
        });
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
        self.dismiss();
        let response = self.inner.close_response.borrow().clone();
        if let Some(id) = response {
            self.emit(&id);
        }
    }

    /// Closes whatever [`Dialog::set_can_close`] says, and runs no handler.
    pub fn force_close(&self) {
        self.dismiss();
    }

    /// Presses the response's button, as a click would.
    #[cfg(test)]
    pub(crate) fn click(&self, id: &str) {
        self.button(id).expect("a response that was added").emit_clicked();
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
        if !self.inner.staying.borrow().iter().any(|response| response == id) {
            self.dismiss();
        }
        self.emit(id);
    }

    fn emit(&self, id: &str) {
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

    /// Alt and a letter, for the dialog's window. GTK gives a mnemonic to its button from the window, before any
    /// widget inside has the key, and so to a button the dialog covers. A controller added after GTK's own runs
    /// ahead of it: this one gives the key to the dialog's widgets alone.
    fn mnemonics(&self) -> gtk::EventControllerKey {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = self.downgrade();
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let alt =
                modifiers.contains(gdk::ModifierType::ALT_MASK) && !modifiers.contains(gdk::ModifierType::CONTROL_MASK);
            let Some(dialog) = weak.upgrade().filter(|_| alt) else {
                return glib::Propagation::Proceed;
            };
            if let Some(label) = mnemonic_label(dialog.inner.card.upcast_ref(), key.to_lower()) {
                label.mnemonic_activate(false);
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
        if self.inner.content.child_focus(gtk::DirectionType::TabForward) {
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
        } else if let Some(backdrop) = surface.downcast_ref::<gtk::Box>() {
            backdrop.remove(&self.inner.frame);
            if let Some(mnemonics) = self.inner.mnemonics.take()
                && let Some(window) = mnemonics.widget()
            {
                window.remove_controller(&mnemonics);
            }
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

/// Whether a [`Dialog`] is open over the window `widget` is in, drawn on its [`crate::Shell`]. While one is, the
/// window's shortcuts and the application's accelerators do nothing there: an action they share with a menu or a
/// button needs no check of its own.
pub fn dialog_open(widget: &impl IsA<gtk::Widget>) -> bool {
    widget
        .as_ref()
        .root()
        .and_downcast::<gtk::Window>()
        .and_then(|window| window.child())
        .and_downcast::<gtk::Overlay>()
        .is_some_and(|overlay| overlay.has_css_class("om-shell") && over(&overlay))
}

/// Whether a dialog's backdrop is on `overlay`.
pub(crate) fn over(overlay: &gtk::Overlay) -> bool {
    let mut child = overlay.first_child();
    while let Some(widget) = child {
        if widget.has_css_class(BACKDROP) {
            return true;
        }
        child = widget.next_sibling();
    }
    false
}

/// Leaves the keyboard and the pointer to the topmost dialog on `overlay`: `arriving` when one is about to be added,
/// else the last one there. Everything else on the overlay and the window's title bar is held back while there is a
/// dialog, and let go when the last one closes, in whatever order they close. So are the shortcuts of all that, and
/// of the window: those of a menu, an accelerator or a page would otherwise act behind the dialog, as GTK hands them
/// keys from anywhere in the window.
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
    let window = overlay.root().and_downcast::<gtk::Window>();
    others.extend(window.as_ref().and_then(|w| w.titlebar()));
    for widget in &others {
        set_held(widget, top.is_some());
    }
    if let Some(top) = &top {
        set_held(top, false);
    }
    if let Some(window) = window {
        let mut shortcuts = Vec::new();
        if top.is_some() {
            shortcuts.extend(
                controllers(window.upcast_ref())
                    .into_iter()
                    .filter(|controller| !WINDOW_KEYS.contains(&controller.name().as_deref().unwrap_or_default())),
            );
            for widget in &others {
                shortcuts_within(widget, &mut shortcuts);
            }
        }
        hold_shortcuts(&window, shortcuts);
    }
}

/// The shortcut controllers on `widget`.
fn controllers(widget: &gtk::Widget) -> Vec<gtk::ShortcutController> {
    widget
        .observe_controllers()
        .iter::<glib::Object>()
        .filter_map(|controller| controller.ok()?.downcast().ok())
        .collect()
}

/// The shortcuts of `widget` and all inside it that GTK runs from the window: those in managed and global scope. A
/// local one runs only while the keyboard is inside its widget, which a dialog does not let it be.
fn shortcuts_within(widget: &gtk::Widget, found: &mut Vec<gtk::ShortcutController>) {
    found.extend(
        controllers(widget)
            .into_iter()
            .filter(|controller| controller.scope() != gtk::ShortcutScope::Local),
    );
    let mut child = widget.first_child();
    while let Some(widget) = child {
        shortcuts_within(&widget, found);
        child = widget.next_sibling();
    }
}

/// Holds back exactly `shortcuts` on `window`: each is given propagation phase None, which also takes a managed one
/// out of the window's reach, and those held before and not now get their phase back.
fn hold_shortcuts(window: &gtk::Window, shortcuts: Vec<gtk::ShortcutController>) {
    HELD_SHORTCUTS.with_borrow_mut(|held| {
        held.retain(|(window, _)| window.upgrade().is_some());
        let index = held.iter().position(|(w, _)| w.upgrade().as_ref() == Some(window));
        let mut before = index.map(|index| held.remove(index).1).unwrap_or_default();
        let mut now = Vec::new();
        for controller in shortcuts {
            match before.iter().position(|(held, _)| *held == controller) {
                Some(index) => now.push(before.remove(index)),
                None if controller.propagation_phase() != gtk::PropagationPhase::None => {
                    let phase = controller.propagation_phase();
                    controller.set_propagation_phase(gtk::PropagationPhase::None);
                    now.push((controller, phase));
                }
                None => {}
            }
        }
        for (controller, phase) in before {
            controller.set_propagation_phase(phase);
        }
        if !now.is_empty() {
            held.push((window.downgrade(), now));
        }
    });
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

/// The label on screen under `widget` whose mnemonic is `key`.
fn mnemonic_label(widget: &gtk::Widget, key: gdk::Key) -> Option<gtk::Label> {
    if let Some(label) = widget.downcast_ref::<gtk::Label>()
        && label.is_mapped()
        && label.mnemonic_keyval().to_lower() == key
    {
        return Some(label.clone());
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        if let Some(label) = mnemonic_label(&widget, key) {
            return Some(label);
        }
        child = widget.next_sibling();
    }
    None
}

/// Asks before `action`: a dialog with Cancel (Alt+C) and a button labelled `action` (which may mark its own
/// mnemonic), drawn as `appearance`. `yes` runs when
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
    dialog.add_response("cancel", "_Cancel");
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

/// The app's name, version and a few lines about it (what it does, who made it, its licence), with a link to its
/// website: what an About item in a window's menu opens (`adw::AboutDialog`). It closes from its close button or
/// Escape.
pub fn about(parent: &impl IsA<gtk::Widget>, name: &str, version: &str, text: &str, website: Option<&str>) {
    let dialog = Dialog::new(Some(name), Some(&format!("Version {version}\n\n{text}")));
    if let Some(website) = website {
        // A link in a label, not a link button: it lines up with the text above it, which a button's padding would
        // not.
        let link = gtk::Label::builder()
            .label(format!("<a href=\"{}\">Website</a>", glib::markup_escape_text(website)))
            .use_markup(true)
            .xalign(0.0)
            .build();
        dialog.set_extra_child(Some(link.upcast_ref()));
    }
    dialog.set_close_button(true);
    // The keyboard starts on the close button, not the link, where Enter pressed as the dialog opens would go to the
    // browser. This runs after the dialog's own choice, which is made as the card is mapped too.
    let close = dialog
        .inner
        .close_button
        .borrow()
        .as_ref()
        .map(|button| button.downgrade());
    dialog.inner.card.connect_map(move |_| {
        if let Some(close) = close.as_ref().and_then(|close| close.upgrade()) {
            close.grab_focus();
        }
    });
    dialog.present(parent);
}
