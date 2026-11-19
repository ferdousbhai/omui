// SPDX-License-Identifier: MIT
//! What needs GTK running: sizes, toasts, and what a dialog holds back. GTK is used from one thread only, so this is
//! one test. It needs a broadway display, which shows nothing: GTK on the desktop the tests are run from would read
//! that desktop's settings. So `cargo test` lists it as ignored, and `dev/gtk-tests.sh` runs it on a display of its
//! own.

use std::{cell::RefCell, rc::Rc};

use gtk::{glib, prelude::*};

use crate::{Dialog, Shell, Split, Toast};

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
