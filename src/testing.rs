// SPDX-License-Identifier: MIT
//! What the tests that need a main loop share. glib's timeouts, file monitors and futures report on the default main
//! context, which one thread at a time may run, and cargo runs tests on several: each such test holds
//! [`main_loop`] for as long as it runs. So does a test that changes what all of them read (the environment); one
//! that switches the language takes [`crate::i18n::lock_language`] too, after this, as an app's tests do.

use std::{
    path::PathBuf,
    sync::{LazyLock, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use gtk::glib;

static CONTEXT: LazyLock<glib::MainContext> = LazyLock::new(glib::MainContext::default);
static TURN: Mutex<()> = Mutex::new(());

/// The default main context, for this test alone until the guard is dropped.
pub struct MainLoop {
    // Dropped in this order: the context is given up before the next test is let in.
    _context: glib::MainContextAcquireGuard<'static>,
    _turn: MutexGuard<'static, ()>,
}

impl Drop for MainLoop {
    /// What the test left on the loop is run out here, on the thread that started it: gtk-rs lets a callback run
    /// only on the thread that made it, and the next test's thread would otherwise be the one to find it.
    fn drop(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut quiet = 0;
        while quiet < 20 && Instant::now() < deadline {
            quiet = if CONTEXT.iteration(false) { 0 } else { quiet + 1 };
            if quiet > 0 {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
}

pub fn main_loop() -> MainLoop {
    // A test that failed while it held the lock has left nothing behind that the next one needs.
    let turn = TURN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    MainLoop {
        _context: CONTEXT
            .acquire()
            .expect("no other thread runs the default main context"),
        _turn: turn,
    }
}

/// Runs the main loop until `done` says so, for `milliseconds` at most. Whether it did.
pub fn spin_until(milliseconds: u64, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_millis(milliseconds);
    loop {
        // A bounded run of what is ready, so a source that is always ready cannot keep `done` from being asked.
        for _ in 0..200 {
            if !CONTEXT.iteration(false) {
                break;
            }
        }
        if done() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Runs the main loop for `milliseconds`: for what must not happen.
pub fn spin_for(milliseconds: u64) {
    spin_until(milliseconds, || false);
}

/// A new, empty directory of this test's own, which the test removes when it has passed.
pub fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("omui-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory under the temporary one");
    dir
}
