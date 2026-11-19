// SPDX-License-Identifier: MIT
//! Every omui widget in one window, for looking at a change in dark and light themes (dev/preview.sh).
//!
//!   OMUI_GALLERY_PAGE=controls|icons|states|dialogs|inputs|menus|views|split|nav|tabs|rows|avatars
//!                                             the page to open on, with the keyboard in it
//!   OMUI_GALLERY_OPEN=confirm|send|rename|upload|stop|tall|alert|about|window|log|dropdown|menu|popover|
//!                     contact|recovery|settings|push|toasts
//!                                             what to open on its page, as a click or an action would
//!   OMUI_GALLERY_TRACE=1                      print each focus change, dialog answer, shortcut, toast dismissed,
//!                                             change of a split, a stack of pages or an expander, a divider moved,
//!                                             and the window's width once it is laid out (dev/keys.sh reads them)
//!   OMUI_SHOT=<file.png>                      with feature `dev`: write the window through `omui::dev::shot`
//!
//! Built with feature `qa` it runs dev/qa.sh's scripts (dev/qa/*.jsonl), and has `win.qa-critical`, which makes GTK
//! log a critical.

use std::{cell::Cell, rc::Rc};

use omui::{
    Appearance, Banner, Dialog, EmptyState, ExpanderRow, Group, Nav, Row, Shell, Split, Theme, Title, Toast,
    gtk::{self, gio, glib, prelude::*},
    icons::name,
};

/// What an app adds on top of omui.css: only its own layout.
const APP_CSS: &str = "
.gallery-page { padding: 20px 28px; }
.reader { font-family: serif; }
.upload { border-spacing: 10px; padding: 18px 20px; }
.note { padding: 8px 10px; }
.tile { padding: 14px 18px; }
.card { border-spacing: 6px; padding: 12px 12px 24px; }
.code { padding: 4px 0 12px; }
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
    ("dialogs", name::CHAT, "Dialogs"),
    ("inputs", name::CHECKED, "Inputs"),
    ("menus", name::DOWN, "Menus"),
    ("views", name::CALENDAR, "Views"),
    ("split", name::SIDEBAR, "Split"),
    ("nav", name::FORWARD, "Pages"),
    ("tabs", name::GRID, "Tabs"),
    ("rows", name::MENU, "Rows"),
    ("avatars", name::USERS, "Avatars"),
];

/// What `OMUI_GALLERY_OPEN` can name: the page it is on, and what a click on it does.
type Openers = Vec<(&'static str, &'static str, Rc<dyn Fn()>)>;

fn trace(line: &str) {
    if std::env::var_os("OMUI_GALLERY_TRACE").is_some() {
        println!("{line}");
    }
}

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
    let toggle = gtk::ToggleButton::builder()
        .icon_name(name::SIDEBAR)
        .css_classes(["header-action"])
        .tooltip_text("Sidebar")
        .active(true)
        .build();
    let forward = omui::icon_button(name::FORWARD, "Forward");
    forward.set_sensitive(false);
    header.pack_start(&toggle);
    header.pack_start(&omui::icon_button(name::BACK, "Back"));
    header.pack_start(&forward);
    let more = gio::Menu::new();
    more.append(Some("Play Sound"), Some("win.play-sound"));
    more.append(Some("Lost Mode…"), Some("win.lost-mode"));
    window.add_action(&gio::SimpleAction::new("play-sound", None));
    window.add_action(&gio::SimpleAction::new("lost-mode", None));
    header.pack_end(&omui::icon_menu_button(name::MORE, "Device actions", &more));
    header.pack_end(&omui::icon_button(name::ADD, "New"));
    header.pack_end(&omui::icon_button(name::SEARCH, "Search"));
    window.set_titlebar(Some(&header));
    // The subtitle names the theme, and follows a switch for as long as the window lives.
    theme.on_change(&window, move |tokens| title.set_subtitle(&tokens.name));

    // As wide as the page it shows needs, not as the widest of them: the window can then be narrow.
    let stack = gtk::Stack::builder().hexpand(true).hhomogeneous(false).build();
    let list = gtk::ListBox::builder().vexpand(true).build();
    // Tab passes the sidebar in one step: on its way through, it would show every page it went by.
    omui::single_tab_stop(&list);
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
        .build();
    sidebar.append(&gtk::ScrolledWindow::builder().child(&list).vexpand(true).build());
    sidebar.append(&footer);

    // A sidebar with a button that shows and hides it, as Find My has: too narrow for both, the window shows the
    // sidebar until a page is chosen.
    let split = Split::new(&sidebar, &stack, (200, 260), Some(720));
    toggle.connect_toggled({
        let split = split.downgrade();
        move |toggle| {
            if let Some(split) = split.upgrade() {
                split.set_show_sidebar(toggle.is_active());
            }
        }
    });
    // A page named on the command line is shown, where a narrow window would start on the sidebar.
    let asked = Cell::new(
        ["OMUI_GALLERY_PAGE", "OMUI_GALLERY_OPEN"]
            .iter()
            .any(|v| std::env::var_os(v).is_some()),
    );
    split.connect_changed({
        let toggle = toggle.downgrade();
        move |split| {
            trace_split("window", split);
            if split.is_collapsed() && asked.take() {
                split.set_show_sidebar(false);
            }
            if let Some(toggle) = toggle.upgrade() {
                toggle.set_active(split.shows_sidebar());
            }
        }
    });
    let shell = Shell::new(&window, &split.widget);
    shortcuts(app, &window, &stack, &shell);

    stack.add_named(&controls(), Some("controls"));
    stack.add_named(&icons(), Some("icons"));
    let mut openers = Openers::new();
    stack.add_named(&states(&shell, &mut openers), Some("states"));
    stack.add_named(&dialogs(app, &mut openers), Some("dialogs"));
    stack.add_named(&inputs(&mut openers), Some("inputs"));
    stack.add_named(&menus(&mut openers), Some("menus"));
    stack.add_named(&views(), Some("views"));
    stack.add_named(&contacts(&shell, &mut openers), Some("split"));
    stack.add_named(&setup(&mut openers), Some("nav"));
    stack.add_named(&tabbed(), Some("tabs"));
    stack.add_named(&rows(&shell, &mut openers), Some("rows"));
    stack.add_named(&avatars(), Some("avatars"));
    list.connect_row_selected({
        let stack = stack.clone();
        move |_, row| {
            if let Some(row) = row {
                stack.set_visible_child_name(&row.widget_name());
            }
        }
    });
    // Chosen, not merely moved to with the arrows: in a narrow window the page takes the sidebar's place.
    list.connect_row_activated({
        let split = split.downgrade();
        move |_, _| {
            if let Some(split) = split.upgrade().filter(|split| split.is_collapsed()) {
                split.set_show_sidebar(false);
            }
        }
    });
    let opener = std::env::var("OMUI_GALLERY_OPEN").ok();
    let opener = openers.iter().find(|(id, ..)| Some(*id) == opener.as_deref());
    let first = opener.map_or_else(
        || std::env::var("OMUI_GALLERY_PAGE").unwrap_or_default(),
        |(_, page, _)| (*page).to_owned(),
    );
    let index = PAGES.iter().position(|(id, ..)| *id == first).unwrap_or(0);
    list.select_row(list.row_at_index(index as i32).as_ref());
    if PAGES[index].0 == "states" && opener.is_none() {
        shell.toast_action("Moved 3 notes to the trash", "Undo", 30, {
            let shell = Rc::downgrade(&shell);
            move || {
                if let Some(shell) = shell.upgrade() {
                    shell.toast("Restored 3 notes");
                }
            }
        });
    }
    if std::env::var_os("OMUI_GALLERY_TRACE").is_some() {
        window.connect_focus_widget_notify(|window| trace(&format!("focus: {}", focus_name(RootExt::focus(window)))));
        // Once the compositor has tiled it: a window wider than its tile is a measure that asks for too much.
        glib::timeout_add_local_once(std::time::Duration::from_millis(1200), {
            let window = window.downgrade();
            move || {
                if let Some(window) = window.upgrade() {
                    trace(&format!("width: {}", window.width()));
                }
            }
        });
    }
    // The shell shows toasts for as long as its `Rc` is held: here, as long as the window.
    let held = std::cell::RefCell::new(Some(shell.clone()));
    window.connect_destroy(move |_| drop(held.take()));
    window.present();
    if opener.is_none() && std::env::var_os("OMUI_GALLERY_PAGE").is_some() {
        stack.child_focus(gtk::DirectionType::TabForward);
    }
    if let Some((.., open)) = opener {
        // After the window has its size, as a click would come.
        glib::timeout_add_local_once(std::time::Duration::from_millis(400), {
            let open = open.clone();
            move || open()
        });
    }

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

/// The keys an app binds in its window, each of the three ways GTK has: Delete from a shortcut in the content
/// (managed, as Photos' viewer has), Ctrl+U as an application accelerator, F5 on the window itself. Each asks or
/// toasts as the app would. None of them may reach the window while a dialog is open over it.
fn shortcuts(app: &gtk::Application, window: &gtk::ApplicationWindow, content: &gtk::Stack, shell: &Rc<Shell>) {
    let delete = gtk::ShortcutController::new();
    delete.set_scope(gtk::ShortcutScope::Managed);
    delete.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("Delete"),
        Some(gtk::CallbackAction::new(|content, _| {
            trace("shortcut: delete");
            omui::confirm(
                content,
                "Delete this photo?",
                "It moves to Recently Deleted on all your devices.",
                "Delete",
                Appearance::Destructive,
                || trace("answer: deleted"),
            );
            glib::Propagation::Stop
        })),
    ));
    content.add_controller(delete);
    let upload = gio::SimpleAction::new("upload", None);
    upload.connect_activate(|_, _| trace("shortcut: upload"));
    window.add_action(&upload);
    app.set_accels_for_action("win.upload", &["<Primary>u"]);
    let refresh = gtk::ShortcutController::new();
    refresh.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("F5"),
        Some(gtk::CallbackAction::new({
            let shell = Rc::downgrade(shell);
            move |_, _| {
                trace("shortcut: refresh");
                if let Some(shell) = shell.upgrade() {
                    shell.toast("Refreshed");
                }
                glib::Propagation::Stop
            }
        })),
    ));
    window.add_controller(refresh);
    // For dev/qa.sh's proof that a GLib critical fails a QA run: a real one, from GTK, as an app's mistake would log.
    #[cfg(feature = "qa")]
    {
        let critical = gio::SimpleAction::new("qa-critical", None);
        critical.connect_activate(|_, _| gtk::Box::default().remove(&gtk::Label::new(None)));
        window.add_action(&critical);
    }
}

/// What has the keyboard, for the trace: a button's label or tooltip, else what kind of widget it is and the first
/// text in it, or the title of the row it is in.
fn focus_name(focus: Option<gtk::Widget>) -> String {
    let Some(widget) = focus else {
        return "nothing".to_owned();
    };
    let label = widget.downcast_ref::<gtk::Button>().and_then(|button| button.label());
    if let Some(name) = label.or_else(|| widget.tooltip_text()) {
        return name.into();
    }
    let kind = widget.css_name();
    // A row of the gallery's own sidebar is "row": the dialogs' cases only need to know the focus came back to it.
    if widget.has_css_class("sidebar-row") {
        return kind.into();
    }
    let row = widget
        .ancestor(gtk::ListBoxRow::static_type())
        .filter(|row| row.has_css_class("om-row"));
    match first_text(&widget).or_else(|| row.as_ref().and_then(first_text)) {
        Some(text) => format!("{kind} {text}"),
        None => kind.into(),
    }
}

fn first_text(widget: &gtk::Widget) -> Option<String> {
    let named = |label: &&gtk::Label| label.is_mapped() && !label.has_css_class("avatar");
    if let Some(label) = widget.downcast_ref::<gtk::Label>().filter(named) {
        return Some(label.label().into());
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        if let Some(text) = first_text(&widget) {
            return Some(text);
        }
        child = widget.next_sibling();
    }
    None
}

fn trace_split(which: &str, split: &Split) {
    let shape = if split.is_collapsed() { "collapsed" } else { "wide" };
    let shown = match (split.is_collapsed(), split.shows_sidebar(), split.shows_content()) {
        (false, true, _) => "both",
        (true, true, false) => "sidebar",
        _ => "content",
    };
    trace(&format!("split: {which} {shape} {shown}"));
}

/// A gallery page of loose widgets. It scrolls sideways too, so the window can be narrower than its widest line.
fn page() -> (gtk::ScrolledWindow, gtk::Box) {
    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["gallery-page"])
        .build();
    let scroll = gtk::ScrolledWindow::builder().child(&column).build();
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
    // A stylesheet the app changes as it runs, as a reader's zoom: at omui's APP_PRIORITY, above the reset.
    let zoom = gtk::CssProvider::new();
    gtk::style_context_add_provider_for_display(&reader.display(), &zoom, omui::APP_PRIORITY);
    let size = Rc::new(Cell::new(100));
    for (icon, tooltip, step) in [(name::ZOOM_OUT, "Smaller", -10), (name::ZOOM_IN, "Larger", 10)] {
        let button = omui::icon_button(icon, tooltip);
        let (zoom, size) = (zoom.clone(), size.clone());
        button.connect_clicked(move |_| {
            size.set((size.get() + step).clamp(70, 200));
            zoom.load_from_string(&format!(".reader {{ font-size: {}%; }}", size.get()));
        });
        reader.append(&button);
    }
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

fn states(shell: &Rc<Shell>, openers: &mut Openers) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let banner = Banner::new(
        "You are signed out of iCloud. Notes will not sync until you sign in.",
        Some("Sign in"),
    );
    banner.set_icon(Some(name::WARNING), "warning");
    banner.set_button_suggested(true);
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
    // Notes' while a sync runs: the button that would start another is disabled.
    let link = Banner::new("This folder is not linked to iCloud yet.", Some("Clone…"));
    link.set_icon(Some(name::CLOUD), "accent");
    link.set_button_suggested(true);
    link.set_button_enabled(false);
    link.set_revealed(true);
    column.append(&link.widget);

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
        .spacing(6)
        .width_request(340)
        .build();
    list.append(&omui::label("Inbox", "section-title"));
    list.append(&omui::label("Loading…", "status-line"));
    list.append(&omui::label("Today", "section-title"));
    list.append(&compact.widget);
    column.append(&row(&[
        empty.widget.upcast_ref(),
        gtk::Separator::new(gtk::Orientation::Vertical).upcast_ref(),
        list.upcast_ref(),
    ]));

    // Photos' toasts: a plain one, then three that can be acted on. None of them takes another's place, the third of
    // those waits until the plain one has gone, and each goes after its own time.
    let toasts = gtk::Button::with_label("Copy, delete, download and delete");
    toasts.set_halign(gtk::Align::Start);
    toasts.set_margin_start(28);
    let show = Rc::new({
        let shell = Rc::downgrade(shell);
        move || {
            let Some(shell) = shell.upgrade() else { return };
            let gone = |what: &'static str| move || trace(&format!("toast: {what} dismissed"));
            shell.add_toast(Toast::new("Copied").seconds(4).on_dismissed(gone("copied")));
            shell.add_toast(
                Toast::new("Moved harbour.jpg to Recently Deleted")
                    .seconds(10)
                    .action("Undo", || trace("toast: undo"))
                    .on_dismissed(gone("harbour deleted")),
            );
            shell.add_toast(
                Toast::new("Saved pier.jpg to Downloads")
                    .seconds(6)
                    .action("Show", || trace("toast: show"))
                    .on_dismissed(gone("saved")),
            );
            shell.add_toast(
                Toast::new("Moved shore.jpg to Recently Deleted")
                    .seconds(10)
                    .action("Undo", || trace("toast: undo"))
                    .on_dismissed(gone("shore deleted")),
            );
        }
    });
    toasts.connect_clicked({
        let show = show.clone();
        move |_| show()
    });
    openers.push(("toasts", "states", show));
    column.append(&toasts);
    column
}

/// Every way a dialog is used: a question, a form with a response that waits for valid input, the app's own content
/// that cannot be closed, one dialog over another, and a window with no shell to draw on.
fn dialogs(app: &gtk::Application, openers: &mut Openers) -> gtk::ScrolledWindow {
    let (scroll, column) = page();
    column.append(&omui::label("Dialogs", "page-title"));
    column.append(&omui::label(
        "Drawn inside the window, over its content. Escape gives the close response, Enter the default one.",
        "dim-label",
    ));
    let answer = omui::label("No answer yet.", "status-line");
    let said = Rc::new({
        let answer = answer.clone();
        move |text: &str| {
            answer.set_label(text);
            trace(&format!("answer: {text}"));
        }
    });
    let mut opener = |id: &'static str, label: &str, open: Rc<dyn Fn(&gtk::Button)>| {
        let button = gtk::Button::with_mnemonic(label);
        button.connect_clicked({
            let open = open.clone();
            move |button| open(button)
        });
        openers.push((id, "dialogs", {
            let button = button.clone();
            Rc::new(move || open(&button))
        }));
        button.upcast::<gtk::Widget>()
    };

    let confirm = opener(
        "confirm",
        "_Discard a draft…",
        Rc::new({
            let said = said.clone();
            move |button| {
                let said = said.clone();
                omui::confirm(
                    button,
                    "Discard this draft?",
                    "The message to Ada has not been sent. A discarded draft cannot be brought back.",
                    "Discard",
                    Appearance::Destructive,
                    move || said("discarded"),
                );
            }
        }),
    );
    let send = opener(
        "send",
        "Send without a subject…",
        Rc::new({
            let said = said.clone();
            move |button| {
                let said = said.clone();
                omui::confirm(
                    button,
                    "Send without a subject?",
                    "The message has no subject line.",
                    "_Send",
                    Appearance::Suggested,
                    move || said("sent"),
                );
            }
        }),
    );
    let rename = opener(
        "rename",
        "Rename a folder…",
        Rc::new({
            let said = said.clone();
            move |button| {
                let dialog = Dialog::new(Some("Rename the folder"), Some("The new name appears on every device."));
                let entry = gtk::Entry::builder().text("Receipts").activates_default(true).build();
                dialog.set_extra_child(Some(entry.upcast_ref()));
                dialog.add_response("cancel", "Cancel");
                dialog.add_response("rename", "Rename");
                dialog.set_response_appearance("rename", Appearance::Suggested);
                dialog.set_default_response("rename");
                dialog.set_close_response("cancel");
                // Weak: the dialog owns the entry.
                entry.connect_changed({
                    let dialog = dialog.downgrade();
                    move |entry| {
                        if let Some(dialog) = dialog.upgrade() {
                            dialog.set_response_enabled("rename", !entry.text().trim().is_empty());
                        }
                    }
                });
                dialog.connect_response({
                    let said = said.clone();
                    move |_, id| said(&format!("{id}: {}", entry.text()))
                });
                dialog.present(button);
            }
        }),
    );
    let upload = |button: &gtk::Button| {
        let dialog = Dialog::new(None, None);
        dialog.set_title("Uploading photos");
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["upload"])
            .build();
        content.append(&omui::label("Uploading 3 photos", "heading"));
        content.append(&gtk::ProgressBar::builder().fraction(0.4).build());
        content.append(&omui::label("2 of 3 · harbour-at-dawn.jpg", "meta"));
        let stop = gtk::Button::builder().label("Stop").halign(gtk::Align::End).build();
        content.append(&stop);
        dialog.set_extra_child(Some(content.upcast_ref()));
        dialog.set_content_size(380, -1);
        // Escape does nothing here: the upload ends or is stopped.
        dialog.set_can_close(false);
        dialog.present(button);
        // Weak: the dialog owns the button.
        let dialog = dialog.downgrade();
        stop.clone().connect_clicked(move |stop| {
            let dialog = dialog.clone();
            omui::confirm(
                stop,
                "Stop the upload?",
                "The photo being sent is left out.",
                "Stop",
                Appearance::Destructive,
                move || {
                    if let Some(dialog) = dialog.upgrade() {
                        dialog.force_close();
                    }
                },
            );
        });
        stop
    };
    let uploading = opener(
        "upload",
        "Upload photos…",
        Rc::new(move |button| {
            upload(button);
        }),
    );
    let window = opener(
        "window",
        "From a window without a shell…",
        Rc::new({
            let app = app.clone();
            let said = said.clone();
            move |_| {
                let delete = gtk::Button::builder()
                    .label("Delete the event…")
                    .halign(gtk::Align::Center)
                    .valign(gtk::Align::Center)
                    .build();
                let editor = gtk::Window::builder()
                    .application(&app)
                    .title("An editor window")
                    .default_width(420)
                    .default_height(260)
                    .child(&delete)
                    .build();
                let ask = {
                    let said = said.clone();
                    move |button: &gtk::Button| {
                        let said = said.clone();
                        omui::confirm(
                            button,
                            "Delete this event?",
                            "It is removed from every calendar it was shared with.",
                            "Delete",
                            Appearance::Destructive,
                            move || said("deleted"),
                        );
                    }
                };
                delete.connect_clicked(ask.clone());
                editor.present();
                ask(&delete);
            }
        }),
    );
    let tall = opener(
        "tall",
        "Choose calendars…",
        Rc::new({
            let said = said.clone();
            move |button| {
                let dialog = Dialog::new(
                    Some("Calendars to show"),
                    Some("More of them than the window is tall: the list scrolls, and the rest stays."),
                );
                let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
                for n in 1..=40 {
                    list.append(
                        &gtk::CheckButton::builder()
                            .label(format!("Calendar {n}"))
                            .active(n % 3 == 0)
                            .build(),
                    );
                }
                dialog.set_extra_child(Some(list.upcast_ref()));
                dialog.add_response("done", "Done");
                dialog.set_default_response("done");
                dialog.set_close_response("done");
                dialog.connect_response({
                    let said = said.clone();
                    move |_, id| said(id)
                });
                dialog.present(button);
            }
        }),
    );
    // Contacts' question before mirroring: its text is the app's own widget, and wraps at the width a dialog has
    // when the app sets none.
    let alert = opener(
        "alert",
        "Mirror contacts…",
        Rc::new({
            let said = said.clone();
            move |button| {
                let dialog = Dialog::new(Some("Mirror your contacts?"), None);
                let text = gtk::Box::new(gtk::Orientation::Vertical, 8);
                text.append(&omui::label(
                    "Every contact in iCloud is copied to Google and every contact in Google to iCloud, and from \
                     then on a change in either is made in the other.",
                    "",
                ));
                text.append(&omui::label(
                    "12 contacts look like the same person in both. You choose for each before anything is copied.",
                    "dim-label",
                ));
                dialog.set_extra_child(Some(text.upcast_ref()));
                dialog.add_response("cancel", "_Cancel");
                dialog.add_response("review", "_Review");
                dialog.set_response_appearance("review", Appearance::Suggested);
                dialog.set_default_response("review");
                dialog.set_close_response("cancel");
                dialog.connect_response({
                    let said = said.clone();
                    move |_, id| said(id)
                });
                dialog.present(button);
            }
        }),
    );
    let about = opener(
        "about",
        "About…",
        Rc::new(|button| {
            omui::about(
                button,
                "omui gallery",
                env!("CARGO_PKG_VERSION"),
                "Every widget of omui in one window.\n\nBy Ferdous Bhai. MIT License.",
                Some("https://github.com/ferdousbhai/omui"),
            )
        }),
    );
    // Notes' sync log: Clear empties it and the dialog stays open.
    let log = opener(
        "log",
        "Sync log…",
        Rc::new({
            let said = said.clone();
            move |button| {
                let dialog = Dialog::new(Some("Sync log"), None);
                let text = gtk::TextView::builder()
                    .editable(false)
                    .monospace(true)
                    .wrap_mode(gtk::WrapMode::WordChar)
                    .build();
                text.buffer()
                    .set_text("09:00 pull: 3 notes changed\n09:00 push: 1 note sent\n09:15 pull: up to date");
                dialog.set_extra_child(Some(gtk::Frame::builder().child(&text).build().upcast_ref()));
                dialog.set_content_size(460, -1);
                dialog.add_response("clear", "C_lear");
                dialog.add_response("close", "_Close");
                dialog.set_response_closes("clear", false);
                dialog.set_default_response("close");
                dialog.set_close_response("close");
                dialog.connect_response({
                    let said = said.clone();
                    move |dialog, id| {
                        if id == "clear" {
                            text.buffer().set_text("");
                            dialog.set_response_enabled("clear", false);
                        }
                        said(id);
                    }
                });
                dialog.present(button);
            }
        }),
    );
    openers.push(("stop", "dialogs", {
        let button = uploading.clone().downcast::<gtk::Button>().expect("a button");
        Rc::new(move || upload(&button).emit_clicked())
    }));
    column.append(&row(&[&confirm, &send, &rename, &uploading]));
    column.append(&row(&[&tall, &alert, &about, &window]));
    column.append(&row(&[&log]));
    column.append(&answer);
    scroll
}

fn inputs(openers: &mut Openers) -> gtk::ScrolledWindow {
    let (scroll, column) = page();
    column.append(&omui::label("Inputs", "page-title"));

    column.append(&omui::label("Switches, checks and radios", "section-title"));
    let switch = |active: bool, sensitive: bool| {
        gtk::Switch::builder()
            .active(active)
            .sensitive(sensitive)
            .valign(gtk::Align::Center)
            .build()
            .upcast::<gtk::Widget>()
    };
    column.append(&row(&[
        &switch(true, true),
        &switch(false, true),
        &switch(true, false),
        &switch(false, false),
    ]));
    let check = |label: &str, active: bool, sensitive: bool| {
        gtk::CheckButton::builder()
            .label(label)
            .active(active)
            .sensitive(sensitive)
            .build()
    };
    let mixed = check("Some", false, true);
    mixed.set_inconsistent(true);
    column.append(&row(&[
        check("Checked", true, true).upcast_ref(),
        check("Unchecked", false, true).upcast_ref(),
        mixed.upcast_ref(),
        check("Disabled", true, false).upcast_ref(),
        check("Disabled", false, false).upcast_ref(),
    ]));
    let week = check("Week", true, true);
    let month = check("Month", false, true);
    month.set_group(Some(&week));
    let year = check("Year", false, false);
    year.set_group(Some(&week));
    let never = check("Disabled", true, false);
    never.set_group(Some(&check("", false, true)));
    column.append(&row(&[
        week.upcast_ref(),
        month.upcast_ref(),
        year.upcast_ref(),
        never.upcast_ref(),
    ]));

    column.append(&omui::label("Choices and numbers", "section-title"));
    let dropdown = gtk::DropDown::from_strings(&["Every day", "Every week", "Every month", "Never"]);
    dropdown.set_selected(1);
    openers.push(("dropdown", "inputs", {
        let dropdown = dropdown.clone();
        Rc::new(move || {
            dropdown.activate();
        })
    }));
    let off = gtk::DropDown::from_strings(&["Disabled"]);
    off.set_sensitive(false);
    let spin = gtk::SpinButton::with_range(0.0, 10.0, 1.0);
    spin.set_value(3.0);
    let spin_off = gtk::SpinButton::with_range(0.0, 10.0, 1.0);
    spin_off.set_sensitive(false);
    column.append(&row(&[
        dropdown.upcast_ref(),
        off.upcast_ref(),
        spin.upcast_ref(),
        spin_off.upcast_ref(),
    ]));
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    scale.set_value(40.0);
    scale.set_hexpand(true);
    for mark in [0.0, 50.0, 100.0] {
        scale.add_mark(mark, gtk::PositionType::Bottom, Some(&format!("{mark}")));
    }
    let plain = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    plain.set_value(70.0);
    plain.set_hexpand(true);
    let scale_off = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    scale_off.set_value(25.0);
    scale_off.set_hexpand(true);
    scale_off.set_sensitive(false);
    column.append(&row(&[scale.upcast_ref(), plain.upcast_ref(), scale_off.upcast_ref()]));

    column.append(&omui::label("Text", "section-title"));
    let search = gtk::SearchEntry::builder().text("ferry").hexpand(true).build();
    let empty = gtk::SearchEntry::builder()
        .placeholder_text("Search mail")
        .hexpand(true)
        .build();
    let password = gtk::PasswordEntry::builder()
        .show_peek_icon(true)
        .text("correct horse")
        .hexpand(true)
        .build();
    column.append(&row(&[search.upcast_ref(), empty.upcast_ref(), password.upcast_ref()]));
    let text = gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::WordChar)
        .css_classes(["note"])
        .height_request(84)
        .build();
    text.buffer()
        .set_text("A text view is bare, so an editor can fill its pane.\nIn a frame it is a field of several lines.");
    column.append(&gtk::Frame::builder().child(&text).build());
    scroll
}

fn menus(openers: &mut Openers) -> gtk::ScrolledWindow {
    let (scroll, column) = page();
    column.append(&omui::label("Menus", "page-title"));
    column.append(&omui::label(
        "A menu from a model, with a check, a choice, a section and a submenu; and a popover of the app's own.",
        "dim-label",
    ));

    let actions = gio::SimpleActionGroup::new();
    actions.add_action(&gio::SimpleAction::new("reply", None));
    actions.add_action(&gio::SimpleAction::new("move", None));
    actions.add_action(&gio::SimpleAction::new_stateful("unread", None, &true.to_variant()));
    actions.add_action(&gio::SimpleAction::new_stateful(
        "density",
        Some(glib::VariantTy::STRING),
        &"cosy".to_variant(),
    ));
    let menu = gio::Menu::new();
    menu.append(Some("Reply"), Some("demo.reply"));
    // No such action, so the item is disabled.
    menu.append(Some("Forward"), Some("demo.forward"));
    menu.append(Some("Mark as unread"), Some("demo.unread"));
    let density = gio::Menu::new();
    density.append(Some("Cosy"), Some("demo.density::cosy"));
    density.append(Some("Compact"), Some("demo.density::compact"));
    menu.append_section(Some("Density"), &density);
    let folders = gio::Menu::new();
    folders.append(Some("Archive"), Some("demo.move"));
    folders.append(Some("Receipts"), Some("demo.move"));
    let more = gio::Menu::new();
    more.append_submenu(Some("Move to"), &folders);
    menu.append_section(None, &more);
    let from_model = gtk::MenuButton::builder().label("Message").menu_model(&menu).build();
    from_model.insert_action_group("demo", Some(&actions));
    openers.push(("menu", "menus", {
        let button = from_model.clone();
        Rc::new(move || button.popup())
    }));

    let form = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    form.append(&omui::label("Show", "heading"));
    form.append(&gtk::CheckButton::builder().label("Read messages").active(true).build());
    form.append(&gtk::CheckButton::builder().label("Drafts").build());
    form.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    form.append(&button("Reset", &["flat"]));
    let popover = gtk::Popover::builder().child(&form).build();
    let own = gtk::MenuButton::builder()
        .icon_name(name::SETTINGS)
        .tooltip_text("View options")
        .popover(&popover)
        .build();
    openers.push(("popover", "menus", {
        let button = own.clone();
        Rc::new(move || button.popup())
    }));
    let up = gtk::MenuButton::builder()
        .label("Upwards")
        .direction(gtk::ArrowType::Up)
        .menu_model(&density)
        .build();
    up.insert_action_group("demo", Some(&actions));
    column.append(&row(&[from_model.upcast_ref(), own.upcast_ref(), up.upcast_ref()]));
    scroll
}

fn views() -> gtk::ScrolledWindow {
    let (scroll, column) = page();
    column.append(&omui::label("Views", "page-title"));

    let notebook = gtk::Notebook::builder().hexpand(true).build();
    for tab in ["Inbox", "Sent", "Archive"] {
        let body = omui::label(&format!("{tab}: a page of a notebook."), "gallery-page");
        notebook.append_page(&body, Some(&gtk::Label::new(Some(tab))));
    }
    let frame = gtk::Frame::builder()
        .label("Frame")
        .child(&omui::label("Inside a frame.", "tile"))
        .build();
    let expander = gtk::Expander::builder()
        .label("Details")
        .expanded(true)
        .child(&omui::label("What an open expander shows.", "tile"))
        .build();
    let closed = gtk::Expander::builder().label("More").build();
    let side = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .hexpand(true)
        .build();
    side.append(&notebook);
    side.append(&frame);
    side.append(&expander);
    side.append(&closed);
    let calendar = gtk::Calendar::builder().valign(gtk::Align::Start).build();
    calendar.mark_day(3);
    calendar.mark_day(24);
    let calendar_off = gtk::Calendar::builder()
        .valign(gtk::Align::Start)
        .sensitive(false)
        .build();
    column.append(&row(&[
        calendar.upcast_ref(),
        calendar_off.upcast_ref(),
        side.upcast_ref(),
    ]));

    column.append(&omui::label("Progress", "section-title"));
    let bar = |fraction: f64| {
        gtk::ProgressBar::builder()
            .fraction(fraction)
            .valign(gtk::Align::Center)
            .hexpand(true)
            .build()
    };
    let with_text = bar(0.7);
    with_text.set_show_text(true);
    with_text.set_text(Some("7 of 10 notes"));
    let stalled = bar(0.25);
    stalled.set_sensitive(false);
    column.append(&row(&[
        bar(0.4).upcast_ref(),
        with_text.upcast_ref(),
        stalled.upcast_ref(),
    ]));

    column.append(&omui::label("Selected and disabled", "section-title"));
    let list = gtk::ListBox::builder().hexpand(true).build();
    for (text, sensitive) in [("Selected row", true), ("A row", true), ("Disabled row", false)] {
        list.append(
            &gtk::ListBoxRow::builder()
                .child(&omui::label(text, "note"))
                .sensitive(sensitive)
                .build(),
        );
    }
    list.select_row(list.row_at_index(0).as_ref());
    let tiles = gtk::FlowBox::builder()
        .hexpand(true)
        .max_children_per_line(3)
        .min_children_per_line(3)
        .valign(gtk::Align::Start)
        .build();
    for text in ["Selected", "Tile", "Tile"] {
        tiles.append(&omui::label(text, "tile"));
    }
    tiles.select_child(&tiles.child_at_index(0).expect("the first tile"));
    column.append(&row(&[list.upcast_ref(), tiles.upcast_ref()]));
    let off = row(&[
        omui::label("Everything here is disabled", "").upcast_ref(),
        omui::icon(name::CLOUD).upcast_ref(),
        &button("Button", &[]),
        &button("Suggested", &["suggested-action"]),
        omui::icon_button(name::EDIT, "Edit").upcast_ref(),
        gtk::Entry::builder().text("Entry").build().upcast_ref(),
        gtk::Expander::builder().label("Expander").build().upcast_ref(),
    ]);
    off.set_sensitive(false);
    column.append(&off);
    scroll
}

/// Name, work, and labelled values.
type Person = (&'static str, &'static str, &'static [(&'static str, &'static str)]);

const PEOPLE: &[Person] = &[
    (
        "Ada Lovelace",
        "Analytical Engines · Mathematician",
        &[
            ("mobile", "+44 20 7946 0958"),
            ("email", "ada@example.org"),
            ("homepage", "https://example.org/ada"),
            ("address", "12 St James's Square\nLondon SW1Y 4JH\nUnited Kingdom"),
            ("notes", "Prefers letters to calls & telegrams."),
        ],
    ),
    (
        "Grace Hopper",
        "Navy · Rear admiral",
        &[("work", "+1 555 0100"), ("email", "grace@example.org")],
    ),
    (
        "Katherine Johnson",
        "Flight Research · Mathematician",
        &[("mobile", "+1 555 0162")],
    ),
    ("Linus", "", &[("email", "linus@example.org")]),
    (
        "Margaret Hamilton",
        "Instrumentation Laboratory · Software lead",
        &[("work", "+1 555 0142"), ("homepage", "example.org/margaret")],
    ),
    ("", "No name", &[("mobile", "+1 555 0199")]),
];

/// A list and the card of what is chosen in it, as Contacts has: too narrow for both, a chosen row's card takes the
/// list's place, with a way back.
fn contacts(shell: &Rc<Shell>, openers: &mut Openers) -> gtk::Box {
    let list = gtk::ListBox::new();
    let rows: Vec<Row> = PEOPLE
        .iter()
        .map(|(name, work, _)| {
            let row = Row::new(
                if name.is_empty() { work } else { name },
                if name.is_empty() { "" } else { work },
            );
            row.set_subtitle_lines(1);
            row.add_prefix(&omui::avatar(32, name));
            list.append(&row.widget);
            row
        })
        .collect();
    let sidebar = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["sidebar"])
        .spacing(5)
        .build();
    sidebar.append(&gtk::SearchEntry::builder().placeholder_text("Search contacts").build());
    sidebar.append(&gtk::ScrolledWindow::builder().child(&list).vexpand(true).build());
    // A switch whose title wraps in a narrow window: the window keeps the width of its tile.
    let mirror = Group::new("", "");
    mirror.add(
        &omui::switch_row("Mirror contacts between iCloud and Google", "Off", false)
            .0
            .widget,
    );
    sidebar.append(&mirror.widget);

    let back = omui::icon_button(name::BACK, "Contacts");
    let edit = omui::icon_button(name::EDIT, "Edit");
    let delete = omui::icon_button(name::DELETE, "Delete");
    // A header bar with buttons and no title: the window's title is the sidebar's to show.
    let bar = omui::header_without_title();
    bar.pack_start(&back);
    bar.pack_end(&delete);
    bar.pack_end(&edit);
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["card"])
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&bar);
    content.append(
        &gtk::ScrolledWindow::builder()
            .child(&omui::clamp(&card, 560))
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build(),
    );

    // The list's width is dragged, or moved from the keyboard, within its limits; an app saves `sidebar_width` and
    // restores it as this does.
    let split = Split::new(&sidebar, &content, (240, 360), Some(600));
    split.set_resizable(true);
    split.set_sidebar_width(280);
    let width = Cell::new(split.sidebar_width());
    split.connect_changed(move |split| {
        if width.replace(split.sidebar_width()) != split.sidebar_width() {
            trace(&format!("divider: {}", split.sidebar_width()));
        }
    });
    back.set_visible(false);
    split.connect_changed({
        let back = back.downgrade();
        move |split| {
            trace_split("contacts", split);
            if let Some(back) = back.upgrade() {
                back.set_visible(split.is_collapsed());
            }
        }
    });
    back.connect_clicked({
        let split = split.downgrade();
        move |_| {
            if let Some(split) = split.upgrade() {
                split.set_show_content(false);
            }
        }
    });
    list.connect_row_selected({
        let (card, shell) = (card.downgrade(), Rc::downgrade(shell));
        move |_, row| {
            if let (Some(card), Some(shell), Some(row)) = (card.upgrade(), shell.upgrade(), row) {
                show_card(&card, &shell, &PEOPLE[row.index() as usize]);
            }
        }
    });
    // Chosen, not merely moved to with the arrows: in a narrow window the card takes the list's place.
    for row in rows {
        let split = split.downgrade();
        row.connect_activated(move || {
            if let Some(split) = split.upgrade() {
                split.set_show_content(true);
            }
        });
    }
    list.select_row(list.row_at_index(0).as_ref());
    edit.connect_clicked(edit_contact);
    openers.push(("contact", "split", Rc::new(move || edit_contact(&edit))));
    split.widget
}

fn show_card(card: &gtk::Box, shell: &Rc<Shell>, (name, work, values): &Person) {
    omui::clear(card);
    card.append(&omui::avatar(96, name));
    let centred = |text: &str, class: &str| {
        gtk::Label::builder()
            .label(text)
            .css_classes([class])
            .wrap(true)
            .justify(gtk::Justification::Center)
            .selectable(true)
            .build()
    };
    if !name.is_empty() {
        card.append(&centred(name, "title-1"));
    }
    card.append(&centred(work, "dim-label"));
    for (label, value) in *values {
        // A row showing a value under its label; activating it copies or opens the value.
        let row = Row::new(label, value).property();
        row.set_subtitle_selectable(true);
        let opens = matches!(*label, "email" | "homepage");
        let hint = omui::icon(if opens { name::EXTERNAL_LINK } else { name::COPY });
        hint.set_tooltip_text(Some(if opens { "Open" } else { "Copy" }));
        row.add_suffix(&hint);
        row.connect_activated({
            let shell = Rc::downgrade(shell);
            let said = if opens {
                format!("Opening {value}")
            } else {
                "Copied".to_owned()
            };
            move || {
                if let Some(shell) = shell.upgrade() {
                    shell.toast(&said);
                }
            }
        });
        let group = Group::new("", "");
        group.add(&row.widget);
        card.append(&group.widget);
    }
}

/// Contacts' edit dialog: groups of entry rows, a choice, notes, and a Save that waits for the form to be valid.
fn edit_contact(parent: &gtk::Button) {
    let dialog = Dialog::new(Some("Edit Contact"), None);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let accounts = gtk::StringList::new(&["iCloud (ada@example.org)", "Google (ada@example.com)"]);
    let (account_row, account) = omui::combo_row("Account", "", &accounts, 0);
    let group = |rows: &[&Row]| {
        let group = Group::new("", "");
        for row in rows {
            group.add(&row.widget);
        }
        body.append(&group.widget);
    };
    group(&[&account_row]);
    let (first_row, first) = omui::entry_row("First name", "Ada");
    let (last_row, last) = omui::entry_row("Last name", "Lovelace");
    let (company_row, _) = omui::entry_row("Company", "Analytical Engines");
    group(&[&first_row, &last_row, &company_row]);
    let (mobile_row, _) = omui::entry_row("mobile", "+44 20 7946 0958");
    let (phone_row, _) = omui::entry_row("Add phone (work=+1 555 0100)", "");
    group(&[&mobile_row, &phone_row]);
    let notes = gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::WordChar)
        .css_classes(["note"])
        .build();
    notes.buffer().set_text("Prefers letters to calls & telegrams.");
    let scroll = gtk::ScrolledWindow::builder()
        .child(&notes)
        .min_content_height(70)
        .build();
    body.append(&gtk::Frame::builder().child(&scroll).build());
    let problem = omui::label("", "error");
    problem.add_css_class("caption");
    problem.set_visible(false);
    body.append(&problem);
    dialog.set_extra_child(Some(body.upcast_ref()));
    dialog.set_content_size(360, -1);
    dialog.add_response("cancel", "_Cancel");
    dialog.add_response("save", "_Save");
    dialog.set_response_appearance("save", Appearance::Suggested);
    dialog.set_default_response("save");
    dialog.set_close_response("cancel");
    // Save is enabled only while the draft is valid, so a mistake never closes the dialog and loses the edits.
    // Weak: the dialog owns the entries.
    let validate = Rc::new({
        let (dialog, first, last) = (dialog.downgrade(), first.downgrade(), last.downgrade());
        move || {
            let (Some(dialog), Some(first), Some(last)) = (dialog.upgrade(), first.upgrade(), last.upgrade()) else {
                return;
            };
            let unnamed = first.text().trim().is_empty() && last.text().trim().is_empty();
            problem.set_visible(unnamed);
            problem.set_label(if unnamed { "A contact needs a name." } else { "" });
            dialog.set_response_enabled("save", !unnamed);
        }
    });
    for entry in [&first, &last] {
        let validate = validate.clone();
        entry.connect_changed(move |_| validate());
    }
    validate();
    dialog.connect_response(move |_, id| {
        let account = account.selected_item().and_downcast::<gtk::StringObject>();
        let account = account.map(|account| account.string()).unwrap_or_default();
        trace(&format!("answer: {id}: {} {} in {account}", first.text(), last.text()));
    });
    dialog.present(parent);
}

/// Omacloud's setup: pages pushed one over another, a form of rows, and the recovery code that is shown once.
fn setup(openers: &mut Openers) -> gtk::Stack {
    let (root, column) = omui::page(560);
    let nav = Nav::new(&root);
    nav.connect_changed(|nav| trace(&format!("nav: {}", nav.visible_tag().as_deref().unwrap_or("root"))));
    column.append(&omui::label("Set up Omacloud", "page-title"));
    let choices = Group::new("This computer", "Its files are encrypted before they leave it.");
    for (tag, title, subtitle) in [
        ("create", "Create an account", "On storage of your own"),
        (
            "join",
            "Join an account",
            "With a code from a computer that already syncs",
        ),
    ] {
        let row = Row::new(title, subtitle);
        row.add_suffix(&omui::icon(name::FORWARD));
        row.connect_activated({
            let nav = nav.downgrade();
            move || {
                if let Some(nav) = nav.upgrade() {
                    nav.push(tag, &account_page(&nav, title, tag == "create"));
                }
            }
        });
        choices.add(&row.widget);
    }
    column.append(&choices.widget);
    openers.push(("recovery", "nav", {
        let stack = nav.widget.clone();
        Rc::new(move || recovery_code(&stack))
    }));
    // A page pushed by an action (a menu, an accelerator), so the keyboard stays where it was.
    openers.push(("push", "nav", {
        let nav = nav.downgrade();
        Rc::new(move || {
            if let Some(nav) = nav.upgrade() {
                nav.push("advanced", &advanced_page(&nav));
            }
        })
    }));
    nav.widget
}

/// A pushed page: a bar with the way back and the page's title, over a form.
fn account_page(nav: &Nav, title: &str, create: bool) -> gtk::Box {
    let bar = gtk::Box::builder().css_classes(["toolbar"]).build();
    let back = omui::icon_button(name::BACK, "Back");
    back.connect_clicked({
        let nav = nav.downgrade();
        move |_| {
            if let Some(nav) = nav.upgrade() {
                nav.pop();
            }
        }
    });
    bar.append(&back);
    bar.append(&omui::label(title, "heading"));
    let (scroll, column) = omui::page(560);
    if create {
        let storage = Group::new("Storage", "Where the encrypted files are kept.");
        let providers = gtk::StringList::new(&["Cloudflare R2", "Hetzner", "Other S3"]);
        storage.add(&omui::combo_row("Provider", "", &providers, 0).0.widget);
        storage.add(&omui::entry_row("Cloudflare account ID", "").0.widget);
        storage.add(&omui::entry_row("Access key ID", "").0.widget);
        storage.add(&omui::password_row("Secret access key").0.widget);
        column.append(&storage.widget);
    } else {
        let join = Group::new("", "");
        join.add(&omui::password_row("Join code").0.widget);
        column.append(&join.widget);
    }
    // For someone who lost every computer: out of the way of everyone else.
    let recovery = Group::new("", "");
    let more = ExpanderRow::new("Lost every computer?", "");
    more.add_row(&omui::password_row("Recovery code").0.widget);
    recovery.add(&more.widget);
    let advanced = Row::new("Advanced", "");
    advanced.add_suffix(&omui::icon(name::FORWARD));
    advanced.connect_activated({
        let nav = nav.downgrade();
        move || {
            if let Some(nav) = nav.upgrade() {
                nav.push("advanced", &advanced_page(&nav));
            }
        }
    });
    recovery.add(&advanced.widget);
    column.append(&recovery.widget);
    let go = gtk::Button::builder()
        .label(if create { "Create" } else { "Join" })
        .css_classes(["suggested-action", "pill"])
        .halign(gtk::Align::End)
        .build();
    go.connect_clicked(recovery_code);
    column.append(&go);
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.append(&bar);
    page.append(&scroll);
    page
}

fn advanced_page(nav: &Nav) -> gtk::Box {
    let (scroll, column) = omui::page(560);
    let group = Group::new("Advanced", "Two pages deep.");
    group.add(
        &omui::switch_row("Sync settings too", "Themes, key bindings and the like", true)
            .0
            .widget,
    );
    column.append(&group.widget);
    let over = gtk::Button::builder()
        .label("Start over")
        .halign(gtk::Align::Start)
        .build();
    over.connect_clicked({
        let nav = nav.downgrade();
        move |_| {
            if let Some(nav) = nav.upgrade() {
                nav.pop_to_root();
            }
        }
    });
    column.append(&over);
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.append(&scroll);
    page
}

/// Omacloud's recovery code, shown once: it is typed back before the dialog can be left, and Copy leaves it open.
fn recovery_code(parent: &impl IsA<gtk::Widget>) {
    let code = "7KQ2-M9XD-4TTP-W3HC";
    let dialog = Dialog::new(
        Some("Your recovery code"),
        Some(
            "Write it down and keep it offline. With it you can add a computer when no other is at hand and recover \
             the account. It is shown only now: without it and without a computer, your files can't be recovered.",
        ),
    );
    // Selected with the pointer, and no stop for the keyboard: the dialog opens with the cursor in the entry.
    let label = gtk::Label::builder()
        .label(code)
        .selectable(true)
        .can_focus(false)
        .css_classes(["title-2", "monospace", "code"])
        .build();
    // Typed back, so nobody clicks past the one chance to keep it.
    let (check_row, check) = omui::entry_row("Type its last four characters", "");
    let group = Group::new("", "");
    group.add(&check_row.widget);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.append(&label);
    body.append(&group.widget);
    dialog.set_extra_child(Some(body.upcast_ref()));
    dialog.add_response("copy", "Copy");
    dialog.add_response("done", "I Wrote It Down");
    dialog.set_response_appearance("done", Appearance::Suggested);
    dialog.set_response_enabled("done", false);
    dialog.set_close_response("copy");
    let tail = code[code.len() - 4..].to_ascii_lowercase();
    // Weak: the dialog owns the entry.
    check.connect_changed({
        let dialog = dialog.downgrade();
        move |check| {
            if let Some(dialog) = dialog.upgrade() {
                dialog.set_response_enabled("done", check.text().to_ascii_lowercase() == tail);
            }
        }
    });
    let asked_from = parent.as_ref().downgrade();
    dialog.connect_response(move |dialog, id| {
        trace(&format!("answer: {id}"));
        if id == "copy"
            && let Some(parent) = asked_from.upgrade()
        {
            parent.clipboard().set_text(code);
            // Copying doesn't close it: the code is still to be written down.
            dialog.present(&parent);
        }
    });
    dialog.present(parent);
}

/// Omacloud's window: the pages of a stack, chosen from tabs that are icons alone in a narrow window.
fn tabbed() -> gtk::Box {
    let stack = gtk::Stack::builder().vexpand(true).build();
    // Before the pages, as an app's header bar is built before its content.
    let tabs = omui::tabs(&stack);
    tabs.set_halign(gtk::Align::Center);
    tabs.set_hexpand(true);
    let bar = gtk::Box::builder().css_classes(["toolbar"]).build();
    bar.append(&tabs);

    let (overview, column) = omui::page(560);
    let working = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    working.append(&omui::spinner());
    working.append(&omui::label("Syncing 3 files…", "status-line"));
    column.append(&working);
    let files = Group::new("Files", "");
    files.add(&Row::new("Synced", "1 204 files · 3.2 GB").widget);
    files.add(&Row::new("Deleted", "Kept for 30 days").widget);
    column.append(&files.widget);

    let (devices, column) = omui::page(560);
    let group = Group::new("Devices", "Each computer chooses what it syncs.");
    for (device, here, battery) in [
        ("harbour-laptop", true, name::BATTERY_CHARGING),
        ("pier-desktop", false, name::BATTERY_FULL),
        ("tide-tablet", false, name::BATTERY_LOW),
    ] {
        let row = ExpanderRow::new(device, if here { "This computer" } else { "" });
        row.add_prefix(&omui::icon(if device.ends_with("tablet") {
            name::TABLET
        } else {
            name::COMPUTER
        }));
        row.add_suffix(&omui::icon(battery));
        for folder in ["Documents", "Pictures"] {
            row.add_row(&omui::switch_row(folder, "", here).0.widget);
        }
        row.set_expanded(here);
        group.add(&row.widget);
    }
    column.append(&group.widget);

    let (settings, column) = omui::page(560);
    let group = Group::new("Settings", "");
    group.add(&omui::switch_row("Start at login", "", true).0.widget);
    column.append(&group.widget);

    for (child, id, title, icon) in [
        (&overview, "overview", "Overview", name::CLOUD),
        (&devices, "devices", "Devices", name::COMPUTER),
        (&settings, "settings", "Settings", name::SETTINGS),
    ] {
        stack.add_titled(child, Some(id), title).set_icon_name(icon);
    }
    // A page without an icon keeps its title at any width.
    stack.add_titled(
        &omui::label("A page with no icon.", "gallery-page"),
        Some("about"),
        "About",
    );

    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.append(&bar);
    page.append(&stack);
    page
}

fn rows(shell: &Rc<Shell>, openers: &mut Openers) -> gtk::ScrolledWindow {
    let (scroll, column) = omui::page(640);
    column.append(&omui::label("Rows", "page-title"));

    let plain = Group::new("Rows", "A title, a subtitle, and widgets before and after them.");
    plain.set_header_suffix(Some(&button("Add", &[])));
    let with_badge = Row::new("Harbour office", "Connected");
    with_badge.add_prefix(&omui::icon(name::CLOUD));
    with_badge.add_suffix(&omui::badge("Synced", "success"));
    plain.add(&with_badge.widget);
    let goes = Row::new("Storage", "3.2 GB of 10 GB");
    goes.add_prefix(&omui::icon(name::DRIVE));
    goes.add_suffix(&omui::icon(name::FORWARD));
    goes.connect_activated({
        let shell = Rc::downgrade(shell);
        move || {
            if let Some(shell) = shell.upgrade() {
                shell.toast("Storage");
            }
        }
    });
    plain.add(&goes.widget);
    let long = Row::new(
        "A subtitle kept to two lines",
        "Each becomes one card with every phone, email and address of them all. Every card is saved first to a \
         backup file, and the backup is kept until you delete it yourself from the folder it was written to.",
    );
    long.set_subtitle_lines(2);
    plain.add(&long.widget);
    let acts = Row::new("Signed in as ada@example.org", "");
    acts.add_suffix(&button("Sign Out", &[]));
    plain.add(&acts.widget);
    let off = Row::new("A disabled row", "Not now");
    off.add_suffix(&omui::icon(name::LOCK));
    off.widget.set_sensitive(false);
    plain.add(&off.widget);
    column.append(&plain.widget);

    let fields = Group::new("Fields", "");
    fields.add(&omui::entry_row("Name", "Ada Lovelace").0.widget);
    fields.add(&omui::entry_row("Email", "").0.widget);
    let (password_row, password) = omui::password_row("Password");
    password.set_text("correct horse");
    fields.add(&password_row.widget);
    let often = gtk::StringList::new(&["Every minute", "Every hour", "Every day"]);
    fields.add(
        &omui::combo_row("Fetch", "How often changes are looked for", &often, 1)
            .0
            .widget,
    );
    fields.add(
        &omui::switch_row("Notifications", "A sound for each new message", true)
            .0
            .widget,
    );
    fields.add(&omui::switch_row("Metered connections", "", false).0.widget);
    column.append(&fields.widget);

    let more = Group::new("Expanders", "");
    let open = ExpanderRow::new("harbour-laptop", "This computer");
    open.add_prefix(&omui::icon(name::COMPUTER));
    open.add_suffix(&omui::badge("Syncing", ""));
    open.add_row(&omui::switch_row("Documents", "~/Documents", true).0.widget);
    open.add_row(&omui::switch_row("Pictures", "~/Pictures", false).0.widget);
    open.set_expanded(true);
    more.add(&open.widget);
    let closed = ExpanderRow::new("Lost every computer?", "");
    closed.add_row(&omui::password_row("Recovery code").0.widget);
    more.add(&closed.widget);
    column.append(&more.widget);

    let details = Group::new("Values", "The title names the value: the property style.");
    let fingerprint = Row::new("Account", "3f9a 77c1 0be2 5d84").property();
    fingerprint.set_subtitle_selectable(true);
    details.add(&fingerprint.widget);
    details.add(&Row::new("Version", "42").property().widget);
    column.append(&details.widget);

    // Settings in a dialog: a page of groups is the dialog's own widget.
    let settings = gtk::Button::builder()
        .label("Settings in a dialog…")
        .halign(gtk::Align::Start)
        .build();
    let open_settings = |parent: &gtk::Button| {
        let dialog = Dialog::new(Some("Settings"), None);
        let (page, column) = omui::page(480);
        let group = Group::new("Sync", "");
        group.add(&omui::switch_row("Start at login", "", true).0.widget);
        group.add(&omui::switch_row("Sync on metered connections", "", false).0.widget);
        column.append(&group.widget);
        let about = Group::new("About", "");
        about.add(&Row::new("Version", env!("CARGO_PKG_VERSION")).property().widget);
        column.append(&about.widget);
        dialog.set_extra_child(Some(page.upcast_ref()));
        dialog.set_content_size(460, 300);
        // Settings take effect as they change: there is nothing to answer, only the dialog to close.
        dialog.set_close_button(true);
        dialog.present(parent);
    };
    settings.connect_clicked(open_settings);
    openers.push(("settings", "rows", {
        let settings = settings.clone();
        Rc::new(move || open_settings(&settings))
    }));
    column.append(&settings);
    scroll
}

fn avatars() -> gtk::ScrolledWindow {
    let (scroll, column) = page();
    column.append(&omui::label("Avatars", "page-title"));
    column.append(&omui::label(
        "The initials of a name, in one of six tints the name picks; a person when there is no name.",
        "dim-label",
    ));
    let sizes = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    for size in [24, 32, 48, 64, 96] {
        sizes.append(&omui::avatar(size, "Ada Lovelace"));
    }
    sizes.append(&omui::avatar(64, ""));
    sizes.append(&omui::avatar(32, ""));
    column.append(&sizes);
    let tints = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    for person in [
        "Ada",
        "Grace Hopper",
        "Linus",
        "Margaret Hamilton",
        "Dennis",
        "Barbara Liskov",
        "Ken",
        "Edsger",
    ] {
        let cell = gtk::Box::new(gtk::Orientation::Vertical, 4);
        cell.append(&omui::avatar(40, person));
        cell.append(&omui::label(person, "caption"));
        tints.append(&cell);
    }
    column.append(&tints);

    column.append(&omui::label("Spinner", "section-title"));
    let large = omui::spinner();
    large.set_size_request(32, 32);
    let waiting = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    waiting.append(&omui::spinner());
    waiting.append(&omui::label("Looking for devices…", "status-line"));
    column.append(&row(&[large.upcast_ref(), waiting.upcast_ref()]));
    scroll
}
