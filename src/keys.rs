// SPDX-License-Identifier: MIT
//! The keyboard, as every Omarchy app has it: single keys that do nothing while the user is typing, an overlay that
//! lists every shortcut, and the keys that quit.
//!
//! An app driven by bare keys (`Space` for its main action, `Q` to quit) shows its shortcuts on `?` and F1. An app
//! that is typed into (an editor, anything whose text field has the keyboard most of the time) cannot give bare keys
//! away: it shows them on F1 and Ctrl+?, quits on Ctrl+Q and closes on Ctrl+W.

use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

use gtk::{gdk, glib, prelude::*};

use crate::{Dialog, Group, Row, i18n};

/// Whether the keyboard is in a text field of the window `widget` is in: an entry, a search or password entry, a
/// spin button, a text view. A single key pressed there is a character typed, not a shortcut.
pub fn text_has_focus(widget: &impl IsA<gtk::Widget>) -> bool {
    let focus = widget.as_ref().root().and_then(|root| root.focus());
    focus.is_some_and(|focus| focus.is::<gtk::Text>() || focus.is::<gtk::TextView>() || focus.is::<gtk::Editable>())
}

type Action = Rc<dyn Fn()>;

struct Key {
    /// The key as the app named it, for [`Keys::activate`].
    name: String,
    /// Whether it acts while a text field has the keyboard.
    typing: bool,
    action: Action,
}

/// The shortcuts of one window (or of one widget, for the keys pressed while the keyboard is inside it).
///
/// ```ignore
/// let keys = omui::Keys::new(&window);
/// keys.on("space", move || play_or_pause());
/// keys.on("q", glib::clone!(#[weak] window, move || window.close()));
/// keys.on_always("<Control>s", move || save());
/// ```
///
/// [`Keys::on`] is for a single key: it does nothing while a text field has the keyboard, where the key is a
/// character. [`Keys::on_always`] is for a key with Ctrl or Alt, or a function key, which acts wherever the
/// keyboard is. Neither acts while a [`Dialog`] is open over the window: the dialog holds the window's shortcuts
/// back, these among them. The widget under the keyboard has each key first, so Space still presses a focused
/// button.
///
/// This is a handle: the widget keeps the shortcuts, and they act for as long as it lives.
#[derive(Clone)]
pub struct Keys {
    widget: glib::WeakRef<gtk::Widget>,
    controller: gtk::ShortcutController,
    keys: Rc<RefCell<Vec<Key>>>,
}

impl Keys {
    pub fn new(widget: &impl IsA<gtk::Widget>) -> Self {
        let controller = gtk::ShortcutController::new();
        widget.as_ref().add_controller(controller.clone());
        Self {
            widget: widget.as_ref().downgrade(),
            controller,
            keys: Rc::default(),
        }
    }

    /// Runs `f` when `key` is pressed, unless a text field has the keyboard. `key` is one character (`"q"`, `"?"`,
    /// `" "`) or a key by GTK's name for it, in its accelerator syntax (`"space"`, `"Delete"`, `"<Shift>Tab"`).
    ///
    /// The key acts only with exactly the modifiers it names: `"r"` is R alone, not Shift+R, Ctrl+R or Alt+R, and
    /// `"R"` or `"<Shift>r"` is Shift+R. A symbol is whatever the layout presses to type it: `"?"` is Shift+/ on a US
    /// keyboard and Shift++ on a Danish one. Caps Lock changes nothing.
    pub fn on(&self, key: &str, f: impl Fn() + 'static) -> &Self {
        self.add(key, false, Rc::new(f));
        self
    }

    /// Runs `f` when `key` is pressed, in a text field too: for a key that types nothing (`"F1"`, `"<Control>s"`,
    /// `"<Control>question"`).
    pub fn on_always(&self, key: &str, f: impl Fn() + 'static) -> &Self {
        self.add(key, true, Rc::new(f));
        self
    }

    fn add(&self, key: &str, typing: bool, action: Action) {
        let Some(trigger) = trigger(key) else {
            glib::g_warning!("omui", "{key:?} is not a key, so its shortcut is left out");
            return;
        };
        let (widget, run) = (self.widget.clone(), action.clone());
        let callback = gtk::CallbackAction::new(move |_, _| {
            // Left to whatever else wants the key: in a text field, the field.
            if !typing && widget.upgrade().is_none_or(|widget| text_has_focus(&widget)) {
                return glib::Propagation::Proceed;
            }
            run();
            glib::Propagation::Stop
        });
        self.controller
            .add_shortcut(gtk::Shortcut::new(Some(trigger), Some(callback)));
        self.keys.borrow_mut().push(Key {
            name: key.to_owned(),
            typing,
            action,
        });
    }

    /// Does what pressing `key` would, by the name it was given to [`Keys::on`] or [`Keys::on_always`], and says
    /// whether anything ran: nothing does for a key that was never added, for a single key while a text field has
    /// the keyboard, and while a dialog holds the window's shortcuts back. For an app's tests, which cannot press a
    /// key.
    pub fn activate(&self, key: &str) -> bool {
        let Some(widget) = self.widget.upgrade() else {
            return false;
        };
        if self.controller.propagation_phase() == gtk::PropagationPhase::None {
            return false;
        }
        let action = self
            .keys
            .borrow()
            .iter()
            .find(|added| added.name == key && (added.typing || !text_has_focus(&widget)))
            .map(|added| added.action.clone());
        action.inspect(|action| action()).is_some()
    }
}

/// [`Keys::on`] for one key of `widget`'s window: `omui::on_key(&window, "q", move || …)`.
pub fn on_key(widget: &impl IsA<gtk::Widget>, key: &str, f: impl Fn() + 'static) -> Keys {
    let keys = Keys::new(widget);
    keys.on(key, f);
    keys
}

/// The trigger for a key named as [`Keys::on`] takes it.
fn trigger(key: &str) -> Option<gtk::ShortcutTrigger> {
    match single_key(key) {
        Some((key, modifiers)) => Some(gtk::KeyvalTrigger::new(key, modifiers).upcast()),
        None => gtk::ShortcutTrigger::parse_string(key),
    }
}

/// The key and the modifiers that a single character names: the key that types it, with exactly the modifiers it
/// says and none more. None for a key named otherwise.
fn single_key(key: &str) -> Option<(gdk::Key, gdk::ModifierType)> {
    let mut characters = key.chars();
    let (Some(character), None) = (characters.next(), characters.next()) else {
        return None;
    };
    // SAFETY: GDK has a key value for every character, and a `gdk::Key` is that value.
    let key: gdk::Key = unsafe { glib::translate::from_glib(gdk::unicode_to_keyval(u32::from(character))) };
    // A letter's case is whether Shift is held: `r` is r alone and `R` is Shift+R, as `<Shift>r` is. GDK matches a
    // key on what it types without Caps Lock, so Caps Lock changes neither.
    if key.to_upper() != key.to_lower() && key == key.to_upper() {
        return Some((key.to_lower(), gdk::ModifierType::SHIFT_MASK));
    }
    // A symbol is the key that types it, whatever modifiers the layout needs to reach it (Shift for `?` on most, none
    // on some): GDK leaves out of the match the modifiers that went into typing the symbol.
    Some((key, gdk::ModifierType::empty()))
}

/// Ctrl+Q closes every window of the application and Ctrl+W this one, as on any desktop: what an app that is typed
/// into quits by, and what any app may add. A window is closed as its close button would close it, so one with
/// unsaved work asks first (its `close-request` handler decides), and the application ends with its last window,
/// once any work it holds on for is done ([`crate::hold`]).
pub fn quit_keys(window: &impl IsA<gtk::Window>) -> Keys {
    let keys = Keys::new(window.as_ref());
    let weak = window.as_ref().downgrade();
    keys.on_always("<Control>q", {
        let weak = weak.clone();
        move || {
            let Some(window) = weak.upgrade() else { return };
            match window.application() {
                Some(application) => {
                    for window in application.windows() {
                        window.close();
                    }
                }
                None => window.close(),
            }
        }
    });
    keys.on_always("<Control>w", move || {
        if let Some(window) = weak.upgrade() {
            window.close();
        }
    });
    keys
}

/// One group of an app's shortcuts in its [`ShortcutOverlay`]: a title and, under it, each key with what it does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutSection {
    pub title: String,
    /// The key as it is shown (`"Space"`, `"Ctrl+Shift+Z"`, `"?"`), and what it does.
    pub keys: Vec<(String, String)>,
}

impl ShortcutSection {
    /// `title` may be empty, for an app with one group. A key that has another doing the same is written with ` / `
    /// between them (`"? / F1"`), and shown as both, with a mark between.
    pub fn new(title: &str, keys: &[(&str, &str)]) -> Self {
        Self {
            title: title.to_owned(),
            keys: keys
                .iter()
                .map(|(key, does)| ((*key).to_owned(), (*does).to_owned()))
                .collect(),
        }
    }

    /// One row more, for several keys that do the same, each shown as its own keys:
    /// `.alternatives(&["?", "F1"], &tr("Show or hide the shortcuts"))`. It is kept as [`ShortcutSection::keys`]
    /// keeps every row, with ` / ` between the keys.
    pub fn alternatives(mut self, keys: &[&str], does: &str) -> Self {
        self.keys.push((keys.join(ALTERNATIVES), does.to_owned()));
        self
    }
}

/// What sets apart two keys that do the same, in a shortcut as [`ShortcutSection`] keeps it. The spaces tell it from
/// the `/` key itself, as in `Ctrl+/`.
const ALTERNATIVES: &str = " / ";

/// The overlay that lists an app's shortcuts, which every Omarchy app shows on `?`:
///
/// ```ignore
/// let shortcuts = omui::ShortcutOverlay::new(|| vec![omui::ShortcutSection::new(&tr("General"), &[
///     ("Space", &tr("Play or pause")),
///     ("? / F1", &tr("Show or hide the shortcuts")),
///     ("Q", &tr("Quit")),
/// ])
/// .alternatives(&["Ctrl+Q", "Ctrl+W"], &tr("Close"))]);
/// shortcuts.install(&window, false);
/// ```
///
/// The sections are asked for each time it opens, so they are in the language of the moment. It is a [`Dialog`]
/// over the window: Escape closes it, and so does the key that opened it. The names of keys are not translated.
#[derive(Clone)]
pub struct ShortcutOverlay {
    inner: Rc<Overlay>,
}

struct Overlay {
    sections: Box<dyn Fn() -> Vec<ShortcutSection>>,
    open: RefCell<Option<Dialog>>,
}

impl ShortcutOverlay {
    pub fn new(sections: impl Fn() -> Vec<ShortcutSection> + 'static) -> Self {
        Self {
            inner: Rc::new(Overlay {
                sections: Box::new(sections),
                open: RefCell::new(None),
            }),
        }
    }

    /// Shows and hides the overlay from `window`'s keyboard: on `?` and F1, or with `typed_into` on F1 and Ctrl+?
    /// alone, for an app whose `?` is a character. `?` does nothing while a text field has the keyboard. The window
    /// keeps the overlay from here on.
    pub fn install(&self, window: &impl IsA<gtk::Window>, typed_into: bool) -> Keys {
        let keys = Keys::new(window.as_ref());
        // The window's keys keep the overlay, so the app may drop its handle; the overlay holds nothing of the
        // window's but the dialog while it is open.
        let toggle = || {
            let (overlay, window) = (self.clone(), window.as_ref().downgrade());
            move || {
                if let Some(window) = window.upgrade() {
                    overlay.toggle(&window);
                }
            }
        };
        keys.on_always("F1", toggle());
        if typed_into {
            // Ctrl with the key that types `?`, which on most layouts is Ctrl+Shift+/.
            keys.on_always("<Control>question", toggle());
        } else {
            keys.on("?", toggle());
        }
        keys
    }

    pub fn is_open(&self) -> bool {
        self.inner.open.borrow().is_some()
    }

    /// Opens the overlay over the window `parent` is in, or closes it when it is open.
    pub fn toggle(&self, parent: &impl IsA<gtk::Widget>) {
        if self.is_open() {
            self.close();
        } else {
            self.present(parent);
        }
    }

    pub fn close(&self) {
        let open = self.inner.open.borrow().clone();
        if let Some(dialog) = open {
            dialog.close();
        }
    }

    /// Opens the overlay over the window `parent` is in.
    pub fn present(&self, parent: &impl IsA<gtk::Widget>) {
        if self.is_open() {
            return;
        }
        let dialog = Dialog::new(Some(&i18n::tr("Keyboard shortcuts")), None);
        // Its heading follows a switch while it is open; the sections are the app's, asked for as it opens.
        dialog.set_heading_translated("Keyboard shortcuts");
        let (scroll, column) = crate::page(480);
        // The dialog scrolls what is too tall for the window; the page's own scrolling would only halve the list.
        scroll.set_vscrollbar_policy(gtk::PolicyType::Never);
        scroll.set_vexpand(false);
        scroll.set_propagate_natural_height(true);
        for section in (self.inner.sections)() {
            let group = Group::new(&section.title, "");
            for (key, does) in &section.keys {
                let row = Row::new(does, "");
                row.widget.set_focusable(false);
                row.add_suffix(&key_caps(key));
                group.add(&row.widget);
            }
            column.append(&group.widget);
        }
        dialog.set_extra_child(Some(scroll.upcast_ref()));
        dialog.set_content_size(480, -1);
        dialog.set_close_button(true);
        // The window's shortcuts are held back while a dialog is open, the one that opened this among them: the key
        // closes it from inside.
        let closing = gtk::ShortcutController::new();
        let weak = dialog.downgrade();
        closing.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("question|<Control>question|F1"),
            Some(gtk::CallbackAction::new(move |_, _| {
                if let Some(dialog) = weak.upgrade() {
                    dialog.close();
                }
                glib::Propagation::Stop
            })),
        ));
        dialog.add_controller(closing);
        let overlay: Weak<Overlay> = Rc::downgrade(&self.inner);
        dialog.connect_response(move |_, _| {
            if let Some(overlay) = overlay.upgrade() {
                overlay.open.take();
            }
        });
        self.inner.open.replace(Some(dialog.clone()));
        dialog.present(parent);
    }
}

/// A shortcut as its keys, each on a cap of its own: `Ctrl+Shift+Z` is three, and `? / F1` two with a mark between.
fn key_caps(shortcut: &str) -> gtk::Box {
    let caps = gtk::Box::builder()
        .css_classes(["keys"])
        .valign(gtk::Align::Center)
        .build();
    for (index, keys) in shortcut.split(ALTERNATIVES).enumerate() {
        if index > 0 {
            caps.append(&gtk::Label::builder().label("/").css_classes(["keys-or"]).build());
        }
        for key in split_keys(keys) {
            caps.append(&gtk::Label::builder().label(key).css_classes(["keycap"]).build());
        }
    }
    caps
}

/// The keys of a shortcut written with `+` between them, where the last may itself be `+` (`Ctrl++`).
fn split_keys(shortcut: &str) -> Vec<&str> {
    let mut keys: Vec<&str> = shortcut.split('+').map(str::trim).collect();
    if keys.ends_with(&["", ""]) {
        keys.truncate(keys.len() - 2);
        keys.push("+");
    }
    keys.retain(|key| !key.is_empty());
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The triggers a single key is given: the key with exactly the modifiers it names.
    #[test]
    fn a_single_key_is_triggered_with_exactly_its_modifiers() {
        let (none, shift) = (gdk::ModifierType::empty(), gdk::ModifierType::SHIFT_MASK);
        assert_eq!(single_key("r"), Some((gdk::Key::r, none)), "r alone, not Shift+R");
        assert_eq!(single_key("R"), Some((gdk::Key::r, shift)), "upper case is Shift");
        assert_eq!(
            single_key("?"),
            Some((gdk::Key::question, none)),
            "the symbol, by whatever the layout types it with"
        );
        assert_eq!(single_key("1"), Some((gdk::Key::_1, none)));
        assert_eq!(single_key("é"), Some((gdk::Key::eacute, none)));
        assert_eq!(single_key("space"), None, "a key by its name is GTK's to read");
    }

    #[test]
    fn a_shortcut_is_shown_as_its_keys() {
        assert_eq!(split_keys("Ctrl+Shift+Z"), ["Ctrl", "Shift", "Z"]);
        assert_eq!(split_keys("Space"), ["Space"]);
        assert_eq!(split_keys("?"), ["?"]);
        assert_eq!(split_keys("Ctrl + ?"), ["Ctrl", "?"]);
        assert_eq!(split_keys("Ctrl++"), ["Ctrl", "+"]);
        assert_eq!(split_keys("+"), ["+"]);
    }
}
