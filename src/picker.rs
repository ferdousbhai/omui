// SPDX-License-Identifier: MIT
//! Choosing a file to open or a place to save, through the desktop's portal (`org.freedesktop.portal.FileChooser`
//! over D-Bus), so the dialog is the desktop's own and not GTK's.
//!
//! The behaviour follows Omacut's `src/portalfilepicker.cpp` (github.com/omacom/omacut, MIT, David Heinemeier
//! Hansson): the response is listened for before the request is made, at the path the portal will derive from the
//! request's token, so it cannot slip past; a portal that hands back another path is heard there; and a request
//! whose answer never comes does not wedge the ones after it. Here every request also ends: with the files, with
//! [`PickError::Cancelled`], or with the failure: [`PickError::Unavailable`] when there is no portal,
//! [`PickError::Refused`] when it will not show the dialog, [`PickError::NoAnswer`] when it does not take the request
//! within the timeout, and [`PickError::Lost`] when it goes away while its dialog is open.
//!
//! An app holds a `Rc<dyn FilePicker>`: [`PortalFilePicker`] in the app, [`FakeFilePicker`] in its tests.

use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::{Rc, Weak},
    time::Duration,
};

use gtk::{gio, glib, prelude::*};

use crate::i18n::tr;

/// The kinds of file a dialog offers under one name: `FileFilter::new("Images", &["*.png", "*.jpg"])`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileFilter {
    /// What the dialog calls them, in the user's language.
    pub name: String,
    /// Glob patterns: `*.png`.
    pub patterns: Vec<String>,
    /// MIME types, which may end in a wildcard: `image/*`.
    pub mime_types: Vec<String>,
}

impl FileFilter {
    pub fn new(name: &str, patterns: &[&str]) -> Self {
        Self {
            name: name.to_owned(),
            patterns: patterns.iter().map(|pattern| (*pattern).to_owned()).collect(),
            mime_types: Vec::new(),
        }
    }

    pub fn mime_types(mut self, mime_types: &[&str]) -> Self {
        self.mime_types = mime_types.iter().map(|mime| (*mime).to_owned()).collect();
        self
    }
}

/// What a file dialog shows. Everything but the title may be left out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PickOptions {
    /// The dialog's title, in the user's language.
    pub title: String,
    /// The label of the button that accepts, in place of the portal's own "Open" or "Save".
    pub accept_label: Option<String>,
    /// Opening: whether several files may be chosen.
    pub multiple: bool,
    /// Opening: folders are chosen, not files.
    pub directory: bool,
    /// The folder the dialog starts in.
    pub folder: Option<PathBuf>,
    /// Saving: the file name suggested.
    pub name: Option<String>,
    /// The kinds of file offered, the first one chosen to begin with. None offers every file.
    pub filters: Vec<FileFilter>,
}

impl PickOptions {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_owned(),
            ..Self::default()
        }
    }
}

/// Why a dialog ended without a file. Its text ([`std::fmt::Display`]) is for the user, in their language, and says
/// which way the dialog failed in words a person reads; what the system said of it is for the log, as
/// [`PickError::detail`] and the error's [`std::error::Error::source`]:
///
/// ```ignore
/// picker.open(options, Box::new(move |picked| match picked {
///     Ok(files) => open(files),
///     Err(omui::PickError::Cancelled) => {}
///     Err(error) => {
///         eprintln!("file chooser: {error} ({})", error.detail().unwrap_or_default());
///         shell.toast(&error.to_string());
///     }
/// }));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickError {
    /// The user closed the dialog, or the app asked for another before this one was answered. Nothing to report.
    Cancelled,
    /// A failure the app describes itself, in the user's language: what a [`FakeFilePicker`] is told to answer, or a
    /// picker of the app's own. omui's pickers answer with the kinds below.
    Failed(String),
    /// There is no file chooser to ask: no session bus, or no portal on it.
    Unavailable(PickDetail),
    /// The portal is there and would not show the dialog (no backend for it, not allowed), or ended it without an
    /// answer.
    Refused(PickDetail),
    /// The portal did not say it had the request within the picker's timeout.
    NoAnswer,
    /// The portal went away, or the bus with it, while its dialog was open.
    Lost,
}

/// What the system said of a failed dialog, in its own words and in English (GIO's `Cannot autolaunch D-Bus without
/// X11 $DISPLAY`): for a log, not for the user.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PickDetail(String);

impl PickDetail {
    pub fn new(detail: &str) -> Self {
        Self(detail.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PickDetail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PickDetail {}

impl PickError {
    /// What the system said of the failure, for a log. None for one that has nothing more to say.
    pub fn detail(&self) -> Option<&str> {
        match self {
            PickError::Unavailable(detail) | PickError::Refused(detail) if !detail.0.is_empty() => Some(&detail.0),
            _ => None,
        }
    }

    /// The failure as GIO reported it, by kind: a bus or a portal that is not there is unavailable, a call that
    /// timed out had no answer, a portal that left without replying is lost, and any other error is the portal
    /// refusing.
    fn from_glib(error: &glib::Error) -> Self {
        let detail = PickDetail::new(error.message());
        let missing = [
            gio::DBusError::ServiceUnknown,
            gio::DBusError::NameHasNoOwner,
            gio::DBusError::UnknownMethod,
            gio::DBusError::UnknownObject,
            gio::DBusError::UnknownInterface,
            gio::DBusError::Disconnected,
            gio::DBusError::NoServer,
            gio::DBusError::FileNotFound,
            gio::DBusError::SpawnExecFailed,
        ];
        if error.matches(gio::IOErrorEnum::TimedOut) || error.matches(gio::DBusError::Timeout) {
            PickError::NoAnswer
        } else if error.matches(gio::DBusError::NoReply) {
            // The bus's word for a portal that left the bus with the request in hand.
            PickError::Lost
        } else if missing.iter().any(|kind| error.matches(*kind))
            || error.matches(gio::IOErrorEnum::NotFound)
            || error.matches(gio::IOErrorEnum::Closed)
            || error.matches(gio::IOErrorEnum::ConnectionRefused)
        {
            PickError::Unavailable(detail)
        } else {
            PickError::Refused(detail)
        }
    }
}

impl std::fmt::Display for PickError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PickError::Cancelled => f.write_str(&tr("The file chooser was closed.")),
            PickError::Failed(message) => f.write_str(message),
            PickError::Unavailable(_) => f.write_str(&tr("No file chooser is available on this desktop.")),
            PickError::Refused(_) => f.write_str(&tr("The desktop's file chooser would not open.")),
            PickError::NoAnswer => f.write_str(&tr("The desktop's file chooser did not answer.")),
            PickError::Lost => f.write_str(&tr("The desktop's file chooser stopped while it was open.")),
        }
    }
}

impl std::error::Error for PickError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PickError::Unavailable(detail) | PickError::Refused(detail) => Some(detail),
            _ => None,
        }
    }
}

/// What a dialog answers: the files chosen (one, unless several were asked for), or why there are none.
pub type Picked = Result<Vec<PathBuf>, PickError>;

/// What runs with the answer, once, on the main loop.
pub type PickDone = Box<dyn FnOnce(Picked)>;

/// A way to ask the user for files. The app's code takes `&dyn FilePicker`, so a test can hand it a
/// [`FakeFilePicker`] where the app has a [`PortalFilePicker`].
pub trait FilePicker {
    /// Asks for a file to open (or several, or a folder, as `options` say). `done` runs once with the answer.
    fn open(&self, options: PickOptions, done: PickDone);
    /// Asks where to save. `done` runs once with the one path chosen; the dialog has asked before replacing a file.
    fn save(&self, options: PickOptions, done: PickDone);
}

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const CHOOSER: &str = "org.freedesktop.portal.FileChooser";
const REQUEST: &str = "org.freedesktop.portal.Request";

/// How long the portal has to take a request. The dialog itself may stay open for as long as the user likes; this
/// is only the portal saying it has the request, which a healthy one does at once.
const TIMEOUT: Duration = Duration::from_secs(10);

/// The file dialogs of the desktop's portal. One dialog at a time: asking again while one is open closes it, as far
/// as the portal still listens, and answers it [`PickError::Cancelled`].
///
/// The dialog is not tied to the app's window: that takes the window's Wayland handle, which needs more than GTK's
/// own crate. A tiling desktop places it beside the window either way.
#[derive(Clone)]
pub struct PortalFilePicker {
    inner: Rc<Portal>,
}

struct Portal {
    /// A connection given by a test, to a bus of its own, in place of the session bus.
    connection: Option<gio::DBusConnection>,
    timeout: Cell<Duration>,
    /// Counted up for each request, so what belongs to a request that was replaced is told apart and dropped.
    generation: Cell<u64>,
    pending: RefCell<Option<Pending>>,
}

/// The request whose answer is awaited.
struct Pending {
    generation: u64,
    connection: gio::DBusConnection,
    /// What the portal ends the path of the request's object with.
    token: String,
    /// The request's object, once it is known or predicted.
    path: Option<String>,
    /// Responses heard for an object that is not known to be this request's, until the portal has said where the
    /// request is.
    early: Vec<(String, Picked)>,
    /// Taken when the request is answered.
    done: Option<PickDone>,
    /// Unsubscribed and disconnected when dropped: the request's response, and the portal leaving the bus.
    _response: gio::SignalSubscription,
    _owner: gio::SignalSubscription,
    closed: Option<glib::SignalHandlerId>,
}

impl Drop for Pending {
    fn drop(&mut self) {
        // Here, on the main thread, rather than whenever the connection itself goes.
        if let Some(closed) = self.closed.take() {
            self.connection.disconnect(closed);
        }
    }
}

impl Default for PortalFilePicker {
    fn default() -> Self {
        Self::new()
    }
}

impl PortalFilePicker {
    /// The portal on the session bus, reached when the first dialog is asked for.
    pub fn new() -> Self {
        Self::build(None)
    }

    /// The portal on the bus `connection` is to, in place of the session bus: for a test with a bus and a portal
    /// (whatever owns `org.freedesktop.portal.Desktop` there) of its own.
    pub fn on_connection(connection: &gio::DBusConnection) -> Self {
        Self::build(Some(connection.clone()))
    }

    fn build(connection: Option<gio::DBusConnection>) -> Self {
        Self {
            inner: Rc::new(Portal {
                connection,
                timeout: Cell::new(TIMEOUT),
                generation: Cell::new(0),
                pending: RefCell::new(None),
            }),
        }
    }

    /// How long the portal gets to take a request before the dialog is given up as failed; ten seconds unless set.
    pub fn set_timeout(&self, timeout: Duration) {
        self.inner.timeout.set(timeout);
    }

    /// Closes the dialog that is open, if one is: its `done` runs with [`PickError::Cancelled`].
    pub fn cancel(&self) {
        self.inner.finish(None, Err(PickError::Cancelled), true);
    }

    fn request(&self, method: &'static str, options: PickOptions, done: PickDone) {
        let portal = &self.inner;
        // A request whose response never arrives (the portal crashed with the dialog up) must not wedge every later
        // dialog behind it: it is closed, as far as anything still hears that, and replaced.
        portal.finish(None, Err(PickError::Cancelled), true);
        // A QA script's answer, given on the main loop as the portal's would be.
        if let Some(answer) = scripted() {
            glib::idle_add_local_once(move || done(answer));
            return;
        }
        let generation = portal.generation.get() + 1;
        portal.generation.set(generation);
        let weak = Rc::downgrade(portal);
        let given = portal.connection.clone();
        glib::spawn_future_local(async move {
            let connection = match given {
                Some(connection) => Ok(connection),
                None => gio::bus_get_future(gio::BusType::Session).await,
            };
            let Some(portal) = weak.upgrade() else { return };
            if portal.generation.get() != generation {
                // Asked again while the bus was being reached: this one was replaced before it began.
                return done(Err(PickError::Cancelled));
            }
            match connection {
                Ok(connection) => portal.send(&connection, generation, method, &options, done),
                // No session bus to reach (none at all, or one GIO cannot start): whatever GIO calls the error, there
                // is no portal to ask.
                Err(error) => done(Err(PickError::Unavailable(PickDetail::new(error.message())))),
            }
        });
    }
}

impl FilePicker for PortalFilePicker {
    fn open(&self, options: PickOptions, done: PickDone) {
        self.request("OpenFile", options, done);
    }

    fn save(&self, options: PickOptions, done: PickDone) {
        self.request("SaveFile", options, done);
    }
}

impl Portal {
    /// Ends the pending request with `answer`, when there is one and it is `generation`'s (or whichever it is, for
    /// none). With `close` the portal is told to close its dialog: when it is the app that ends the request.
    fn finish(&self, generation: Option<u64>, answer: Picked, close: bool) {
        let pending = {
            let mut pending = self.pending.borrow_mut();
            if generation.is_some_and(|generation| pending.as_ref().map(|p| p.generation) != Some(generation)) {
                return;
            }
            pending.take()
        };
        let Some(mut pending) = pending else { return };
        if close && let Some(path) = &pending.path {
            // Best effort, and not waited for: there may be no one left to answer.
            pending.connection.call(
                Some(PORTAL),
                path,
                REQUEST,
                "Close",
                None,
                None,
                gio::DBusCallFlags::NO_AUTO_START,
                1000,
                gio::Cancellable::NONE,
                |_| {},
            );
        }
        let done = pending.done.take();
        // Before the app hears: it may ask for the next dialog from there.
        drop(pending);
        if let Some(done) = done {
            done(answer);
        }
    }

    fn send(
        self: &Rc<Self>,
        connection: &gio::DBusConnection,
        generation: u64,
        method: &'static str,
        options: &PickOptions,
        done: PickDone,
    ) {
        // The portal names the request's object after the caller and this token, so the response can be listened
        // for before the request is made.
        let token = format!("omui_{}", glib::random_int());
        let predicted = connection.unique_name().map(|name| {
            let sender = name.trim_start_matches(':').replace('.', "_");
            format!("{PORTAL_PATH}/request/{sender}/{token}")
        });
        // Heard at whatever path it comes from, before the request is made, and told apart by the path: a
        // response cannot arrive between the portal's taking the request and a subscription to where it put it.
        let weak = Rc::downgrade(self);
        let response = connection.subscribe_to_signal(
            Some(PORTAL),
            Some(REQUEST),
            Some("Response"),
            None,
            None,
            gio::DBusSignalFlags::NONE,
            move |signal| {
                let (path, answer) = (signal.object_path.to_owned(), picked(signal.parameters));
                // After this handler: an answer drops the subscription the handler belongs to.
                let weak = weak.clone();
                glib::idle_add_local_once(move || {
                    if let Some(portal) = weak.upgrade() {
                        portal.heard(generation, path, answer);
                    }
                });
            },
        );
        // The portal leaving the bus takes its dialog with it, and no response will follow.
        let weak = Rc::downgrade(self);
        let owner = connection.subscribe_to_signal(
            Some("org.freedesktop.DBus"),
            Some("org.freedesktop.DBus"),
            Some("NameOwnerChanged"),
            Some("/org/freedesktop/DBus"),
            Some(PORTAL),
            gio::DBusSignalFlags::NONE,
            move |_| lost(&weak, generation),
        );
        // So does the bus itself going. The connection may be used from any thread, so its handler must be one that
        // could run on any; this one hands on to the main loop, where the picker lives, and only there looks at
        // what it holds.
        let weak = std::sync::Arc::new(glib::thread_guard::ThreadGuard::new(Rc::downgrade(self)));
        let closed = connection.connect_closed(move |_, _, _| {
            let weak = weak.clone();
            glib::idle_add_once(move || lost(weak.get_ref(), generation));
        });
        self.pending.replace(Some(Pending {
            generation,
            connection: connection.clone(),
            token: token.clone(),
            path: predicted,
            early: Vec::new(),
            done: Some(done),
            _response: response,
            _owner: owner,
            closed: Some(closed),
        }));

        let parameters = glib::Variant::tuple_from_iter([
            // No parent window: see `PortalFilePicker`.
            "".to_variant(),
            options.title.to_variant(),
            chooser_options(options, &token, method == "SaveFile"),
        ]);
        let call = connection.call_future(
            Some(PORTAL),
            PORTAL_PATH,
            CHOOSER,
            method,
            Some(&parameters),
            Some(glib::VariantTy::new("(o)").expect("a valid type string")),
            gio::DBusCallFlags::NONE,
            self.timeout.get().as_millis().try_into().unwrap_or(i32::MAX),
        );
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let reply = call.await;
            let Some(portal) = weak.upgrade() else { return };
            match reply {
                // A portal that did not answer in time, is not installed, or refused.
                Err(error) => portal.finish(Some(generation), Err(PickError::from_glib(&error)), false),
                Ok(reply) => {
                    let Some(actual) = reply.child_value(0).str().map(str::to_owned) else {
                        return;
                    };
                    // Old portals put the request elsewhere than the token predicts: the response is the one
                    // from where the portal says the request is, which may have been heard already.
                    let heard = {
                        let mut pending = portal.pending.borrow_mut();
                        let Some(pending) = pending.as_mut().filter(|pending| pending.generation == generation) else {
                            return;
                        };
                        let early = std::mem::take(&mut pending.early);
                        pending.path = Some(actual.clone());
                        early.into_iter().find(|(path, _)| *path == actual)
                    };
                    if let Some((_, answer)) = heard {
                        portal.finish(Some(generation), answer, false);
                    }
                }
            }
        });
    }

    /// A response was heard from the request object at `path`, while the request `generation` was the one made.
    /// It answers that request when the object is its own: the one the portal said, or one named by its token.
    fn heard(&self, generation: u64, path: String, answer: Picked) {
        let ours = {
            let mut pending = self.pending.borrow_mut();
            let Some(pending) = pending.as_mut().filter(|pending| pending.generation == generation) else {
                return;
            };
            let ours = pending.path.as_deref() == Some(&path) || path.ends_with(&format!("/{}", pending.token));
            if !ours {
                pending.early.push((path, answer.clone()));
            }
            ours
        };
        if ours {
            self.finish(Some(generation), answer, false);
        }
    }
}

/// The portal went away with the dialog open.
fn lost(portal: &Weak<Portal>, generation: u64) {
    let portal = portal.clone();
    // After the handler that saw it: finishing drops that handler's own subscription.
    glib::idle_add_local_once(move || {
        if let Some(portal) = portal.upgrade() {
            portal.finish(Some(generation), Err(PickError::Lost), false);
        }
    });
}

/// The `a{sv}` of options the portal takes.
fn chooser_options(options: &PickOptions, token: &str, saving: bool) -> glib::Variant {
    let dict = glib::VariantDict::new(None);
    dict.insert("handle_token", token);
    dict.insert("modal", true);
    if let Some(label) = &options.accept_label {
        dict.insert("accept_label", label.as_str());
    }
    if saving {
        if let Some(name) = &options.name {
            dict.insert("current_name", name.as_str());
        }
    } else {
        dict.insert("multiple", options.multiple);
        dict.insert("directory", options.directory);
    }
    if let Some(folder) = &options.folder {
        // A path is bytes ending in a NUL, whatever its encoding.
        let mut bytes = folder.as_os_str().as_encoded_bytes().to_vec();
        bytes.push(0);
        dict.insert_value("current_folder", &glib::Variant::array_from_fixed_array(&bytes));
    }
    // Each filter is a name and its rules: 0 with a glob, 1 with a MIME type.
    type Filter = (String, Vec<(u32, String)>);
    let filters: Vec<Filter> = options
        .filters
        .iter()
        .map(|filter| {
            let globs = filter.patterns.iter().map(|pattern| (0u32, pattern.clone()));
            let mimes = filter.mime_types.iter().map(|mime| (1u32, mime.clone()));
            (filter.name.clone(), globs.chain(mimes).collect())
        })
        .collect();
    if let Some(first) = filters.first() {
        dict.insert_value("current_filter", &first.to_variant());
        dict.insert_value("filters", &filters.to_variant());
    }
    dict.end()
}

/// What a `Response` signal's `(ua{sv})` says: 0 with the files' URIs, 1 for a dialog the user closed.
fn picked(response: &glib::Variant) -> Picked {
    let ended = || PickError::Refused(PickDetail::new("the portal ended the request without files"));
    if response.n_children() < 2 {
        return Err(ended());
    }
    match response.child_value(0).get::<u32>() {
        Some(0) => {}
        Some(1) => return Err(PickError::Cancelled),
        _ => return Err(ended()),
    }
    let results = glib::VariantDict::new(Some(&response.child_value(1)));
    let uris = results.lookup::<Vec<String>>("uris").ok().flatten().unwrap_or_default();
    let paths: Vec<PathBuf> = uris.iter().filter_map(|uri| gio::File::for_uri(uri).path()).collect();
    // An answer of yes with nothing chosen, or with nothing that is a file on this machine.
    if paths.is_empty() { Err(ended()) } else { Ok(paths) }
}

#[cfg(feature = "qa")]
thread_local! {
    /// The answers a QA script gave (`picker`), for the next requests of any picker of omui's, ahead of the portal.
    static SCRIPTED: RefCell<std::collections::VecDeque<Picked>> = RefCell::default();
}

/// Has the next request of any of omui's pickers answered with `picked`, without asking the portal: QA's `picker`,
/// so that a script reaches the paths a real desktop takes (a portal missing or refusing) on a display that has none.
#[cfg(feature = "qa")]
pub(crate) fn script(picked: Picked) {
    let _ = SCRIPTED.try_with(|scripted| scripted.borrow_mut().push_back(picked));
}

/// The answer a QA script gave for the next request, if it gave one.
fn scripted() -> Option<Picked> {
    #[cfg(feature = "qa")]
    {
        SCRIPTED
            .try_with(|scripted| scripted.borrow_mut().pop_front())
            .ok()
            .flatten()
    }
    #[cfg(not(feature = "qa"))]
    {
        None
    }
}

/// A [`FilePicker`] for an app's tests: it shows nothing, remembers what it was asked, and answers each request with
/// the next answer it was given, or [`PickError::Cancelled`] when it has none left.
#[derive(Default)]
pub struct FakeFilePicker {
    answers: RefCell<std::collections::VecDeque<Picked>>,
    asked: RefCell<Vec<(bool, PickOptions)>>,
}

impl FakeFilePicker {
    pub fn new() -> Self {
        Self::default()
    }

    /// The answer to the next request that has none yet.
    pub fn answer(&self, picked: Picked) {
        self.answers.borrow_mut().push_back(picked);
    }

    /// Every request so far, in order: whether it was to save, and its options.
    pub fn asked(&self) -> Vec<(bool, PickOptions)> {
        self.asked.borrow().clone()
    }

    fn ask(&self, saving: bool, options: PickOptions, done: PickDone) {
        self.asked.borrow_mut().push((saving, options));
        let answer = scripted().or_else(|| self.answers.borrow_mut().pop_front());
        done(answer.unwrap_or(Err(PickError::Cancelled)));
    }
}

impl FilePicker for FakeFilePicker {
    fn open(&self, options: PickOptions, done: PickDone) {
        self.ask(false, options, done);
    }

    fn save(&self, options: PickOptions, done: PickDone) {
        self.ask(true, options, done);
    }
}

#[cfg(test)]
mod tests {
    use std::process::{Child, Command, Stdio};

    use super::*;
    use crate::testing;

    const INTERFACES: &str = r#"<node>
        <interface name="org.freedesktop.portal.FileChooser">
          <method name="OpenFile">
            <arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="a{sv}" direction="in"/>
            <arg type="o" direction="out"/>
          </method>
          <method name="SaveFile">
            <arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="a{sv}" direction="in"/>
            <arg type="o" direction="out"/>
          </method>
        </interface>
        <interface name="org.freedesktop.portal.Request">
          <method name="Close"/>
        </interface>
      </node>"#;

    /// How the test's portal treats the next request.
    #[derive(Clone, Copy, PartialEq)]
    enum Mood {
        /// Takes the request and answers it at once, with this response and these URIs.
        Answers(u32, &'static [&'static str]),
        /// As an old portal: the request is somewhere the token does not predict, and is answered from there
        /// before the portal has said where that is.
        Elsewhere(&'static [&'static str]),
        /// Takes the request and shows its dialog, which nobody answers.
        Shows,
        /// Never says it has the request.
        Silent,
        Refuses,
    }

    /// A bus of the test's own (a `dbus-daemon` on a socket in a temporary directory) with a portal on it: nothing
    /// of the desktop's session is touched, and no variable of the environment says where it is.
    struct TestPortal {
        daemon: Child,
        /// The app's connection to the bus.
        client: gio::DBusConnection,
        /// The portal's.
        served: gio::DBusConnection,
        mood: Rc<Cell<Mood>>,
        /// Each request as the portal got it: the method, the title and the options.
        requests: Rc<RefCell<Vec<(String, String, glib::Variant)>>>,
        /// The request objects that were told to close.
        closed: Rc<RefCell<Vec<String>>>,
    }

    impl Drop for TestPortal {
        fn drop(&mut self) {
            let _ = self.daemon.kill();
            let _ = self.daemon.wait();
        }
    }

    /// A connection to the bus at `address`.
    fn connect(address: &str) -> gio::DBusConnection {
        let flags = gio::DBusConnectionFlags::AUTHENTICATION_CLIENT | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION;
        let connecting = gio::DBusConnection::for_address_future(address, flags, None);
        let connection = Rc::new(RefCell::new(None));
        glib::spawn_future_local({
            let connection = connection.clone();
            async move {
                connection.replace(Some(connecting.await.expect("the test's bus takes connections")));
            }
        });
        assert!(testing::spin_until(5000, || connection.borrow().is_some()));
        let connection = connection.take().unwrap();
        // The test ends the bus itself; the process is not to end with it.
        connection.set_exit_on_close(false);
        connection
    }

    impl TestPortal {
        fn start(dir: &std::path::Path) -> Self {
            let socket = dir.join("bus");
            let address = format!("unix:path={}", socket.display());
            // The tool is part of what the tests need: without it this fails, and does not pass by skipping.
            let daemon = Command::new("dbus-daemon")
                .args(["--session", "--nofork", "--nopidfile", "--address", &address])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("dbus-daemon, for a bus of the test's own");
            assert!(testing::spin_until(5000, || socket.exists()), "the bus is up");
            let (client, served) = (connect(&address), connect(&address));
            let mood = Rc::new(Cell::new(Mood::Shows));
            let requests = Rc::new(RefCell::new(Vec::new()));
            let closed = Rc::new(RefCell::new(Vec::new()));
            let node = gio::DBusNodeInfo::for_xml(INTERFACES).unwrap();
            let (chooser, request) = (
                node.lookup_interface(CHOOSER).unwrap(),
                node.lookup_interface(REQUEST).unwrap(),
            );
            served
                .register_object(PORTAL_PATH, &chooser)
                .method_call({
                    let (mood, requests, closed) = (mood.clone(), requests.clone(), closed.clone());
                    move |connection, sender, _, _, method, parameters, invocation| {
                        let title = parameters.child_value(1).str().unwrap().to_owned();
                        let options = parameters.child_value(2);
                        let token: String = glib::VariantDict::new(Some(&options))
                            .lookup("handle_token")
                            .unwrap()
                            .unwrap();
                        requests.borrow_mut().push((method.to_owned(), title, options));
                        // Where a portal puts a request: under the caller's name and the token it gave.
                        let caller = sender.unwrap().trim_start_matches(':').replace('.', "_");
                        let path = match mood.get() {
                            Mood::Elsewhere(_) => format!("{PORTAL_PATH}/request/old/{}", requests.borrow().len()),
                            _ => format!("{PORTAL_PATH}/request/{caller}/{token}"),
                        };
                        let closed = closed.clone();
                        connection
                            .register_object(&path, &request)
                            .method_call(move |_, _, path, _, _, _, invocation| {
                                closed.borrow_mut().push(path.to_owned());
                                invocation.return_value(None);
                            })
                            .build()
                            .unwrap();
                        let object = glib::variant::ObjectPath::try_from(path.clone()).unwrap();
                        let handle = glib::Variant::tuple_from_iter([object.to_variant()]);
                        let respond = |response: u32, uris: &[&str]| {
                            let results = glib::VariantDict::new(None);
                            results.insert("uris", uris.iter().map(|uri| (*uri).to_owned()).collect::<Vec<_>>());
                            let response = glib::Variant::tuple_from_iter([response.to_variant(), results.end()]);
                            connection
                                .emit_signal(sender, &path, REQUEST, "Response", Some(&response))
                                .unwrap();
                        };
                        match mood.get() {
                            Mood::Silent => {
                                // Kept, never answered.
                                std::mem::forget(invocation);
                            }
                            Mood::Refuses => invocation.return_error(gio::IOErrorEnum::NotSupported, "no backend"),
                            Mood::Shows => invocation.return_value(Some(&handle)),
                            Mood::Answers(response, uris) => {
                                invocation.return_value(Some(&handle));
                                respond(response, uris);
                            }
                            Mood::Elsewhere(uris) => {
                                respond(0, uris);
                                invocation.return_value(Some(&handle));
                            }
                        }
                    }
                })
                .build()
                .unwrap();
            let owned = Rc::new(Cell::new(false));
            let request = served.call_future(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "RequestName",
                Some(&(PORTAL, 0u32).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                -1,
            );
            glib::spawn_future_local({
                let owned = owned.clone();
                async move {
                    request
                        .await
                        .expect("the portal's name is free on a bus of the test's own");
                    owned.set(true);
                }
            });
            assert!(testing::spin_until(5000, || owned.get()));
            Self {
                daemon,
                client,
                served,
                mood,
                requests,
                closed,
            }
        }
    }

    /// Where a request's answer lands.
    fn answer() -> (Rc<RefCell<Option<Picked>>>, PickDone) {
        let got = Rc::new(RefCell::new(None));
        let done = {
            let got = got.clone();
            Box::new(move |picked| {
                assert!(got.replace(Some(picked)).is_none(), "a request is answered once");
            })
        };
        (got, done)
    }

    #[test]
    fn the_portal_is_asked_and_its_answer_is_the_files() {
        let _main = testing::main_loop();
        let dir = testing::temp_dir("picker");
        let portal = TestPortal::start(&dir);
        let picker = PortalFilePicker::on_connection(&portal.client);

        portal
            .mood
            .set(Mood::Answers(0, &["file:///tmp/a%20photo.png", "file:///tmp/b.jpg"]));
        let (got, done) = answer();
        let options = PickOptions {
            accept_label: Some("Import".to_owned()),
            multiple: true,
            folder: Some(PathBuf::from("/home/ada/Pictures")),
            filters: vec![
                FileFilter::new("Images", &["*.png", "*.jpg"]).mime_types(&["image/*"]),
                FileFilter::new("All files", &["*"]),
            ],
            ..PickOptions::new("Import photos")
        };
        picker.open(options, done);
        assert!(testing::spin_until(5000, || got.borrow().is_some()));
        assert_eq!(
            got.take().unwrap(),
            Ok(vec![PathBuf::from("/tmp/a photo.png"), PathBuf::from("/tmp/b.jpg")])
        );
        assert!(picker.inner.pending.borrow().is_none());
        {
            let requests = portal.requests.borrow();
            let (method, title, sent) = &requests[0];
            assert_eq!((method.as_str(), title.as_str()), ("OpenFile", "Import photos"));
            let sent = glib::VariantDict::new(Some(sent));
            assert_eq!(
                sent.lookup::<String>("accept_label").unwrap().as_deref(),
                Some("Import")
            );
            assert_eq!(sent.lookup::<bool>("multiple").unwrap(), Some(true));
            assert_eq!(sent.lookup::<bool>("directory").unwrap(), Some(false));
            assert_eq!(
                sent.lookup::<Vec<u8>>("current_folder").unwrap().as_deref(),
                Some(&b"/home/ada/Pictures\0"[..])
            );
            type Filter = (String, Vec<(u32, String)>);
            let filters = sent.lookup::<Vec<Filter>>("filters").unwrap().unwrap();
            assert_eq!(filters.len(), 2);
            assert_eq!(
                filters[0],
                (
                    "Images".to_owned(),
                    vec![
                        (0, "*.png".to_owned()),
                        (0, "*.jpg".to_owned()),
                        (1, "image/*".to_owned())
                    ]
                )
            );
            assert_eq!(
                sent.lookup::<Filter>("current_filter").unwrap(),
                Some(filters[0].clone())
            );
        }

        // Saving: one path, with the name suggested and no word about how many.
        portal.mood.set(Mood::Answers(0, &["file:///tmp/out.mp4"]));
        let (got, done) = answer();
        let options = PickOptions {
            name: Some("out.mp4".to_owned()),
            ..PickOptions::new("Export")
        };
        picker.save(options, done);
        assert!(testing::spin_until(5000, || got.borrow().is_some()));
        assert_eq!(got.take().unwrap(), Ok(vec![PathBuf::from("/tmp/out.mp4")]));
        {
            let requests = portal.requests.borrow();
            let (method, _, sent) = &requests[1];
            let sent = glib::VariantDict::new(Some(sent));
            assert_eq!(method, "SaveFile");
            assert_eq!(
                sent.lookup::<String>("current_name").unwrap().as_deref(),
                Some("out.mp4")
            );
            assert!(!sent.contains("multiple") && !sent.contains("filters"));
        }

        // An old portal puts the request where the token does not say, and answers before saying where.
        portal.mood.set(Mood::Elsewhere(&["file:///tmp/old.txt"]));
        let (got, done) = answer();
        picker.open(PickOptions::new("Open"), done);
        assert!(testing::spin_until(5000, || got.borrow().is_some()));
        assert_eq!(got.take().unwrap(), Ok(vec![PathBuf::from("/tmp/old.txt")]));

        // The user closes the dialog; and a dialog that says yes with nothing chosen is no answer.
        for (mood, cancelled) in [
            (Mood::Answers(1, &[]), true),
            (Mood::Answers(2, &[]), false),
            (Mood::Answers(0, &[]), false),
        ] {
            portal.mood.set(mood);
            let (got, done) = answer();
            picker.open(PickOptions::new("Open"), done);
            assert!(testing::spin_until(5000, || got.borrow().is_some()));
            let error = got.take().unwrap().unwrap_err();
            assert_eq!(error == PickError::Cancelled, cancelled, "{error:?}");
        }
        testing::spin_for(100);
        assert!(
            portal.closed.borrow().is_empty(),
            "a request the portal answered is not told to close"
        );
        drop(portal);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_portal_that_does_not_answer_ends_in_an_error_not_a_wait() {
        let _main = testing::main_loop();
        let dir = testing::temp_dir("picker-silent");
        let portal = TestPortal::start(&dir);
        let picker = PortalFilePicker::on_connection(&portal.client);

        // The portal refuses: no backend for file dialogs.
        portal.mood.set(Mood::Refuses);
        let (got, done) = answer();
        picker.open(PickOptions::new("Open"), done);
        assert!(testing::spin_until(5000, || got.borrow().is_some()));
        let refused = got.take().unwrap().unwrap_err();
        assert_eq!(refused.to_string(), "The desktop's file chooser would not open.");
        assert!(
            refused.detail().is_some_and(|detail| detail.contains("no backend")),
            "{refused:?}"
        );
        let source = std::error::Error::source(&refused).map(ToString::to_string);
        assert!(
            source.is_some_and(|source| source.contains("no backend")),
            "the cause, for a log"
        );

        // The portal never says it has the request: given up after the timeout.
        portal.mood.set(Mood::Silent);
        picker.set_timeout(Duration::from_millis(400));
        let (got, done) = answer();
        picker.open(PickOptions::new("Open"), done);
        testing::spin_for(100);
        assert!(got.borrow().is_none(), "not before its time");
        assert!(testing::spin_until(5000, || got.borrow().is_some()));
        let silent = got.take().unwrap().unwrap_err();
        assert_eq!(
            (&silent, silent.to_string().as_str()),
            (&PickError::NoAnswer, "The desktop's file chooser did not answer.")
        );
        assert!(picker.inner.pending.borrow().is_none());
        picker.set_timeout(TIMEOUT);

        // The dialog is up and nobody answers it. The app asks again: the first is closed and answered as
        // cancelled, and the second is the one whose answer counts.
        portal.mood.set(Mood::Shows);
        let (first, done) = answer();
        picker.open(PickOptions::new("First"), done);
        assert!(testing::spin_until(5000, || portal.requests.borrow().len() == 3));
        portal.mood.set(Mood::Answers(0, &["file:///tmp/second.txt"]));
        let (second, done) = answer();
        picker.open(PickOptions::new("Second"), done);
        assert_eq!(first.take(), Some(Err(PickError::Cancelled)), "at once");
        assert!(testing::spin_until(5000, || {
            second.borrow().is_some() && portal.closed.borrow().len() == 1
        }));
        assert_eq!(second.take().unwrap(), Ok(vec![PathBuf::from("/tmp/second.txt")]));
        assert!(
            portal.closed.borrow()[0].contains("/omui_"),
            "the first request's own object"
        );

        // The app gives up a dialog itself.
        portal.mood.set(Mood::Shows);
        let (got, done) = answer();
        picker.save(PickOptions::new("Save"), done);
        assert!(testing::spin_until(5000, || portal.requests.borrow().len() == 5));
        picker.cancel();
        assert_eq!(got.take(), Some(Err(PickError::Cancelled)));
        picker.cancel();
        assert!(testing::spin_until(5000, || portal.closed.borrow().len() == 2));

        // The portal goes away with its dialog open: an error, where the response would never have come.
        let (got, done) = answer();
        picker.open(PickOptions::new("Open"), done);
        assert!(testing::spin_until(5000, || portal.requests.borrow().len() == 6));
        assert!(got.borrow().is_none());
        portal.served.close(gio::Cancellable::NONE, |_| {});
        assert!(testing::spin_until(5000, || got.borrow().is_some()));
        let lost = got.take().unwrap().unwrap_err();
        assert_eq!(
            (&lost, lost.to_string().as_str()),
            (
                &PickError::Lost,
                "The desktop's file chooser stopped while it was open."
            )
        );
        // And with no portal left to ask, the next request fails too rather than waiting.
        let (got, done) = answer();
        picker.open(PickOptions::new("Open"), done);
        assert!(testing::spin_until(5000, || got.borrow().is_some()));
        // The bus may tell this request too that the portal left, if it says so late.
        let missing = got.take().unwrap().unwrap_err();
        match &missing {
            PickError::Unavailable(_) => {
                assert_eq!(missing.to_string(), "No file chooser is available on this desktop.");
                assert!(missing.detail().is_some(), "GIO's words, for the log");
            }
            PickError::Lost => {}
            _ => panic!("{missing:?}"),
        }
        drop(portal);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// What a QA script's `picker` gives: the next answer of any picker, ahead of its own.
    #[cfg(feature = "qa")]
    #[test]
    fn a_qa_script_answers_ahead_of_the_picker() {
        let fake = FakeFilePicker::new();
        fake.answer(Ok(vec![PathBuf::from("/tmp/own.txt")]));
        script(Err(PickError::Unavailable(PickDetail::new("no portal"))));
        let (got, done) = answer();
        fake.open(PickOptions::new("Open"), done);
        let error = got.take().unwrap().unwrap_err();
        assert_eq!(error.to_string(), "No file chooser is available on this desktop.");
        assert_eq!(error.detail(), Some("no portal"));
        let (got, done) = answer();
        fake.open(PickOptions::new("Open"), done);
        assert_eq!(
            got.take(),
            Some(Ok(vec![PathBuf::from("/tmp/own.txt")])),
            "then its own"
        );
        assert_eq!(fake.asked().len(), 2, "both asked");
    }

    #[test]
    fn a_fake_answers_what_it_was_given_and_remembers_what_it_was_asked() {
        let fake = FakeFilePicker::new();
        fake.answer(Ok(vec![PathBuf::from("/tmp/in.txt")]));
        // What an app holds: the trait, not the portal.
        let picker: &dyn FilePicker = &fake;
        let (got, done) = answer();
        picker.open(PickOptions::new("Open"), done);
        assert_eq!(got.take(), Some(Ok(vec![PathBuf::from("/tmp/in.txt")])));
        let (got, done) = answer();
        picker.save(PickOptions::new("Save as"), done);
        assert_eq!(
            got.take(),
            Some(Err(PickError::Cancelled)),
            "with no answer left, the dialog was closed"
        );
        let asked = fake.asked();
        assert_eq!((asked[0].0, asked[0].1.title.as_str()), (false, "Open"));
        assert_eq!((asked[1].0, asked[1].1.title.as_str()), (true, "Save as"));
    }
}
