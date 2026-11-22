// SPDX-License-Identifier: MIT
//! What needs GTK running: sizes, toasts, dialogs, a split's divider, banners and the app's stylesheet priority. GTK is used from one thread only, so this is
//! one test. It needs a broadway display, which shows nothing: GTK on the desktop the tests are run from would read
//! that desktop's settings. So `cargo test` lists it as ignored, and `dev/gtk-tests.sh` runs it on a display of its
//! own.

use std::{cell::RefCell, rc::Rc};

use gtk::{glib, prelude::*};

use crate::{Banner, Dialog, Keys, Shell, ShortcutOverlay, ShortcutSection, Split, Toast, i18n};

#[test]
#[ignore = "needs a broadway display: dev/gtk-tests.sh"]
fn with_gtk() {
    assert_eq!(
        std::env::var("GDK_BACKEND").as_deref(),
        Ok("broadway"),
        "run by dev/gtk-tests.sh, on a display that shows nothing"
    );
    gtk::init().expect("the broadway display");
    a_split_needs_as_little_height_at_any_width();
    a_toast_with_a_button_stays_and_each_says_when_it_goes();
    a_dialog_holds_back_the_windows_shortcuts_until_it_closes();
    a_dialog_with_text_has_a_width_of_its_own();
    a_response_can_leave_the_dialog_open();
    a_response_turned_off_hands_the_keyboard_on();
    a_resizable_split_keeps_its_sidebar_within_its_limits();
    a_banner_has_an_icon_and_a_button_of_either_kind();
    a_dialog_put_away_always_answers();
    a_single_key_is_a_character_while_a_field_has_the_keyboard();
    the_shortcuts_overlay_lists_the_keys_and_closes_as_it_opened();
    a_width_is_followed_by_layouts_not_by_frames();
    a_switch_of_language_reaches_bound_text_and_turns_the_windows();
    a_dialog_follows_a_switch_of_language_while_it_is_open();
    a_dialog_keeps_its_keys_when_its_focused_widget_is_taken_away();
    groups_and_rows_are_relabelled_in_place();
    a_translated_setter_replaces_its_earlier_binding();
    a_confirmation_with_a_name_follows_a_switch_of_language();
    every_window_is_named_by_the_apps_short_name();
    #[cfg(feature = "qa")]
    qa_selects_a_list_views_item_through_its_model();
    #[cfg(feature = "qa")]
    qa_names_a_rows_control_by_its_title();
    #[cfg(feature = "qa")]
    qa_sends_the_key_a_keyboard_would();
    // Last: the theme styles every widget made after it.
    an_app_provider_at_app_priority_is_above_the_reset();
    the_stylesheet_parses_and_takes_its_corners_and_colours_from_the_theme();
    #[cfg(feature = "dev")]
    a_dividers_line_is_two_whole_pixels();
    // After the stylesheet's test: its application installs the theme, which that test installs its own way.
    an_application_that_opens_files_is_handed_them();
}

fn wrapping(text: &str) -> gtk::Label {
    gtk::Label::builder().label(text).wrap(true).build()
}

fn a_split_needs_as_little_height_at_any_width() {
    let sidebar = wrapping(&"Mirror contacts between iCloud and Google. ".repeat(6));
    let split = Split::new(&sidebar, &gtk::Label::new(Some("content")), (240, 320), Some(600));
    let height = |width| split.widget.measure(gtk::Orientation::Vertical, width);
    let (narrow, wide) = (height(240), height(1200));
    // So GTK does not look for a window's least size along its shape, which could make it wider than its tile.
    assert_eq!(narrow.0, wide.0, "the least height is the same at every width");
    assert!(narrow.1 > wide.1, "naturally, the text is taller where it is narrower");
    // Given less than its text needs, the pane is laid out at what it needs and cut off at the bottom.
    let needs = sidebar.measure(gtk::Orientation::Vertical, 240).0;
    assert!(narrow.0 < needs);
    split.widget.allocate(240, narrow.0, -1, None);
    assert_eq!(sidebar.height(), needs);
}

/// The toasts on screen, by their text.
fn toasts(shell: &Shell) -> Vec<String> {
    let mut child = shell.overlay.first_child();
    while let Some(widget) = child {
        if widget.has_css_class("toasts") {
            let mut texts = Vec::new();
            let mut toast = widget.first_child();
            while let Some(widget) = toast {
                texts.push(
                    widget
                        .first_child()
                        .and_downcast::<gtk::Label>()
                        .unwrap()
                        .label()
                        .into(),
                );
                toast = widget.next_sibling();
            }
            return texts;
        }
        child = widget.next_sibling();
    }
    panic!("the shell has no toasts");
}

fn a_toast_with_a_button_stays_and_each_says_when_it_goes() {
    let window = gtk::Window::new();
    let shell = Shell::new(&window, &gtk::Box::new(gtk::Orientation::Vertical, 0));
    let said = Rc::new(RefCell::new(Vec::new()));
    let say = |what: &'static str| {
        let said = said.clone();
        move || said.borrow_mut().push(what)
    };
    shell.add_toast(
        Toast::new("Deleted")
            .seconds(10)
            .action("Undo", say("undo"))
            .on_dismissed(say("deleted gone")),
    );
    shell.add_toast(Toast::new("Saved").action("Show", say("show")));
    shell.add_toast(Toast::new("Copied").on_dismissed(say("copied gone")));
    assert_eq!(toasts(&shell), ["Deleted", "Saved", "Copied"]);
    // A plain toast takes the place of a plain one alone.
    shell.toast("Copied again");
    assert_eq!(toasts(&shell), ["Deleted", "Saved", "Copied again"]);
    assert_eq!(*said.borrow(), ["copied gone"]);
    // Its button runs the action, then the toast goes and says so.
    let deleted = shell.overlay.last_child().unwrap().first_child().unwrap();
    deleted
        .first_child()
        .unwrap()
        .next_sibling()
        .and_downcast::<gtk::Button>()
        .unwrap()
        .emit_clicked();
    assert_eq!(toasts(&shell), ["Saved", "Copied again"]);
    assert_eq!(*said.borrow(), ["copied gone", "undo", "deleted gone"]);
    // Its time runs out.
    shell.add_toast(
        Toast::new("Brief")
            .seconds(1)
            .action("Retry", || {})
            .on_dismissed(say("brief gone")),
    );
    let main = glib::MainContext::default();
    // A timer in seconds runs within a second of when it is due.
    let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while said.borrow().last() != Some(&"brief gone") && std::time::Instant::now() < end {
        main.iteration(false);
    }
    assert!(!toasts(&shell).contains(&"Brief".to_owned()));
    assert_eq!(said.borrow().last(), Some(&"brief gone"));

    // Three at most are on screen; the rest wait, and their time starts when they appear.
    let window = gtk::Window::new();
    let shell = Shell::new(&window, &gtk::Box::new(gtk::Orientation::Vertical, 0));
    for text in ["One", "Two", "Three"] {
        shell.toast_action(text, "Undo", 30, || {});
    }
    shell.add_toast(
        Toast::new("Four")
            .seconds(1)
            .action("Undo", || {})
            .on_dismissed(say("four gone")),
    );
    assert_eq!(toasts(&shell), ["One", "Two", "Three"]);
    let run_for = |millis| {
        let end = std::time::Instant::now() + std::time::Duration::from_millis(millis);
        while said.borrow().last() != Some(&"four gone") && std::time::Instant::now() < end {
            main.iteration(false);
        }
    };
    run_for(1500);
    assert_ne!(
        said.borrow().last(),
        Some(&"four gone"),
        "a toast waiting has its time still to come"
    );
    // The first one's close button.
    let first = shell.overlay.last_child().unwrap().first_child().unwrap();
    first.last_child().and_downcast::<gtk::Button>().unwrap().emit_clicked();
    assert_eq!(toasts(&shell), ["Two", "Three", "Four"]);
    run_for(3000);
    assert_eq!(said.borrow().last(), Some(&"four gone"));
    assert_eq!(toasts(&shell), ["Two", "Three"]);
}

fn a_dialog_holds_back_the_windows_shortcuts_until_it_closes() {
    let window = gtk::Window::new();
    let content = gtk::Button::with_label("Delete");
    // A page's managed shortcut, and one on the window itself.
    let managed = gtk::ShortcutController::new();
    managed.set_scope(gtk::ShortcutScope::Managed);
    content.add_controller(managed.clone());
    let own = gtk::ShortcutController::new();
    own.set_propagation_phase(gtk::PropagationPhase::Capture);
    window.add_controller(own.clone());
    let shell = Shell::new(&window, &content);
    // Inside the dialog, a managed shortcut stays.
    let field = gtk::Entry::new();
    let inside = gtk::ShortcutController::new();
    inside.set_scope(gtk::ShortcutScope::Managed);
    field.add_controller(inside.clone());

    let phases =
        || [&managed, &own, &inside].map(|controller| controller.propagation_phase() != gtk::PropagationPhase::None);
    assert!(!shell.dialog_open());
    let dialog = Dialog::new(Some("Delete this photo?"), None);
    dialog.set_extra_child(Some(field.upcast_ref()));
    dialog.set_can_close(false);
    dialog.present(&content);
    assert!(shell.dialog_open() && crate::dialog_open(&content));
    assert_eq!(phases(), [false, false, true]);
    // One over it keeps them held, and so does the first closing before the second.
    let over = Dialog::new(Some("Stop the upload?"), None);
    over.present(&content);
    assert_eq!(
        phases(),
        [false, false, false],
        "the dialog under the top one is held back too"
    );
    dialog.close();
    assert!(shell.dialog_open(), "a dialog that cannot be closed stays");
    dialog.force_close();
    assert_eq!(phases(), [false, false, true]);
    over.close();
    assert!(!shell.dialog_open());
    assert_eq!(phases(), [true, true, true]);
    assert_eq!(
        own.propagation_phase(),
        gtk::PropagationPhase::Capture,
        "each gets its own phase back"
    );
}

fn a_dialog_with_text_has_a_width_of_its_own() {
    let window = gtk::Window::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let shell = Shell::new(&window, &content);
    let natural_width = |dialog: &Dialog| {
        dialog.present(&content);
        let frame = shell.overlay.last_child().unwrap().first_child().unwrap();
        let width = frame.measure(gtk::Orientation::Horizontal, -1);
        dialog.force_close();
        width
    };
    let alert = Dialog::new(Some("Mirror your contacts?"), None);
    alert.set_extra_child(Some(wrapping(&"Every contact is copied. ".repeat(20)).upcast_ref()));
    let (minimum, natural, ..) = natural_width(&alert);
    assert_eq!(natural, crate::dialog::DIALOG_WIDTH);
    assert!(minimum < natural, "and narrower in a narrow window");
    alert.set_content_size(600, -1);
    assert!(natural_width(&alert).1 >= 600, "the app's width is the widget's");
    let bare = Dialog::new(None, None);
    bare.set_extra_child(Some(gtk::Label::new(Some("x")).upcast_ref()));
    assert!(
        natural_width(&bare).1 < 100,
        "a dialog of the app's widget alone is as wide as the widget"
    );
}

fn a_response_turned_off_hands_the_keyboard_on() {
    let window = gtk::Window::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let _shell = Shell::new(&window, &content);
    let dialog = Dialog::new(Some("Rename"), None);
    let entry = gtk::Entry::new();
    dialog.set_extra_child(Some(entry.upcast_ref()));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("save", "Save");
    window.present();
    dialog.present(&content);
    let focus = || gtk::prelude::RootExt::focus(&window);
    assert!(dialog.focus("save"));
    assert!(
        focus().is_some_and(|on| on.is::<gtk::Button>()),
        "Save has the keyboard"
    );
    dialog.set_response_enabled("save", false);
    let now = focus().expect("the keyboard is somewhere at once, with no turn of the main loop");
    assert!(
        now.is_ancestor(&entry),
        "on the field, where the dialog puts it: {now:?}"
    );
    window.destroy();
}

fn a_response_can_leave_the_dialog_open() {
    let window = gtk::Window::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let shell = Shell::new(&window, &content);
    let dialog = Dialog::new(Some("Sync log"), None);
    dialog.add_response("clear", "Clear");
    dialog.add_response("close", "Close");
    dialog.set_response_closes("clear", false);
    dialog.set_close_response("close");
    let said = Rc::new(RefCell::new(Vec::new()));
    dialog.connect_response({
        let said = said.clone();
        move |_, id| said.borrow_mut().push(id.to_owned())
    });
    dialog.present(&content);
    dialog.click("clear");
    assert!(shell.dialog_open(), "Clear leaves it open");
    dialog.click("close");
    assert!(!shell.dialog_open());
    assert_eq!(*said.borrow(), ["clear", "close"]);
    // The close response closes whatever the others do, and turned back on a response closes again.
    dialog.set_response_closes("clear", true);
    dialog.present(&content);
    dialog.click("clear");
    assert!(!shell.dialog_open());
    dialog.set_response_closes("close", false);
    dialog.present(&content);
    dialog.close();
    assert!(
        !shell.dialog_open(),
        "Escape closes, even with a close response that would not"
    );
    dialog.present(&content);
    dialog.click("close");
    assert!(!shell.dialog_open(), "and so does the close response's own button");
    assert_eq!(*said.borrow(), ["clear", "close", "clear", "close", "close"]);
}

fn a_resizable_split_keeps_its_sidebar_within_its_limits() {
    let lay_out_split = |split: &Split, width| {
        split.widget.measure(gtk::Orientation::Horizontal, -1);
        split.widget.allocate(width, 600, -1, None);
    };
    // A width restored before the split is first laid out is what a screen reader hears once it is.
    let restored = Split::new(&gtk::Label::new(None), &gtk::Label::new(None), (240, 360), Some(600));
    restored.set_resizable(true);
    restored.set_sidebar_width(280);
    lay_out_split(&restored, 1200);
    assert_eq!(divider_says(&restored.widget.last_child().unwrap()), (240, 360, 280));

    let sidebar = gtk::Label::new(Some("list"));
    let content = gtk::Label::new(Some("content"));
    let split = Split::new(&sidebar, &content, (240, 360), Some(600));
    split.set_resizable(true);
    let divider = split.widget.last_child().unwrap();
    assert!(divider.has_css_class("om-divider") && divider.is_focusable());
    let lay_out = |width| lay_out_split(&split, width);
    lay_out(1200);
    assert_eq!(split.sidebar_width(), 300, "a quarter until a width is set");
    split.set_sidebar_width(500);
    lay_out(1200);
    assert_eq!(split.sidebar_width(), 360, "no more than the most");
    split.set_sidebar_width(280);
    lay_out(1200);
    assert_eq!((split.sidebar_width(), sidebar.width()), (280, 280));
    let edge = divider
        .compute_point(&split.widget, &gtk::graphene::Point::zero())
        .unwrap()
        .x() as i32;
    assert_eq!(edge + divider.width() / 2, 280, "the divider is centred on the edge");
    // Narrower, the content keeps what it needs, down to the sidebar's least.
    let needs = content.measure(gtk::Orientation::Horizontal, -1).0;
    lay_out(280 + needs - 10);
    assert_eq!(sidebar.width(), 270);
    lay_out(200);
    assert_eq!(sidebar.width(), 240);
    assert_eq!(split.sidebar_width(), 280, "the width to save is the one chosen");
    assert_eq!(
        divider_says(&divider),
        (240, 240, 240),
        "a narrower window narrows the limits"
    );
    // New limits, for a sidebar that now shows something else: the width chosen is kept within them.
    split.set_sidebar_limits((300, 400));
    lay_out(1200);
    assert_eq!((sidebar.width(), divider_says(&divider)), (300, (300, 400, 300)));
    split.set_sidebar_width(380);
    lay_out(1200);
    assert_eq!(sidebar.width(), 380);
    split.set_sidebar_limits((240, 360));
    // With the sidebar put away there is no edge to drag.
    split.set_show_sidebar(false);
    assert!(!divider.is_child_visible());
    split.set_show_sidebar(true);
    assert!(divider.is_child_visible());
    split.set_resizable(false);
    assert_eq!(
        split.widget.last_child().as_ref(),
        Some(content.parent().unwrap().upcast_ref())
    );
}

/// The least, the most and the width the divider tells a screen reader, of a whole number of pixels from 0 to 1000.
fn divider_says(divider: &gtk::Widget) -> (i32, i32, i32) {
    use gtk::{ffi, glib::translate::ToGlibPtr};
    let says = |property, value: i32| {
        // SAFETY: the property is a double, so one double follows it; the answer is NULL or a new string.
        let wrong = unsafe {
            ffi::gtk_test_accessible_check_property(
                divider.upcast_ref::<gtk::Accessible>().to_glib_none().0,
                property,
                f64::from(value),
            )
        };
        // SAFETY: GTK's to free.
        unsafe { glib::ffi::g_free(wrong.cast()) };
        wrong.is_null()
    };
    let find = |property| (0..=1000).find(|&value| says(property, value)).unwrap_or(-1);
    (
        find(ffi::GTK_ACCESSIBLE_PROPERTY_VALUE_MIN),
        find(ffi::GTK_ACCESSIBLE_PROPERTY_VALUE_MAX),
        find(ffi::GTK_ACCESSIBLE_PROPERTY_VALUE_NOW),
    )
}

fn a_banner_has_an_icon_and_a_button_of_either_kind() {
    let banner = Banner::new("Not linked to iCloud yet.", Some("Clone…"));
    let content = banner.widget.child().unwrap();
    let icon = content.first_child().and_downcast::<gtk::Image>().unwrap();
    let button = content.last_child().and_downcast::<gtk::Button>().unwrap();
    assert!(!icon.is_visible(), "no icon until one is set");
    banner.set_icon(Some(crate::icons::name::CLOUD), "accent");
    assert!(icon.is_visible() && icon.has_css_class("accent"));
    banner.set_icon(Some(crate::icons::name::WARNING), "warning");
    assert!(icon.has_css_class("warning") && !icon.has_css_class("accent"));
    banner.set_button_suggested(true);
    banner.set_button_enabled(false);
    assert!(button.has_css_class("suggested-action") && !button.is_sensitive());
    banner.set_icon(None, "");
    assert!(!icon.is_visible());
}

/// QA's `select` on an item of a list view, as on a grid view's (dev/qa/gallery.jsonl): by a label in it, through the
/// view's selection model; a model that selects nothing says so.
#[cfg(feature = "qa")]
fn qa_selects_a_list_views_item_through_its_model() {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        item.set_child(Some(&gtk::Label::new(None)));
    });
    factory.connect_bind(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let text = item.item().and_downcast::<gtk::StringObject>().unwrap().string();
        item.child().and_downcast::<gtk::Label>().unwrap().set_text(&text);
    });
    let model = gtk::StringList::new(&["Inbox", "Sent", "Archive"]);
    let selection = gtk::SingleSelection::new(Some(model.clone()));
    let view = gtk::ListView::new(Some(selection.clone()), Some(factory.clone()));
    let window = gtk::Window::builder().child(&view).build();
    window.present();
    let label = |view: &gtk::ListView, text: &str| {
        for _ in 0..500 {
            let mut found = None;
            let mut item = view.first_child();
            while let Some(widget) = item {
                let label = widget.first_child().and_downcast::<gtk::Label>();
                found = found.or(label.filter(|label| label.text() == text && label.is_mapped()));
                item = widget.next_sibling();
            }
            if let Some(label) = found {
                return label;
            }
            glib::MainContext::default().iteration(false);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the view showed no {text:?}");
    };
    assert_eq!(selection.selected(), 0);
    crate::qa::select(label(&view, "Sent").upcast_ref()).unwrap();
    assert_eq!(selection.selected(), 1);
    let widget = label(&view, "Sent").parent().unwrap();
    assert!(widget.state_flags().contains(gtk::StateFlags::SELECTED));
    // A view whose model selects nothing.
    let unselectable = gtk::ListView::new(Some(gtk::NoSelection::new(Some(model))), Some(factory));
    window.set_child(Some(&unselectable));
    let error = crate::qa::select(label(&unselectable, "Sent").upcast_ref()).unwrap_err();
    assert!(error.contains("does not select it"), "{error}");
    window.destroy();
}

/// A row's control is labelled by the row's title, which a screen reader says and QA finds it by, and follows a new
/// title; it is described by the row's subtitle.
#[cfg(feature = "qa")]
fn qa_names_a_rows_control_by_its_title() {
    use glib::translate::*;
    let (entry_row, entry) = crate::entry_row("Name", "Ada");
    let (password_row, password) = crate::password_row("Secret access key");
    let (switch_row, switch) = crate::switch_row("Sync settings too", "Themes and key bindings", true);
    let often = gtk::StringList::new(&["Hourly", "Daily"]);
    let (combo_row, dropdown) = crate::combo_row("Fetch", "", &often, 0);
    let group = crate::Group::new("", "");
    for row in [&entry_row, &password_row, &switch_row, &combo_row] {
        group.add(&row.widget);
    }
    let window = gtk::Window::builder().child(&group.widget).build();
    let first = |widget: &gtk::Widget| crate::qa::names(widget).into_iter().next();
    assert_eq!(first(entry.upcast_ref()).as_deref(), Some("Name"));
    assert_eq!(first(password.upcast_ref()).as_deref(), Some("Secret access key"));
    assert_eq!(first(switch.upcast_ref()).as_deref(), Some("Sync settings too"));
    assert_eq!(first(dropdown.upcast_ref()).as_deref(), Some("Fetch"));
    entry_row.set_title("Full name");
    assert_eq!(first(entry.upcast_ref()).as_deref(), Some("Full name"));
    // The switch, not the row, has the keyboard: it is described by the row's subtitle.
    fn label(widget: &gtk::Widget, text: &str) -> Option<gtk::Widget> {
        if widget
            .downcast_ref::<gtk::Label>()
            .is_some_and(|label| label.label() == text)
        {
            return Some(widget.clone());
        }
        let mut child = widget.first_child();
        while let Some(inner) = child {
            if let Some(found) = label(&inner, text) {
                return Some(found);
            }
            child = inner.next_sibling();
        }
        None
    }
    let subtitle = label(switch_row.widget.upcast_ref(), "Themes and key bindings").expect("the row's subtitle");
    // SAFETY: a NULL-terminated list of accessibles follows the relation; the answer is a new string, or NULL when
    // the relation holds exactly them.
    let mismatch: Option<glib::GString> = unsafe {
        from_glib_full(gtk::ffi::gtk_test_accessible_check_relation(
            switch.upcast_ref::<gtk::Accessible>().to_glib_none().0,
            gtk::ffi::GTK_ACCESSIBLE_RELATION_DESCRIBED_BY,
            subtitle.upcast_ref::<gtk::Accessible>().as_ptr(),
            std::ptr::null::<gtk::ffi::GtkAccessible>(),
        ))
    };
    assert_eq!(mismatch, None, "the switch is described by the row's subtitle");
    window.destroy();
}

/// QA's `key` names a letter as GTK's accelerators do, and sends the one a keyboard would: upper case with Shift.
#[cfg(feature = "qa")]
fn qa_sends_the_key_a_keyboard_would() {
    use gtk::gdk::{Key, ModifierType};
    let key = crate::qa::keyboard_key;
    let shift = ModifierType::SHIFT_MASK;
    assert_eq!(key("R"), Ok((Key::R, shift)));
    assert_eq!(key("<Shift>r"), Ok((Key::R, shift)));
    assert_eq!(key("<Shift>R"), Ok((Key::R, shift)));
    assert_eq!(key("r"), Ok((Key::r, ModifierType::empty())));
    assert_eq!(key("<Control>u"), Ok((Key::u, ModifierType::CONTROL_MASK)));
    // Upper case is Shift with any modifiers, where GTK's accelerators would read Ctrl+u.
    assert_eq!(key("<Control>U"), Ok((Key::U, ModifierType::CONTROL_MASK | shift)));
    assert_eq!(key("question"), Ok((Key::question, ModifierType::empty())));
    assert_eq!(key("<Shift>Tab"), Ok((Key::Tab, shift)));
    assert!(key("NoSuchKey").is_err());
}

/// Runs the main loop until `done`, for five seconds at most.
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let main = glib::MainContext::default();
    let start = std::time::Instant::now();
    while !done() {
        assert!(start.elapsed().as_secs() < 5, "{what}");
        main.iteration(false);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// Runs the main loop for a while: for what must not happen.
fn wait_for(milliseconds: u64) {
    let main = glib::MainContext::default();
    let start = std::time::Instant::now();
    while start.elapsed().as_millis() < u128::from(milliseconds) {
        main.iteration(false);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// Escape on a dialog always reaches its handlers, with the answer that loses nothing: the one the app named, else
/// Cancel, else "close".
fn a_dialog_put_away_always_answers() {
    let window = gtk::Window::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let shell = Shell::new(&window, &content);
    let said = Rc::new(RefCell::new(Vec::new()));
    let hear = |dialog: &Dialog| {
        let said = said.clone();
        dialog.connect_response(move |_, id| said.borrow_mut().push(id.to_owned()));
    };
    // No close response and no Cancel: it used to close without a word.
    let question = Dialog::new(Some("Keep the changes?"), None);
    question.add_response("discard", "Discard");
    question.add_response("keep", "Keep");
    hear(&question);
    assert_eq!(question.close_response(), Dialog::CLOSE);
    question.present(&content);
    question.close();
    assert!(!shell.dialog_open());
    assert_eq!(*said.borrow(), ["close"]);
    // The app names the answer that loses nothing.
    question.set_close_response("keep");
    question.present(&content);
    question.close();
    assert_eq!(*said.borrow(), ["close", "keep"]);
    // A dialog with Cancel gives that.
    let confirm = Dialog::new(Some("Delete it?"), None);
    confirm.add_response("cancel", "Cancel");
    confirm.add_response("delete", "Delete");
    confirm.set_response_closes("cancel", false);
    hear(&confirm);
    confirm.present(&content);
    confirm.close();
    assert_eq!(said.borrow().last().unwrap(), "cancel");
    confirm.present(&content);
    confirm.click("cancel");
    assert!(
        !shell.dialog_open(),
        "and Cancel, being the close response, closes from its button too"
    );
    // A dialog with nothing but the app's widget, and one in a window of its own.
    let bare = Dialog::new(None, None);
    bare.set_extra_child(Some(gtk::Label::new(Some("About")).upcast_ref()));
    hear(&bare);
    bare.present(&content);
    bare.close();
    let alone = Dialog::new(Some("No shell here"), None);
    hear(&alone);
    alone.present(&gtk::Window::new());
    alone.close();
    assert_eq!(*said.borrow(), ["close", "keep", "cancel", "cancel", "close", "close"]);
    // What does not dismiss says nothing: a dialog that cannot be closed, one closed by force, one not open.
    let count = said.borrow().len();
    question.set_can_close(false);
    question.present(&content);
    question.close();
    assert!(shell.dialog_open());
    question.force_close();
    question.close();
    assert_eq!(said.borrow().len(), count);
    window.destroy();
    drop((window, content, shell));

    // A window that closes with a question open over it takes the question with it, which is answered as if it had
    // been put away: drawn on the window's shell, and in a window of its own over one without.
    question.set_can_close(true);
    question.set_close_response("keep");
    {
        let window = gtk::Window::new();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let _shell = Shell::new(&window, &content);
        window.present();
        question.present(&content);
        window.destroy();
    }
    wait_for(100);
    assert_eq!(said.borrow()[count..], ["keep"]);
    {
        let parent = gtk::Window::new();
        parent.present();
        alone.present(&parent);
        parent.destroy();
    }
    wait_for(100);
    assert_eq!(said.borrow()[count..], ["keep", "close"]);
    // And either can be presented again.
    question.present(&gtk::Window::new());
    question.close();
    assert_eq!(said.borrow().last().unwrap(), "keep");
}

fn a_single_key_is_a_character_while_a_field_has_the_keyboard() {
    let window = gtk::Window::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let (button, entry, view) = (gtk::Button::with_label("Play"), gtk::Entry::new(), gtk::TextView::new());
    let search = gtk::SearchEntry::new();
    content.append(&button);
    content.append(&entry);
    content.append(&view);
    content.append(&search);
    let _shell = Shell::new(&window, &content);
    let pressed = Rc::new(RefCell::new(Vec::new()));
    let press = |key: &'static str| {
        let pressed = pressed.clone();
        move || pressed.borrow_mut().push(key)
    };
    let keys = Keys::new(&window);
    keys.on("q", press("q")).on("?", press("?")).on("space", press("space"));
    keys.on_always("F1", press("F1")).on_always("<Control>s", press("save"));
    // A key that is none is left out, with a warning: caught here, so that a warning the test run prints is always
    // a fault (dev/gtk-tests.sh fails on one).
    let warned = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let handler = glib::log_set_handler(Some("omui"), glib::LogLevels::LEVEL_WARNING, false, false, {
        let warned = warned.clone();
        move |_, _, message| warned.lock().unwrap().push(message.to_owned())
    });
    keys.on("no such key", press("never"));
    glib::log_remove_handler(Some("omui"), handler);
    assert_eq!(
        *warned.lock().unwrap(),
        ["\"no such key\" is not a key, so its shortcut is left out"]
    );
    window.present();

    assert!(button.grab_focus() && !crate::text_has_focus(&button));
    assert!(keys.activate("q") && keys.activate("?") && keys.activate("space") && keys.activate("F1"));
    assert!(
        !keys.activate("x") && !keys.activate("no such key"),
        "a key that was never added does nothing"
    );
    assert_eq!(*pressed.borrow(), ["q", "?", "space", "F1"]);
    // In a field of any kind a single key is typed; a key that types nothing still acts.
    for field in [
        entry.upcast_ref::<gtk::Widget>(),
        view.upcast_ref(),
        search.upcast_ref(),
    ] {
        assert!(field.grab_focus() && crate::text_has_focus(&window), "{field:?}");
        assert!(!keys.activate("q") && !keys.activate("?") && !keys.activate("space"));
        assert!(keys.activate("F1") && keys.activate("<Control>s"));
    }
    assert_eq!(pressed.borrow().len(), 4 + 3 * 2);
    // Under a dialog the window's keys are held back, these among them, and given back after.
    button.grab_focus();
    let dialog = Dialog::new(Some("Sure?"), None);
    dialog.present(&content);
    assert!(!keys.activate("q") && !keys.activate("F1"));
    dialog.close();
    assert!(keys.activate("q"));

    // Ctrl+W closes the window, as its close button would: a window that asks first is asked.
    let asked = Rc::new(std::cell::Cell::new(0));
    window.connect_close_request({
        let asked = asked.clone();
        move |_| {
            asked.set(asked.get() + 1);
            glib::Propagation::Stop
        }
    });
    let quit = crate::quit_keys(&window);
    assert!(quit.activate("<Control>w") && quit.activate("<Control>q"));
    entry.grab_focus();
    assert!(quit.activate("<Control>q"), "from a text field too");
    assert_eq!(asked.get(), 3);
    assert!(window.is_visible(), "the window said no");
    window.destroy();
}

/// The labels under `widget`, in order.
fn labels(widget: &gtk::Widget, found: &mut Vec<String>) {
    if let Some(label) = widget.downcast_ref::<gtk::Label>() {
        found.push(label.label().into());
    }
    let mut child = widget.first_child();
    while let Some(inner) = child {
        labels(&inner, found);
        child = inner.next_sibling();
    }
}

fn the_shortcuts_overlay_lists_the_keys_and_closes_as_it_opened() {
    let window = gtk::Window::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let (button, entry) = (gtk::Button::with_label("Play"), gtk::Entry::new());
    content.append(&button);
    content.append(&entry);
    let shell = Shell::new(&window, &content);
    let asked = Rc::new(std::cell::Cell::new(0));
    let overlay = ShortcutOverlay::new({
        let asked = asked.clone();
        move || {
            asked.set(asked.get() + 1);
            vec![
                ShortcutSection::new("Playback", &[("Space", "Play or pause"), ("Ctrl+Shift+Z", "Redo")]),
                ShortcutSection::new("", &[("? / F1", "Show or hide the shortcuts")])
                    .alternatives(&["Ctrl+Q", "Ctrl+W"], "Close"),
            ]
        }
    });
    let keys = overlay.install(&window, false);
    window.present();
    button.grab_focus();
    assert!(!overlay.is_open() && keys.activate("?"));
    assert!(overlay.is_open() && shell.dialog_open());
    let mut shown = Vec::new();
    labels(shell.overlay.last_child().unwrap().upcast_ref(), &mut shown);
    assert_eq!(
        shown,
        [
            "Keyboard shortcuts",
            "Playback",
            "",
            "Play or pause",
            "",
            "Space",
            "Redo",
            "",
            "Ctrl",
            "Shift",
            "Z",
            "",
            "",
            "Show or hide the shortcuts",
            "",
            "?",
            "/",
            "F1",
            "Close",
            "",
            "Ctrl",
            "Q",
            "/",
            "Ctrl",
            "W"
        ],
        "the heading, then each section's title and description, and each row's title, subtitle and keys"
    );
    // The window's keys are held back while it is open; the overlay itself closes on the key, and on Escape.
    assert!(!keys.activate("?"));
    overlay.toggle(&content);
    assert!(!overlay.is_open() && !shell.dialog_open());
    assert!(keys.activate("F1") && overlay.is_open());
    overlay.close();
    assert!(!overlay.is_open());
    assert_eq!(
        asked.get(),
        2,
        "the sections are asked for each time, so they are in the language of the moment"
    );
    // While the user types, `?` is a question mark. An app that is typed into opens it with Ctrl+? and F1 alone.
    entry.grab_focus();
    assert!(!keys.activate("?") && !overlay.is_open());
    let typed = ShortcutOverlay::new(Vec::new);
    let keys = typed.install(&window, true);
    assert!(!keys.activate("?"));
    assert!(keys.activate("<Control>question") && typed.is_open());
    typed.close();
    assert!(keys.activate("F1") && typed.is_open());
    typed.close();
    window.destroy();
}

/// `on_narrow` and `tabs` hear of a width from the layouts that change it. Nothing runs once a frame: a window that
/// is left alone draws no frames at all.
fn a_width_is_followed_by_layouts_not_by_frames() {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let stack = gtk::Stack::new();
    stack
        .add_titled(&gtk::Label::new(Some("one")), Some("one"), "Inbox")
        .set_icon_name(crate::icons::name::DOCUMENT);
    stack
        .add_titled(&gtk::Label::new(Some("two")), Some("two"), "Archive")
        .set_icon_name(crate::icons::name::DOCUMENT);
    let bar = crate::tabs(&stack);
    content.append(&bar);
    content.append(&stack);
    let window = gtk::Window::builder()
        .child(&content)
        .default_width(900)
        .default_height(300)
        .build();
    let told = Rc::new(RefCell::new(Vec::new()));
    crate::on_narrow(&content, 640, {
        let told = told.clone();
        move |narrow| told.borrow_mut().push(narrow)
    });
    window.present();
    wait_until("told once it has a width", || !told.borrow().is_empty());
    assert_eq!(*told.borrow(), [false]);
    assert!(!bar.has_css_class("narrow"));
    window.set_default_size(500, 300);
    wait_until("told when it crosses", || {
        told.borrow().len() == 2 && bar.has_css_class("narrow")
    });
    assert_eq!(*told.borrow(), [false, true]);
    let title = bar.first_child().unwrap().first_child().unwrap().last_child().unwrap();
    assert!(!title.is_visible(), "a narrow window's tabs are their icons");
    window.set_default_size(560, 300);
    wait_for(200);
    assert_eq!(
        told.borrow().len(),
        2,
        "a width that stays on its side is not told again"
    );
    // Left alone, the window asks for no frames. A tick callback would have it draw sixty a second.
    let clock = window.frame_clock().expect("a window on screen has a clock");
    let frames = Rc::new(std::cell::Cell::new(0));
    let counting = clock.connect_update({
        let frames = frames.clone();
        move |_| frames.set(frames.get() + 1)
    });
    wait_for(500);
    clock.disconnect(counting);
    assert!(
        frames.get() <= 2,
        "{} frames in half a second of nothing happening",
        frames.get()
    );
    // A widget laid out already is told at once, and one that has left its window is no longer followed.
    let late = Rc::new(RefCell::new(Vec::new()));
    crate::on_narrow(&stack, 640, {
        let late = late.clone();
        move |narrow| late.borrow_mut().push(narrow)
    });
    wait_until("a widget on screen already", || !late.borrow().is_empty());
    assert_eq!(*late.borrow(), [true]);
    window.destroy();
}

/// Every window the app has and every one omui makes carries the app's short name as its Wayland app id. Broadway
/// has no app id to read, so this sees that each window is named, and that naming one where there is no Wayland does
/// nothing; that the compositor gets the name is checked on a real session (README.md).
fn every_window_is_named_by_the_apps_short_name() {
    // `omui::application` would need a session bus to register; an application with no id does not, and is wired
    // the same way.
    let app = gtk::Application::new(None::<&str>, gtk::gio::ApplicationFlags::NON_UNIQUE);
    crate::name_windows(&app);
    app.register(None::<&gtk::gio::Cancellable>)
        .expect("an application with no id registers without a bus");
    let added = gtk::Window::new();
    app.add_window(&added);
    let built = gtk::ApplicationWindow::builder().application(&app).build();
    let outside = gtk::Window::new();
    assert!(crate::is_named(&added) && crate::is_named(&built));
    assert!(
        !crate::is_named(&outside),
        "a window kept out of the application is the app's to name"
    );
    // A dialog over a window without a shell gets a window of its own, and so does the shortcuts overlay.
    let open_windows = || {
        gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|window| window.downcast::<gtk::Window>().ok())
            .filter(|window| window.transient_for().as_ref() == Some(&outside))
            .collect::<Vec<_>>()
    };
    outside.present();
    let dialog = Dialog::new(Some("Rename"), None);
    dialog.present(&outside);
    let overlay = ShortcutOverlay::new(|| vec![ShortcutSection::new("General", &[("Q", "Quit")])]);
    overlay.present(&outside);
    let dialogs = open_windows();
    assert_eq!(dialogs.len(), 2);
    assert!(dialogs.iter().all(crate::is_named));
    // Mapped on a backend that is not Wayland, and again, a window is named with nothing to do.
    built.present();
    wait_until("the window is mapped", || built.is_mapped());
    built.set_visible(false);
    built.present();
    wait_until("the window is mapped again", || built.is_mapped());
    crate::name_window(&built);
    crate::name_window(&outside);
    assert!(crate::is_named(&outside));
    dialog.close();
    overlay.close();
    for window in [added, built.upcast(), outside] {
        window.destroy();
    }
}

fn a_switch_of_language_reaches_bound_text_and_turns_the_windows() {
    let _language = i18n::lock_language();
    i18n::set_locale_registry(None);
    i18n::register_all(&[
        (
            "en",
            r#"{"omui-gtk Save": "omui-gtk Save", "omui-gtk Undo": "omui-gtk Undo"}"#,
        ),
        (
            "da",
            r#"{"omui-gtk Save": "Gem", "omui-gtk Undo": "Fortryd", "Sidebar width": "Sidebjælkens bredde"}"#,
        ),
        ("ar", r#"{"omui-gtk Save": "حفظ"}"#),
    ]);
    let (sidebar, content) = (gtk::Label::new(Some("list")), gtk::Label::new(Some("content")));
    let split = Split::new(&sidebar, &content, (240, 360), Some(600));
    split.set_resizable(true);
    let button = gtk::Button::new();
    i18n::bind_label(&button, "omui-gtk Save");
    let undo = crate::icon_button(crate::icons::name::CLOSE, "");
    i18n::bind_tooltip(&undo, "omui-gtk Undo");
    let status = gtk::Label::new(None);
    let count = Rc::new(std::cell::Cell::new(0));
    i18n::bind(&status, {
        let count = count.clone();
        move |status: &gtk::Label| {
            count.set(count.get() + 1);
            status.set_label(&format!("{} {}", i18n::language(), i18n::format_number(1234.5, 1)));
        }
    });
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.append(&split.widget);
    page.append(&button);
    page.append(&undo);
    page.append(&status);
    let window = gtk::Window::builder()
        .child(&page)
        .default_width(1000)
        .default_height(300)
        .build();
    window.present();
    wait_until("the split is laid out", || sidebar.width() > 0);
    let left_of = |pane: &gtk::Label| {
        pane.compute_point(&split.widget, &gtk::graphene::Point::zero())
            .unwrap()
            .x() as i32
    };
    assert_eq!(
        (button.label().as_deref(), status.label().as_str()),
        (Some("omui-gtk Save"), "en 1,234.5")
    );
    assert_eq!((left_of(&sidebar), window.direction()), (0, gtk::TextDirection::Ltr));

    i18n::set_language("da");
    assert_eq!(button.label().as_deref(), Some("Gem"));
    assert_eq!(undo.tooltip_text().as_deref(), Some("Fortryd"));
    assert!(status.label().starts_with("da "));
    assert_eq!(count.get(), 2, "bound once and told once");

    // Arabic reads right to left: every window turns, the one open already too, and the sidebar changes sides.
    i18n::set_language("ar");
    assert_eq!(button.label().as_deref(), Some("حفظ"));
    assert_eq!(
        undo.tooltip_text().as_deref(),
        Some("omui-gtk Undo"),
        "English, where Arabic has none"
    );
    assert_eq!(gtk::Widget::default_direction(), gtk::TextDirection::Rtl);
    assert_eq!(window.direction(), gtk::TextDirection::Rtl);
    wait_until("the split is laid out anew", || left_of(&sidebar) > 0);
    assert_eq!(
        left_of(&sidebar) + sidebar.width(),
        split.widget.width(),
        "the sidebar is where reading starts"
    );
    assert_eq!(left_of(&content), 0);
    let divider = split.widget.last_child().unwrap();
    let edge = divider
        .compute_point(&split.widget, &gtk::graphene::Point::zero())
        .unwrap()
        .x() as i32;
    assert_eq!(
        edge + divider.width() / 2,
        left_of(&sidebar),
        "and the divider is on the edge between them"
    );

    i18n::set_language("en");
    assert_eq!(gtk::Widget::default_direction(), gtk::TextDirection::Ltr);
    assert_eq!(button.label().as_deref(), Some("omui-gtk Save"));
    // A widget that has gone is no longer told. The test's own references go with the window: GTK keeps what had
    // the keyboard in a window for as long as the window itself is held.
    let told = count.get();
    let gone = status.downgrade();
    window.destroy();
    drop((window, page, status, button, undo));
    wait_for(100);
    assert!(gone.upgrade().is_none(), "the label has gone");
    i18n::set_language("da");
    i18n::set_language("en");
    assert_eq!(count.get(), told);
}

/// What a dialog says is relabelled in place on a switch of language: given as English with the `_translated`
/// setters, or set again from `Dialog::bind`; `confirm` and the About dialog do so of themselves.
fn a_dialog_follows_a_switch_of_language_while_it_is_open() {
    let _language = i18n::lock_language();
    i18n::set_locale_registry(None);
    i18n::register_all(&[(
        "da",
        r#"{"omui-gtk Delete it?": "Slet den?", "omui-gtk Gone for good.": "Væk for altid.",
            "omui-gtk _Delete": "_Slet", "_Cancel": "_Annuller", "Version {}": "Udgave {}",
            "omui-gtk An app.": "En app.", "omui-gtk {} files": "{} filer"}"#,
    )]);
    let window = gtk::Window::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let shell = Shell::new(&window, &content);
    window.present();
    let shown = || {
        let mut shown = Vec::new();
        labels(shell.overlay.last_child().unwrap().upcast_ref(), &mut shown);
        shown.retain(|label| !label.is_empty());
        shown
    };

    let dialog = Dialog::new(Some("omui-gtk Delete it?"), Some(""));
    dialog.set_heading_translated("omui-gtk Delete it?");
    dialog.bind(|dialog| dialog.set_body(&crate::tr!("omui-gtk {} files", 3)));
    dialog.add_response("delete", "omui-gtk _Delete");
    dialog.set_response_translated("delete", "omui-gtk _Delete");
    dialog.present(&content);
    assert_eq!(shown(), ["omui-gtk Delete it?", "omui-gtk 3 files", "omui-gtk _Delete"]);
    i18n::set_language("da");
    assert_eq!(shown(), ["Slet den?", "3 filer", "_Slet"], "open, in the new language");
    i18n::set_language("en");
    assert_eq!(shown(), ["omui-gtk Delete it?", "omui-gtk 3 files", "omui-gtk _Delete"]);
    dialog.close();

    // `confirm` translates what it is given, and shows as it is what no catalog has.
    crate::confirm(
        &content,
        "omui-gtk Delete it?",
        "omui-gtk Gone for good.",
        "omui-gtk _Delete",
        crate::Appearance::Destructive,
        || {},
    );
    i18n::set_language("da");
    assert_eq!(shown(), ["Slet den?", "Væk for altid.", "_Annuller", "_Slet"]);
    i18n::set_language("en");
    assert_eq!(
        shown(),
        [
            "omui-gtk Delete it?",
            "omui-gtk Gone for good.",
            "_Cancel",
            "omui-gtk _Delete"
        ]
    );
    escape_on_top(&shell);
    assert!(!shell.dialog_open());
    crate::confirm(
        &content,
        "Allerede dansk",
        "Ingen katalog",
        "_Ja",
        crate::Appearance::Suggested,
        || {},
    );
    i18n::set_language("da");
    assert_eq!(shown(), ["Allerede dansk", "Ingen katalog", "_Annuller", "_Ja"]);
    i18n::set_language("en");
    escape_on_top(&shell);

    // The About dialog: omui's words and the app's text.
    crate::about(&content, "Notes", "1.2", "omui-gtk An app.", None);
    i18n::set_language("da");
    assert_eq!(shown(), ["Notes", "Udgave 1.2\n\nEn app."]);
    i18n::set_language("en");
    assert_eq!(shown(), ["Notes", "Version 1.2\n\nomui-gtk An app."]);
    escape_on_top(&shell);

    // In a window of its own, the window's title follows the heading.
    let alone = Dialog::new(Some("omui-gtk Delete it?"), None);
    alone.set_heading_translated("omui-gtk Delete it?");
    let parent = gtk::Window::new();
    alone.present(&parent);
    let own = gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|window| window.downcast::<gtk::Window>().ok())
        .find(|window| window.transient_for().as_ref() == Some(&parent))
        .expect("the dialog's own window");
    i18n::set_language("da");
    assert_eq!(own.title().as_deref(), Some("Slet den?"));
    i18n::set_language("en");
    alone.close();
    parent.destroy();
    window.destroy();
}

/// Escape on the dialog on top of `shell`, as its backdrop has the key from the widget that has the keyboard.
fn escape_on_top(shell: &Shell) {
    use gtk::glib::translate::IntoGlib;
    let backdrop = shell.overlay.last_child().expect("a dialog's backdrop");
    let taken = backdrop.observe_controllers().iter::<glib::Object>().any(|controller| {
        controller
            .ok()
            .and_downcast::<gtk::EventControllerKey>()
            .is_some_and(|keys| {
                let key = gtk::gdk::Key::Escape.into_glib();
                keys.emit_by_name::<bool>("key-pressed", &[&key, &0u32, &gtk::gdk::ModifierType::empty()])
            })
    });
    assert!(taken, "the dialog took Escape");
}

/// Escape, as the window's capture handlers have it from the keyboard: what a dialog over the window hears even
/// with the keyboard nowhere in it. Whether one took it.
fn escape_on(window: &gtk::Window) -> bool {
    use gtk::glib::translate::IntoGlib;
    let mut taken = false;
    for controller in window.observe_controllers().iter::<glib::Object>() {
        let Some(keys) = controller.ok().and_downcast::<gtk::EventControllerKey>() else {
            continue;
        };
        if keys.propagation_phase() != gtk::PropagationPhase::Capture {
            continue;
        }
        let key = gtk::gdk::Key::Escape.into_glib();
        taken |= keys.emit_by_name::<bool>("key-pressed", &[&key, &0u32, &gtk::gdk::ModifierType::empty()]);
        if taken {
            break;
        }
    }
    taken
}

/// GTK drops the focus when the focused widget is disabled or taken away, and a dialog takes it back only a moment
/// later. Escape before then still closes the dialog.
fn a_dialog_keeps_its_keys_when_its_focused_widget_is_taken_away() {
    let window = gtk::Window::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let shell = Shell::new(&window, &content);
    window.present();
    let said = Rc::new(RefCell::new(Vec::new()));
    let dialog = Dialog::new(Some("Recently deleted"), None);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let (first, second) = (
        gtk::Button::with_label("Put Back"),
        gtk::Button::with_label("Put Back too"),
    );
    list.append(&first);
    list.append(&second);
    dialog.set_extra_child(Some(list.upcast_ref()));
    dialog.connect_response({
        let said = said.clone();
        move |_, id| said.borrow_mut().push(id.to_owned())
    });
    dialog.present(&content);
    assert!(first.grab_focus(), "the button has the keyboard");
    // With the keyboard in it, Escape is the focused widget's and then the dialog's, as ever: not the window's.
    assert!(!escape_on(&window) && shell.dialog_open());
    // The button that had the keyboard is pressed, and goes with its row.
    first.set_sensitive(false);
    list.remove(&first);
    let focus = gtk::prelude::RootExt::focus(&window);
    assert!(
        focus.is_none_or(|focus| !focus.is_ancestor(&list)),
        "GTK has left the keyboard nowhere in the dialog"
    );
    assert!(
        escape_on(&window),
        "Escape at once, before the dialog takes the keyboard back"
    );
    assert!(!shell.dialog_open());
    assert_eq!(*said.borrow(), ["close"]);
    // Tab, likewise, brings the keyboard back into the dialog rather than to what is under it.
    dialog.present(&content);
    assert!(second.grab_focus(), "the other button has the keyboard");
    list.remove(&second);
    let tab = |window: &gtk::Window| {
        use gtk::glib::translate::IntoGlib;
        window.observe_controllers().iter::<glib::Object>().any(|controller| {
            controller
                .ok()
                .and_downcast::<gtk::EventControllerKey>()
                .filter(|keys| keys.propagation_phase() == gtk::PropagationPhase::Capture)
                .is_some_and(|keys| {
                    keys.emit_by_name::<bool>(
                        "key-pressed",
                        &[&gtk::gdk::Key::Tab.into_glib(), &0u32, &gtk::gdk::ModifierType::empty()],
                    )
                })
        })
    };
    assert!(tab(&window));
    let focus = gtk::prelude::RootExt::focus(&window).expect("a widget has the keyboard");
    assert!(focus.is_ancestor(&shell.overlay.last_child().unwrap()), "in the dialog");
    dialog.close();
    window.destroy();
}

/// A group's title and description and a row's title and subtitle change in place, and follow the language when
/// given as English.
fn groups_and_rows_are_relabelled_in_place() {
    let _language = i18n::lock_language();
    i18n::set_locale_registry(None);
    i18n::register_all(&[(
        "da",
        r#"{"omui-gtk Accounts": "Konti", "omui-gtk Signed in": "Logget ind", "omui-gtk Sync": "Synk",
            "omui-gtk Every hour": "Hver time", "omui-gtk Advanced": "Avanceret", "omui-gtk More": "Mere"}"#,
    )]);
    let group = crate::Group::new("", "");
    assert!(
        !group.widget.first_child().unwrap().is_visible(),
        "no header without a title"
    );
    group.set_title("Accounts");
    let row = crate::Row::new("", "");
    let expander = crate::ExpanderRow::new("Advanced", "");
    group.add(&row.widget);
    group.add(&expander.widget);
    let mut shown = Vec::new();
    labels(group.widget.upcast_ref(), &mut shown);
    assert_eq!(shown[0], "Accounts");
    assert!(
        group.widget.first_child().unwrap().is_visible(),
        "a title makes the header"
    );
    group.set_title_translated("omui-gtk Accounts");
    group.set_description_translated("omui-gtk Signed in");
    row.set_title_translated("omui-gtk Sync");
    row.set_subtitle_translated("omui-gtk Every hour");
    expander.set_title_translated("omui-gtk Advanced");
    expander.set_subtitle("omui-gtk More");
    expander.set_subtitle_translated("omui-gtk More");
    let shown = || {
        let mut shown = Vec::new();
        labels(group.widget.upcast_ref(), &mut shown);
        shown.retain(|label| !label.is_empty());
        shown
    };
    assert_eq!(
        shown(),
        [
            "omui-gtk Accounts",
            "omui-gtk Signed in",
            "omui-gtk Sync",
            "omui-gtk Every hour",
            "omui-gtk Advanced",
            "omui-gtk More"
        ]
    );
    i18n::set_language("da");
    assert_eq!(
        shown(),
        ["Konti", "Logget ind", "Synk", "Hver time", "Avanceret", "Mere"]
    );
    i18n::set_language("en");
    assert_eq!(shown()[0], "omui-gtk Accounts");
}

/// Each translated setter binds one thing of its widget: set twice, the second replaces the first, so a switch of
/// language writes the newest text and the listeners do not pile up; and the plain setter after it ends it.
fn a_translated_setter_replaces_its_earlier_binding() {
    let _language = i18n::lock_language();
    i18n::set_locale_registry(None);
    i18n::register_all(&[(
        "da",
        r#"{"omui-gtk Old": "Gammel", "omui-gtk New": "Ny", "omui-gtk Older": "Ældre", "omui-gtk Newer": "Nyere"}"#,
    )]);
    let window = gtk::Window::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let shell = Shell::new(&window, &content);
    window.present();
    let group = crate::Group::new("", "");
    let row = crate::Row::new("", "");
    let expander = crate::ExpanderRow::new("", "");
    group.add(&row.widget);
    group.add(&expander.widget);
    let label = gtk::Label::new(None);
    let button = gtk::Button::new();
    let dialog = Dialog::new(Some(""), Some(""));
    dialog.add_response("go", "");
    dialog.present(&content);

    let set = |old: &str, new: &str| {
        group.set_title_translated(old);
        group.set_title_translated(new);
        group.set_description_translated(old);
        group.set_description_translated(new);
        row.set_title_translated(old);
        row.set_title_translated(new);
        row.set_subtitle_translated(old);
        row.set_subtitle_translated(new);
        expander.set_title_translated(old);
        expander.set_title_translated(new);
        expander.set_subtitle_translated(old);
        expander.set_subtitle_translated(new);
        i18n::bind_label(&label, old);
        i18n::bind_label(&label, new);
        i18n::bind_tooltip(&button, old);
        i18n::bind_tooltip(&button, new);
        dialog.set_heading_translated(old);
        dialog.set_heading_translated(new);
        dialog.set_body_translated(old);
        dialog.set_body_translated(new);
        dialog.set_response_translated("go", old);
        dialog.set_response_translated("go", new);
    };
    let shown = || {
        let mut shown = Vec::new();
        labels(group.widget.upcast_ref(), &mut shown);
        labels(shell.overlay.last_child().unwrap().upcast_ref(), &mut shown);
        shown.retain(|label| !label.is_empty());
        shown.push(label.label().into());
        shown.push(button.tooltip_text().unwrap_or_default().into());
        shown
    };
    set("omui-gtk Old", "omui-gtk New");
    let listeners = i18n::listener_count();
    set("omui-gtk Older", "omui-gtk Newer");
    assert_eq!(i18n::listener_count(), listeners, "set again, nothing more listens");
    i18n::set_language("da");
    assert_eq!(shown(), ["Nyere"; 11], "the newest text, in the new language");
    i18n::set_language("en");
    assert_eq!(shown(), ["omui-gtk Newer"; 11]);

    // The plain setters end the bindings: what they set stays through a switch.
    group.set_title("Plain");
    group.set_description("Plain");
    row.set_title("Plain");
    row.set_subtitle("Plain");
    expander.set_title("Plain");
    expander.set_subtitle("Plain");
    label.set_label("Plain");
    button.set_tooltip_text(Some("Plain"));
    dialog.set_heading("Plain");
    dialog.set_body("Plain");
    dialog.set_response_label("go", "Plain");
    assert_eq!(i18n::listener_count(), listeners - 11, "each binding ended");
    i18n::set_language("da");
    assert_eq!(shown(), ["Plain"; 11]);
    i18n::set_language("en");

    // A binding ends with its widget too.
    i18n::bind_label(&label, "omui-gtk New");
    let listening = i18n::listener_count();
    drop(label);
    assert_eq!(i18n::listener_count(), listening - 1, "gone with the label");
    dialog.close();
    window.destroy();
}

/// `confirmf` fills its words with the name it is given in each language, and keeps `confirm`'s safety: Escape
/// cancels, and Enter cancels a destructive one.
fn a_confirmation_with_a_name_follows_a_switch_of_language() {
    let _language = i18n::lock_language();
    i18n::set_locale_registry(None);
    i18n::register_all(&[(
        "da",
        r#"{"omui-gtk Delete {}?": "Slet {}?", "omui-gtk {} will be gone.": "{} forsvinder.",
            "omui-gtk _Delete {}": "_Slet {}", "_Cancel": "_Annuller"}"#,
    )]);
    let window = gtk::Window::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let shell = Shell::new(&window, &content);
    window.present();
    let shown = || {
        let mut shown = Vec::new();
        labels(shell.overlay.last_child().unwrap().upcast_ref(), &mut shown);
        shown.retain(|label| !label.is_empty());
        shown
    };
    let confirmed = Rc::new(std::cell::Cell::new(false));
    let name = String::from("Ada");
    crate::confirmf(
        &content,
        "omui-gtk Delete {}?",
        "omui-gtk {} will be gone.",
        "omui-gtk _Delete {}",
        &[&name],
        crate::Appearance::Destructive,
        {
            let confirmed = confirmed.clone();
            move || confirmed.set(true)
        },
    );
    assert_eq!(
        shown(),
        [
            "omui-gtk Delete Ada?",
            "omui-gtk Ada will be gone.",
            "_Cancel",
            "omui-gtk _Delete Ada"
        ]
    );
    i18n::set_language("da");
    assert_eq!(shown(), ["Slet Ada?", "Ada forsvinder.", "_Annuller", "_Slet Ada"]);
    i18n::set_language("en");
    assert_eq!(shown()[0], "omui-gtk Delete Ada?");
    escape_on_top(&shell);
    assert!(!shell.dialog_open() && !confirmed.get(), "Escape cancelled");
    window.destroy();
}

/// `application_opening` is `application` with `HANDLES_OPEN`: what it is asked to open reaches the app's handler,
/// and an activation still opens the window.
fn an_application_that_opens_files_is_handed_them() {
    let _language = i18n::lock_language();
    let opened = Rc::new(RefCell::new(Vec::<String>::new()));
    let activated = Rc::new(std::cell::Cell::new(0));
    let app = crate::application_opening(
        "org.omui.OpeningTest",
        "omui-opening-test",
        "Opening test",
        "",
        {
            let activated = activated.clone();
            move |_| activated.set(activated.get() + 1)
        },
        {
            let opened = opened.clone();
            move |_, files| {
                opened
                    .borrow_mut()
                    .extend(files.iter().map(|file| file.uri().to_string()))
            }
        },
    );
    assert!(app.flags().contains(gtk::gio::ApplicationFlags::HANDLES_OPEN));
    assert_eq!(glib::prgname().as_deref(), Some("omui-opening-test"));
    app.register(None::<&gtk::gio::Cancellable>)
        .expect("registers without a session bus, as the only instance");
    app.open(&[gtk::gio::File::for_uri("mailto:someone@example.com")], "");
    assert_eq!(*opened.borrow(), ["mailto:someone@example.com"]);
    assert_eq!(activated.get(), 0, "an open is no activation");
    app.activate();
    assert_eq!(activated.get(), 1);
}

/// After the theme is installed, with a `hyprctl` of the test's own that says the windows are rounded by ten.
fn the_stylesheet_parses_and_takes_its_corners_and_colours_from_the_theme() {
    // Every rule of the stylesheet is one GTK understands: a rule it does not is dropped with a warning nobody reads.
    let errors = Rc::new(RefCell::new(Vec::new()));
    let provider = gtk::CssProvider::new();
    provider.connect_parsing_error({
        let errors = errors.clone();
        move |_, section, error| errors.borrow_mut().push(format!("{section}: {error}"))
    });
    provider.load_from_string(include_str!("omui.css"));
    assert_eq!(*errors.borrow(), Vec::<String>::new());

    let display = gtk::gdk::Display::default().unwrap();
    let theme = crate::Theme::get().expect("installed by the test before this one");
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        ".om-test-radius { min-width: var(--om-radius); min-height: var(--om-radius-small); }
         .om-test-inner { min-width: var(--om-radius-inner); min-height: var(--om-radius-tiny); }
         .om-test-on-accent { color: @om_accent_fg; }
         .om-test-blue { color: @om_color_blue; }
         .om-test-selection { color: @om_color_selection; }",
    );
    gtk::style_context_add_provider_for_display(&display, &provider, crate::APP_PRIORITY);
    let boxed = |class: &str| gtk::Box::builder().css_classes([class]).build();
    let size = |widget: &gtk::Box| {
        (
            widget.measure(gtk::Orientation::Horizontal, -1).0,
            widget.measure(gtk::Orientation::Vertical, -1).0,
        )
    };
    // One inside a window, as an app's are, and one that is its own root.
    let (inside, alone, inner) = (boxed("om-test-radius"), boxed("om-test-radius"), boxed("om-test-inner"));
    let window = gtk::Window::builder().child(&inside).build();
    wait_until("hyprctl has answered", || theme.radius() == 10);
    let _ = std::fs::remove_dir_all(std::env::temp_dir().join(format!("omui-gtk-hyprctl-{}", std::process::id())));
    assert_eq!(
        (size(&inside), size(&alone)),
        ((10, 4), (10, 4)),
        "a small corner stops at four"
    );
    assert_eq!(size(&inner), (9, 2));
    let hex = |class: &str| {
        let color = boxed(class).color();
        let channel = |value: f32| (value * 255.0).round() as u8;
        format!(
            "#{:02x}{:02x}{:02x}",
            channel(color.red()),
            channel(color.green()),
            channel(color.blue())
        )
    };
    // With no Omarchy in the test's home the palette is the fallback, whose accent is Omarchy's yellow.
    assert_eq!(theme.tokens().accent, "#ffd60a");
    assert_eq!(hex("om-test-on-accent"), "#000000");
    assert_eq!(hex("om-test-blue"), "#7aa2f7");
    assert_eq!(hex("om-test-selection"), "#3a3a40");
    assert_eq!(theme.color("selection").as_deref(), Some("#3a3a40"));
    assert_eq!(theme.color("magenta").as_deref(), Some("#bb9af7"));
    assert_eq!(
        (theme.color("mauve"), theme.accent_foreground()),
        (None, "#000000".to_owned())
    );
    window.destroy();
}

fn an_app_provider_at_app_priority_is_above_the_reset() {
    let display = gtk::gdk::Display::default().unwrap();
    // A hyprctl of the test's own, in place of the desktop's: the windows are rounded by ten.
    let dir = std::env::temp_dir().join(format!("omui-gtk-hyprctl-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let hyprctl = dir.join("hyprctl");
    std::fs::write(
        &hyprctl,
        "#!/bin/sh\necho '{\"option\": \"decoration:rounding\", \"int\": 10, \"set\": true}'\n",
    )
    .unwrap();
    std::fs::set_permissions(&hyprctl, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    crate::Theme::install_following(&display, ".om-test-app-css { min-width: 50px; }", Some(hyprctl), None);
    let provider = |css: &str, priority: u32| {
        let provider = gtk::CssProvider::new();
        provider.load_from_string(css);
        gtk::style_context_add_provider_for_display(&display, &provider, priority);
    };
    provider(".om-test-app-css { min-width: 120px; }", crate::APP_PRIORITY);
    provider(
        ".om-test-application { min-width: 120px; }",
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
    );
    let width = |class: &str| {
        let widget = gtk::Box::builder().css_classes([class]).build();
        widget.measure(gtk::Orientation::Horizontal, -1).0
    };
    assert_eq!(
        width("om-test-app-css"),
        120,
        "added later at the same priority, it wins over the app's stylesheet"
    );
    assert_eq!(
        width("om-test-application"),
        0,
        "the reset unsets what GTK's application priority sets"
    );
}

/// After the theme is installed: the line is the stylesheet's.
#[cfg(feature = "dev")]
fn a_dividers_line_is_two_whole_pixels() {
    let split = Split::new(&gtk::Label::new(None), &gtk::Label::new(None), (240, 360), Some(600));
    split.set_resizable(true);
    let divider = split.widget.last_child().unwrap();
    // As while it is dragged: the line shows without the pointer or the keyboard on it.
    divider.add_css_class("dragging");
    let window = gtk::Window::builder()
        .child(&split.widget)
        .default_width(900)
        .default_height(200)
        .build();
    window.present();
    let main = glib::MainContext::default();
    let start = std::time::Instant::now();
    while !(divider.is_mapped() && divider.width() > 0) {
        assert!(start.elapsed().as_secs() < 5, "the window is laid out");
        main.iteration(false);
    }
    let path = std::env::temp_dir().join(format!("omui-divider-{}.png", std::process::id()));
    crate::dev::shot(&divider, &path).unwrap();
    let texture = gtk::gdk::Texture::from_filename(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    let width = texture.width() as usize;
    let mut pixels = vec![0u8; width * 4 * texture.height() as usize];
    texture.download(&mut pixels, width * 4);
    let row: Vec<&[u8]> = pixels[..width * 4].chunks(4).collect();
    let line: Vec<usize> = (0..width).filter(|&x| row[x] != row[0]).collect();
    assert_eq!(
        (width, line.as_slice()),
        (7, &[2, 3][..]),
        "two columns beside each other, none between the line's colour and the window's"
    );
    assert_eq!(row[2], row[3], "and both the line's whole colour");
    window.destroy();
}
