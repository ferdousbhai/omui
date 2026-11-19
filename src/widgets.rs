// SPDX-License-Identifier: MIT
//! The small widgets every app builds the same way.

use gtk::prelude::*;

use crate::icons;

/// A left-aligned label that wraps, with the style class `class` (none when empty).
pub fn label(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::builder().label(text).xalign(0.0).wrap(true).build();
    if !class.is_empty() {
        label.add_css_class(class);
    }
    label
}

/// Empties a container through its own remove: a list box or flow box keeps bookkeeping a bare unparent would break.
pub fn clear(container: &impl IsA<gtk::Widget>) {
    let container = container.as_ref();
    if let Some(list) = container.downcast_ref::<gtk::ListBox>() {
        list.remove_all();
    } else if let Some(flow) = container.downcast_ref::<gtk::FlowBox>() {
        flow.remove_all();
    } else if let Some(b) = container.downcast_ref::<gtk::Box>() {
        while let Some(child) = b.first_child() {
            b.remove(&child);
        }
    } else {
        while let Some(child) = container.first_child() {
            child.unparent();
        }
    }
}

/// Adds or removes a style class.
pub fn set_class(widget: &impl IsA<gtk::Widget>, class: &str, on: bool) {
    if on {
        widget.add_css_class(class);
    } else {
        widget.remove_css_class(class);
    }
}

/// A row of a `.sidebar` list: an icon, a name, and a count at the end. `id` becomes the row's widget name, which is
/// how a selection handler tells rows apart.
pub fn sidebar_row(id: &str, icon: Option<&str>, text: &str, count: Option<&str>) -> gtk::ListBoxRow {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    if let Some(icon) = icon {
        content.append(&icons::icon(icon));
    }
    content.append(
        &gtk::Label::builder()
            .label(text)
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build(),
    );
    if let Some(count) = count {
        content.append(
            &gtk::Label::builder()
                .label(count)
                .css_classes(["count", "numeric"])
                .build(),
        );
    }
    let row = gtk::ListBoxRow::builder()
        .name(id)
        .css_classes(["sidebar-row"])
        .child(&content)
        .build();
    let name = count.map_or_else(|| text.to_owned(), |count| format!("{text}, {count}"));
    row.update_property(&[gtk::accessible::Property::Label(&name)]);
    row
}

/// A small tinted label: a count, a state. `class` picks the tint (`error`, `warning`, `success`; accent when empty).
pub fn badge(text: &str, class: &str) -> gtk::Label {
    let badge = gtk::Label::builder()
        .label(text)
        .css_classes(["badge"])
        .valign(gtk::Align::Center)
        .build();
    if !class.is_empty() {
        badge.add_css_class(class);
    }
    badge
}

/// A message across the top of a window or pane with up to a few buttons, hidden until revealed (`adw::Banner`).
pub struct Banner {
    pub widget: gtk::Revealer,
    content: gtk::Box,
    title: gtk::Label,
    button: gtk::Button,
}

impl Banner {
    /// `button` labels the banner's own button; None leaves it out.
    pub fn new(title: &str, button: Option<&str>) -> Self {
        let this = Self {
            widget: gtk::Revealer::builder()
                .transition_type(gtk::RevealerTransitionType::SlideDown)
                .build(),
            content: gtk::Box::builder().css_classes(["banner"]).build(),
            title: gtk::Label::builder().xalign(0.0).wrap(true).hexpand(true).build(),
            button: gtk::Button::new(),
        };
        this.content.append(&this.title);
        this.content.append(&this.button);
        this.widget.set_child(Some(&this.content));
        this.set_title(title);
        this.set_button_label(button);
        this
    }

    pub fn set_title(&self, title: &str) {
        self.title.set_label(title);
    }

    pub fn set_button_label(&self, label: Option<&str>) {
        self.button.set_label(label.unwrap_or_default());
        self.button.set_visible(label.is_some());
    }

    /// One more button, after those already there. The caller connects it.
    pub fn add_button(&self, label: &str, suggested: bool) -> gtk::Button {
        let button = gtk::Button::with_label(label);
        set_class(&button, "suggested-action", suggested);
        self.content.append(&button);
        button
    }

    pub fn set_revealed(&self, on: bool) {
        self.widget.set_reveal_child(on);
    }

    pub fn is_revealed(&self) -> bool {
        self.widget.reveals_child()
    }

    pub fn connect_button_clicked(&self, f: impl Fn() + 'static) {
        self.button.connect_clicked(move |_| f());
    }
}

/// What a page or list shows when it has nothing: an icon, a title, a line of explanation, and room for one widget
/// such as a button (`adw::StatusPage`). It centres itself in the space it is given.
pub struct EmptyState {
    pub widget: gtk::Box,
    title: gtk::Label,
    description: gtk::Label,
    child: gtk::Box,
}

impl EmptyState {
    pub fn new(icon: Option<&str>, title: &str, description: Option<&str>) -> Self {
        let line = || {
            gtk::Label::builder()
                .wrap(true)
                .justify(gtk::Justification::Center)
                .max_width_chars(48)
                .build()
        };
        let this = Self {
            widget: gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .css_classes(["empty-state"])
                .halign(gtk::Align::Center)
                .valign(gtk::Align::Center)
                .hexpand(true)
                .vexpand(true)
                .accessible_role(gtk::AccessibleRole::Group)
                .build(),
            title: line(),
            description: line(),
            child: gtk::Box::builder().halign(gtk::Align::Center).visible(false).build(),
        };
        if let Some(icon) = icon {
            this.widget.append(&icons::icon(icon));
        }
        this.title.add_css_class("title-2");
        this.widget.append(&this.title);
        this.widget.append(&this.description);
        this.widget.append(&this.child);
        this.set_title(title);
        this.set_description(description.unwrap_or_default());
        this
    }

    /// The small form, for an empty list inside a page rather than a whole empty page.
    pub fn compact(self) -> Self {
        self.widget.add_css_class("compact");
        self
    }

    /// The widget under the text, or none.
    pub fn set_child(&self, child: Option<&gtk::Widget>) {
        clear(&self.child);
        if let Some(child) = child {
            self.child.append(child);
        }
        self.child.set_visible(child.is_some());
    }

    pub fn set_title(&self, title: &str) {
        self.title.set_label(title);
        self.widget.update_property(&[gtk::accessible::Property::Label(title)]);
    }

    /// An empty description takes no space.
    pub fn set_description(&self, description: &str) {
        self.description.set_label(description);
        self.description.set_visible(!description.is_empty());
    }
}
