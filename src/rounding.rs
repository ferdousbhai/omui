// SPDX-License-Identifier: MIT
//! Hyprland's window rounding (`decoration:rounding`), followed live, so an app's corners match its window's.
//!
//! Themes and a user's own config can both set it, so the value in effect is asked of `hyprctl` rather than read from
//! a file: once at the start, and again each time Hyprland's event socket announces `configreloaded`, which it sends
//! once the new values are in effect. Nothing polls. A socket that cannot be reached, or that drops, is tried again,
//! less often each time, and the value is asked for anew on every connection, since a reload may have come between.
//! Away from Hyprland nothing is asked and the corners stay square, as Omarchy's own are by default.

use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

use gtk::{gio, glib, prelude::*};

/// How long `hyprctl` gets to answer. A compositor that is busy or gone must not leave a query behind.
const QUERY_TIMEOUT: Duration = Duration::from_millis(500);
/// The wait before the event socket is tried again, doubled after each failure up to the last.
const FIRST_RETRY: Duration = Duration::from_secs(1);
const LAST_RETRY: Duration = Duration::from_secs(60);

/// Hyprland's rounding in pixels, followed until this is dropped. It needs a main loop but no display.
pub struct Rounding {
    hyprctl: Option<PathBuf>,
    events: Option<PathBuf>,
    radius: Cell<i32>,
    /// A query is running, and whether an event came while it was: it may have read the value from before.
    asking: Cell<bool>,
    again: Cell<bool>,
    retry: Cell<Duration>,
    /// The query that is running or ran last, and the task that listens: both stopped when this is dropped.
    query: RefCell<Option<glib::JoinHandle<()>>>,
    listener: RefCell<Option<glib::JoinHandle<()>>>,
    changed: Box<dyn Fn(i32)>,
}

impl Rounding {
    /// Where this session's Hyprland is: `hyprctl` on the path and the event socket
    /// `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock`. Neither when the session is not
    /// Hyprland's.
    pub fn session() -> (Option<PathBuf>, Option<PathBuf>) {
        let set = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
        let Some(signature) = set("HYPRLAND_INSTANCE_SIGNATURE") else {
            return (None, None);
        };
        let events = set("XDG_RUNTIME_DIR").map(|runtime| {
            PathBuf::from(runtime)
                .join("hypr")
                .join(signature)
                .join(".socket2.sock")
        });
        (glib::find_program_in_path("hyprctl"), events)
    }

    /// Follows the rounding `hyprctl` reports, asking again when the socket at `events` says the config was
    /// reloaded. `changed` runs on the main loop each time the value differs from the last. With no `hyprctl` the
    /// rounding is 0 and stays so. Tests pass a script and a socket of their own.
    pub fn follow(hyprctl: Option<PathBuf>, events: Option<PathBuf>, changed: impl Fn(i32) + 'static) -> Rc<Self> {
        let rounding = Rc::new(Self {
            // Without a hyprctl to ask there is nothing an event could lead to.
            events: events.filter(|_| hyprctl.is_some()),
            hyprctl,
            radius: Cell::new(0),
            asking: Cell::new(false),
            again: Cell::new(false),
            retry: Cell::new(FIRST_RETRY),
            query: RefCell::new(None),
            listener: RefCell::new(None),
            changed: Box::new(changed),
        });
        rounding.ask();
        rounding.listen();
        rounding
    }

    /// The rounding last read, in pixels; 0 until `hyprctl` has answered, and away from Hyprland.
    pub fn radius(&self) -> i32 {
        self.radius.get()
    }

    fn ask(self: &Rc<Self>) {
        let Some(hyprctl) = self.hyprctl.clone() else {
            return;
        };
        if self.asking.replace(true) {
            self.again.set(true);
            return;
        }
        let weak = Rc::downgrade(self);
        let task = glib::spawn_future_local(async move {
            let answer = query(&hyprctl).await;
            let Some(this) = weak.upgrade() else { return };
            this.asking.set(false);
            // No answer leaves the rounding as it was: a compositor mid-reload is not a reason to square the corners.
            if let Some(radius) = answer
                && this.radius.replace(radius) != radius
            {
                (this.changed)(radius);
            }
            if this.again.replace(false) {
                this.ask();
            }
        });
        self.query.replace(Some(task));
    }

    fn listen(self: &Rc<Self>) {
        let Some(path) = self.events.clone() else {
            return;
        };
        let weak = Rc::downgrade(self);
        let task = glib::spawn_future_local(async move {
            loop {
                let address = gio::UnixSocketAddress::new(&path);
                if let Ok(connection) = gio::SocketClient::new().connect_future(&address).await {
                    match weak.upgrade() {
                        Some(this) => {
                            this.retry.set(FIRST_RETRY);
                            this.ask();
                        }
                        None => return,
                    }
                    // Events arrive in pieces; only a whole line is one. Bytes, not text: a window's title is in some
                    // of them, and need not be UTF-8.
                    let lines = gio::DataInputStream::new(&connection.input_stream());
                    while let Ok(Some(line)) = lines.read_line_future(glib::Priority::DEFAULT).await {
                        if line.starts_with(b"configreloaded>>") {
                            match weak.upgrade() {
                                Some(this) => this.ask(),
                                None => return,
                            }
                        }
                    }
                }
                // Not there yet, or gone: Hyprland restarting, or a session that is ending.
                let wait = match weak.upgrade() {
                    Some(this) => this.retry.replace((this.retry.get() * 2).min(LAST_RETRY)),
                    None => return,
                };
                glib::timeout_future(wait).await;
            }
        });
        self.listener.replace(Some(task));
    }
}

impl Drop for Rounding {
    fn drop(&mut self) {
        for task in [self.query.take(), self.listener.take()].into_iter().flatten() {
            task.abort();
        }
    }
}

/// `hyprctl -j getoption decoration:rounding`, whose answer is `{"option": …, "int": 4, "set": true}`. None when it
/// cannot be run, fails, does not answer in time, or says something else.
async fn query(hyprctl: &Path) -> Option<i32> {
    let arguments = [
        hyprctl.as_os_str(),
        "-j".as_ref(),
        "getoption".as_ref(),
        "decoration:rounding".as_ref(),
    ];
    let flags = gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE;
    let process = gio::Subprocess::newv(&arguments, flags).ok()?;
    let Ok(answer) = glib::future_with_timeout(QUERY_TIMEOUT, process.communicate_utf8_future(None)).await else {
        process.force_exit();
        return None;
    };
    let (output, _) = answer.ok()?;
    if !process.is_successful() {
        return None;
    }
    let radius = crate::json::parse(&output?).ok()?.get("int")?.as_f64()?;
    (0.0..=1000.0).contains(&radius).then_some(radius as i32)
}

#[cfg(test)]
mod tests {
    use std::{
        io::Write,
        os::unix::{fs::PermissionsExt, net::UnixListener},
    };

    use super::*;
    use crate::testing;

    /// A `hyprctl` that answers with `response.json` beside it and adds a line to `calls` each time it is run.
    fn fake_hyprctl(dir: &Path) -> PathBuf {
        let command = dir.join("hyprctl");
        let script = format!(
            "#!/bin/sh\necho >> '{0}/calls'\ncat '{0}/response.json'\n",
            dir.display()
        );
        std::fs::write(&command, script).unwrap();
        std::fs::set_permissions(&command, std::fs::Permissions::from_mode(0o755)).unwrap();
        command
    }

    fn calls(dir: &Path) -> usize {
        std::fs::read_to_string(dir.join("calls")).map_or(0, |calls| calls.lines().count())
    }

    fn answer(dir: &Path, radius: i32) {
        std::fs::write(dir.join("response.json"), format!("{{\"int\":{radius}}}")).unwrap();
    }

    #[test]
    fn hyprland_is_asked_again_only_when_it_reloads() {
        let _main = testing::main_loop();
        let dir = testing::temp_dir("rounding-reload");
        let hyprctl = fake_hyprctl(&dir);
        answer(&dir, 4);
        let socket = dir.join("events.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();

        let seen = Rc::new(RefCell::new(Vec::new()));
        let rounding = Rounding::follow(Some(hyprctl), Some(socket), {
            let seen = seen.clone();
            move |radius| seen.borrow_mut().push(radius)
        });
        assert_eq!(rounding.radius(), 0, "square until hyprctl has answered");
        // Once at the start, and once more when the event socket connects.
        let mut hyprland = None;
        assert!(testing::spin_until(5000, || {
            hyprland = hyprland
                .take()
                .or_else(|| listener.accept().ok().map(|(stream, _)| stream));
            // Both answered: a query still running would read the next answer written below.
            hyprland.is_some() && calls(&dir) == 2 && rounding.radius() == 4 && !rounding.asking.get()
        }));
        let mut hyprland = hyprland.unwrap();

        answer(&dir, 10);
        testing::spin_for(300);
        assert_eq!(calls(&dir), 2, "nothing asks while nothing is announced");
        // Events arrive in pieces; only a whole configreloaded line counts.
        hyprland.write_all(b"activewindow>>kitty,\xff~\nconfigrel").unwrap();
        testing::spin_for(150);
        assert_eq!(calls(&dir), 2);
        hyprland.write_all(b"oaded>>\n").unwrap();
        assert!(testing::spin_until(5000, || rounding.radius() == 10 && !rounding.asking.get()));
        assert_eq!(calls(&dir), 3);
        assert_eq!(*seen.borrow(), [4, 10], "told once for each new value");

        // The socket drops (Hyprland restarted): it is reached again, and asked again.
        answer(&dir, 6);
        drop(hyprland);
        let mut again = None;
        assert!(testing::spin_until(5000, || {
            again = again.take().or_else(|| listener.accept().ok());
            again.is_some() && rounding.radius() == 6 && !rounding.asking.get()
        }));

        // An answer that is not one leaves the rounding as it was.
        std::fs::write(dir.join("response.json"), "unknown request").unwrap();
        again.unwrap().0.write_all(b"configreloaded>>\n").unwrap();
        let asked = calls(&dir);
        assert!(testing::spin_until(5000, || calls(&dir) > asked && !rounding.asking.get()));
        assert_eq!(rounding.radius(), 6);
        drop(rounding);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hyprland_is_reached_once_it_listens() {
        let _main = testing::main_loop();
        let dir = testing::temp_dir("rounding-late");
        let hyprctl = fake_hyprctl(&dir);
        answer(&dir, 4);
        let socket = dir.join("events.sock");
        let rounding = Rounding::follow(Some(hyprctl), Some(socket.clone()), |_| {});
        assert!(testing::spin_until(5000, || rounding.radius() == 4));

        answer(&dir, 8);
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut hyprland = None;
        assert!(testing::spin_until(10_000, || {
            hyprland = hyprland.take().or_else(|| listener.accept().ok());
            rounding.radius() == 8
        }));
        drop(rounding);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn away_from_hyprland_the_corners_are_square_and_nothing_is_asked() {
        let _main = testing::main_loop();
        let dir = testing::temp_dir("rounding-none");
        let rounding = Rounding::follow(None, Some(dir.join("events.sock")), |_| panic!("nothing changes"));
        testing::spin_for(100);
        assert_eq!(rounding.radius(), 0);
        assert!(rounding.events.is_none() && rounding.listener.borrow().is_none());
        // A hyprctl that is not there, or hangs, is no answer either.
        let missing = Rounding::follow(Some(dir.join("no-such-hyprctl")), None, |_| panic!("nothing changes"));
        let hanging = dir.join("hyprctl");
        std::fs::write(&hanging, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&hanging, std::fs::Permissions::from_mode(0o755)).unwrap();
        let slow = Rounding::follow(Some(hanging), None, |_| panic!("nothing changes"));
        assert!(testing::spin_until(5000, || !slow.asking.get() && !missing.asking.get()));
        assert_eq!((missing.radius(), slow.radius()), (0, 0));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
