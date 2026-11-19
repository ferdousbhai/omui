// SPDX-License-Identifier: MIT
//! What needs GTK running: sizes, toasts, dialogs, a split's divider, banners and the app's stylesheet priority. GTK is used from one thread only, so this is
//! one test. It needs a broadway display, which shows nothing: GTK on the desktop the tests are run from would read
//! that desktop's settings. So `cargo test` lists it as ignored, and `dev/gtk-tests.sh` runs it on a display of its
//! own.

use std::{cell::RefCell, rc::Rc};

use gtk::{glib, prelude::*};

use crate::{Banner, Dialog, Shell, Split, Toast};

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
    a_resizable_split_keeps_its_sidebar_within_its_limits();
    a_banner_has_an_icon_and_a_button_of_either_kind();
    #[cfg(feature = "qa")]
    qa_selects_a_list_views_item_through_its_model();
    // Last: the theme styles every widget made after it.
    an_app_provider_at_app_priority_is_above_the_reset();
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

fn an_app_provider_at_app_priority_is_above_the_reset() {
    let display = gtk::gdk::Display::default().unwrap();
    crate::Theme::install(&display, ".om-test-app-css { min-width: 50px; }");
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
