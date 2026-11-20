// SPDX-License-Identifier: MIT
//! Scripted QA, behind feature `qa`: with `OMUI_QA=1` an app built with it reads commands from stdin, one JSON object
//! a line, and answers each on the file descriptor `OMUI_QA_FD` names, in order, so that what the app prints itself
//! stays out of the answers. Widgets are found by accessible role and name, as a screen reader would find them. Any
//! GLib warning or critical fails the run. `dev/qa.sh` runs an app this way on a broadway display of its own;
//! README.md has the commands.
//!
//! [`crate::application`] starts it; an app that builds its own `gtk::Application` calls [`attach`].

use std::{
    cell::{Cell, RefCell},
    fs::File,
    io::{BufRead, Read, Write},
    net::TcpStream,
    os::fd::{BorrowedFd, FromRawFd, OwnedFd},
    rc::Rc,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use gtk::{
    gdk, gio,
    glib::{self, translate::*},
    prelude::*,
};
use serde_json::{Map, Value, json};

/// The warnings and criticals GLib has logged and no answer has reported yet.
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// What GTK says when it finds no accessibility bus, as on a private display with no session bus: nothing is wrong
/// with the app.
const NO_A11Y_BUS: &[&str] = &[
    "Unable to acquire session bus",
    "Unable to acquire the address of the accessibility bus",
];

/// What GDK's broadway backend says (domain Gdk), each once a run (`g_warning_once`), when a page answers its frames,
/// as the page that presses keys here does: each answer marks the surface's pending frame submitted at 1/30 s
/// (`_gdk_broadway_roundtrip_notify`, GTK 4.24). A frame the clock had already finished with another interval draws
/// the first. A popover shares its window's frame clock and is answered for the same frame as the window, whose
/// answer has marked it submitted already: an open menu draws the second. A browser showing the display gets them
/// as well; the app is not at fault. Broadway marks no frame presented, so `gdk_frame_timings_presented()`'s like
/// warnings cannot come from here, and are not let pass.
const PAGE_FRAMES: &[&str] = &[
    "Duplicate call to gdk_frame_timings_submitted() with different values.",
    "gdk_frame_timings_submitted() called on submitted frame.",
];

/// What GTK says (domain Gtk) of a turning spinner on the same display, now and then and more under load: its SVG
/// animation's own time was some microseconds ahead of the frame time it was handed (`gtk_svg_advance`, GTK 4.24).
/// Seen only here, where the page's answers time the frames; the spinner turns on and the app is not at fault, so
/// it is let pass, like the frame warnings, while the page is connected. Why GTK's times disagree is not established.
const PAGE_ANIMATION: &str = "gtk_svg_advance: time went backward";

/// How long a command waits for its widget, or `wait` for its condition, unless it gives `timeout_ms`.
const TIMEOUT: Duration = Duration::from_secs(5);

/// Starts scripted QA for `app` when this run asks for it (`OMUI_QA=1`), as [`crate::application`] does for its own:
/// a display other than broadway ends the run as the application starts, and commands are read once the first
/// `activate` or `open` has run the app's own handlers, from the window they opened. Call it before `app.run()`,
/// once, for an app that builds its own `gtk::Application`; with no `OMUI_QA=1` it does nothing.
pub fn attach(app: &gtk::Application) {
    if !wanted() {
        return;
    }
    app.connect_startup(|_| {
        require_hidden_display();
        draw_whole();
    });
    let started = Rc::new(Cell::new(false));
    for signal in ["activate", "open"] {
        let started = started.clone();
        // After the app's handlers, which open the window.
        app.connect_local(signal, true, move |values| {
            let app = values[0]
                .get::<gtk::Application>()
                .expect("the signal's own application");
            if !started.replace(true) {
                start(&app);
            }
            None
        });
    }
}

/// Whether this run is QA mode, read once. In it, GLib's warnings and criticals are recorded from here on.
fn wanted() -> bool {
    static WANTED: OnceLock<bool> = OnceLock::new();
    *WANTED.get_or_init(|| {
        let wanted = std::env::var_os("OMUI_QA").is_some_and(|value| value == "1");
        if wanted {
            begin();
        }
        wanted
    })
}

fn begin() {
    // Before anything else of the app's opens a file, which could be given the number.
    let answers = answers_fd().unwrap_or_else(|e| {
        eprintln!("omui qa: {e}");
        std::process::exit(2);
    });
    ANSWERS.get_or_init(|| Mutex::new(File::from(answers)));
    glib::log_set_writer_func(record);
    // A panic in a command would leave the main loop running with no answer, until the run timed out.
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        report(info);
        std::process::exit(101);
    }));
    // Before the app has a window: broadway then has the page from the start, and never answers a frame itself.
    if let Ok(address) = std::env::var("OMUI_QA_INPUT") {
        INPUT.get_or_init(|| connect(&address));
    }
}

/// Where answers go: the open file descriptor `OMUI_QA_FD` names (dev/qa.sh opens 3), not stdout, which is the app's.
static ANSWERS: OnceLock<Mutex<File>> = OnceLock::new();

/// `OMUI_QA_FD`'s descriptor, moved to one that a program the app starts does not inherit.
fn answers_fd() -> Result<OwnedFd, String> {
    let named = std::env::var("OMUI_QA_FD")
        .map_err(|_| "OMUI_QA=1 needs OMUI_QA_FD, the descriptor answers go to (dev/qa.sh sets it)")?;
    let fd: i32 = named
        .parse()
        .ok()
        .filter(|fd| *fd > 2)
        .ok_or(format!("OMUI_QA_FD={named} is not a descriptor above stderr's"))?;
    // SAFETY: the descriptor is only duplicated here; one that is not open fails with EBADF.
    let moved = unsafe { BorrowedFd::borrow_raw(fd) }
        .try_clone_to_owned()
        .map_err(|e| format!("OMUI_QA_FD={fd} is not open: {e}"))?;
    // SAFETY: it is open, and the app has not been handed it: it was the runner's to give.
    drop(unsafe { OwnedFd::from_raw_fd(fd) });
    Ok(moved)
}

fn record(level: glib::LogLevel, fields: &[glib::LogField<'_>]) -> glib::LogWriterOutput {
    let field = |key| {
        fields
            .iter()
            .find(|field| field.key() == key)
            .and_then(|field| field.value_str())
            .unwrap_or_default()
    };
    let kind = match level {
        glib::LogLevel::Error => Some("ERROR"),
        glib::LogLevel::Critical => Some("CRITICAL"),
        glib::LogLevel::Warning => Some("WARNING"),
        _ => None,
    };
    let message = field("MESSAGE");
    let known = NO_A11Y_BUS.iter().any(|known| message.starts_with(known))
        || (matches!(INPUT.get(), Some(Ok(_)))
            && match field("GLIB_DOMAIN") {
                "Gdk" => PAGE_FRAMES.contains(&message),
                "Gtk" => message.starts_with(PAGE_ANIMATION),
                _ => false,
            });
    if let Some(kind) = kind.filter(|_| !known) {
        let line = format!("{}-{kind}: {message}", field("GLIB_DOMAIN"));
        LOGGED.lock().unwrap_or_else(|e| e.into_inner()).push(line);
    }
    glib::log_writer_default(level, fields)
}

/// Ends the run unless the app is on a broadway display, as dev/qa.sh gives it, which no one sees: a script run on
/// the desktop would click about in a window there. Called as the application starts, before it has a window.
fn require_hidden_display() {
    let display = gdk::Display::default().map(|display| display.type_().name());
    if display != Some("GdkBroadwayDisplay") {
        eprintln!(
            "omui qa: OMUI_QA=1 runs on a broadway display alone (dev/qa.sh), not on {}",
            display.unwrap_or("no display")
        );
        std::process::exit(2);
    }
}

/// Has broadway draw each window as one picture, so that the page that presses keys here cannot make it abort.
///
/// GTK 4.24's broadway renderer sends a node it sent the frame before by reference to it (`BROADWAY_NODE_REUSE`, by
/// the node's pointer), and does so for each place the node is drawn. An SVG paintable keeps the node it drew last
/// and draws that again, and every image of one icon, from the icon theme or a CSS `-gtk-icon-source`, shares one
/// paintable: the same icon twice in widgets drawn anew (a list rebuilt, a window resized) refers to one old node
/// twice, and gtk4-broadwayd, building the frame for its page, asserts `!reused_node->consumed` and aborts.
///
/// A color matrix node is one that broadway's renderer rasterises whole, with all under it, as a texture: with one
/// around each window, no node in it is sent alone, by reference or otherwise. `brightness(1)` changes no colour; the
/// pixels are cairo's, as broadway's were for an icon, and as `shot`'s are.
fn draw_whole() {
    let Some(display) = gdk::Display::default() else { return };
    let provider = gtk::CssProvider::new();
    provider.load_from_string("window, popover, tooltip { filter: brightness(1); }");
    // Above every layer omui and the app add, the reset among them, which would unset it.
    gtk::style_context_add_provider_for_display(&display, &provider, u32::MAX);
}

/// Starts reading commands, for the window the app has just opened.
fn start(app: &gtk::Application) {
    let Some(window) = app.active_window() else {
        eprintln!("omui qa: the application opened no window");
        std::process::exit(1);
    };
    let qa = Rc::new(Qa {
        app: app.clone(),
        window,
        keys_seen: Rc::new(Cell::new(0)),
        watched: RefCell::new(Vec::new()),
    });
    glib::spawn_future_local(async move {
        let mut failed = false;
        loop {
            let Some(line) = read_line().await else {
                // A script cut short, or a pipe closed early, has not passed.
                answer("quit", Err("the script ended without `quit`".to_owned()));
                failed = true;
                break;
            };
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let command = serde_json::from_str::<Value>(line).map_err(|e| format!("not a JSON line: {e}"));
            let verb = command
                .as_ref()
                .ok()
                .and_then(|c| c["do"].as_str())
                .unwrap_or_default()
                .to_owned();
            if verb == "quit" {
                break;
            }
            // Each command finds the window as it is drawn, laid out after the last, as a person would.
            let result = match (painted(&qa.window, || ()).await, command) {
                (Err(e), _) | (_, Err(e)) => Err(e),
                (_, Ok(Value::Object(command))) => qa.run(&verb, &command).await,
                (_, Ok(_)) => Err("a command is a JSON object".to_owned()),
            };
            failed = !answer(&verb, result);
            if failed {
                break;
            }
        }
        qa.finish(failed).await;
    });
}

/// Waits until `window` has been drawn once more, and runs `then` as it has been: until then GTK has no picture of
/// what has changed in it. A moment later its picture may be gone again, as whatever asks for the next frame (a
/// relayout, an animation) drops it: what reads the picture (`shot`) does so here.
async fn painted<T: 'static>(window: &gtk::Window, then: impl FnOnce() -> T + 'static) -> Result<T, String> {
    let start = Instant::now();
    let stopped = || (start.elapsed() > TIMEOUT).then(|| "the window is not being drawn".to_owned());
    let clock = loop {
        match window.frame_clock() {
            Some(clock) => break clock,
            None => stopped().map_or(Ok(()), Err)?,
        }
        glib::timeout_future(Duration::from_millis(10)).await;
    };
    let done = Rc::new(RefCell::new(None));
    let then = RefCell::new(Some(then));
    let handler = clock.connect_after_paint({
        let done = done.clone();
        move |_| {
            if let Some(then) = then.take() {
                done.replace(Some(then()));
            }
        }
    });
    window.queue_draw();
    let result = loop {
        if let Some(value) = done.take() {
            break Ok(value);
        }
        if let Some(e) = stopped() {
            break Err(e);
        }
        glib::timeout_future(Duration::from_millis(10)).await;
    };
    clock.disconnect(handler);
    result
}

/// The next line of stdin, or `None` at its end. Read on a worker thread, so the window goes on while it waits.
async fn read_line() -> Option<String> {
    gio::spawn_blocking(|| {
        let mut line = String::new();
        let read = std::io::stdin().lock().read_line(&mut line).ok()?;
        (read > 0).then_some(line)
    })
    .await
    .ok()
    .flatten()
}

/// Writes the answer to `verb`, with whatever GLib logged while it ran; false when either failed.
fn answer(verb: &str, result: Answer) -> bool {
    let logged = std::mem::take(&mut *LOGGED.lock().unwrap_or_else(|e| e.into_inner()));
    let mut line = Map::new();
    line.insert("do".into(), verb.into());
    let ok = result.is_ok() && logged.is_empty();
    line.insert("ok".into(), ok.into());
    match result {
        Ok(fields) => line.extend(fields),
        Err(error) => drop(line.insert("error".into(), error.into())),
    }
    if !logged.is_empty() {
        line.insert("logged".into(), logged.into());
    }
    // Set before any answer; a reader that has gone cannot be told anything: the exit status still says how it went.
    if let Some(out) = ANSWERS.get() {
        let mut out = out.lock().unwrap_or_else(|e| e.into_inner());
        let _ = writeln!(out, "{}", Value::Object(line)).and_then(|()| out.flush());
    }
    ok
}

struct Qa {
    app: gtk::Application,
    /// The window the app opened: what `state`, `resize` and `shot` mean.
    window: gtk::Window,
    /// Key presses and releases the app's windows and popovers have had, counted as they arrive.
    keys_seen: Rc<Cell<u64>>,
    /// The windows and popovers that count them.
    watched: RefCell<Vec<(glib::WeakRef<gtk::Widget>, gtk::EventControllerLegacy)>>,
}

type Answer = Result<Map<String, Value>, String>;

impl Qa {
    async fn run(&self, verb: &str, command: &Map<String, Value>) -> Answer {
        let timeout = command
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .map_or(TIMEOUT, Duration::from_millis);
        let target = Target::parse(command)?;
        match verb {
            "click" => {
                let widget = self.wait_for(target.as_ref(), timeout).await?;
                clickable(&widget)?;
                if let Some(chosen) = choose(&widget) {
                    chosen?;
                    return Ok(Map::new());
                }
                // A menu button or a drop-down is the button inside it, under the pointer.
                let mut pressed = widget.clone();
                let mut child = widget.first_child();
                while let Some(inner) = child {
                    if nested_button(&inner) {
                        pressed = inner;
                        break;
                    }
                    child = inner.next_sibling();
                }
                // A click on a button gives it the keyboard, unless it is one that does not take it so.
                if pressed.is_focusable() && pressed.gets_focus_on_click() {
                    pressed.grab_focus();
                }
                // A button clicks at once, as under the pointer; `activate` would hold it pressed for a moment first,
                // as Enter does, and a menu would open only then.
                if let Some(button) = pressed.downcast_ref::<gtk::Button>() {
                    button.emit_clicked();
                } else if !widget.activate() {
                    return Err(format!("{} cannot be activated", describe(&widget)));
                }
                Ok(Map::new())
            }
            "focus" => {
                let widget = self.wait_for(target.as_ref(), timeout).await?;
                clickable(&widget)?;
                if !widget.grab_focus() {
                    return Err(format!("{} cannot take the focus", describe(&widget)));
                }
                Ok(Map::new())
            }
            "set" => {
                let widget = self.wait_for(target.as_ref(), timeout).await?;
                clickable(&widget)?;
                let on = command
                    .get("checked")
                    .and_then(Value::as_bool)
                    .ok_or("`set` needs `checked`: true or false")?;
                if let Some(switch) = widget.downcast_ref::<gtk::Switch>() {
                    switch.set_active(on);
                } else if let Some(check) = widget.downcast_ref::<gtk::CheckButton>() {
                    check.set_active(on);
                } else if let Some(toggle) = widget.downcast_ref::<gtk::ToggleButton>() {
                    toggle.set_active(on);
                } else {
                    return Err(format!("{} is not a switch, check box or toggle", describe(&widget)));
                }
                Ok(Map::new())
            }
            "select" => {
                let widget = self.wait_for(target.as_ref(), timeout).await?;
                clickable(&widget)?;
                select(&widget)?;
                Ok(Map::new())
            }
            "key" => {
                let keys = command
                    .get("keys")
                    .and_then(Value::as_str)
                    .ok_or("`key` needs `keys`, as \"<Control>u\"")?;
                let (key, modifiers) = keyboard_key(keys)?;
                self.press(key, modifiers).await?;
                Ok(Map::new())
            }
            "type" => {
                if target.is_some() {
                    let widget = self.wait_for(target.as_ref(), timeout).await?;
                    clickable(&widget)?;
                    widget.grab_focus();
                }
                let text = command
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or("`type` needs `text`")?;
                for c in text.chars() {
                    // SAFETY: GDK answers with a key symbol, one for any character.
                    let key = unsafe { from_glib(gdk::unicode_to_keyval(c.into())) };
                    self.press(key, gdk::ModifierType::empty()).await?;
                }
                Ok(Map::new())
            }
            "action" => {
                let name = command
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or("`action` needs `name`, as \"win.save\"")?;
                let parameter = match command.get("parameter").and_then(Value::as_str) {
                    Some(text) => Some(glib::Variant::parse(None, text).map_err(|e| e.to_string())?),
                    None => None,
                };
                self.window
                    .activate_action(name, parameter.as_ref())
                    .map_err(|e| e.to_string())?;
                Ok(Map::new())
            }
            "query" => {
                let target = target.ok_or("`query` needs a role or a name")?;
                let widget = target
                    .find(&widgets(&self.roots()))?
                    .ok_or_else(|| format!("found no {target}"))?;
                Ok(properties(&widget))
            }
            "state" => Ok(self.state()),
            "wait" => self.wait(target.as_ref(), command, timeout).await,
            "resize" => {
                let size = |key: &str| {
                    command
                        .get(key)
                        .and_then(Value::as_i64)
                        .ok_or(format!("`resize` needs `{key}`"))
                };
                self.window
                    .set_default_size(size("width")? as i32, size("height")? as i32);
                Ok(Map::new())
            }
            "shot" => {
                let path = command
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or("`shot` needs `path`")?;
                let (window, path) = (self.window.clone(), std::path::PathBuf::from(path));
                painted(&self.window, move || crate::dev::shot(&window, &path)).await??;
                Ok(Map::new())
            }
            "list" => {
                let target = target.unwrap_or_default();
                let mut found = Vec::new();
                for root in self.roots() {
                    walk(root.upcast_ref(), &mut |widget| {
                        if widget.is_mapped() && target.matches(widget) && !names(widget).is_empty() {
                            found.push(json!({"role": role(widget), "name": name(widget)}));
                        }
                    });
                }
                Ok(Map::from_iter([("widgets".to_owned(), found.into())]))
            }
            "" => Err("a command names what to do in `do`".to_owned()),
            _ => Err(format!("no such command: {verb}")),
        }
    }

    /// Every window, those on top first: the one with the keyboard, then the newest (`list_toplevels` gives them
    /// newest first). Not only the app's: a dialog over a window that has no `Shell` is a modal window of no
    /// application.
    fn roots(&self) -> Vec<gtk::Window> {
        let mut windows: Vec<_> = gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|window| window.downcast::<gtk::Window>().ok())
            .collect();
        windows.sort_by_key(|window| !window.is_active());
        windows
    }

    /// The window that has the keyboard, else the app's.
    fn keyboard_window(&self) -> gtk::Window {
        self.roots()
            .into_iter()
            .find(|window| window.is_active() && window.is_mapped())
            .unwrap_or_else(|| self.window.clone())
    }

    /// The widget `target` names, once there is one on screen.
    async fn wait_for(&self, target: Option<&Target>, timeout: Duration) -> Result<gtk::Widget, String> {
        let target = target.ok_or("this command needs a role or a name")?;
        let start = Instant::now();
        loop {
            match target.find(&widgets(&self.roots())) {
                Ok(Some(widget)) if widget.is_mapped() => return Ok(widget),
                // More than one may be a page going while the next comes: an error only if it stays so.
                Err(e) if start.elapsed() > timeout => return Err(e),
                Ok(_) if start.elapsed() > timeout => return Err(format!("found no {target} on screen")),
                _ => glib::timeout_future(Duration::from_millis(20)).await,
            }
        }
    }

    /// Waits until each key of `command` other than the target's has the value `query` or `state` would answer.
    async fn wait(&self, target: Option<&Target>, command: &Map<String, Value>, timeout: Duration) -> Answer {
        let expected: Vec<_> = command
            .iter()
            .filter(|(key, _)| !["do", "role", "name", "in", "timeout_ms"].contains(&key.as_str()))
            .collect();
        if target.is_none() && expected.is_empty() {
            return Err("`wait` needs a role or a name, or what to wait for".to_owned());
        }
        let start = Instant::now();
        loop {
            let now = match target {
                // Not there is not on screen, for a wait until something has gone.
                Some(target) => match target.find(&widgets(&self.roots())) {
                    Ok(Some(widget)) => properties(&widget),
                    Ok(None) => Map::from_iter([("visible".to_owned(), false.into())]),
                    // More than one for now, as while one page goes and the next comes: waited out like any
                    // other state that is not the one asked for, and the error if it stays.
                    Err(e) if start.elapsed() > timeout => return Err(e),
                    Err(_) => {
                        glib::timeout_future(Duration::from_millis(20)).await;
                        continue;
                    }
                },
                None => self.state(),
            };
            // With nothing asked, the widget is waited for.
            let holds = if expected.is_empty() {
                now.get("visible") == Some(&Value::Bool(true))
            } else {
                expected
                    .iter()
                    .all(|(key, value)| now.get(*key).is_some_and(|now| meets(now, value)))
            };
            if holds {
                return Ok(now);
            }
            if start.elapsed() > timeout {
                return Err(format!("still {} after {} ms", Value::Object(now), timeout.as_millis()));
            }
            glib::timeout_future(Duration::from_millis(20)).await;
        }
    }

    /// The app's window's size, and of all the windows the dialog on top, the toasts showing, and what has the
    /// keyboard.
    fn state(&self) -> Map<String, Value> {
        let (mut dialog, mut toasts) = (Value::Null, Vec::new());
        for root in self.roots().iter().rev() {
            walk(root.upcast_ref(), &mut |widget| {
                if !widget.is_mapped() {
                    return;
                }
                match widget.accessible_role() {
                    gtk::AccessibleRole::Dialog | gtk::AccessibleRole::AlertDialog => dialog = name(widget).into(),
                    gtk::AccessibleRole::Alert => toasts.push(name(widget)),
                    _ => {}
                }
            });
        }
        // An entry's text is a part of it with no role of its own: the entry is what has the keyboard. A drop-down's
        // button is a part of it too, which GTK names by the drop-down, as a screen reader hears it.
        let mut focus = RootExt::focus(&self.keyboard_window());
        while let Some(part) = focus
            .clone()
            .filter(|w| w.accessible_role() == gtk::AccessibleRole::None || nested_button(w))
        {
            focus = part.parent();
        }
        let focus = match focus {
            Some(widget) => json!({"role": role(&widget), "name": name(&widget)}),
            None => Value::Null,
        };
        Map::from_iter([
            ("size".to_owned(), json!([self.window.width(), self.window.height()])),
            ("dialog".to_owned(), dialog),
            ("toasts".to_owned(), toasts.into()),
            ("focus".to_owned(), focus),
        ])
    }

    /// Presses and releases `key` on the broadway display, as a keyboard would, and waits until the app has had it.
    /// A key that opens a window (a shortcuts window, an editor) has its release go to that window, and one that
    /// closes its window has its release go to none: a release is counted on whatever window it reaches, and waited
    /// for only while a window on screen has the keyboard.
    async fn press(&self, key: gdk::Key, modifiers: gdk::ModifierType) -> Result<(), String> {
        if key == gdk::Key::VoidSymbol {
            return Err("that key has no key symbol".to_owned());
        }
        for native in self.natives() {
            self.watch(&native);
        }
        let input = INPUT
            .get()
            .ok_or("`key` and `type` need a broadway display's input (OMUI_QA_INPUT, as dev/qa.sh sets)")?
            .as_ref()?;
        // As a keyboard sends them: each modifier's own key down first, the key, and all up in reverse.
        let held: Vec<_> = MODIFIER_KEYS
            .iter()
            .filter(|(modifier, _)| modifiers.contains(*modifier))
            .collect();
        let mut events = Vec::new();
        let mut state = gdk::ModifierType::empty();
        for (modifier, key) in &held {
            events.push((KEY_PRESS, *key, state));
            state |= *modifier;
        }
        events.push((KEY_PRESS, key, state));
        events.push((KEY_RELEASE, key, state));
        for (modifier, key) in held.iter().rev() {
            events.push((KEY_RELEASE, *key, state));
            state.remove(*modifier);
        }
        // One at a time, each once the app has had the last, as fingers are slower than the app.
        for (event, key, state) in events {
            if event == KEY_PRESS {
                self.keyboard_ready().await?;
            }
            // A window or menu the last key opened counts too.
            for native in self.natives() {
                if !self.watching(&native) {
                    self.watch(&native);
                }
            }
            let before = self.keys_seen.get();
            let words = [event, 0, 0, key.into_glib() as i32, state.bits() as i32];
            send(&mut input.lock().unwrap_or_else(|e| e.into_inner()), &words)
                .map_err(|e| format!("broadway input: {e}"))?;
            let start = Instant::now();
            while self.keys_seen.get() == before {
                let waited = start.elapsed() > Duration::from_secs(2);
                if event == KEY_PRESS && waited {
                    return Err("the key never reached a window".to_owned());
                }
                // A release lost with the window its press closed is lost as a keyboard's would be: the press is what
                // the app acts on.
                if event == KEY_RELEASE && (waited || !self.has_keyboard()) {
                    break;
                }
                glib::timeout_future(Duration::from_millis(5)).await;
            }
        }
        // What the key started (a dialog closing, a page going back) has run by the next turn of the loop.
        glib::timeout_future(Duration::ZERO).await;
        Ok(())
    }

    /// Whether a window on screen has the keyboard.
    fn has_keyboard(&self) -> bool {
        self.roots()
            .iter()
            .any(|window| window.is_active() && window.is_mapped())
    }

    /// Waits until a window on screen has the keyboard, so that a key goes to it: when a window closes, broadway gives
    /// the keyboard back to the window under it a moment later (a key sent sooner goes to the closed one), and to none
    /// when the window was only hidden. After a while the topmost window on screen is given it, as a click would.
    async fn keyboard_ready(&self) -> Result<(), String> {
        let start = Instant::now();
        let mut asked = false;
        while !self.has_keyboard() {
            if !asked && start.elapsed() > Duration::from_millis(500) {
                if let Some(top) = self.roots().into_iter().find(|window| window.is_mapped()) {
                    top.present();
                }
                asked = true;
            }
            if start.elapsed() > TIMEOUT {
                return Err("no window on screen has the keyboard".to_owned());
            }
            glib::timeout_future(Duration::from_millis(10)).await;
        }
        Ok(())
    }

    /// Every window, and every popover on screen in one: each has a surface of its own, and a key that reaches a
    /// popover (a menu that has the keyboard) goes through its widgets alone, not the window's.
    fn natives(&self) -> Vec<gtk::Widget> {
        let mut natives = Vec::new();
        for window in self.roots() {
            walk(window.upcast_ref(), &mut |widget| {
                if widget.is::<gtk::Window>() || (widget.is::<gtk::Popover>() && widget.is_mapped()) {
                    natives.push(widget.clone());
                }
            });
        }
        natives
    }

    fn watching(&self, native: &gtk::Widget) -> bool {
        self.watched
            .borrow()
            .iter()
            .any(|(w, _)| w.upgrade().as_ref() == Some(native))
    }

    /// Counts the key presses and releases `native` (a window or popover) has, before any of its widgets sees them:
    /// its counter is put first among its controllers, where GTK puts the one added last, as an open dialog adds its
    /// own.
    fn watch(&self, native: &gtk::Widget) {
        let mut watched = self.watched.borrow_mut();
        watched.retain(|(w, _)| w.upgrade().is_some());
        if let Some(index) = watched.iter().position(|(w, _)| w.upgrade().as_ref() == Some(native)) {
            native.remove_controller(&watched.remove(index).1);
        }
        // Every event: a key controller would not see a release whose press another controller took.
        let events = gtk::EventControllerLegacy::new();
        events.set_propagation_phase(gtk::PropagationPhase::Capture);
        let seen = self.keys_seen.clone();
        events.connect_event(move |_, event| {
            if matches!(
                event.event_type(),
                gdk::EventType::KeyPress | gdk::EventType::KeyRelease
            ) {
                seen.set(seen.get() + 1);
            }
            glib::Propagation::Proceed
        });
        native.add_controller(events.clone());
        watched.push((native.downgrade(), events));
    }

    /// Ends the run: the windows are destroyed, so what GLib logs as they go counts too, and the last answer is
    /// `quit`'s, unless a command failed.
    async fn finish(&self, failed: bool) {
        // Without a window the application would end its run, with status 0, before the answer.
        let _held = self.app.hold();
        for window in self.roots() {
            window.destroy();
        }
        glib::timeout_future(Duration::from_millis(50)).await;
        let ok = !failed && answer("quit", Ok(Map::new()));
        std::process::exit(if ok { 0 } else { 1 });
    }
}

/// What a command names: a role, a name, and an ancestor to look in.
#[derive(Default)]
struct Target {
    role: Option<gtk::AccessibleRole>,
    name: Option<String>,
    within: Option<Box<Target>>,
}

impl Target {
    fn parse(command: &Map<String, Value>) -> Result<Option<Target>, String> {
        let role = match command.get("role").and_then(Value::as_str) {
            Some(nick) => Some(role_by_nick(nick).ok_or_else(|| format!("no such role: {nick}"))?),
            None => None,
        };
        let name = command.get("name").and_then(Value::as_str).map(str::to_owned);
        let within = match command.get("in") {
            Some(Value::Object(within)) => Some(Box::new(
                Target::parse(within)?.ok_or("`in` names a role, a name or both")?,
            )),
            Some(_) => return Err("`in` is an object: {\"role\": …, \"name\": …}".to_owned()),
            None => None,
        };
        // An action's name is the action's, not a widget's.
        let name = name.filter(|_| command.get("do").and_then(Value::as_str) != Some("action"));
        Ok((role.is_some() || name.is_some()).then_some(Target { role, name, within }))
    }

    fn matches(&self, widget: &gtk::Widget) -> bool {
        self.role.is_none_or(|role| widget.accessible_role() == role)
            && self.name.as_ref().is_none_or(|name| names(widget).contains(name))
    }

    /// The one widget under `roots` this names: the innermost, of those on screen if any are. More than one is an
    /// error, which `in` resolves.
    fn find(&self, roots: &[gtk::Widget]) -> Result<Option<gtk::Widget>, String> {
        let roots = match &self.within {
            Some(within) => match within.find(roots)? {
                Some(ancestor) => vec![ancestor],
                None => return Ok(None),
            },
            None => roots.to_vec(),
        };
        let mut found = Vec::new();
        for root in &roots {
            walk(root, &mut |widget| {
                if self.matches(widget) {
                    found.push(widget.clone());
                }
            });
        }
        let inner: Vec<_> = found
            .iter()
            .filter(|widget| !found.iter().any(|other| other != *widget && other.is_ancestor(*widget)))
            .cloned()
            .collect();
        let shown: Vec<_> = inner.iter().filter(|widget| widget.is_mapped()).cloned().collect();
        let candidates = if shown.is_empty() { inner } else { shown };
        match candidates.len() {
            0 => Ok(None),
            1 => Ok(candidates.into_iter().next()),
            n => Err(format!("{n} widgets are {self}; narrow it with `in`")),
        }
    }
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let role = self.role.map_or("widget".to_owned(), nick);
        write!(f, "{role}")?;
        if let Some(name) = &self.name {
            write!(f, " named {name:?}")?;
        }
        if let Some(within) = &self.within {
            write!(f, " in {within}")?;
        }
        Ok(())
    }
}

fn widgets(windows: &[gtk::Window]) -> Vec<gtk::Widget> {
    windows.iter().map(|window| window.clone().upcast()).collect()
}

/// `f` on `widget` and every widget inside it, popovers included.
fn walk(widget: &gtk::Widget, f: &mut impl FnMut(&gtk::Widget)) {
    f(widget);
    let mut child = widget.first_child();
    while let Some(widget) = child {
        walk(&widget, f);
        child = widget.next_sibling();
    }
}

/// The roles whose name is their content's, as a screen reader takes a button's or a row's from the text in it.
const NAMED_BY_CONTENT: &[gtk::AccessibleRole] = &[
    gtk::AccessibleRole::Alert,
    gtk::AccessibleRole::AlertDialog,
    gtk::AccessibleRole::Button,
    gtk::AccessibleRole::Checkbox,
    gtk::AccessibleRole::Dialog,
    gtk::AccessibleRole::GridCell,
    gtk::AccessibleRole::Group,
    gtk::AccessibleRole::Link,
    gtk::AccessibleRole::ListItem,
    gtk::AccessibleRole::MenuItem,
    gtk::AccessibleRole::MenuItemCheckbox,
    gtk::AccessibleRole::MenuItemRadio,
    gtk::AccessibleRole::Radio,
    gtk::AccessibleRole::Row,
    gtk::AccessibleRole::Tab,
    gtk::AccessibleRole::ToggleButton,
];

/// Every name `widget` answers to, the one a screen reader would say first: the text of the labels it is labelled by,
/// its accessible label, its own label, title or placeholder, its tooltip, and for a role named by its content the
/// text of each label inside it.
pub(crate) fn names(widget: &gtk::Widget) -> Vec<String> {
    let mut names = labelled_by(widget);
    names.extend(accessible_label(widget));
    if let Some(window) = widget.downcast_ref::<gtk::Window>() {
        names.extend(window.title().map(Into::into));
    }
    names.extend(own_label(widget));
    if widget.find_property("placeholder-text").is_some() {
        names.extend(widget.property::<Option<String>>("placeholder-text"));
    }
    names.extend(widget.tooltip_text().map(Into::into));
    if NAMED_BY_CONTENT.contains(&widget.accessible_role()) {
        shown_labels(widget, &mut names);
    }
    names.retain(|name| !name.is_empty());
    names
}

/// The text of the labels inside `widget`, but those hidden or in a hidden widget, which GTK leaves out of a name
/// (accessible name step 2.A): a drop-down's button holds a hidden "(None)" for when nothing is chosen.
fn shown_labels(widget: &gtk::Widget, names: &mut Vec<String>) {
    let mut child = widget.first_child();
    while let Some(inner) = child {
        if inner.is_visible() {
            if inner.is::<gtk::Label>() {
                names.extend(own_label(&inner));
            }
            shown_labels(&inner, names);
        }
        child = inner.next_sibling();
    }
}

/// Whether `widget` is the button inside a widget that GTK names it by (`gtk_at_context_is_nested_button`): a
/// drop-down's or menu button's toggle, the button of a colour, font or scale button.
fn nested_button(widget: &gtk::Widget) -> bool {
    let Some(parent) = widget.parent() else { return false };
    (widget.is::<gtk::ToggleButton>() && (parent.is::<gtk::DropDown>() || parent.is::<gtk::MenuButton>()))
        || (widget.is::<gtk::Button>()
            && (parent.is::<gtk::ColorDialogButton>()
                || parent.is::<gtk::FontDialogButton>()
                || parent.is::<gtk::ScaleButton>()))
}

fn name(widget: &gtk::Widget) -> String {
    names(widget).into_iter().next().unwrap_or_default()
}

/// The text a label, a button, a check box, a menu button or an expander shows, without its mnemonic's underscore.
fn own_label(widget: &gtk::Widget) -> Option<String> {
    if let Some(label) = widget.downcast_ref::<gtk::Label>() {
        return Some(label.text().into());
    }
    let spec = widget.find_property("label")?;
    if spec.value_type() != String::static_type() {
        return None;
    }
    let text: String = widget.property::<Option<String>>("label")?;
    let underline = widget.find_property("use-underline").is_some() && widget.property::<bool>("use-underline");
    Some(if underline { without_mnemonic(&text) } else { text })
}

/// `_Save` as it is shown, `Save`; `__` is an underscore.
fn without_mnemonic(text: &str) -> String {
    let mut shown = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '_' => shown.extend(chars.next()),
            c => shown.push(c),
        }
    }
    shown
}

/// The label an app gave `widget` for a screen reader (`Property::Label`). GTK has no getter for it; its test API,
/// asked whether the label is a value no label is, answers with the label it found.
fn accessible_label(widget: &gtk::Widget) -> Option<String> {
    const NO_LABEL: &std::ffi::CStr = c"\u{1}";
    // SAFETY: the property is a string, so one `const char *` follows it; the answer is a new string or NULL.
    let found: Option<glib::GString> = unsafe {
        from_glib_full(gtk::ffi::gtk_test_accessible_check_property(
            widget.upcast_ref::<gtk::Accessible>().to_glib_none().0,
            gtk::ffi::GTK_ACCESSIBLE_PROPERTY_LABEL,
            NO_LABEL.as_ptr(),
        ))
    };
    // An unset label is "undefined"; a label the app set to that word is lost with it, which no app does.
    found.filter(|found| found.as_str() != "undefined").map(Into::into)
}

/// The text of the labels an app named `widget` by (`Relation::LabelledBy`), as omui's rows name their control by
/// their title: a screen reader says them before any other name. GTK has no getter for it either; its test API, asked
/// whether the relation is empty, answers with each widget in it as `GtkLabel<0x…>`, found here by that address among
/// the widgets of `widget`'s window.
fn labelled_by(widget: &gtk::Widget) -> Vec<String> {
    // SAFETY: the relation is a list of references, so a NULL-terminated list of them follows it, here an empty one;
    // the answer is a new string or NULL.
    let found: Option<glib::GString> = unsafe {
        from_glib_full(gtk::ffi::gtk_test_accessible_check_relation(
            widget.upcast_ref::<gtk::Accessible>().to_glib_none().0,
            gtk::ffi::GTK_ACCESSIBLE_RELATION_LABELLED_BY,
            std::ptr::null::<gtk::ffi::GtkAccessible>(),
        ))
    };
    let (Some(found), Some(root)) = (found, widget.root()) else {
        return Vec::new();
    };
    let addresses: Vec<&str> = found
        .split('>')
        .filter_map(|reference| reference.split_once('<').map(|(_, address)| address))
        .collect();
    let mut labels = vec![None; addresses.len()];
    walk(root.upcast_ref(), &mut |other| {
        let address = format!("{:p}", other.as_ptr());
        if let Some(at) = addresses.iter().position(|a| *a == address) {
            labels[at] = own_label(other);
        }
    });
    labels.into_iter().flatten().collect()
}

fn role(widget: &gtk::Widget) -> String {
    nick(widget.accessible_role())
}

fn nick(role: gtk::AccessibleRole) -> String {
    glib::EnumValue::from_value(&role.to_value()).map_or_else(String::new, |(_, value)| value.nick().to_owned())
}

fn role_by_nick(nick: &str) -> Option<gtk::AccessibleRole> {
    let class = glib::EnumClass::with_type(gtk::AccessibleRole::static_type())?;
    let value = class.value_by_nick(nick)?;
    // SAFETY: the value is one of the enum's own.
    Some(unsafe { from_glib(value.value()) })
}

fn describe(widget: &gtk::Widget) -> String {
    format!("the {} named {:?}", role(widget), name(widget))
}

/// What `query` answers for `widget`.
fn properties(widget: &gtk::Widget) -> Map<String, Value> {
    let focused = RootExt::focus(&widget.root().expect("found under a window"))
        .is_some_and(|focus| &focus == widget || focus.is_ancestor(widget));
    let text: Option<String> = if let Some(editable) = widget.dynamic_cast_ref::<gtk::Editable>() {
        Some(editable.text().into())
    } else if let Some(view) = widget.downcast_ref::<gtk::TextView>() {
        let buffer = view.buffer();
        Some(buffer.text(&buffer.start_iter(), &buffer.end_iter(), false).into())
    } else if let Some(dropdown) = widget.downcast_ref::<gtk::DropDown>() {
        // What it shows as chosen, where its items are strings.
        dropdown
            .selected_item()
            .and_downcast::<gtk::StringObject>()
            .map(|chosen| chosen.string().into())
    } else {
        own_label(widget)
    };
    let checked = if let Some(switch) = widget.downcast_ref::<gtk::Switch>() {
        Some(switch.is_active())
    } else if let Some(check) = widget.downcast_ref::<gtk::CheckButton>() {
        Some(check.is_active())
    } else {
        widget
            .downcast_ref::<gtk::ToggleButton>()
            .map(|toggle| toggle.is_active())
    };
    let selected = match widget.downcast_ref::<gtk::ListBoxRow>() {
        Some(row) => Some(row.is_selected()),
        None => view_item(widget).then(|| widget.state_flags().contains(gtk::StateFlags::SELECTED)),
    };
    let mut map = Map::from_iter([
        ("role".to_owned(), role(widget).into()),
        ("name".to_owned(), name(widget).into()),
        ("visible".to_owned(), widget.is_mapped().into()),
        ("sensitive".to_owned(), widget.is_sensitive().into()),
        ("focused".to_owned(), focused.into()),
        ("width".to_owned(), widget.width().into()),
        ("height".to_owned(), widget.height().into()),
    ]);
    for (key, value) in [("text", text.map(Value::from)), ("checked", checked.map(Value::from))] {
        map.insert(key.to_owned(), value.unwrap_or(Value::Null));
    }
    if let Some(selected) = selected {
        map.insert("selected".to_owned(), selected.into());
    }
    map
}

/// Selects the list row, or the item of a list or grid view, that `widget` is or is in: a row through its list box,
/// an item through its view's selection model, as a click on it would.
pub(crate) fn select(widget: &gtk::Widget) -> Result<(), String> {
    let mut ancestor = Some(widget.clone());
    while let Some(item) = ancestor {
        if let Some(row) = item.downcast_ref::<gtk::ListBoxRow>() {
            let list = row
                .parent()
                .and_downcast::<gtk::ListBox>()
                .ok_or("the row is in no list")?;
            list.select_row(Some(row));
            return Ok(());
        }
        if view_item(&item) {
            // The view's own action for a click: select this item, and no other.
            item.activate_action("listitem.select", Some(&(false, false).to_variant()))
                .map_err(|e| format!("{} cannot be selected: {e}", describe(&item)))?;
            if !item.state_flags().contains(gtk::StateFlags::SELECTED) {
                return Err(format!(
                    "{} stays unselected: its view's model does not select it",
                    describe(&item)
                ));
            }
            return Ok(());
        }
        ancestor = item.parent();
    }
    Err(format!(
        "{} is in no list row, nor an item of a list or grid view",
        describe(widget)
    ))
}

/// A click on an item of a view that takes one click for a choice (a drop-down's list): the item is selected, then
/// activated at the place its model now has selected, which is how the view's own click finds it. `None` for any
/// other widget.
fn choose(widget: &gtk::Widget) -> Option<Result<(), String>> {
    let view = widget.parent().filter(|_| view_item(widget))?;
    let (model, one_click) = match view.downcast_ref::<gtk::ListView>() {
        Some(list) => (list.model(), list.is_single_click_activate()),
        None => {
            let grid = view.downcast_ref::<gtk::GridView>()?;
            (grid.model(), grid.is_single_click_activate())
        }
    };
    if !one_click {
        return None;
    }
    Some(select(widget).and_then(|()| {
        let selected = model.map(|model| model.selection()).filter(|chosen| !chosen.is_empty());
        let place = selected.ok_or_else(|| format!("{} has no place in its view's model", describe(widget)))?;
        view.activate_action("list.activate-item", Some(&place.minimum().to_variant()))
            .map_err(|e| format!("{} cannot be activated: {e}", describe(widget)))
    }))
}

/// Whether what `wait` finds is what it was asked for: the same value, or for a number a range it is in,
/// `{"min": 30}`, `{"max": 400}` or both, where the exact number is the font's or the theme's to say.
fn meets(now: &Value, asked: &Value) -> bool {
    let bounds = asked.as_object().filter(|range| {
        !range.is_empty()
            && range
                .iter()
                .all(|(key, bound)| (key == "min" || key == "max") && bound.is_number())
    });
    match (bounds, now.as_f64()) {
        (Some(range), Some(now)) => {
            range.get("min").and_then(Value::as_f64).is_none_or(|min| now >= min)
                && range.get("max").and_then(Value::as_f64).is_none_or(|max| now <= max)
        }
        _ => now == asked,
    }
}

/// Whether `widget` is an item of a list or grid view: one of the widgets the view makes for its model's items.
fn view_item(widget: &gtk::Widget) -> bool {
    widget
        .parent()
        .is_some_and(|view| view.is::<gtk::ListView>() || view.is::<gtk::GridView>())
}

/// Whether a person could click `widget`: it is on screen, enabled, and nothing (a dialog over it, in its window or a
/// modal window of its own) keeps the pointer from it.
fn clickable(widget: &gtk::Widget) -> Result<(), String> {
    let blocked = |w: &gtk::Widget| !w.can_target();
    let mut ancestor = Some(widget.clone());
    let window = widget.root().and_downcast::<gtk::Window>();
    let mut under_dialog = window.is_some_and(|window| {
        gtk::Window::list_toplevels()
            .iter()
            .filter_map(|other| other.downcast_ref::<gtk::Window>())
            .any(|other| other.is_modal() && other.is_mapped() && is_over(other, &window))
    });
    while let Some(w) = ancestor {
        under_dialog |= blocked(&w);
        ancestor = w.parent();
    }
    if !widget.is_mapped() {
        Err(format!("{} is not on screen", describe(widget)))
    } else if !widget.is_sensitive() {
        Err(format!("{} is disabled", describe(widget)))
    } else if under_dialog {
        Err(format!("{} takes no pointer: a dialog is over it", describe(widget)))
    } else {
        Ok(())
    }
}

/// Whether `window` is transient for `under`, or for a window that is.
fn is_over(window: &gtk::Window, under: &gtk::Window) -> bool {
    let mut parent = window.transient_for();
    while let Some(window) = parent {
        if &window == under {
            return true;
        }
        parent = window.transient_for();
    }
    false
}

/// The key and the modifiers `keys` names (GTK accelerator syntax), as a keyboard sends them. GTK's parser gives a
/// letter in lower case, so that "R" and "<Shift>R" are r; a keyboard sends the letter it types, upper case with
/// Shift held. An upper-case letter is typed so with other modifiers too: "<Control>U" is Ctrl+Shift+U, where GTK
/// would read Ctrl+u, so that a letter's case always says whether Shift is held.
pub(crate) fn keyboard_key(keys: &str) -> Result<(gdk::Key, gdk::ModifierType), String> {
    let (key, mut modifiers) = gtk::accelerator_parse(keys).ok_or_else(|| format!("no such key: {keys}"))?;
    let named = keys.rsplit('>').next().and_then(gdk::Key::from_name);
    if named.is_some_and(|named| named != named.to_lower()) {
        modifiers |= gdk::ModifierType::SHIFT_MASK;
    }
    let shifted = modifiers.contains(gdk::ModifierType::SHIFT_MASK);
    Ok((if shifted { key.to_upper() } else { key }, modifiers))
}

/// broadway's numbers for what its page sends (`BROADWAY_EVENT_*`) and what it is sent (`BROADWAY_OP_*`).
const KEY_PRESS: i32 = 7;
const KEY_RELEASE: i32 = 8;
const ROUNDTRIP_NOTIFY: i32 = 14;
const OP_ROUNDTRIP: u8 = 16;

/// The modifiers a key may be pressed with, and the key that holds each.
const MODIFIER_KEYS: &[(gdk::ModifierType, gdk::Key)] = &[
    (gdk::ModifierType::CONTROL_MASK, gdk::Key::Control_L),
    (gdk::ModifierType::SHIFT_MASK, gdk::Key::Shift_L),
    (gdk::ModifierType::ALT_MASK, gdk::Key::Alt_L),
    (gdk::ModifierType::SUPER_MASK, gdk::Key::Super_L),
];

/// The socket to the broadway display's input, written by the main loop and the thread that answers the display.
type Input = Arc<Mutex<TcpStream>>;

/// The broadway display's input, or why there is none.
static INPUT: OnceLock<Result<Input, String>> = OnceLock::new();

/// Connects to the input of the broadway display the app is on, as its web page does: a WebSocket at `address`
/// (`OMUI_QA_INPUT`, which dev/qa.sh sets).
fn connect(address: &str) -> Result<Input, String> {
    let mut socket = TcpStream::connect(address).map_err(|e| format!("broadway input at {address}: {e}"))?;
    let request = format!(
        "GET /socket HTTP/1.1\r\nHost: {address}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Key: b21lZ2FvbXVpcWFrZXlzIQ==\r\nSec-WebSocket-Version: 13\r\n\
         Sec-WebSocket-Protocol: broadway\r\n\r\n"
    );
    socket.write_all(request.as_bytes()).map_err(|e| e.to_string())?;
    let mut reply = Vec::new();
    let mut byte = [0];
    while !reply.ends_with(b"\r\n\r\n") {
        socket
            .read_exact(&mut byte)
            .map_err(|e| format!("broadway input: {e}"))?;
        reply.push(byte[0]);
    }
    if !reply.starts_with(b"HTTP/1.1 101") {
        return Err(format!("broadway input refused: {}", String::from_utf8_lossy(&reply)));
    }
    let input = Arc::new(Mutex::new(socket.try_clone().map_err(|e| e.to_string())?));
    let answers = input.clone();
    // Until the app ends; the display's going ends it sooner.
    std::thread::spawn(move || answer_frames(socket, &answers));
    Ok(input)
}

/// Reads what the display sends its page: the drawing, dropped, and after each frame a roundtrip, which the page
/// answers. Unanswered, the app would draw no further frame. A roundtrip is the last command of its message (the
/// display sends it at once): its op, then the serial, the surface and the tag, little-endian.
fn answer_frames(mut socket: TcpStream, input: &Mutex<TcpStream>) -> std::io::Result<()> {
    loop {
        let mut head = [0; 2];
        socket.read_exact(&mut head)?;
        let length = match head[1] & 0x7f {
            126 => {
                let mut length = [0; 2];
                socket.read_exact(&mut length)?;
                u16::from_be_bytes(length).into()
            }
            127 => {
                let mut length = [0; 8];
                socket.read_exact(&mut length)?;
                u64::from_be_bytes(length)
            }
            length => length.into(),
        };
        let mut message = vec![0; length as usize];
        socket.read_exact(&mut message)?;
        let Some(roundtrip) = message.len().checked_sub(11).map(|at| &message[at..]) else {
            continue;
        };
        if roundtrip[0] == OP_ROUNDTRIP {
            let word = |at: usize| u32::from_le_bytes(roundtrip[at..at + 4].try_into().expect("four bytes"));
            let surface = u16::from_le_bytes([roundtrip[5], roundtrip[6]]);
            let mut input = input.lock().unwrap_or_else(|e| e.into_inner());
            send(
                &mut input,
                &[ROUNDTRIP_NOTIFY, word(1) as i32, 0, surface.into(), word(7) as i32],
            )?;
        }
    }
}

/// One input event as broadway's page sends it: a masked binary WebSocket frame of big-endian 32-bit words, the
/// event, the last serial seen, a time, then the event's own.
fn send(socket: &mut TcpStream, words: &[i32]) -> std::io::Result<()> {
    // A zero mask, which leaves the words as they are.
    let mut frame = vec![0x82, 0x80 | (words.len() * 4) as u8, 0, 0, 0, 0];
    for word in words {
        frame.extend(word.to_be_bytes());
    }
    socket.write_all(&frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mnemonics_are_read_as_shown() {
        assert_eq!(without_mnemonic("C_lear"), "Clear");
        assert_eq!(without_mnemonic("_Save"), "Save");
        assert_eq!(without_mnemonic("snake__case"), "snake_case");
    }

    #[test]
    fn roles_go_by_gtk_nicks() {
        assert_eq!(role_by_nick("toggle-button"), Some(gtk::AccessibleRole::ToggleButton));
        assert_eq!(nick(gtk::AccessibleRole::SearchBox), "search-box");
        assert_eq!(role_by_nick("no-such-role"), None);
    }

    #[test]
    fn a_target_takes_a_role_a_name_and_an_ancestor() {
        let command = |line: &str| serde_json::from_str::<Map<String, Value>>(line).unwrap();
        let target = Target::parse(&command(
            r#"{"do":"click","role":"button","name":"Rename","in":{"role":"dialog"}}"#,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(target.to_string(), "button named \"Rename\" in dialog");
        assert!(Target::parse(&command(r#"{"do":"state"}"#)).unwrap().is_none());
        // An action's name is not a widget's.
        assert!(
            Target::parse(&command(r#"{"do":"action","name":"win.save"}"#))
                .unwrap()
                .is_none()
        );
        assert!(Target::parse(&command(r#"{"do":"click","role":"buton"}"#)).is_err());
        // `wait`: the same value, or a number within a range
        assert!(meets(&json!(36), &json!(36)) && !meets(&json!(36), &json!(30)));
        assert!(meets(&json!(36), &json!({"min": 30})) && !meets(&json!(36), &json!({"min": 40})));
        assert!(meets(&json!(36), &json!({"min": 30, "max": 36})) && !meets(&json!(36), &json!({"max": 35})));
        assert!(!meets(&json!("36"), &json!({"min": 30})) && !meets(&json!(36), &json!({"least": 30})));
        assert!(meets(&json!({"role": "button"}), &json!({"role": "button"})));
    }
}
