// SPDX-License-Identifier: MIT
//! Every omui widget in one window, for looking at a change in dark and light themes (dev/preview.sh).
//!
//!   OMUI_GALLERY_PAGE=controls|icons|states|dialogs|inputs|menus|views   the page to open on
//!   OMUI_GALLERY_OPEN=confirm|send|rename|upload|stop|tall|window|dropdown|menu|popover
//!                                             what to open on its page, as a click would
//!   OMUI_GALLERY_TRACE=1                      print each focus change and dialog answer (dev/keys.sh reads them)
//!   OMUI_SHOT=<file.png>                      with feature `dev`: write the window through `omui::dev::shot`

use std::rc::Rc;

use omui::{
    Appearance, Banner, Dialog, EmptyState, Shell, Theme, Title,
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
    let mut openers = Openers::new();
    stack.add_named(&dialogs(app, &mut openers), Some("dialogs"));
    stack.add_named(&inputs(&mut openers), Some("inputs"));
    stack.add_named(&menus(&mut openers), Some("menus"));
    stack.add_named(&views(), Some("views"));
    list.connect_row_selected({
        let stack = stack.clone();
        move |_, row| {
            if let Some(row) = row {
                stack.set_visible_child_name(&row.widget_name());
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
    if std::env::var_os("OMUI_GALLERY_TRACE").is_some() {
        window.connect_focus_widget_notify(|window| {
            let focus = RootExt::focus(window);
            let button = focus.as_ref().and_then(|w| w.downcast_ref::<gtk::Button>());
            let name = match (button.and_then(|b| b.label()), &focus) {
                (Some(label), _) => label.to_string(),
                (None, Some(widget)) => widget.css_name().to_string(),
                (None, None) => "nothing".to_owned(),
            };
            trace(&format!("focus: {name}"));
        });
    }
    window.present();
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
    openers.push(("stop", "dialogs", {
        let button = uploading.clone().downcast::<gtk::Button>().expect("a button");
        Rc::new(move || upload(&button).emit_clicked())
    }));
    column.append(&row(&[&confirm, &send, &rename, &uploading]));
    column.append(&row(&[&tall, &window]));
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
