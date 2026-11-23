// SPDX-License-Identifier: MIT
//! The language: Omarchy's, picked under Setup > Region > Language, followed live as the theme is.
//!
//! The English text is the key. An app writes `tr("Save")`, and embeds one catalog per language: a flat JSON object
//! from the English to its translation, named by Omarchy's language code (`i18n/da.json`, `i18n/pt-BR.json`), with
//! `en.json` listing every string the app translates, each as itself. A string no catalog has shows in English.
//!
//! ```ignore
//! omui::i18n::register_all(&[
//!     ("en", include_str!("i18n/en.json")),
//!     ("da", include_str!("i18n/da.json")),
//! ]);
//! let title = omui::tr("Save");
//! let moved = omui::tr!("Moved {} to {}", name, folder);
//! let count = omui::trn("{n} clips", clips.len() as i64);
//! omui::i18n::bind_label(&button, "Save"); // and again in the new language after a switch
//! ```
//!
//! **Placeholders** are Rust's: `{}` takes the next argument, `{0}` and `{1}` name one by its place, so a translation
//! can put them in another order, and `{{` is a brace. `{n}` is the count of a counted string, written with the
//! language's digit grouping. Only [`trf`], [`trn`], [`trnf`] and the macros fill them; [`tr`] returns the text as it
//! is.
//!
//! **A counted string** is an object of plural forms by CLDR category: `"{n} clips": {"one": "{n} clip", "other":
//! "{n} clips"}` in `en.json`, and in each language the categories it uses (Russian `one`, `few`, `many`, `other`;
//! Japanese `other` alone), always with `other`. The forms are picked by the rules of the catalog that supplied
//! them, so an English fallback in Russian takes English's.
//!
//! **Fallback** is per string: `pt-BR`, then `pt`, then English.
//!
//! [`Translator`] reads `~/.config/omarchy/language` and watches it; [`crate::application`] installs it. On a
//! switch the catalogs are swapped, every window is turned to read right to left or left to right, and what was
//! registered with [`on_change`], [`bind`] and its kin runs. A string an app translated once and kept (a status line)
//! stays in the old language: build it again in [`on_change`], or keep the English and translate where it is shown.
//!
//! [`CatalogCheck`] is the test every app keeps: its `en.json` lists exactly the strings its sources translate.

mod check;

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    fmt::Display,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, LazyLock, RwLock, RwLockReadGuard},
    time::Duration,
};

use gtk::{gio, glib, prelude::*};

pub use check::CatalogCheck;

use crate::json;

/// omui's own strings: the few its widgets say themselves.
const OMUI_ENGLISH: &str = include_str!("i18n/en.json");

/// One string of a catalog: its translation, or the plural forms of a counted one by CLDR category.
#[derive(Clone, Debug, PartialEq)]
enum Entry {
    Text(String),
    Forms(Vec<(String, String)>),
}

/// The entries of one catalog by their English text.
type Catalog = HashMap<String, Entry>;

/// The catalogs registered, by language code, in the order they came: a later one for the same language adds to the
/// earlier and wins where both have a string, so an app can say one of omui's own strings differently.
#[derive(Default)]
struct Catalogs {
    by_code: Vec<(String, Catalog)>,
}

/// What a language's strings resolve to: each English text to its entry and the language whose catalog supplied it.
type Table = HashMap<String, (Entry, Arc<str>)>;

impl Catalogs {
    /// Adds a catalog's strings. A member that is neither text nor an object of forms, or is empty, is left out, so
    /// the string falls back.
    fn register(&mut self, code: &str, source: &str) -> Result<(), String> {
        let value = json::parse(source)?;
        let json::Value::Object(members) = value else {
            return Err("a catalog is a JSON object".to_owned());
        };
        let mut catalog = Catalog::new();
        for (text, value) in members {
            let entry = match value {
                json::Value::String(translation) if !translation.is_empty() => Entry::Text(translation),
                json::Value::Object(forms) => Entry::Forms(
                    forms
                        .into_iter()
                        .filter_map(|(category, form)| match form {
                            json::Value::String(form) if !form.is_empty() => Some((category, form)),
                            _ => None,
                        })
                        .collect(),
                ),
                _ => continue,
            };
            catalog.insert(text, entry);
        }
        self.by_code.push((code.to_owned(), catalog));
        Ok(())
    }

    /// English first (it carries the plural forms of counted strings), then the base language and the regional
    /// catalog over it (`pt`, then `pt-BR`), so a missing string falls back toward English one step at a time.
    fn table(&self, language: &str) -> Table {
        let base = language.split('-').next().unwrap_or(language);
        let mut codes = vec!["en"];
        for code in [base, language] {
            if !codes.contains(&code) {
                codes.push(code);
            }
        }
        let mut table = Table::new();
        for code in codes {
            let origin: Arc<str> = Arc::from(code);
            for (_, catalog) in self.by_code.iter().filter(|(of, _)| of == code) {
                for (text, entry) in catalog {
                    table.insert(text.clone(), (entry.clone(), origin.clone()));
                }
            }
        }
        table
    }
}

/// The text for `text` in `table`, with the form for `count` when it is a counted string. English when the table
/// has none.
fn resolve(table: &Table, text: &str, count: Option<i64>) -> String {
    match table.get(text) {
        Some((Entry::Text(translation), _)) => translation.clone(),
        Some((Entry::Forms(forms), origin)) => {
            let category = count.map_or("other", |count| plural_category(origin, count));
            let form = |wanted: &str| forms.iter().find(|(category, _)| category == wanted);
            form(category)
                .or_else(|| form("other"))
                .map_or_else(|| text.to_owned(), |(_, form)| form.clone())
        }
        None => text.to_owned(),
    }
}

/// How a language writes a number: what stands for the decimal point, and what sets off each three digits.
#[derive(Clone, Debug, PartialEq)]
struct Numbers {
    decimal: String,
    group: String,
}

impl Numbers {
    fn english() -> Self {
        Self {
            decimal: ".".to_owned(),
            group: ",".to_owned(),
        }
    }
}

struct State {
    language: String,
    catalogs: Catalogs,
    table: Table,
    /// Omarchy's registry of languages (`$OMARCHY_PATH/default/i18n/locales.json`), when there is one.
    registry: Option<PathBuf>,
    /// The system locale Omarchy formats this language with (`da_DK.UTF-8`), when it is known.
    locale: Option<String>,
    numbers: Numbers,
    rtl: bool,
}

static STATE: LazyLock<RwLock<State>> = LazyLock::new(|| {
    let mut catalogs = Catalogs::default();
    catalogs
        .register("en", OMUI_ENGLISH)
        .expect("omui's own catalog is a JSON object");
    let table = catalogs.table("en");
    RwLock::new(State {
        language: "en".to_owned(),
        catalogs,
        table,
        registry: None,
        locale: None,
        numbers: Numbers::english(),
        rtl: false,
    })
});

/// Translating also happens on worker threads (an error built where it occurred), while a switch replaces the table
/// on the main one. A thread that panicked holding the lock has left the table whole: it is swapped in one step.
fn state() -> RwLockReadGuard<'static, State> {
    STATE.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn change(f: impl FnOnce(&mut State)) {
    f(&mut STATE.write().unwrap_or_else(|poisoned| poisoned.into_inner()));
}

/// Adds a catalog for the language `code` (Omarchy's: `da`, `pt-BR`, `zh-CN`), a JSON object from English text to
/// its translation or, for a counted string, to its plural forms. An app registers its catalogs first thing in
/// `main`, `en.json` among them: English carries the plural forms of its counted strings. A catalog that is not
/// JSON is reported on stderr and left out; [`CatalogCheck`] is what catches it before a release.
pub fn register(code: &str, catalog_json: &str) {
    change(|state| match state.catalogs.register(code, catalog_json) {
        Ok(()) => state.table = state.catalogs.table(&state.language),
        Err(error) => glib::g_warning!("omui", "the {code} catalog is left out: {error}"),
    });
}

/// A made-up catalog for `code`, for QA (`language` with `made_up`): every English string registered so far, the
/// app's and omui's, as `[da] Save`, each form of a counted one too, placeholders and mnemonics kept. A script then
/// sees every text that follows the language take its new words, with no translation written.
#[cfg(feature = "qa")]
pub(crate) fn register_made_up(code: &str) {
    change(|state| {
        let mark = |text: &str| format!("[{code}] {text}");
        let mut made = Catalog::new();
        for (_, catalog) in state.catalogs.by_code.iter().filter(|(of, _)| of == "en") {
            for (text, entry) in catalog {
                let entry = match entry {
                    Entry::Text(_) => Entry::Text(mark(text)),
                    Entry::Forms(forms) => Entry::Forms(
                        forms
                            .iter()
                            .map(|(category, form)| (category.clone(), mark(form)))
                            .collect(),
                    ),
                };
                made.insert(text.clone(), entry);
            }
        }
        state.catalogs.by_code.push((code.to_owned(), made));
        state.table = state.catalogs.table(&state.language);
    });
}

/// [`register`] for each `(code, catalog)`: `register_all(&[("en", include_str!("i18n/en.json")), …])`.
pub fn register_all(catalogs: &[(&str, &str)]) {
    for (code, catalog) in catalogs {
        register(code, catalog);
    }
}

/// The language in use, as Omarchy's code: `en` until [`Translator`] or [`set_language`] says otherwise.
pub fn language() -> String {
    state().language.clone()
}

/// Whether the language reads right to left: Arabic, Hebrew, Persian, Urdu.
pub fn is_rtl() -> bool {
    state().rtl
}

/// `text` in the language in use, for a text that is not written in the source: one read from a file or built at
/// run time. A literal goes through [`tr`], which [`CatalogCheck`] can check against the catalogs.
pub fn translate(text: &str) -> String {
    resolve(&state().table, text, None)
}

/// `text` in the language in use, as it is: English when no catalog has it. `text` is a string literal, so that
/// [`CatalogCheck`] finds it.
pub fn tr(text: &str) -> String {
    translate(text)
}

/// [`tr`] with its placeholders filled: `trf("Moved {} to {}", &[&name, &folder])`. `{}` takes the next of `args`
/// and `{0}`, `{1}` the one at that place, so a translation may reorder them; `{{` and `}}` are braces. The macro
/// [`crate::tr!`] spares the slice.
pub fn trf(text: &str, args: &[&dyn Display]) -> String {
    fill(&translate(text), None, args)
}

/// A counted string in the form the language has for `count`, with `{n}` as the count in the language's digit
/// grouping: `trn("{n} clips", 3)`. `text` is the English for several, which is also its key in the catalogs.
pub fn trn(text: &str, count: i64) -> String {
    fill_translated(text, Some(count), &[])
}

/// [`trn`] with other placeholders beside `{n}`: `trnf("{n} clips in {}", 3, &[&folder])`.
pub fn trnf(text: &str, count: i64, args: &[&dyn Display]) -> String {
    let form = resolve(&state().table, text, Some(count));
    fill(&form, Some(&format_int(count)), args)
}

/// [`tr`], or with arguments [`trf`]: `tr!("Save")`, `tr!("Moved {} to {}", name, folder)`.
#[macro_export]
macro_rules! tr {
    ($text:expr $(,)?) => {
        $crate::i18n::translate($text)
    };
    ($text:expr, $($arg:expr),+ $(,)?) => {
        $crate::i18n::fill_translated($text, ::core::option::Option::None, &[$(&$arg as &dyn ::std::fmt::Display),+])
    };
}

/// [`trn`], or with more arguments [`trnf`]: `trn!("{n} clips", clips.len())`, `trn!("{n} clips in {}", n, folder)`.
/// The count is any integer.
#[macro_export]
macro_rules! trn {
    ($text:expr, $count:expr $(,)?) => {
        $crate::i18n::fill_translated($text, ::core::option::Option::Some(($count) as i64), &[])
    };
    ($text:expr, $count:expr, $($arg:expr),+ $(,)?) => {
        $crate::i18n::fill_translated(
            $text,
            ::core::option::Option::Some(($count) as i64),
            &[$(&$arg as &dyn ::std::fmt::Display),+],
        )
    };
}

/// What the macros expand to.
#[doc(hidden)]
pub fn fill_translated(text: &str, count: Option<i64>, args: &[&dyn Display]) -> String {
    let form = resolve(&state().table, text, count);
    fill(&form, count.map(format_int).as_deref(), args)
}

/// Fills the placeholders of `template`. One that names nothing given (`{3}` of two arguments, `{n}` without a
/// count, `{name}`) stays as it is written: a mistake that shows is better than a panic in a translated string.
fn fill(template: &str, count: Option<&str>, args: &[&dyn Display]) -> String {
    let mut filled = String::with_capacity(template.len());
    let mut next = 0;
    let mut rest = template;
    while let Some(at) = rest.find(['{', '}']) {
        filled.push_str(&rest[..at]);
        let brace = &rest[at..=at];
        rest = &rest[at + 1..];
        if rest.starts_with(brace) {
            filled.push_str(brace);
            rest = &rest[1..];
            continue;
        }
        let name = (brace == "{").then(|| rest.find('}')).flatten().map(|end| &rest[..end]);
        let value = match name {
            Some("") => {
                next += 1;
                args.get(next - 1).map(|arg| arg.to_string())
            }
            Some("n") => count.map(str::to_owned),
            Some(index) => index
                .parse::<usize>()
                .ok()
                .and_then(|index| args.get(index))
                .map(|arg| arg.to_string()),
            None => None,
        };
        match (name, value) {
            (Some(name), Some(value)) => {
                filled.push_str(&value);
                rest = &rest[name.len() + 1..];
            }
            _ => filled.push_str(brace),
        }
    }
    filled.push_str(rest);
    filled
}

/// The placeholders of a text, sorted: `0`, `1`, … for the arguments (a bare `{}` counted from the first) and `n` for
/// the count. Two texts with the same list take the same arguments.
fn placeholders(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut next = 0;
    let mut rest = text;
    while let Some(at) = rest.find(['{', '}']) {
        let brace = &rest[at..=at];
        rest = &rest[at + 1..];
        if rest.starts_with(brace) {
            rest = &rest[1..];
        } else if brace == "{"
            && let Some(end) = rest.find('}')
        {
            let name = &rest[..end];
            if name.is_empty() {
                found.push(next.to_string());
                next += 1;
            } else if name == "n" || name.parse::<usize>().is_ok() {
                found.push(name.to_owned());
            }
            rest = &rest[end + 1..];
        }
    }
    found.sort();
    found
}

/// The CLDR cardinal plural category of a whole `count` in `language`, for Omarchy's languages (the rules of
/// Omarchy's `shell/Commons/Plural.js` and of the app template's `Translator::pluralCategory`). A language it does
/// not know counts as English does.
pub fn plural_category(language: &str, count: i64) -> &'static str {
    let n = count.unsigned_abs();
    let (n10, n100) = (n % 10, n % 100);
    let millions = n != 0 && n.is_multiple_of(1_000_000);
    let teens = (12..=14).contains(&n100);
    let one_few_many = |other: &'static str| {
        if n10 == 1 && n100 != 11 {
            "one"
        } else if (2..=4).contains(&n10) && !teens {
            "few"
        } else {
            other
        }
    };
    match language {
        "ja" | "ko" | "zh-CN" | "zh-TW" | "th" | "vi" | "id" | "ms" => "other",
        "hi" | "bn" | "fa" | "si" if n <= 1 => "one",
        "hi" | "bn" | "fa" | "si" => "other",
        "fr" | "pt-BR" if n <= 1 => "one",
        "ca" | "es-MX" | "it" | "pt" if n == 1 => "one",
        "fr" | "pt-BR" | "ca" | "es-MX" | "it" | "pt" if millions => "many",
        "fr" | "pt-BR" | "ca" | "es-MX" | "it" | "pt" => "other",
        "fil" if matches!(n10, 4 | 6 | 9) => "other",
        "fil" => "one",
        "is" if n10 == 1 && n100 != 11 => "one",
        "is" => "other",
        "cs" | "sk" if n == 1 => "one",
        "cs" | "sk" if (2..=4).contains(&n) => "few",
        "cs" | "sk" => "other",
        "pl" if n == 1 => "one",
        "pl" if (2..=4).contains(&n10) && !teens => "few",
        "pl" => "many",
        "ru" | "uk" => one_few_many("many"),
        "hr" | "sr" => one_few_many("other"),
        "sl" => match n100 {
            1 => "one",
            2 => "two",
            3 | 4 => "few",
            _ => "other",
        },
        "lt" if (11..=19).contains(&n100) => "other",
        "lt" if n10 == 1 => "one",
        "lt" if n10 >= 2 => "few",
        "lt" => "other",
        "lv" if n10 == 0 || (11..=19).contains(&n100) => "zero",
        "lv" if n10 == 1 => "one",
        "lv" => "other",
        "ro" if n == 1 => "one",
        "ro" if n == 0 || (1..=19).contains(&n100) => "few",
        "ro" => "other",
        "ar" => match (n, n100) {
            (0, _) => "zero",
            (1, _) => "one",
            (2, _) => "two",
            (_, 3..=10) => "few",
            (_, 11..) => "many",
            _ => "other",
        },
        "he" | "ga" if n == 1 => "one",
        "he" | "ga" if n == 2 => "two",
        "ga" if (3..=6).contains(&n) => "few",
        "ga" if (7..=10).contains(&n) => "many",
        "he" | "ga" => "other",
        _ if n == 1 => "one",
        _ => "other",
    }
}

/// One of Omarchy's codes, as its language file's first line must be: `da`, `fil`, `pt-BR`. Anything else is
/// English.
fn valid_code(code: &str) -> &str {
    let (base, region) = code
        .split_once('-')
        .map_or((code, None), |(base, region)| (base, Some(region)));
    let lower = (2..=3).contains(&base.len()) && base.bytes().all(|b| b.is_ascii_lowercase());
    let regional = region.is_none_or(|region| region.len() == 2 && region.bytes().all(|b| b.is_ascii_alphabetic()));
    if lower && regional { code } else { "en" }
}

/// The languages that read right to left, for a desktop whose registry does not say.
const RTL: [&str; 4] = ["ar", "he", "fa", "ur"];

impl State {
    /// Takes up `language`: its strings, and how it is formatted and which way it reads, from Omarchy's registry
    /// where there is one, so that an app's numbers and dates match the desktop's.
    fn take_up(&mut self, language: &str) {
        self.language = language.to_owned();
        self.table = self.catalogs.table(language);
        let entry = self
            .registry
            .as_deref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|registry| json::parse(&registry).ok())
            .and_then(|registry| registry.get(language).cloned());
        let said = |name: &str| entry.as_ref()?.get(name)?.as_str().map(str::to_owned);
        let base = language.split('-').next().unwrap_or(language);
        self.rtl = said("direction").map_or_else(|| RTL.contains(&base), |direction| direction == "rtl");
        self.locale = said("locale").or_else(|| (base == "en").then(|| "en_US.UTF-8".to_owned()));
        // The system's own idea of the language's separators; failing that the registry's decimal mark with no
        // grouping, which no language misreads; and English as English writes it.
        self.numbers = self
            .locale
            .as_deref()
            .and_then(libc_locale::numbers)
            .or_else(|| {
                (base != "en").then(|| Numbers {
                    decimal: said("decimal").unwrap_or_else(|| ".".to_owned()),
                    group: String::new(),
                })
            })
            .unwrap_or_else(Numbers::english);
    }
}

/// Where Omarchy's registry of languages is: it says which system locale formats each language, and which read
/// right to left. [`Translator::install`] sets it from `$OMARCHY_PATH`; a test sets a file of its own, or none.
pub fn set_locale_registry(path: Option<PathBuf>) {
    change(|state| {
        state.registry = path;
        let language = state.language.clone();
        state.take_up(&language);
    });
}

/// Changes the language to `code`, as a switch in Omarchy does through [`Translator`]: the strings, the formats,
/// the direction every window reads in, and then whatever follows the language ([`on_change`], [`bind`]). A code
/// that is not one of Omarchy's is English. On the main thread; an app has no need of it unless it offers a language
/// of its own choosing.
pub fn set_language(code: &str) {
    let code = valid_code(code.trim());
    if state().language == code {
        return;
    }
    change(|state| state.take_up(code));
    apply_direction();
    let listeners: Vec<Listener> = LISTENERS
        .try_with(|listeners| listeners.borrow().iter().map(|(_, f)| f.clone()).collect())
        .unwrap_or_default();
    for listener in listeners {
        listener();
    }
}

/// What [`lock_language`] holds: the language is this test's alone until it is dropped, and then it is put back.
#[must_use = "the language is only the test's own until this is dropped"]
pub struct LanguageLock {
    /// The language as the test found it.
    before: String,
    _turn: std::sync::MutexGuard<'static, ()>,
}

impl Drop for LanguageLock {
    fn drop(&mut self) {
        // Before the next test is let in, which finds the language as this one did.
        set_language(&self.before);
    }
}

/// The language for one test alone. The language is one for the whole process, and cargo runs tests on several
/// threads at once: a test that switches it ([`set_language`], a [`Translator`] following a file of its own) takes
/// this first, and so does a test that reads translated text and needs it in a known language. Each waits for the
/// one before to drop its lock, and finds the language as that one found it:
///
/// ```ignore
/// #[test]
/// fn the_toolbar_follows_the_language() {
///     let _language = omui::i18n::lock_language();
///     omui::i18n::register("da", r#"{"Save": "Gem"}"#);
///     omui::i18n::set_language("da");
///     assert_eq!(omui::tr("Save"), "Gem");
/// } // English again here, for the next test.
/// ```
///
/// A test that failed while it held the lock does not keep the others out. The lock is not reentrant: a test takes
/// it once.
pub fn lock_language() -> LanguageLock {
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let turn = TURN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    LanguageLock {
        before: language(),
        _turn: turn,
    }
}

/// Turns every window to read the way the language does, those open now and those opened later. Without a display
/// (a test, a command line) there is nothing to turn.
fn apply_direction() {
    if gtk::is_initialized_main_thread() {
        gtk::Widget::set_default_direction(if is_rtl() {
            gtk::TextDirection::Rtl
        } else {
            gtk::TextDirection::Ltr
        });
    }
}

type Listener = Rc<dyn Fn()>;

thread_local! {
    /// What runs after a switch, each with the id it is removed by. On the main thread, where a switch happens.
    static LISTENERS: RefCell<Vec<(u64, Listener)>> = const { RefCell::new(Vec::new()) };
    static NEXT_LISTENER: Cell<u64> = const { Cell::new(0) };
    static CURRENT: RefCell<Option<Rc<Translator>>> = const { RefCell::new(None) };
}

/// What [`connect_changed`] returns, for [`disconnect`].
#[derive(Debug, PartialEq, Eq)]
pub struct ChangeHandler(u64);

/// Runs `f` after each switch of language, until [`disconnect`]: for what is not a widget's. A widget's own text is
/// easier with [`on_change`] or [`bind`], which stop when the widget goes.
pub fn connect_changed(f: impl Fn() + 'static) -> ChangeHandler {
    // Past the thread's end (a destructor that makes a listener) the id is never used, and `f` is dropped here.
    let id = NEXT_LISTENER
        .try_with(|next| next.replace(next.get() + 1))
        .unwrap_or(u64::MAX);
    let _ = LISTENERS.try_with(|listeners| listeners.borrow_mut().push((id, Rc::new(f))));
    ChangeHandler(id)
}

/// Stops a listener of [`connect_changed`]. Safe from anywhere, a widget's destroy handler among them, at any time:
/// as the process ends, the thread's listeners are dropped with the thread, and a widget one of them held may be
/// destroyed then, after this thread's state is gone. There is nothing left to stop by then, and nothing is done.
pub fn disconnect(handler: ChangeHandler) {
    // The listener is dropped after the list is let go: what it held (a widget) may be destroyed as it goes, and its
    // destroy handler come back here.
    let removed = LISTENERS.try_with(|listeners| {
        let mut listeners = listeners.try_borrow_mut().ok()?;
        let at = listeners.iter().position(|(id, _)| *id == handler.0)?;
        Some(listeners.remove(at))
    });
    drop(removed);
}

/// Runs `f` after each switch of language for as long as `anchor` lives: where an app builds its text again. `f`
/// must not hold `anchor` strongly, or neither is freed.
pub fn on_change(anchor: &impl IsA<gtk::Widget>, f: impl Fn() + 'static) {
    let ChangeHandler(id) = connect_changed(f);
    anchor.connect_destroy(move |_| disconnect(ChangeHandler(id)));
}

/// Runs `set` on `widget` now and after each switch of language, for as long as the widget lives: whatever of it is
/// text is written there, with [`tr`], and so follows the language.
///
/// ```ignore
/// omui::i18n::bind(&status, move |status| status.set_label(&omui::trn("{n} clips", count.get())));
/// ```
pub fn bind<W: IsA<gtk::Widget>>(widget: &W, set: impl Fn(&W) + 'static) {
    set(widget);
    let weak = widget.downgrade();
    on_change(widget, move || {
        if let Some(widget) = weak.upgrade() {
            set(&widget);
        }
    });
}

/// Makes `text`, translated, the label of `widget` (a `gtk::Label`, a `gtk::Button`, anything with a `label`
/// property), now and after each switch of language. `text` is a string literal, as [`tr`]'s is.
///
/// A later call for the same widget replaces this one, and so does setting the label any other way
/// (`set_label`): the label is the newest text given, in a switch of language too.
pub fn bind_label(widget: &impl IsA<gtk::Widget>, text: &str) {
    let widget = widget.as_ref();
    if widget.find_property("label").is_none() {
        return;
    }
    let text = text.to_owned();
    let weak = widget.downgrade();
    bind_slot(widget, "label", &["label"], move || {
        if let Some(widget) = weak.upgrade() {
            widget.set_property("label", translate(&text));
        }
    });
}

/// Makes `text`, translated, the tooltip of `widget` and what a screen reader calls it, now and after each switch
/// of language: an icon button's name. `text` is a string literal, as [`tr`]'s is.
///
/// A later call for the same widget replaces this one, and so does setting the tooltip any other way
/// (`set_tooltip_text`), as with [`bind_label`].
pub fn bind_tooltip(widget: &impl IsA<gtk::Widget>, text: &str) {
    let widget = widget.as_ref();
    let text = text.to_owned();
    let weak = widget.downgrade();
    bind_slot(widget, "tooltip", &["tooltip-text", "tooltip-markup"], move || {
        if let Some(widget) = weak.upgrade() {
            let text = translate(&text);
            widget.set_tooltip_text(Some(&text));
            widget.update_property(&[gtk::accessible::Property::Label(&text)]);
        }
    });
}

/// One binding of a widget's property to the language: the listener that writes it after a switch, and the signal
/// handlers on the widget that end the binding with the widget or with the property set another way.
struct Slot {
    listener: u64,
    handlers: Vec<glib::SignalHandlerId>,
}

thread_local! {
    /// The bindings of [`bind_slot`], by the widget's address and the slot's name. An address is only ever a live
    /// widget's: a binding is taken out as its widget is destroyed, before the address can be another's.
    static SLOTS: RefCell<HashMap<(usize, String), Slot>> = RefCell::new(HashMap::new());
    /// How deep in a binding's own write this thread is, for the `notify` handlers that tell it from the app's.
    static WRITING: Cell<u32> = const { Cell::new(0) };
}

/// Runs `set` now and after each switch of language, for as long as `widget` lives, as the binding called `slot` of
/// that widget: a translated setter (`set_title_translated`, [`bind_label`]). A widget has one binding a slot, so a
/// second call replaces the first rather than adding a listener beside it, which would pile up with each call and
/// could write the older text over the newer one on a switch. A change of one of `properties` that is not the
/// binding's own write ends it too, as [`unbind_slot`] does: the app has set the text plainly since.
pub(crate) fn bind_slot(widget: &gtk::Widget, slot: &str, properties: &[&str], set: impl Fn() + 'static) {
    unbind_slot(widget, slot);
    let set = Rc::new(set);
    write(&*set);
    let listener = connect_changed({
        let set = set.clone();
        move || write(&*set)
    })
    .0;
    let key = (widget.as_ptr() as usize, slot.to_owned());
    let mut handlers = Vec::new();
    {
        let key = key.clone();
        handlers.push(widget.connect_destroy(move |_| {
            // The signal handlers go with the widget; only the listener is left to stop.
            let slot = SLOTS
                .try_with(|slots| slots.try_borrow_mut().ok()?.remove(&key))
                .ok()
                .flatten();
            if let Some(slot) = slot {
                disconnect(ChangeHandler(slot.listener));
            }
        }));
    }
    for property in properties {
        let (widget_key, slot) = (key.0, slot.to_owned());
        handlers.push(widget.connect_notify_local(Some(property), move |widget, _| {
            if WRITING.try_with(Cell::get).unwrap_or(0) == 0 && widget.as_ptr() as usize == widget_key {
                unbind_slot(widget, &slot);
            }
        }));
    }
    let _ = SLOTS.try_with(|slots| slots.borrow_mut().insert(key, Slot { listener, handlers }));
}

/// Ends the binding called `slot` of `widget`, if it has one: what a plain setter does, so that the text it sets is
/// not written over at the next switch of language.
pub(crate) fn unbind_slot(widget: &gtk::Widget, slot: &str) {
    let key = (widget.as_ptr() as usize, slot.to_owned());
    // Taken out before anything is stopped: disconnecting drops closures, which may come back here.
    let removed = SLOTS
        .try_with(|slots| slots.try_borrow_mut().ok()?.remove(&key))
        .ok()
        .flatten();
    if let Some(slot) = removed {
        for handler in slot.handlers {
            widget.disconnect(handler);
        }
        disconnect(ChangeHandler(slot.listener));
    }
}

/// Runs a binding's write, marked as the binding's own so that the `notify` it sets off does not end it.
fn write(set: &dyn Fn()) {
    let _ = WRITING.try_with(|writing| writing.set(writing.get() + 1));
    set();
    let _ = WRITING.try_with(|writing| writing.set(writing.get() - 1));
}

/// How many listeners of the language there are: what a test counts to see that a setter replaces its binding.
#[cfg(test)]
pub(crate) fn listener_count() -> usize {
    LISTENERS.with_borrow(Vec::len)
}

/// A language switch is one rename; the wait gathers the events of a first pick, which makes a directory and then
/// the file.
const DEBOUNCE: Duration = Duration::from_millis(100);

/// Omarchy's language, followed live from its language file: one code on the first line, as `omarchy language da`
/// writes it. It needs a main loop but no display.
pub struct Translator {
    file: PathBuf,
    monitors: RefCell<Vec<gio::FileMonitor>>,
    pending: RefCell<Option<glib::SourceId>>,
}

impl Translator {
    /// Follows this desktop's language, `~/.config/omarchy/language`, and formats as its registry says. One per
    /// process: a later call returns the first. [`crate::application`] calls it; an app that builds its own
    /// `gtk::Application` calls it as that starts, after registering its catalogs.
    pub fn install() -> Rc<Translator> {
        if let Some(translator) = Translator::get() {
            return translator;
        }
        let set = |name: &str| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
        };
        // `omarchy-language` writes under XDG_CONFIG_HOME when that is set.
        let config = set("XDG_CONFIG_HOME")
            .or_else(|| set("HOME").map(|home| home.join(".config")))
            .unwrap_or_else(|| glib::home_dir().join(".config"));
        set_locale_registry(set("OMARCHY_PATH").map(|omarchy| omarchy.join("default/i18n/locales.json")));
        let translator = Translator::follow(config.join("omarchy/language"));
        // GTK chose a direction from the session's locale as it started; the language picked in Omarchy decides.
        apply_direction();
        let _ = CURRENT.try_with(|current| current.replace(Some(translator.clone())));
        translator
    }

    /// The translator [`Translator::install`] installed, on the main thread. None once the thread is ending.
    pub fn get() -> Option<Rc<Translator>> {
        CURRENT.try_with(|current| current.borrow().clone()).ok().flatten()
    }

    /// Follows the language file at `file` until this is dropped: what [`Translator::install`] does for the
    /// desktop's own, and what a test does with a file in a temporary directory, so that it never depends on the
    /// desktop it is run from.
    pub fn follow(file: impl Into<PathBuf>) -> Rc<Translator> {
        let translator = Rc::new(Translator {
            file: file.into(),
            monitors: RefCell::default(),
            pending: RefCell::default(),
        });
        translator.reload();
        translator.watch();
        translator
    }

    /// The language file followed.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// The code on the first line of a language file. No file, or anything else on that line, is English.
    pub fn read_language(file: &Path) -> String {
        let contents = std::fs::read_to_string(file).unwrap_or_default();
        valid_code(contents.lines().next().unwrap_or_default().trim()).to_owned()
    }

    fn reload(&self) {
        set_language(&Self::read_language(&self.file));
    }

    /// `omarchy language` replaces the file, and a first pick creates it and maybe its directory. So the directory
    /// is watched, not the file, and with it the nearest directory above that exists, to see the directory arrive;
    /// they are looked for again after every change.
    fn watch(self: &Rc<Self>) {
        let mut dirs: Vec<PathBuf> = Vec::new();
        let parent = self.file.parent().unwrap_or(Path::new("/"));
        if let Some(existing) = parent.ancestors().find(|dir| dir.is_dir()) {
            dirs.push(existing.to_owned());
        }
        let mut monitors = Vec::new();
        for dir in dirs {
            let file = gio::File::for_path(&dir);
            let Ok(monitor) = file.monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) else {
                continue;
            };
            let weak = Rc::downgrade(self);
            monitor.connect_changed(move |_, _, _, _| {
                if let Some(translator) = weak.upgrade() {
                    translator.schedule();
                }
            });
            monitors.push(monitor);
        }
        self.monitors.replace(monitors);
    }

    fn schedule(self: &Rc<Self>) {
        if let Some(id) = self.pending.take() {
            id.remove();
        }
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(DEBOUNCE, move || {
            if let Some(translator) = weak.upgrade() {
                translator.pending.take();
                translator.reload();
                translator.watch();
            }
        });
        self.pending.replace(Some(id));
    }
}

impl Drop for Translator {
    fn drop(&mut self) {
        if let Some(id) = self.pending.take() {
            id.remove();
        }
    }
}

/// A whole number as the language writes it, its digits in threes: `1,234,567` in English, `1.234.567` in Danish.
///
/// The separator is the system locale's for the language, which Omarchy generates when the language is picked.
/// Where that locale is missing the digits are not grouped at all, rather than grouped as another language would.
/// Digits are always in threes (India's lakh grouping is not followed) and always Western, as Omarchy's own are.
pub fn format_int(value: i64) -> String {
    let numbers = state().numbers.clone();
    let mut text = group_digits(&value.unsigned_abs().to_string(), &numbers.group);
    if value < 0 {
        text.insert(0, '-');
    }
    text
}

/// A number with `decimals` digits after the decimal mark, as the language writes it: `1,234.50` in English,
/// `1.234,50` in Danish. The limits are [`format_int`]'s.
pub fn format_number(value: f64, decimals: usize) -> String {
    let numbers = state().numbers.clone();
    let plain = format!("{:.*}", decimals, value.abs());
    let (whole, fraction) = plain.split_once('.').unwrap_or((&plain, ""));
    if !whole.bytes().all(|b| b.is_ascii_digit()) {
        // Not a number, or infinite: as Rust writes it.
        return format!("{value}");
    }
    let mut text = group_digits(whole, &numbers.group);
    if !fraction.is_empty() {
        text.push_str(&numbers.decimal);
        text.push_str(fraction);
    }
    // Nothing that rounds to zero is negative.
    if value < 0.0 && plain.bytes().any(|b| (b'1'..=b'9').contains(&b)) {
        text.insert(0, '-');
    }
    text
}

fn group_digits(digits: &str, separator: &str) -> String {
    let mut text = String::with_capacity(digits.len() * 2);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push_str(separator);
        }
        text.push(digit);
    }
    text
}

/// A date and time in the language, by a `strftime` format: `"%x"` is the language's own short date, `"%X"` its
/// time, `"%c"` both, and `"%A %e %B"` spells out the day and the month in the language.
///
/// The names and the orders are the system locale's for the language (glibc's), which Omarchy generates when the
/// language is picked. Where that locale is missing, they are those of the locale the app was started in, which
/// after a switch made while the app runs is the language from before. On a system that is not Linux the app's own
/// locale is always the one used.
pub fn format_datetime(when: &glib::DateTime, format: &str) -> String {
    let locale = state().locale.clone();
    locale
        .and_then(|locale| libc_locale::format(&locale, when, format))
        .or_else(|| when.format(format).ok().map(Into::into))
        .unwrap_or_default()
}

/// A date as the language writes one in short: [`format_datetime`] with `"%x"`.
pub fn format_date(when: &glib::DateTime) -> String {
    format_datetime(when, "%x")
}

/// A time of day as the language writes one: [`format_datetime`] with `"%X"`.
pub fn format_time(when: &glib::DateTime) -> String {
    format_datetime(when, "%X")
}

/// A time of day in short, hours and minutes, as the language writes one: `02:05 PM` in American English, `14:05`
/// in Danish, `14時05分` in Japanese. Whether the clock runs to 12 or 24 hours is the language's.
///
/// The format is the system locale's time ([`format_time`]'s) with its seconds taken out, so the limits are
/// [`format_datetime`]'s: where the language's locale is missing it is the app's own, and on a system that is not
/// Linux it is always 24 hours.
pub fn format_time_short(when: &glib::DateTime) -> String {
    let locale = state().locale.clone();
    let format = locale
        .and_then(|locale| libc_locale::time_format(&locale))
        // The locale the app was started in, as `format_datetime` falls back to.
        .or_else(|| libc_locale::time_format(""))
        .map_or_else(|| "%H:%M".to_owned(), |format| without_seconds(&format));
    format_datetime(when, &format)
}

/// A day of the year with no year, the month by name, as the language writes it beside the day: `March 14` in
/// American English, `14 March` in British, `14. marts` in Danish, `3月 14日` in Japanese. For what comes round each
/// year (a birthday, an anniversary), or a date in the current year where the year would only be noise.
///
/// The order and the marks around the day are those of the system locale's own long date (glibc's `date_fmt`, what
/// `date` prints), with the month's full name in the form the language uses after a day (`marca`, not `marzec`, in
/// Polish). A locale that has no such date gives the order of its short date, with nothing around the day. The limits
/// are [`format_datetime`]'s: where the language's locale is missing it is the app's own.
pub fn format_day_month(when: &glib::DateTime) -> String {
    let locale = state().locale.clone();
    let formats = locale
        .and_then(|locale| libc_locale::date_formats(&locale))
        // The locale the app was started in, as `format_datetime` falls back to.
        .or_else(|| libc_locale::date_formats(""));
    let format = formats.map_or_else(|| "%-e %B".to_owned(), |(long, short)| day_month_format(&long, &short));
    format_datetime(when, &format)
}

/// One piece of a `strftime` format: a conversion (`%e`, `%-d`, `%OB`) or the text between them.
fn conversions(format: &str) -> Vec<(bool, &str)> {
    let mut pieces = Vec::new();
    let mut rest = format;
    while !rest.is_empty() {
        let Some(at) = rest.find('%') else {
            pieces.push((false, rest));
            break;
        };
        if at > 0 {
            pieces.push((false, &rest[..at]));
        }
        // `%`, a flag, `E` or `O`, and the letter.
        let mut end = at + 1;
        let bytes = rest.as_bytes();
        while end < bytes.len() && b"-_0^#EO".contains(&bytes[end]) {
            end += 1;
        }
        end = (end + 1).min(bytes.len());
        while !rest.is_char_boundary(end) {
            end += 1;
        }
        pieces.push((true, &rest[at..end]));
        rest = &rest[end..];
    }
    pieces
}

/// The format of a day and a month by name, from a locale's long date `long` (`date_fmt`: `%a %e. %b %Y %T %Z`
/// gives `%-e. %B`) or, where that has no day beside a month's name, from the order of its short date `short`
/// (`%m/%d/%Y` gives `%B %-e`). The day is not padded, and the month is named in full.
fn day_month_format(long: &str, short: &str) -> String {
    let pieces = conversions(long);
    let last = |piece: &str| piece.chars().next_back().unwrap_or_default();
    let is_day = |(conversion, piece): &(bool, &str)| *conversion && matches!(last(piece), 'd' | 'e');
    let is_month = |(conversion, piece): &(bool, &str)| *conversion && matches!(last(piece), 'b' | 'B' | 'h');
    if let (Some(day), Some(month)) = (pieces.iter().position(is_day), pieces.iter().position(is_month)) {
        let (from, to) = (day.min(month), day.max(month));
        // Only text between them: a weekday or a year between the two is no part of a day and a month.
        if pieces[from + 1..to].iter().all(|(conversion, _)| !conversion) {
            let mut format = String::new();
            for (index, (_, piece)) in pieces.iter().enumerate().take(to + 1).skip(from) {
                format.push_str(match index {
                    _ if index == day => "%-e",
                    _ if index == month => "%B",
                    _ => piece,
                });
            }
            // A mark that ends the day when it comes last (`14日`, `14.` in Hungarian), up to a space or a comma.
            if day > month
                && let Some((false, after)) = pieces.get(day + 1)
            {
                format.extend(after.chars().take_while(|c| !c.is_whitespace() && *c != ','));
            }
            return format;
        }
    }
    let at = |letter: char| {
        conversions(short)
            .iter()
            .position(|(conversion, piece)| *conversion && last(piece) == letter)
    };
    match (at('m'), at('d').or_else(|| at('e'))) {
        (Some(month), Some(day)) if month < day => "%B %-e".to_owned(),
        _ => "%-e %B".to_owned(),
    }
}

/// `format`, a locale's time format, without its seconds: `%I:%M:%S %p` becomes `%I:%M %p`, and `%H時%M分%S秒`
/// becomes `%H時%M分`. The seconds go with what sets them off: the mark before them (`:`, `.`), or the space before
/// them and the word after (`%S초`, as Korean has it). A format without seconds is kept as it is.
fn without_seconds(format: &str) -> String {
    // `%T` and `%r` hold seconds too, so they are written out first, `%r` as the C locale has it. A locale's own
    // `%r` has been written out already by `time_format`.
    let format = format.replace("%T", "%H:%M:%S").replace("%r", "%I:%M:%S %p");
    let Some(at) = format.find("%S") else {
        return format;
    };
    let before = &format[..at];
    let before = match before.chars().next_back() {
        Some(space) if space.is_whitespace() => before.trim_end(),
        // A mark between the minutes and the seconds goes with the seconds. A letter or a word stays: in
        // `%M分%S秒` it names the minutes.
        Some(mark) if !mark.is_alphanumeric() => &before[..before.len() - mark.len_utf8()],
        _ => before,
    };
    // The word that names the seconds (`秒`, `초`) goes with them, up to the next space or conversion.
    let after = format[at + 2..].trim_start_matches(|c: char| !c.is_whitespace() && c != '%');
    format!("{before}{after}")
}

/// The system's locale data for one language, asked of the C library for that locale alone. The process's own
/// locale is left as it is: changing it is not safe once other threads run, and a worker may be formatting.
#[cfg(target_os = "linux")]
mod libc_locale {
    use std::ffi::{CStr, CString, c_char, c_int, c_long, c_void};

    use gtk::glib;

    use super::Numbers;

    /// `struct tm` as glibc and musl lay it out.
    #[repr(C)]
    struct Tm {
        sec: c_int,
        min: c_int,
        hour: c_int,
        mday: c_int,
        mon: c_int,
        year: c_int,
        wday: c_int,
        yday: c_int,
        isdst: c_int,
        gmtoff: c_long,
        zone: *const c_char,
    }

    unsafe extern "C" {
        fn newlocale(mask: c_int, locale: *const c_char, base: *mut c_void) -> *mut c_void;
        fn freelocale(locale: *mut c_void);
        fn strftime_l(
            text: *mut c_char,
            size: usize,
            format: *const c_char,
            time: *const Tm,
            locale: *mut c_void,
        ) -> usize;
        fn nl_langinfo_l(item: c_int, locale: *mut c_void) -> *const c_char;
    }

    /// `LC_CTYPE_MASK | LC_NUMERIC_MASK | LC_TIME_MASK`, the same bits in glibc and musl.
    const MASK: c_int = 1 | 2 | 4;
    /// `RADIXCHAR` and `THOUSEP`, the first two items of `LC_NUMERIC`.
    const RADIXCHAR: c_int = 0x10000;
    const THOUSEP: c_int = 0x10001;
    /// `T_FMT` and `T_FMT_AMPM`, the time formats of `LC_TIME` (its items from 0x20000), the same in glibc and musl.
    const T_FMT: c_int = 0x2002A;
    const T_FMT_AMPM: c_int = 0x2002B;
    /// `D_FMT`, the short date, the same in glibc and musl; and glibc's `_DATE_FMT`, its long date, which musl does
    /// not have (it answers an empty string for an item it does not know, and the short date's order is used).
    const D_FMT: c_int = 0x20029;
    const DATE_FMT: c_int = 0x2006C;

    /// A locale of the C library's, freed when dropped.
    struct Locale(*mut c_void);

    impl Locale {
        /// None when the system has not generated the locale.
        fn open(name: &str) -> Option<Self> {
            let name = CString::new(name).ok()?;
            // SAFETY: the name is a C string that outlives the call, and a null base asks for a new locale.
            let locale = unsafe { newlocale(MASK, name.as_ptr(), std::ptr::null_mut()) };
            // Made only for a locale that was opened: dropping one frees it.
            (!locale.is_null()).then(|| Self(locale))
        }

        fn info(&self, item: c_int) -> String {
            // SAFETY: the locale is open; the answer is a C string the locale owns until it is freed, copied here.
            let text = unsafe { CStr::from_ptr(nl_langinfo_l(item, self.0)) };
            text.to_string_lossy().into_owned()
        }
    }

    impl Drop for Locale {
        fn drop(&mut self) {
            // SAFETY: opened by `newlocale`, and freed once.
            unsafe { freelocale(self.0) };
        }
    }

    pub fn numbers(locale: &str) -> Option<Numbers> {
        let locale = Locale::open(locale)?;
        let decimal = locale.info(RADIXCHAR);
        Some(Numbers {
            decimal: if decimal.is_empty() { ".".to_owned() } else { decimal },
            group: locale.info(THOUSEP),
        })
    }

    /// The locale's time format (`%X`'s), with a `%r` in it written out as the locale's own 12-hour time. An empty
    /// `locale` is the one the app was started in, by its environment.
    pub fn time_format(locale: &str) -> Option<String> {
        let locale = Locale::open(locale)?;
        let format = locale.info(T_FMT);
        let twelve_hours = locale.info(T_FMT_AMPM);
        let format = if twelve_hours.is_empty() {
            format
        } else {
            format.replace("%r", &twelve_hours)
        };
        (!format.is_empty()).then_some(format)
    }

    /// The locale's long date (`date_fmt`, what `date` prints) and its short one (`%x`'s). An empty `locale` is the
    /// one the app was started in.
    pub fn date_formats(locale: &str) -> Option<(String, String)> {
        let locale = Locale::open(locale)?;
        let short = locale.info(D_FMT);
        (!short.is_empty()).then(|| (locale.info(DATE_FMT), short))
    }

    pub fn format(locale: &str, when: &glib::DateTime, format: &str) -> Option<String> {
        let locale = Locale::open(locale)?;
        let format = CString::new(format).ok()?;
        let zone = CString::new(when.timezone_abbreviation().as_str()).ok()?;
        let time = Tm {
            sec: when.second(),
            min: when.minute(),
            hour: when.hour(),
            mday: when.day_of_month(),
            mon: when.month() - 1,
            year: when.year() - 1900,
            // glib counts Monday as 1 and Sunday as 7; C counts Sunday as 0.
            wday: when.day_of_week() % 7,
            yday: when.day_of_year() - 1,
            isdst: c_int::from(when.is_daylight_savings()),
            gmtoff: (when.utc_offset().as_seconds()) as c_long,
            zone: zone.as_ptr(),
        };
        let mut text = [0u8; 512];
        // SAFETY: the buffer is as long as the size given, the format and the zone are C strings that outlive the
        // call, and the locale is open. What is written is NUL-terminated and `written` bytes long.
        let written = unsafe { strftime_l(text.as_mut_ptr().cast(), text.len(), format.as_ptr(), &time, locale.0) };
        // Nothing written is a format that came to nothing, or to more than the buffer: the caller falls back.
        (written > 0).then(|| String::from_utf8_lossy(&text[..written]).into_owned())
    }
}

#[cfg(not(target_os = "linux"))]
mod libc_locale {
    use gtk::glib;

    use super::Numbers;

    pub fn numbers(_locale: &str) -> Option<Numbers> {
        None
    }

    pub fn time_format(_locale: &str) -> Option<String> {
        None
    }

    pub fn date_formats(_locale: &str) -> Option<(String, String)> {
        None
    }

    pub fn format(_locale: &str, _when: &glib::DateTime, _format: &str) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing;

    fn catalogs() -> Catalogs {
        let mut catalogs = Catalogs::default();
        for (code, source) in [
            (
                "en",
                r#"{"Save": "Save", "Open": "Open", "{n} clips": {"one": "{n} clip", "other": "{n} clips"},
                    "{n} files": {"one": "{n} file", "other": "{n} files"}}"#,
            ),
            ("pt", r#"{"Save": "Guardar", "Open": "Abrir", "Quit": ""}"#),
            ("pt-BR", r#"{"Save": "Salvar"}"#),
            (
                "ru",
                r#"{"{n} clips": {"one": "{n} клип", "few": "{n} клипа", "many": "{n} клипов", "other": "{n} клипа"}}"#,
            ),
        ] {
            catalogs.register(code, source).unwrap();
        }
        catalogs
    }

    #[test]
    fn a_string_falls_back_from_the_region_to_the_language_to_english() {
        let catalogs = catalogs();
        let brazil = catalogs.table("pt-BR");
        assert_eq!(resolve(&brazil, "Save", None), "Salvar");
        assert_eq!(
            resolve(&brazil, "Open", None),
            "Abrir",
            "Portugal's, where Brazil's has none"
        );
        assert_eq!(resolve(&brazil, "Quit", None), "Quit", "an empty translation is none");
        assert_eq!(resolve(&brazil, "Never registered", None), "Never registered");
        assert_eq!(resolve(&catalogs.table("pt"), "Save", None), "Guardar");
        assert_eq!(
            resolve(&catalogs.table("da"), "Save", None),
            "Save",
            "a language with no catalog is English"
        );
        // A later catalog for a language adds to the earlier one, and wins where both have the string.
        let mut catalogs = catalogs;
        catalogs
            .register("pt", r#"{"Open": "Abre", "Close": "Fechar"}"#)
            .unwrap();
        let portugal = catalogs.table("pt");
        assert_eq!(
            ["Save", "Open", "Close"].map(|text| resolve(&portugal, text, None)),
            ["Guardar", "Abre", "Fechar"]
        );
        assert!(catalogs.register("da", "[]").is_err() && catalogs.register("da", "{").is_err());
    }

    #[test]
    fn a_counted_string_takes_the_form_of_the_catalog_that_has_it() {
        let catalogs = catalogs();
        let russian = catalogs.table("ru");
        let form = |text, count| resolve(&russian, text, Some(count));
        assert_eq!(form("{n} clips", 21), "{n} клип");
        assert_eq!(form("{n} clips", 22), "{n} клипа");
        assert_eq!(form("{n} clips", 25), "{n} клипов");
        // An untranslated counted string falls back to English, with English's rules: 21 is not "one" there.
        assert_eq!(form("{n} files", 21), "{n} files");
        assert_eq!(form("{n} files", 1), "{n} file");
        assert_eq!(
            resolve(&russian, "{n} clips", None),
            "{n} клипа",
            "with no count, the other form"
        );
        // A language with a form missing takes its other form.
        let mut catalogs = catalogs;
        catalogs
            .register("pl", r#"{"{n} clips": {"other": "{n} klipów"}}"#)
            .unwrap();
        assert_eq!(resolve(&catalogs.table("pl"), "{n} clips", Some(1)), "{n} klipów");
    }

    #[test]
    fn plural_categories_are_cldrs() {
        let categories = |language: &str, counts: &[i64]| -> Vec<&str> {
            counts.iter().map(|count| plural_category(language, *count)).collect()
        };
        assert_eq!(categories("en", &[0, 1, 2, -1]), ["other", "one", "other", "one"]);
        assert_eq!(categories("da", &[1, 5]), ["one", "other"]);
        assert_eq!(categories("ja", &[1, 5]), ["other", "other"]);
        assert_eq!(categories("fr", &[0, 1, 2, 1_000_000]), ["one", "one", "other", "many"]);
        assert_eq!(categories("pt", &[0, 1, 2_000_000]), ["other", "one", "many"]);
        assert_eq!(
            categories("ru", &[1, 2, 5, 11, 12, 21, 22, 25, 111]),
            ["one", "few", "many", "many", "many", "one", "few", "many", "many"]
        );
        assert_eq!(
            categories("pl", &[1, 2, 5, 22, 21]),
            ["one", "few", "many", "few", "many"]
        );
        assert_eq!(categories("cs", &[1, 3, 5]), ["one", "few", "other"]);
        assert_eq!(categories("hr", &[1, 3, 5]), ["one", "few", "other"]);
        assert_eq!(
            categories("sl", &[1, 2, 3, 5, 101]),
            ["one", "two", "few", "other", "one"]
        );
        assert_eq!(categories("lt", &[1, 2, 10, 11]), ["one", "few", "other", "other"]);
        assert_eq!(categories("lv", &[0, 1, 2, 11]), ["zero", "one", "other", "zero"]);
        assert_eq!(categories("ro", &[1, 0, 19, 20]), ["one", "few", "few", "other"]);
        assert_eq!(
            categories("ar", &[0, 1, 2, 3, 10, 11, 99, 100, 102]),
            ["zero", "one", "two", "few", "few", "many", "many", "other", "other"]
        );
        assert_eq!(categories("he", &[1, 2, 3]), ["one", "two", "other"]);
        assert_eq!(
            categories("ga", &[1, 2, 3, 7, 11]),
            ["one", "two", "few", "many", "other"]
        );
        assert_eq!(categories("hi", &[0, 1, 2]), ["one", "one", "other"]);
        assert_eq!(categories("fil", &[1, 4, 5, 6]), ["one", "other", "one", "other"]);
        assert_eq!(categories("is", &[1, 11, 21]), ["one", "other", "one"]);
    }

    #[test]
    fn placeholders_are_filled_in_order_or_by_place() {
        let (name, folder) = ("a.txt", "Trash");
        assert_eq!(fill("Moved {} to {}", None, &[&name, &folder]), "Moved a.txt to Trash");
        assert_eq!(
            fill("Til {1} flyttedes {0}", None, &[&name, &folder]),
            "Til Trash flyttedes a.txt",
            "a translation may reorder them"
        );
        assert_eq!(fill("{n} of {}", Some("1,234"), &[&7]), "1,234 of 7");
        assert_eq!(fill("{{}} is {} and }} too", None, &[&1]), "{} is 1 and } too");
        // What names nothing given stays as written.
        assert_eq!(fill("{} {} {5} {n} {name} {", None, &[&1]), "1 {} {5} {n} {name} {");
        assert_eq!(fill("no placeholders", None, &[]), "no placeholders");
        assert_eq!(fill("ünï {} cödé", None, &[&"é"]), "ünï é cödé");

        assert_eq!(placeholders("Moved {} to {}"), ["0", "1"]);
        assert_eq!(placeholders("Til {1} flyttedes {0}"), ["0", "1"]);
        assert_eq!(placeholders("{n} of {} {{x}} {name}"), ["0", "n"]);
        assert!(placeholders("plain").is_empty());
    }

    #[test]
    fn only_omarchys_codes_are_languages() {
        for code in ["da", "fil", "pt-BR", "zh-CN", "en"] {
            assert_eq!(valid_code(code), code);
        }
        for code in ["", "d", "danish", "DA", "pt_BR", "pt-BRA", "../x", "da\nru"] {
            assert_eq!(valid_code(code), "en", "{code:?}");
        }
    }

    /// The global language, a language file followed live in a temporary directory, and what is told of a switch.
    /// No display is involved.
    #[test]
    fn the_language_file_is_followed_and_a_switch_is_announced() {
        let _main = testing::main_loop();
        let _language = lock_language();
        let dir = testing::temp_dir("language");
        set_locale_registry(None);
        register_all(&[
            (
                "en",
                r#"{"omui-test Save": "omui-test Save",
                    "{n} omui-test clips": {"one": "{n} omui-test clip", "other": "{n} omui-test clips"}}"#,
            ),
            (
                "da",
                r#"{"omui-test Save": "Gem", "Moved {} to {}": "Flyttede {1}: {0}"}"#,
            ),
            ("ar", r#"{"omui-test Save": "حفظ"}"#),
            ("broken", "not json"),
        ]);
        // Before the first pick there is no directory to hold the file, and so nothing of its own to watch.
        let file = dir.join("config/omarchy/language");
        let translator = Translator::follow(&file);
        assert_eq!(
            (language(), tr("omui-test Save")),
            ("en".to_owned(), "omui-test Save".to_owned())
        );
        assert_eq!(trn("{n} omui-test clips", 1), "1 omui-test clip");
        assert_eq!(trn("{n} omui-test clips", 1234), "1,234 omui-test clips");
        assert_eq!(crate::trn!("{n} omui-test clips", 2usize), "2 omui-test clips");

        let switches = Rc::new(Cell::new(0));
        let handler = connect_changed({
            let switches = switches.clone();
            move || switches.set(switches.get() + 1)
        });
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "da\n").unwrap();
        assert!(testing::spin_until(5000, || language() == "da"), "a first pick is seen");
        assert_eq!(switches.get(), 1);
        assert_eq!(tr("omui-test Save"), "Gem");
        assert_eq!(crate::tr!("omui-test Save"), "Gem");
        assert_eq!(trf("Moved {} to {}", &[&"a", &"b"]), "Flyttede b: a");
        assert_eq!(crate::tr!("Moved {} to {}", "a", "b"), "Flyttede b: a");
        assert_eq!(
            trn("{n} omui-test clips", 1),
            "1 omui-test clip",
            "English's forms, where Danish has none"
        );
        assert!(!is_rtl());

        // `omarchy language` replaces the file: a new file renamed over the old one. And again, to see the watch
        // was set anew.
        for (code, text, rtl) in [("ar", "حفظ", true), ("da", "Gem", false)] {
            std::fs::write(dir.join("next"), format!("{code}\n")).unwrap();
            std::fs::rename(dir.join("next"), &file).unwrap();
            assert!(testing::spin_until(5000, || language() == code), "{code}");
            assert_eq!((tr("omui-test Save"), is_rtl()), (text.to_owned(), rtl));
        }
        assert_eq!(switches.get(), 3);
        // The same language written again is no switch, and a code that is none is English.
        std::fs::write(&file, "da\n").unwrap();
        testing::spin_for(300);
        assert_eq!(switches.get(), 3);
        std::fs::write(&file, "klingon\n").unwrap();
        assert!(testing::spin_until(5000, || language() == "en"));
        // No longer told, and no longer followed.
        disconnect(handler);
        set_language("da");
        assert_eq!(switches.get(), 4);
        drop(translator);
        std::fs::write(&file, "ar\n").unwrap();
        testing::spin_for(300);
        assert_eq!(language(), "da");
        set_language("en");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn numbers_and_dates_are_written_as_the_language_does() {
        let _main = testing::main_loop();
        let _language = lock_language();
        let dir = testing::temp_dir("formats");
        set_language("en");
        assert_eq!(
            [0, 7, 999, 1000, -1234, 1_234_567].map(format_int),
            ["0", "7", "999", "1,000", "-1,234", "1,234,567"]
        );
        assert_eq!(format_number(1234.5, 2), "1,234.50");
        assert_eq!(
            format_number(-0.004, 2),
            "0.00",
            "what rounds to nothing is not negative"
        );
        assert_eq!(format_number(-1234.567, 1), "-1,234.6");
        assert_eq!(format_number(12.0, 0), "12");
        assert_eq!(format_number(f64::NAN, 1), "NaN");
        let when = glib::DateTime::from_utc(2026, 11, 21, 14, 5, 9.0).unwrap();
        assert_eq!(format_datetime(&when, "%Y-%m-%d %H:%M:%S"), "2026-11-21 14:05:09");
        assert_eq!(format_datetime(&when, "%A %e %B"), "Saturday 21 November");
        assert!(!format_date(&when).is_empty() && !format_time(&when).is_empty());

        // A registry like Omarchy's says how a language is formatted and which way it reads. This one names a
        // locale no system has, so the registry's own decimal mark is what is left, with no grouping.
        let registry = dir.join("locales.json");
        std::fs::write(
            &registry,
            r#"{"da": {"locale": "xx_XX.UTF-8", "decimal": ","},
                "xx": {"locale": "xx_XX.UTF-8", "direction": "rtl", "decimal": "."}}"#,
        )
        .unwrap();
        set_locale_registry(Some(registry));
        set_language("da");
        assert_eq!(
            (format_int(1_234_567), format_number(1234.5, 1)),
            ("1234567".to_owned(), "1234,5".to_owned())
        );
        assert_eq!(
            format_datetime(&when, "%Y-%m-%d"),
            "2026-11-21",
            "the app's own locale, failing the language's"
        );
        set_language("xx");
        assert!(is_rtl(), "the registry says which languages read right to left");
        set_locale_registry(None);
        assert!(!is_rtl());
        set_language("ur");
        assert!(is_rtl(), "and without one, the four that do are known");
        set_language("en");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// omui ships its own strings in English only; an app translates them in its own catalogs, registered after
    /// omui's as every app catalog is, and its translation is the one shown.
    #[test]
    fn an_apps_catalog_translates_omuis_own_strings() {
        let mut catalogs = Catalogs::default();
        catalogs.register("en", OMUI_ENGLISH).unwrap();
        catalogs.register("en", r#"{"Save": "Save"}"#).unwrap();
        catalogs
            .register(
                "da",
                r#"{"Save": "Gem", "Keyboard shortcuts": "Tastaturgenveje", "_Cancel": "_Annuller"}"#,
            )
            .unwrap();
        let danish = catalogs.table("da");
        assert_eq!(
            ["Keyboard shortcuts", "_Cancel", "Save", "Close"].map(|text| resolve(&danish, text, None)),
            ["Tastaturgenveje", "_Annuller", "Gem", "Close"],
            "what the app did not translate stays omui's English"
        );
        let omui = json::parse(OMUI_ENGLISH).unwrap();
        assert!(
            omui.members().iter().any(|(text, _)| text == "Keyboard shortcuts"),
            "the shortcuts overlay's title is one of omui's strings, which its catalog test keeps listed"
        );
    }

    #[test]
    fn a_short_time_drops_the_seconds_and_keeps_the_languages_clock() {
        for (format, short) in [
            ("%I:%M:%S %p", "%I:%M %p"),
            ("%r", "%I:%M %p"),
            ("%H:%M:%S", "%H:%M"),
            ("%T", "%H:%M"),
            ("%H.%M.%S", "%H.%M"),
            ("%H時%M分%S秒", "%H時%M分"),
            ("%p %I시 %M분 %S초", "%p %I시 %M분"),
            ("%H:%M:%S %Z", "%H:%M %Z"),
            ("%H:%M", "%H:%M"),
        ] {
            assert_eq!(without_seconds(format), short, "{format}");
        }

        let _main = testing::main_loop();
        let _language = lock_language();
        let dir = testing::temp_dir("short-times");
        let when = glib::DateTime::from_utc(2026, 11, 21, 14, 5, 9.0).unwrap();
        // The C locale is on every system, and runs to 24 hours. American English runs to 12, where it has been
        // generated.
        let registry = dir.join("locales.json");
        std::fs::write(
            &registry,
            r#"{"en": {"locale": "en_US.UTF-8"}, "da": {"locale": "C.UTF-8"}}"#,
        )
        .unwrap();
        set_locale_registry(Some(registry));
        set_language("da");
        assert_eq!(format_time_short(&when), "14:05");
        assert_eq!(format_time(&when), "14:05:09");
        set_language("en");
        if libc_locale::time_format("en_US.UTF-8").is_some() {
            assert_eq!(format_time_short(&when), "02:05 PM");
            assert_eq!(format_time(&when), "02:05:09 PM");
        }
        set_locale_registry(None);
        set_language("en");
        assert!(
            !format_time_short(&when).contains("09"),
            "with no locale known, the app's own, without seconds"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_day_and_a_month_are_written_in_the_languages_order() {
        // The long dates of glibc's locales, and the short ones beside them.
        for (long, short, format) in [
            ("%a %b %e %r %Z %Y", "%m/%d/%Y", "%B %-e"),
            ("%a %e %b %H:%M:%S %Z %Y", "%d/%m/%y", "%-e %B"),
            ("%a %e. %b %Y %T %Z", "%d-%m-%Y", "%-e. %B"),
            ("%a %-d. %b %H:%M:%S %Z %Y", "%d.%m.%Y", "%-e. %B"),
            ("%Y年 %b %e日 %A %H:%M:%S %Z", "%Y年%m月%d日", "%B %-e日"),
            ("%Y. %b %-e., %A, %H:%M:%S %Z", "%Y-%m-%d", "%B %-e."),
            // No day beside a month's name, or no long date at all (musl): the short date's order.
            ("%a %d %Y %b", "%m/%d/%Y", "%B %-e"),
            ("", "%d.%m.%Y", "%-e %B"),
            ("", "%Y-%m-%d", "%B %-e"),
        ] {
            assert_eq!(day_month_format(long, short), format, "{long:?}");
        }

        let _main = testing::main_loop();
        let _language = lock_language();
        let dir = testing::temp_dir("day-month");
        let registry = dir.join("locales.json");
        std::fs::write(
            &registry,
            r#"{"en": {"locale": "en_US.UTF-8"}, "da": {"locale": "C.UTF-8"}}"#,
        )
        .unwrap();
        set_locale_registry(Some(registry));
        let when = glib::DateTime::from_utc(2026, 3, 4, 14, 5, 9.0).unwrap();
        if libc_locale::date_formats("en_US.UTF-8").is_some() {
            set_language("en");
            assert_eq!(format_day_month(&when), "March 4", "not padded, and no year");
        }
        // The C locale's long date is American English's.
        set_language("da");
        assert_eq!(format_day_month(&when), "March 4");
        set_locale_registry(None);
        set_language("en");
        assert!(!format_day_month(&when).contains("2026"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A listener that holds what stops it, as a widget held by its own `on_change` closure does (its destroy handler
    /// calls `disconnect`), is dropped with the thread's listeners as the thread ends: after the list is gone, which
    /// must be no panic. And one dropped as another is stopped stops it in turn.
    #[test]
    fn a_listener_stopped_from_its_own_drop_is_quiet_even_as_the_thread_ends() {
        struct StopsOnDrop(Cell<Option<ChangeHandler>>);
        impl Drop for StopsOnDrop {
            fn drop(&mut self) {
                if let Some(handler) = self.0.take() {
                    disconnect(handler);
                }
                // What a destroy handler may also ask as the process ends.
                let _ = (Translator::get(), crate::Theme::get());
            }
        }
        let ended = std::thread::spawn(|| {
            let count = || LISTENERS.with_borrow(Vec::len);
            // While the thread runs: stopping the first drops what stops the second.
            let second = connect_changed(|| {});
            let anchor = StopsOnDrop(Cell::new(Some(second)));
            let first = connect_changed(move || {
                let _ = &anchor;
            });
            assert_eq!(count(), 2);
            disconnect(first);
            assert_eq!(count(), 0, "both stopped");
            // As the thread ends: the listener holds the anchor that stops it.
            let anchor = Rc::new(StopsOnDrop(Cell::new(None)));
            let handler = connect_changed({
                let anchor = anchor.clone();
                move || {
                    let _ = &anchor;
                }
            });
            anchor.0.set(Some(handler));
        })
        .join();
        assert!(ended.is_ok(), "the thread ended without a panic");
    }

    #[test]
    fn a_test_has_the_language_to_itself_and_leaves_it_as_it_found_it() {
        let found = {
            let _language = lock_language();
            register("da", r#"{"omui-test Lock": "Lås"}"#);
            let found = language();
            set_language("da");
            assert_eq!(tr("omui-test Lock"), "Lås");
            found
        };
        // Taken again only once the first has gone, which put the language back.
        let _language = lock_language();
        assert_eq!(language(), found);
    }

    /// omui's own strings: `src/i18n/en.json` lists exactly those its sources translate.
    #[test]
    fn omuis_catalog_lists_exactly_the_strings_its_sources_translate() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        CatalogCheck::new(&root, root.join("i18n")).assert_ok();
    }
}
