// SPDX-License-Identifier: MIT
//! Scripted QA, behind feature `qa`: with `OMUI_QA=1` an app built with it reads commands from stdin, one JSON object
//! a line, and answers each on stdout, in order. Widgets are found by accessible role and name, as a screen reader
//! would find them. Any GLib warning or critical fails the run. `dev/qa.sh` runs an app this way on a broadway
//! display of its own; README.md has the commands.

use std::{
    cell::{Cell, RefCell},
    io::{BufRead, Read, Write},
    net::TcpStream,
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

/// What GDK's broadway backend says, once a run (`g_warning_once`), when a page answers its frames, as the page that
/// presses keys here does: each answer marks the surface's pending frame submitted at 1/30 s
/// (`_gdk_broadway_roundtrip_notify`, GTK 4.24), and a frame the clock had already finished with another interval
/// draws this. A browser showing the display gets it as well; the app is not at fault. Broadway marks no frame
/// presented, so `gdk_frame_timings_presented()`'s like warning cannot come from here, and is not let pass.
const PAGE_FRAMES: (&str, &str) = (
    "Gdk",
    "Duplicate call to gdk_frame_timings_submitted() with different values.",
);

/// How long a command waits for its widget, or `wait` for its condition, unless it gives `timeout_ms`.
const TIMEOUT: Duration = Duration::from_secs(5);

/// Whether this run is QA mode. In it, GLib's warnings and criticals are recorded from here on.
pub(crate) fn wanted() -> bool {
    let wanted = std::env::var_os("OMUI_QA").is_some_and(|value| value == "1");
    if wanted {
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
    wanted
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
        || ((field("GLIB_DOMAIN"), message) == PAGE_FRAMES && matches!(INPUT.get(), Some(Ok(_))));
    if let Some(kind) = kind.filter(|_| !known) {
        let line = format!("{}-{kind}: {message}", field("GLIB_DOMAIN"));
        LOGGED.lock().unwrap_or_else(|e| e.into_inner()).push(line);
    }
    glib::log_writer_default(level, fields)
}

/// Ends the run unless the app is on a broadway display, as dev/qa.sh gives it, which no one sees: a script run on
/// the desktop would click about in a window there. Called as the application starts, before it has a window.
pub(crate) fn require_hidden_display() {
    let display = gdk::Display::default().map(|display| display.type_().name());
    if display != Some("GdkBroadwayDisplay") {
        eprintln!(
            "omui qa: OMUI_QA=1 runs on a broadway display alone (dev/qa.sh), not on {}",
            display.unwrap_or("no display")
        );
        std::process::exit(2);
    }
}

/// Starts reading commands, for the window `open` has just built.
pub(crate) fn start(app: &gtk::Application) {
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
            let result = match (painted(&qa.window).await, command) {
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

/// Waits until `window` has been drawn once more: until then GTK has no picture of what has changed in it.
async fn painted(window: &gtk::Window) -> Result<(), String> {
    let start = Instant::now();
    let stopped = || (start.elapsed() > TIMEOUT).then(|| "the window is not being drawn".to_owned());
    let clock = loop {
        match window.frame_clock() {
            Some(clock) => break clock,
            None => stopped().map_or(Ok(()), Err)?,
        }
        glib::timeout_future(Duration::from_millis(10)).await;
    };
    let done = Rc::new(Cell::new(false));
    let handler = clock.connect_after_paint({
        let done = done.clone();
        move |_| done.set(true)
    });
    window.queue_draw();
    let mut result = Ok(());
    while !done.get() && result.is_ok() {
        result = stopped().map_or(Ok(()), Err);
        glib::timeout_future(Duration::from_millis(10)).await;
    }
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
    let mut out = std::io::stdout().lock();
    // A reader that has gone cannot be told anything; the exit status still says how the run went.
    let _ = writeln!(out, "{}", Value::Object(line)).and_then(|()| out.flush());
    ok
}

struct Qa {
    app: gtk::Application,
    /// The window the app opened: what `state`, `resize` and `shot` mean.
    window: gtk::Window,
    /// Key presses and releases the app's windows have had, counted as they arrive.
    keys_seen: Rc<Cell<u64>>,
    /// The windows that count them.
    watched: RefCell<Vec<(glib::WeakRef<gtk::Window>, gtk::EventControllerLegacy)>>,
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
                // A click on a button gives it the keyboard, unless it is one that does not take it so.
                if widget.is_focusable() && widget.gets_focus_on_click() {
                    widget.grab_focus();
                }
                // A button clicks at once, as under the pointer; `activate` would hold it pressed for a moment first,
                // as Enter does.
                if let Some(button) = widget.downcast_ref::<gtk::Button>() {
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
                let row = widget
                    .downcast_ref::<gtk::ListBoxRow>()
                    .ok_or_else(|| format!("{} is not a list row", describe(&widget)))?;
                let list = row
                    .parent()
                    .and_downcast::<gtk::ListBox>()
                    .ok_or("the row is in no list")?;
                list.select_row(Some(row));
                Ok(Map::new())
            }
            "key" => {
                let keys = command
                    .get("keys")
                    .and_then(Value::as_str)
                    .ok_or("`key` needs `keys`, as \"<Control>u\"")?;
                let (key, modifiers) = gtk::accelerator_parse(keys).ok_or_else(|| format!("no such key: {keys}"))?;
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
                crate::dev::shot(&self.window, std::path::Path::new(path))?;
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

    /// Every window, those on top first: the newest, and the one with the keyboard before all. Not only the app's:
    /// a dialog over a window that has no `Shell` is a modal window of no application.
    fn roots(&self) -> Vec<gtk::Window> {
        let mut windows: Vec<_> = gtk::Window::list_toplevels()
            .into_iter()
            .rev()
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
                Err(e) => return Err(e),
                _ if start.elapsed() > timeout => return Err(format!("found no {target} on screen")),
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
                Some(target) => match target.find(&widgets(&self.roots()))? {
                    Some(widget) => properties(&widget),
                    None => Map::from_iter([("visible".to_owned(), false.into())]),
                },
                None => self.state(),
            };
            // With nothing asked, the widget is waited for.
            let holds = if expected.is_empty() {
                now.get("visible") == Some(&Value::Bool(true))
            } else {
                expected.iter().all(|(key, value)| now.get(*key) == Some(value))
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
        // An entry's text is a part of it with no role of its own: the entry is what has the keyboard.
        let mut focus = RootExt::focus(&self.keyboard_window());
        while let Some(part) = focus
            .clone()
            .filter(|w| w.accessible_role() == gtk::AccessibleRole::None)
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
    async fn press(&self, key: gdk::Key, modifiers: gdk::ModifierType) -> Result<(), String> {
        if key == gdk::Key::VoidSymbol {
            return Err("that key has no key symbol".to_owned());
        }
        for window in self.roots() {
            self.watch(&window);
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
            let before = self.keys_seen.get();
            let words = [event, 0, 0, key.into_glib() as i32, state.bits() as i32];
            send(&mut input.lock().unwrap_or_else(|e| e.into_inner()), &words)
                .map_err(|e| format!("broadway input: {e}"))?;
            let start = Instant::now();
            while self.keys_seen.get() == before {
                if start.elapsed() > Duration::from_secs(2) {
                    return Err("the key never reached a window".to_owned());
                }
                glib::timeout_future(Duration::from_millis(5)).await;
            }
        }
        // What the key started (a dialog closing, a page going back) has run by the next turn of the loop.
        glib::timeout_future(Duration::ZERO).await;
        Ok(())
    }

    /// Counts the key presses and releases `window` has, before any of its widgets sees them: its counter is put
    /// first among the window's controllers, where GTK puts the one added last, as an open dialog adds its own.
    fn watch(&self, window: &gtk::Window) {
        let mut watched = self.watched.borrow_mut();
        watched.retain(|(w, _)| w.upgrade().is_some());
        if let Some(index) = watched.iter().position(|(w, _)| w.upgrade().as_ref() == Some(window)) {
            window.remove_controller(&watched.remove(index).1);
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
        window.add_controller(events.clone());
        watched.push((window.downgrade(), events));
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

/// Every name `widget` answers to, the one a screen reader would say first: its accessible label, its own label,
/// title or placeholder, its tooltip, and for a role named by its content the text of each label inside it.
fn names(widget: &gtk::Widget) -> Vec<String> {
    let mut names = Vec::new();
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
        let mut child = widget.first_child();
        while let Some(inner) = child {
            walk(&inner, &mut |inner| {
                if inner.is::<gtk::Label>() {
                    names.extend(own_label(inner));
                }
            });
            child = inner.next_sibling();
        }
    }
    names.retain(|name| !name.is_empty());
    names
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
    let selected = widget.downcast_ref::<gtk::ListBoxRow>().map(|row| row.is_selected());
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
    }
}
