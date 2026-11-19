// SPDX-License-Identifier: MIT
//! Every omui widget in one window, for looking at a change in dark and light themes (dev/preview.sh).
//!
//!   OMUI_GALLERY_PAGE=controls|icons|states|stock   the page to open on
//!   OMUI_SHOT=<file.png>                      with feature `dev`: write the window through `omui::dev::shot`

use std::rc::Rc;

use omui::{
    Banner, EmptyState, Shell, Theme, Title,
    gtk::{self, glib, prelude::*},
    icons::name,
};

/// What an app adds on top of omui.css: only its own layout.
const APP_CSS: &str = "
.gallery-page { padding: 20px 28px; }
.reader { font-family: serif; }
.swatch { border: 1px solid alpha(@om_border, 0.6); border-radius: 5px; min-height: 28px; min-width: 76px; }
.swatch.bg { background: @om_bg; }
.swatch.surface { background: @om_surface; }
.swatch.muted { background: @om_muted; }
.swatch.highlight { background: @om_highlight; }
.swatch.border { background: @om_border; }
.swatch.dim_text { background: @om_dim_text; }
.swatch.text { background: @om_text; }
.swatch.accent { background: @om_accent; }
.swatch.danger { background: @om_danger; }
.swatch.success { background: @om_success; }
.swatch.warning { background: @om_warning; }
";

const PAGES: &[(&str, &str, &str)] = &[
    ("controls", name::SETTINGS, "Controls"),
    ("icons", name::IMAGE, "Icons"),
    ("states", name::INFO, "States"),
    ("stock", name::CHECKED, "GTK's own"),
];

fn main() -> glib::ExitCode {
    omui::application(
        "io.github.ferdousbhai.omui.Gallery",
        "omui-gallery",
        "omui gallery",
        APP_CSS,
        open,
    )
    .run()
}

fn open(app: &gtk::Application) {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("omui gallery")
        .default_width(1100)
        .default_height(760)
        .build();
    let theme = Theme::get().expect("omui::application installs the theme");

    let title = Rc::new(Title::new("omui gallery", &theme.tokens().name));
    let header = omui::header(Some(title.widget.upcast_ref()));
    let toggle = omui::icon_button(name::SIDEBAR, "Hide the sidebar");
    let forward = omui::icon_button(name::FORWARD, "Forward");
    forward.set_sensitive(false);
    header.pack_start(&toggle);
    header.pack_start(&omui::icon_button(name::BACK, "Back"));
    header.pack_start(&forward);
    header.pack_end(&omui::icon_button(name::ADD, "New"));
    header.pack_end(&omui::icon_button(name::SEARCH, "Search"));
    window.set_titlebar(Some(&header));
    // The subtitle names the theme, and follows a switch for as long as the window lives.
    theme.on_change(&window, move |tokens| title.set_subtitle(&tokens.name));

    let stack = gtk::Stack::builder().hexpand(true).build();
    let list = gtk::ListBox::builder().vexpand(true).build();
    for (id, icon, text) in PAGES {
        list.append(&omui::sidebar_row(
            id,
            Some(icon),
            text,
            (*id == "icons").then(|| name::ALL.len().to_string()).as_deref(),
        ));
    }
    let footer = gtk::Button::builder().css_classes(["sidebar-footer"]).build();
    let footer_content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    footer_content.append(&omui::icon(name::OK));
    footer_content.append(&omui::label("Connected", ""));
    footer.set_child(Some(&footer_content));
    let sidebar = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["sidebar"])
        .width_request(220)
        .build();
    sidebar.append(&list);
    sidebar.append(&footer);
    toggle.connect_clicked({
        let sidebar = sidebar.clone();
        move |_| sidebar.set_visible(!sidebar.is_visible())
    });

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    content.append(&sidebar);
    content.append(&stack);
    let shell = Shell::new(&window, &content);

    stack.add_named(&controls(), Some("controls"));
    stack.add_named(&icons(), Some("icons"));
    stack.add_named(&states(&shell), Some("states"));
    stack.add_named(&stock(), Some("stock"));
    list.connect_row_selected({
        let stack = stack.clone();
        move |_, row| {
            if let Some(row) = row {
                stack.set_visible_child_name(&row.widget_name());
            }
        }
    });
    let first = std::env::var("OMUI_GALLERY_PAGE").unwrap_or_default();
    let index = PAGES.iter().position(|(id, ..)| *id == first).unwrap_or(0);
    list.select_row(list.row_at_index(index as i32).as_ref());
    if PAGES[index].0 == "states" {
        shell.toast_action("Moved 3 notes to the trash", "Undo", 30, {
            let shell = Rc::downgrade(&shell);
            move || {
                if let Some(shell) = shell.upgrade() {
                    shell.toast("Restored 3 notes");
                }
            }
        });
    }
    window.present();

    #[cfg(feature = "dev")]
    if let Some(path) = std::env::var_os("OMUI_SHOT") {
        glib::timeout_add_local_once(std::time::Duration::from_millis(1500), move || {
            match omui::dev::shot(&window, std::path::Path::new(&path)) {
                Ok(()) => println!("gallery: wrote {}", path.display()),
                Err(e) => eprintln!("gallery: {e}"),
            }
        });
    }
}

fn page() -> (gtk::ScrolledWindow, gtk::Box) {
    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["gallery-page"])
        .build();
    let scroll = gtk::ScrolledWindow::builder()
        .child(&column)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    (scroll, column)
}

fn row(children: &[&gtk::Widget]) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    for child in children {
        row.append(*child);
    }
    row
}

fn button(text: &str, classes: &[&str]) -> gtk::Widget {
    gtk::Button::builder()
        .label(text)
        .css_classes(classes.to_vec())
        .build()
        .upcast()
}

fn controls() -> gtk::ScrolledWindow {
    let (scroll, column) = page();
    column.append(&omui::label("Controls", "page-title"));

    column.append(&omui::label("Buttons", "section-title"));
    let disabled = button("Disabled", &[]);
    disabled.set_sensitive(false);
    let with_icon = gtk::Button::new();
    let with_icon_content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    with_icon_content.append(&omui::icon(name::REFRESH));
    with_icon_content.append(&gtk::Label::new(Some("Sync now")));
    with_icon.set_child(Some(&with_icon_content));
    let circular = gtk::Button::builder()
        .icon_name(name::ADD)
        .css_classes(["circular"])
        .tooltip_text("Add")
        .build();
    column.append(&row(&[
        &button("Default", &[]),
        &button("Suggested", &["suggested-action"]),
        &button("Destructive", &["destructive-action"]),
        &button("Flat", &["flat"]),
        &button("Pill", &["pill"]),
        circular.upcast_ref(),
        with_icon.upcast_ref(),
        &disabled,
    ]));
    let chips: Vec<gtk::Widget> = ["All", "Unread", "Starred", "With attachments"]
        .iter()
        .enumerate()
        .map(|(i, text)| {
            gtk::ToggleButton::builder()
                .label(*text)
                .css_classes(["chip"])
                .active(i == 1)
                .build()
                .upcast()
        })
        .collect();
    column.append(&row(&chips.iter().collect::<Vec<_>>()));

    column.append(&omui::label("Fields", "section-title"));
    let search = gtk::Entry::builder()
        .placeholder_text("Search everything")
        .primary_icon_name(name::SEARCH)
        .css_classes(["search-large"])
        .hexpand(true)
        .build();
    column.append(&search);
    let filled = gtk::Entry::builder().text("north pier, 09:00").hexpand(true).build();
    let empty = gtk::Entry::builder()
        .placeholder_text("Placeholder")
        .hexpand(true)
        .build();
    let off = gtk::Entry::builder()
        .text("Disabled")
        .sensitive(false)
        .hexpand(true)
        .build();
    column.append(&row(&[filled.upcast_ref(), empty.upcast_ref(), off.upcast_ref()]));

    column.append(&omui::label("Text", "section-title"));
    let kinds = |kinds: &[(&str, &str)]| {
        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .hexpand(true)
            .build();
        for (class, text) in kinds {
            column.append(&omui::label(text, class));
        }
        column
    };
    let titles = kinds(&[
        ("title-1", "title-1, page-title"),
        ("title-2", "title-2"),
        ("title-4", "title-4, section-title"),
    ]);
    let body = kinds(&[
        ("heading", "heading"),
        ("", "Body text in the interface font, JetBrains Mono."),
        ("dim-label", "dim-label: secondary text"),
        ("meta", "meta: yesterday, 14:02 · 3 items"),
        ("caption", "caption"),
    ]);
    column.append(&row(&[titles.upcast_ref(), body.upcast_ref()]));
    let link = gtk::Label::builder()
        .label("A <a href=\"https://omarchy.org\">link</a> inside a label.")
        .use_markup(true)
        .xalign(0.0)
        .build();
    column.append(&link);
    // The family is the app's to change, for a pane as much as for one widget.
    let reader = gtk::Box::builder().css_classes(["reader"]).spacing(12).build();
    reader.append(&omui::label("An app's own rule sets a proportional face here,", ""));
    reader.append(&omui::label(
        "and monospace returns to the interface font.",
        "monospace",
    ));
    column.append(&reader);
    column.append(&row(&[
        omui::label("error", "error").upcast_ref(),
        omui::label("warning", "warning").upcast_ref(),
        omui::label("success", "success").upcast_ref(),
        omui::label("accent", "accent").upcast_ref(),
        omui::label("1 234.50 numeric", "numeric").upcast_ref(),
        omui::badge("12", "").upcast_ref(),
        omui::badge("Failed", "error").upcast_ref(),
        omui::badge("Stale", "warning").upcast_ref(),
        omui::badge("Synced", "success").upcast_ref(),
    ]));

    column.append(&omui::label("Tokens", "section-title"));
    let swatches = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    for token in [
        "bg",
        "surface",
        "muted",
        "highlight",
        "border",
        "dim_text",
        "text",
        "accent",
        "danger",
        "success",
        "warning",
    ] {
        let swatch = gtk::Box::builder().css_classes(["swatch", token]).build();
        let cell = gtk::Box::new(gtk::Orientation::Vertical, 2);
        cell.append(&swatch);
        cell.append(&omui::label(token, "caption"));
        swatches.append(&cell);
    }
    column.append(&swatches);

    column.append(&omui::label("Results", "section-title"));
    let results = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).build();
    for (title, snippet, when) in [
        (
            "Your ferry booking",
            "Two adults, north pier, 09:00. Please arrive 15 minutes early.",
            "today",
        ),
        ("Hotel confirmation", "Check-in from 14:00.", "yesterday"),
    ] {
        let text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .hexpand(true)
            .build();
        text.append(&omui::label(title, "result-title"));
        text.append(&omui::label(snippet, "snippet"));
        let content = row(&[
            omui::icon(name::MAIL).upcast_ref(),
            text.upcast_ref(),
            omui::label(when, "meta").upcast_ref(),
        ]);
        results.append(
            &gtk::ListBoxRow::builder()
                .css_classes(["result"])
                .child(&content)
                .margin_bottom(6)
                .build(),
        );
    }
    column.append(&results);
    scroll
}

fn icons() -> gtk::ScrolledWindow {
    let (scroll, column) = page();
    column.append(&omui::label("Icons", "page-title"));
    column.append(&omui::label(
        "Lucide outlines as symbolic icons: each takes the colour of the text around it.",
        "dim-label",
    ));
    let grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .min_children_per_line(4)
        .max_children_per_line(6)
        .homogeneous(true)
        .row_spacing(4)
        .column_spacing(4)
        .build();
    for icon in name::ALL {
        let short = icon.trim_start_matches("om-").trim_end_matches("-symbolic");
        grid.append(&row(&[
            omui::icon(icon).upcast_ref(),
            omui::label(short, "meta").upcast_ref(),
        ]));
    }
    column.append(&grid);

    column.append(&omui::label("Sizes and colours", "section-title"));
    let sized = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    for size in [16, 24, 32, 48, 64] {
        let image = omui::icon(name::SETTINGS);
        image.set_pixel_size(size);
        sized.append(&image);
    }
    for (icon, class) in [
        (name::WARNING, "warning"),
        (name::DELETE, "error"),
        (name::OK, "success"),
        (name::STARRED, "accent"),
        (name::STAR, "dim-label"),
    ] {
        let image = omui::icon(icon);
        image.set_pixel_size(32);
        image.add_css_class(class);
        sized.append(&image);
    }
    column.append(&sized);
    column.append(&omui::label("Icon buttons", "section-title"));
    let off = omui::icon_button(name::DELETE, "Delete (disabled)");
    off.set_sensitive(false);
    column.append(&row(&[
        omui::icon_button(name::EDIT, "Edit").upcast_ref(),
        omui::icon_button(name::COPY, "Copy").upcast_ref(),
        omui::icon_button(name::EXTERNAL_LINK, "Open in the browser").upcast_ref(),
        off.upcast_ref(),
    ]));
    scroll
}

fn states(shell: &Rc<Shell>) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let banner = Banner::new(
        "You are signed out of iCloud. Notes will not sync until you sign in.",
        Some("Sign in"),
    );
    let later = banner.add_button("Later", false);
    banner.set_revealed(true);
    banner.connect_button_clicked({
        let shell = Rc::downgrade(shell);
        move || {
            if let Some(shell) = shell.upgrade() {
                shell.toast("Signing in…");
            }
        }
    });
    later.connect_clicked({
        let banner = banner.widget.clone();
        move |_| banner.set_reveal_child(false)
    });
    column.append(&banner.widget);

    let empty = EmptyState::new(
        Some(name::SEARCH),
        "No results",
        Some("Nothing matches “ferry timetable”. Try fewer words, or search another source."),
    );
    empty.set_child(Some(&button("Clear the search", &["pill", "suggested-action"])));
    let compact = EmptyState::new(
        Some(name::CALENDAR),
        "Nothing today",
        Some("Events appear here as they are added."),
    )
    .compact();
    let list = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["gallery-page"])
        .width_request(340)
        .build();
    list.append(&omui::label("Today", "section-title"));
    list.append(&compact.widget);
    list.append(&omui::label("Inbox", "section-title"));
    list.append(&omui::label("Loading…", "status-line"));
    column.append(&row(&[
        empty.widget.upcast_ref(),
        gtk::Separator::new(gtk::Orientation::Vertical).upcast_ref(),
        list.upcast_ref(),
    ]));
    column
}

/// Widgets omui 0.1 has no rule for. They keep GTK's built-in look, which has to be the palette's variant: a white
/// calendar in a dark window is the built-in theme left light.
fn stock() -> gtk::ScrolledWindow {
    let (scroll, column) = page();
    column.append(&omui::label("GTK's own", "page-title"));
    column.append(&omui::label(
        "Widgets without an omui rule yet, in the built-in theme's variant for this palette.",
        "dim-label",
    ));
    let radio = gtk::CheckButton::builder().label("Radio").active(true).build();
    let other = gtk::CheckButton::builder().label("Other").group(&radio).build();
    column.append(&row(&[
        gtk::Switch::builder()
            .active(true)
            .valign(gtk::Align::Center)
            .build()
            .upcast_ref(),
        gtk::CheckButton::builder()
            .label("Check")
            .active(true)
            .build()
            .upcast_ref(),
        radio.upcast_ref(),
        other.upcast_ref(),
        gtk::SpinButton::with_range(0.0, 10.0, 1.0).upcast_ref(),
        gtk::DropDown::from_strings(&["Week", "Month"]).upcast_ref(),
    ]));
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    scale.set_value(40.0);
    scale.set_hexpand(true);
    let progress = gtk::ProgressBar::builder()
        .fraction(0.4)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .build();
    column.append(&row(&[scale.upcast_ref(), progress.upcast_ref()]));
    let notebook = gtk::Notebook::builder().hexpand(true).build();
    for tab in ["First", "Second"] {
        let body = omui::label(&format!("{tab} page of a notebook."), "gallery-page");
        notebook.append_page(&body, Some(&gtk::Label::new(Some(tab))));
    }
    let frame = gtk::Frame::builder()
        .label("Frame")
        .child(&omui::label("Inside a frame.", "gallery-page"))
        .build();
    let side = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .hexpand(true)
        .build();
    side.append(&notebook);
    side.append(&frame);
    column.append(&row(&[gtk::Calendar::new().upcast_ref(), side.upcast_ref()]));
    scroll
}
