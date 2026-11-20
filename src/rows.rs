// SPDX-License-Identifier: MIT
//! Rows of a settings page or a form, and the titled group that holds them: what libadwaita's `PreferencesGroup`,
//! `ActionRow`, `EntryRow`, `PasswordEntryRow`, `SwitchRow`, `ComboRow` and `ExpanderRow` do. Titles and subtitles
//! are plain text, not markup.
//!
//! The rows are `gtk::ListBoxRow`s, for a [`Group`] or for a `gtk::ListBox` of the app's with the class `boxed-list`.
//! In a list Tab goes from row to row, and so do the Up and Down arrows, into and out of an [`ExpanderRow`] and from
//! one [`Group`] to the next. A row that carries a control (an entry, a switch, a drop-down) is not a stop itself:
//! the control is, and a click anywhere on the row goes to it.

use std::{cell::RefCell, rc::Rc};

use gtk::{gdk, glib, prelude::*};

use crate::icons;

/// A titled group of rows in a bordered list (`adw::PreferencesGroup`).
pub struct Group {
    pub widget: gtk::Box,
    /// The bordered list (class `boxed-list`).
    pub list: gtk::ListBox,
    header: gtk::Box,
    title: gtk::Label,
    description: gtk::Label,
    suffix: gtk::Box,
}

impl Group {
    /// An empty `title` or `description` takes no space.
    pub fn new(title: &str, description: &str) -> Self {
        let line = |text: &str, class: &str| {
            gtk::Label::builder()
                .label(text)
                .css_classes([class])
                .xalign(0.0)
                .wrap(true)
                .visible(!text.is_empty())
                .build()
        };
        let this = Self {
            widget: gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .css_classes(["group"])
                .accessible_role(gtk::AccessibleRole::Group)
                .build(),
            list: gtk::ListBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .css_classes(["boxed-list"])
                .visible(false)
                .build(),
            header: gtk::Box::builder().css_classes(["group-header"]).build(),
            title: line(title, "group-title"),
            description: line(description, "group-description"),
            suffix: gtk::Box::builder().valign(gtk::Align::Center).visible(false).build(),
        };
        let text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        text.append(&this.title);
        text.append(&this.description);
        this.header.append(&text);
        this.header.append(&this.suffix);
        this.widget.append(&this.header);
        this.widget.append(&this.list);
        if !title.is_empty() {
            this.widget
                .update_relation(&[gtk::accessible::Relation::LabelledBy(&[this.title.upcast_ref()])]);
        }
        this.show_header();
        this
    }

    /// A row goes into the list; any other widget goes under it.
    pub fn add(&self, widget: &impl IsA<gtk::Widget>) {
        if widget.as_ref().is::<gtk::ListBoxRow>() {
            self.list.append(widget);
            self.list.set_visible(true);
        } else {
            self.widget.append(widget);
        }
    }

    pub fn remove(&self, widget: &impl IsA<gtk::Widget>) {
        if widget.as_ref().is::<gtk::ListBoxRow>() {
            self.list.remove(widget);
            self.list.set_visible(self.list.first_child().is_some());
        } else {
            self.widget.remove(widget);
        }
    }

    pub fn set_description(&self, description: &str) {
        self.description.set_label(description);
        self.description.set_visible(!description.is_empty());
        self.show_header();
    }

    /// A widget at the end of the title's line: a button for the whole group.
    pub fn set_header_suffix(&self, widget: Option<&gtk::Widget>) {
        crate::clear(&self.suffix);
        if let Some(widget) = widget {
            self.suffix.append(widget);
        }
        self.suffix.set_visible(widget.is_some());
        self.show_header();
    }

    fn show_header(&self) {
        self.header
            .set_visible(self.title.is_visible() || self.description.is_visible() || self.suffix.is_visible());
    }
}

/// A row with a title, a subtitle under it, and widgets before and after them (`adw::ActionRow`).
#[derive(Clone)]
pub struct Row {
    pub widget: gtk::ListBoxRow,
    prefixes: gtk::Box,
    text: gtk::Box,
    title: gtk::Label,
    subtitle: gtk::Label,
    suffixes: gtk::Box,
}

impl Row {
    /// An empty subtitle takes no space.
    pub fn new(title: &str, subtitle: &str) -> Self {
        let line = |class: &str| {
            gtk::Label::builder()
                .css_classes([class])
                .xalign(0.0)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .build()
        };
        let side = || {
            gtk::Box::builder()
                .css_classes(["row-side"])
                .valign(gtk::Align::Center)
                .visible(false)
                .build()
        };
        let this = Self {
            widget: gtk::ListBoxRow::builder()
                .css_classes(["om-row"])
                .activatable(false)
                .build(),
            prefixes: side(),
            text: gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .css_classes(["row-text"])
                .hexpand(true)
                .valign(gtk::Align::Center)
                .build(),
            title: line("row-title"),
            subtitle: line("row-subtitle"),
            suffixes: side(),
        };
        this.text.append(&this.title);
        this.text.append(&this.subtitle);
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        content.append(&this.prefixes);
        content.append(&this.text);
        content.append(&this.suffixes);
        this.widget.set_child(Some(&content));
        this.widget.add_controller(arrows());
        this.set_title(title);
        this.set_subtitle(subtitle);
        this
    }

    /// A row that shows a value: the title is the small line that names it, and the subtitle the value (the
    /// `property` class of libadwaita).
    pub fn property(self) -> Self {
        self.widget.add_css_class("property");
        self
    }

    /// A widget before the title: an icon, an avatar.
    pub fn add_prefix(&self, widget: &impl IsA<gtk::Widget>) {
        self.prefixes.append(widget);
        self.prefixes.set_visible(true);
    }

    /// A widget at the end of the row, after those already there.
    pub fn add_suffix(&self, widget: &impl IsA<gtk::Widget>) {
        self.suffixes.append(widget);
        self.suffixes.set_visible(true);
    }

    pub fn set_title(&self, title: &str) {
        self.title.set_label(title);
        self.title.set_visible(!title.is_empty());
        self.widget.update_property(&[gtk::accessible::Property::Label(title)]);
    }

    pub fn set_subtitle(&self, subtitle: &str) {
        self.subtitle.set_label(subtitle);
        self.subtitle.set_visible(!subtitle.is_empty());
        self.widget
            .update_property(&[gtk::accessible::Property::Description(subtitle)]);
    }

    /// Lets the subtitle be selected and copied: an address, a fingerprint.
    pub fn set_subtitle_selectable(&self, on: bool) {
        self.subtitle.set_selectable(on);
    }

    /// The most lines the subtitle takes before it ends in an ellipsis; 0 for as many as it needs.
    pub fn set_subtitle_lines(&self, lines: i32) {
        self.subtitle.set_lines(if lines > 0 { lines } else { -1 });
        self.subtitle.set_ellipsize(if lines > 0 {
            gtk::pango::EllipsizeMode::End
        } else {
            gtk::pango::EllipsizeMode::None
        });
    }

    /// Makes a click on the row, or Enter on it, go to `widget`: press a button, flip a switch, put the cursor in
    /// an entry.
    pub fn set_activatable_widget(&self, widget: &impl IsA<gtk::Widget>) {
        let widget = widget.as_ref().downgrade();
        self.connect_activated(move || {
            if let Some(widget) = widget.upgrade() {
                widget.mnemonic_activate(false);
            }
        });
    }

    /// Runs `f` when the row is clicked, or Enter or Space is pressed on it. The row becomes activatable.
    pub fn connect_activated(&self, f: impl Fn() + 'static) {
        self.widget.set_activatable(true);
        // A list box reports an activated row on the list, so the handler goes on whichever list the row is in.
        let on_list: RefCell<Option<(glib::WeakRef<gtk::ListBox>, glib::SignalHandlerId)>> = RefCell::new(None);
        let f = Rc::new(f);
        let follow = move |row: &gtk::ListBoxRow| {
            if let Some((list, handler)) = on_list.take()
                && let Some(list) = list.upgrade()
            {
                list.disconnect(handler);
            }
            let Some(list) = row.parent().and_downcast::<gtk::ListBox>() else {
                return;
            };
            let (row, f) = (row.downgrade(), f.clone());
            let handler = list.connect_row_activated(move |_, activated| {
                if row.upgrade().as_ref() == Some(activated) {
                    f();
                }
            });
            on_list.replace(Some((list.downgrade(), handler)));
        };
        follow(&self.widget);
        self.widget.connect_parent_notify(follow);
    }

    /// A row whose control is where the keyboard stops, with a click anywhere on the row going to it. The row's title
    /// names the control and its subtitle describes it, as a screen reader on the control says them: by the labels,
    /// so that a new title or subtitle reaches it, and an empty, hidden subtitle adds nothing.
    fn carrying(self, control: &impl IsA<gtk::Widget>) -> Self {
        self.widget.set_focusable(false);
        self.set_activatable_widget(control);
        control.as_ref().update_relation(&[
            gtk::accessible::Relation::LabelledBy(&[self.title.upcast_ref()]),
            gtk::accessible::Relation::DescribedBy(&[self.subtitle.upcast_ref()]),
        ]);
        self
    }

    /// A row whose second line is a field: the title names it, in small.
    fn field(title: &str, field: &impl IsA<gtk::Widget>) -> Self {
        let row = Row::new(title, "");
        row.widget.add_css_class("entry-row");
        field.as_ref().set_hexpand(true);
        row.text.append(field);
        // The row is outlined while the keyboard is in the field: omui.css draws it by this class.
        let typing = gtk::EventControllerFocus::new();
        typing.connect_contains_focus_notify({
            let widget = row.widget.downgrade();
            move |typing| {
                if let Some(widget) = widget.upgrade() {
                    crate::set_class(&widget, "typing", typing.contains_focus());
                }
            }
        });
        field.as_ref().add_controller(typing);
        row.carrying(field)
    }
}

/// Up and Down, for a row: to the row before or after it, from the row or from a control in it. A list box's own
/// arrow keys go only to a row that takes the keyboard itself, and never out of the list or into a list inside it.
/// Past the last row the keyboard stays where it is.
fn arrows() -> gtk::EventControllerKey {
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(|keys, key, _, modifiers| {
        let down = match key {
            gdk::Key::Up | gdk::Key::KP_Up => false,
            gdk::Key::Down | gdk::Key::KP_Down => true,
            _ => return glib::Propagation::Proceed,
        };
        let held = gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK | gdk::ModifierType::SHIFT_MASK;
        let row = keys.widget();
        let focus = row.as_ref().and_then(|row| row.root()).and_then(|root| root.focus());
        let list = row.as_ref().and_then(|row| row.parent()).and_downcast::<gtk::ListBox>();
        let (Some(row), Some(focus), Some(list)) = (row, focus, list) else {
            return glib::Propagation::Proceed;
        };
        // In a popover of the row's (an open drop-down) the arrows are the popover's. In a list that selects, they
        // are the list's: its selection goes with them.
        if modifiers.intersects(held)
            || focus.native() != row.native()
            || list.selection_mode() != gtk::SelectionMode::None
        {
            return glib::Propagation::Proceed;
        }
        // The outermost list: an expander's rows are in lists inside a row of it. Then, for a group's list, the
        // groups beside it.
        let mut within: gtk::Widget = list.upcast();
        while let Some(outer) = within
            .parent()
            .and_then(|parent| parent.ancestor(gtk::ListBox::static_type()))
        {
            within = outer;
        }
        if let Some(column) = within
            .parent()
            .filter(|group| group.has_css_class("group"))
            .and_then(|group| group.parent())
        {
            within = column;
        }
        let mut rows = Vec::new();
        rows_under(&within, &mut rows);
        let at = rows.iter().position(|other| *other == row).unwrap_or(0);
        let mut onward: Box<dyn Iterator<Item = &gtk::Widget>> = if down {
            Box::new(rows[(at + 1).min(rows.len())..].iter())
        } else {
            Box::new(rows[..at].iter().rev())
        };
        // The row, or the control it carries. A disabled row takes neither, and is passed by.
        onward.any(|row| row.grab_focus() || row.child_focus(gtk::DirectionType::TabForward));
        glib::Propagation::Stop
    });
    keys
}

/// The rows on screen under `widget`, in order: an expander's own row, then the rows it shows while open.
fn rows_under(widget: &gtk::Widget, rows: &mut Vec<gtk::Widget>) {
    let mut child = widget.first_child();
    while let Some(next) = child {
        if next.is_mapped() {
            if next.is::<gtk::ListBoxRow>() && !next.has_css_class("expander-row") {
                rows.push(next.clone());
            } else {
                rows_under(&next, rows);
            }
        }
        child = next.next_sibling();
    }
}

/// A row to type one line in, named by `title` (`adw::EntryRow`).
pub fn entry_row(title: &str, text: &str) -> (Row, gtk::Entry) {
    let entry = gtk::Entry::builder().text(text).has_frame(false).build();
    (Row::field(title, &entry), entry)
}

/// A row to type a secret in, with a button that shows what was typed (`adw::PasswordEntryRow`).
pub fn password_row(title: &str) -> (Row, gtk::PasswordEntry) {
    let entry = gtk::PasswordEntry::new();
    let row = Row::field(title, &entry);
    // GTK's own peek icon comes from the desktop's icon theme, and nothing in CSS tells its two states apart.
    let peek = icons::icon_button(icons::name::SHOW, SHOW_TEXT);
    peek.set_valign(gtk::Align::Center);
    peek.connect_clicked({
        let entry = entry.downgrade();
        move |peek| {
            // A password entry keeps its text in a `gtk::Text`, which is what hides it.
            let text = entry
                .upgrade()
                .and_then(|entry| entry.delegate())
                .and_downcast::<gtk::Text>();
            let Some(text) = text else { return };
            // On a `gtk::Text` this is whether its characters are shown, not the widget.
            let shown = !gtk::Text::is_visible(&text);
            text.set_visibility(shown);
            peek.set_icon_name(if shown { icons::name::HIDE } else { icons::name::SHOW });
            let says = if shown { HIDE_TEXT } else { SHOW_TEXT };
            peek.set_tooltip_text(Some(says));
            peek.update_property(&[gtk::accessible::Property::Label(says)]);
        }
    });
    row.add_suffix(&peek);
    (row, entry)
}

const SHOW_TEXT: &str = "Show the text";
const HIDE_TEXT: &str = "Hide the text";

/// A row with a switch at its end (`adw::SwitchRow`).
pub fn switch_row(title: &str, subtitle: &str, active: bool) -> (Row, gtk::Switch) {
    let switch = gtk::Switch::builder().active(active).valign(gtk::Align::Center).build();
    let row = Row::new(title, subtitle);
    row.add_suffix(&switch);
    (row.carrying(&switch), switch)
}

/// A row with a choice from `model` at its end (`adw::ComboRow`).
pub fn combo_row(title: &str, subtitle: &str, model: &gtk::StringList, selected: u32) -> (Row, gtk::DropDown) {
    let dropdown = gtk::DropDown::builder()
        .model(model)
        .selected(selected)
        .valign(gtk::Align::Center)
        .build();
    let row = Row::new(title, subtitle);
    row.add_suffix(&dropdown);
    (row.carrying(&dropdown), dropdown)
}

/// A row that opens to show more rows under it (`adw::ExpanderRow`).
pub struct ExpanderRow {
    pub widget: gtk::ListBoxRow,
    header: Row,
    arrow: gtk::Image,
    revealer: gtk::Revealer,
    rows: gtk::ListBox,
}

impl ExpanderRow {
    pub fn new(title: &str, subtitle: &str) -> Self {
        let this = Self {
            // The outer row only holds the two lists: the header is what is clicked and focused.
            widget: gtk::ListBoxRow::builder()
                .css_classes(["expander-row"])
                .activatable(false)
                .focusable(false)
                .build(),
            header: Row::new(title, subtitle),
            arrow: icons::icon(icons::name::FORWARD),
            revealer: gtk::Revealer::new(),
            rows: gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).build(),
        };
        this.header.suffixes.append(&this.arrow);
        this.header.suffixes.set_visible(true);
        let header = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).build();
        header.append(&this.header.widget);
        this.revealer.set_child(Some(&this.rows));
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&header);
        content.append(&this.revealer);
        this.widget.set_child(Some(&content));
        this.set_expanded(false);
        this.header.connect_activated({
            let (widget, arrow, revealer, header) = (
                this.widget.downgrade(),
                this.arrow.downgrade(),
                this.revealer.downgrade(),
                this.header.widget.downgrade(),
            );
            move || {
                if let (Some(widget), Some(arrow), Some(revealer), Some(header)) =
                    (widget.upgrade(), arrow.upgrade(), revealer.upgrade(), header.upgrade())
                {
                    expand(&widget, &header, &arrow, &revealer, !revealer.reveals_child());
                }
            }
        });
        this
    }

    /// A row under the header, shown while the row is expanded.
    pub fn add_row(&self, widget: &impl IsA<gtk::Widget>) {
        self.rows.append(widget);
    }

    pub fn add_prefix(&self, widget: &impl IsA<gtk::Widget>) {
        self.header.add_prefix(widget);
    }

    /// A widget at the end of the header, before the arrow.
    pub fn add_suffix(&self, widget: &impl IsA<gtk::Widget>) {
        widget.as_ref().insert_before(&self.header.suffixes, Some(&self.arrow));
    }

    pub fn set_expanded(&self, on: bool) {
        expand(&self.widget, &self.header.widget, &self.arrow, &self.revealer, on);
    }
}

fn expand(widget: &gtk::ListBoxRow, header: &gtk::ListBoxRow, arrow: &gtk::Image, revealer: &gtk::Revealer, on: bool) {
    revealer.set_reveal_child(on);
    crate::set_class(widget, "expanded", on);
    arrow.set_icon_name(Some(if on { icons::name::DOWN } else { icons::name::FORWARD }));
    header.update_state(&[gtk::accessible::State::Expanded(Some(on))]);
}
