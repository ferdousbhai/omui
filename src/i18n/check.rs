// SPDX-License-Identifier: MIT
//! The catalog test: an app's `en.json` lists exactly the strings its sources translate, and every other catalog
//! translates only those, with the same placeholders and every plural form its language uses. It is what catches a
//! string someone added without a catalog entry, before a user sees it in English.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use super::{OMUI_ENGLISH, placeholders, plural_category, valid_code};
use crate::json::{self, Value};

/// A function or macro whose argument at `argument` is the English text of a translated string.
struct Function {
    name: String,
    /// The path a call must name the function by, outermost first: `["omui"]` for `omui::confirm`, nothing for a
    /// declaration by bare name, which every call of that name matches.
    path: Vec<String>,
    argument: usize,
    counted: bool,
    /// One of omui's own, which an app's declaration of the same name replaces.
    omui: bool,
}

/// Checks an app's catalogs against its sources, for a test of the app's:
///
/// ```ignore
/// #[test]
/// fn the_catalogs_match_the_sources() {
///     let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
///     omui::i18n::CatalogCheck::new(root.join("src"), root.join("src/i18n")).assert_ok();
/// }
/// ```
///
/// The sources are every `.rs` file under the directory, test code left out: an item under `#[cfg(test)]`, a
/// `tests.rs`, and a directory named `tests`. A translated string is the literal handed to [`super::tr`],
/// [`super::trf`], [`super::trn`], [`super::trnf`], [`super::bind_label`], [`super::bind_tooltip`], or the macros
/// `tr!` and `trn!`; a text that is not a literal cannot be in a catalog, and is a problem unless it goes through
/// [`super::translate`]. It asks that:
///
/// - `en.json` has every such string, as itself, or for a counted one as its `one` and `other` forms with the text
///   as `other`, and has nothing else;
/// - every other catalog is named by one of Omarchy's codes, and translates only strings of `en.json` (or of omui's
///   own, which an app may translate too), each with the placeholders of the English, and each counted one with
///   every form its language uses, `other` among them.
pub struct CatalogCheck {
    sources: PathBuf,
    catalogs: PathBuf,
    functions: Vec<Function>,
    skipped: Vec<String>,
    /// Files and directories left out by their path under `sources`.
    excluded: Vec<PathBuf>,
    /// What was wrong with a declaration, reported with the other problems.
    declared: Vec<String>,
}

impl CatalogCheck {
    /// `sources` is the directory of the app's Rust files and `catalogs` the one that holds `en.json` and the rest.
    pub fn new(sources: impl Into<PathBuf>, catalogs: impl Into<PathBuf>) -> Self {
        let function = |name: &str, argument, counted| Function {
            name: name.to_owned(),
            path: Vec::new(),
            argument,
            counted,
            omui: true,
        };
        Self {
            sources: sources.into(),
            catalogs: catalogs.into(),
            functions: vec![
                function("tr", 0, false),
                function("trf", 0, false),
                function("trn", 0, true),
                function("trnf", 0, true),
                function("bind_label", 1, false),
                function("bind_tooltip", 1, false),
                // The literal-source setters of omui's widgets, which follow the language as `bind_label` does.
                function("set_heading_translated", 0, false),
                function("set_body_translated", 0, false),
                function("set_response_translated", 1, false),
                function("set_title_translated", 0, false),
                function("set_subtitle_translated", 0, false),
                function("set_description_translated", 0, false),
                // `omui::confirmf`, whose name no app's own function is likely to have.
                function("confirmf", 1, false),
                function("confirmf", 2, false),
                function("confirmf", 3, false),
            ],
            skipped: vec!["tests.rs".to_owned(), "tests".to_owned()],
            excluded: Vec::new(),
            declared: Vec::new(),
        }
    }

    /// Counts a function or macro of the app's own as one that translates: its argument at `argument`, from 0, is
    /// the English text, and `counted` says whether it is a counted string's.
    ///
    /// A name may be given several times, for a function with English text in more than one argument (a
    /// `confirm(parent, heading, body, action, …)` is `1`, `2` and `3`), or for two functions of one name: every
    /// argument given is checked in each call that has it. An app's name that is also one of omui's own (`tr`,
    /// `set_title_translated`) replaces omui's arguments for it, so an app's function of that name is read as the
    /// app says.
    ///
    /// `name` may be a path, `omui::confirm` or `dialog::confirm`: then only a call written with that path, or one
    /// ending in it (`crate::ui::dialog::confirm` for `dialog::confirm`), is read, and a function of the same bare
    /// name elsewhere (a command line's own `confirm`) is left alone. A call by bare name after a `use` is not one
    /// of those: declare the bare name for it, or leave out the file that has the other function
    /// ([`CatalogCheck::exclude`]). A path does not replace omui's own declaration of the name; a bare name does. A
    /// name that is not a Rust path is a problem of the check's.
    pub fn function(mut self, name: &str, argument: usize, counted: bool) -> Self {
        let mut path: Vec<String> = name.split("::").map(|segment| segment.trim().to_owned()).collect();
        let identifier = |segment: &String| {
            segment
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && segment.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        };
        if !path.iter().all(identifier) {
            self.declared.push(format!(
                "function {name:?}: not a function's name or path (as \"confirm\" or \"omui::confirm\")"
            ));
            return self;
        }
        let name = path.pop().unwrap_or_default();
        if path.is_empty() {
            self.functions
                .retain(|function| !(function.omui && function.name == name));
        }
        self.functions.push(Function {
            name,
            path,
            argument,
            counted,
            omui: false,
        });
        self
    }

    /// [`CatalogCheck::function`] for each of `arguments`: `.arguments("confirm", &[1, 2, 3], false)`.
    pub fn arguments(self, name: &str, arguments: &[usize], counted: bool) -> Self {
        arguments
            .iter()
            .fold(self, |check, argument| check.function(name, *argument, counted))
    }

    /// Leaves out the files and directories of this name, wherever they are under the sources.
    pub fn skip(mut self, name: &str) -> Self {
        self.skipped.push(name.to_owned());
        self
    }

    /// Leaves out the one file or directory at `path` under the sources (`cli/prompt.rs`), where [`CatalogCheck::skip`]
    /// leaves out every one of a name: for a file whose own function has the name of one that translates. A path
    /// that is nothing under the sources is a problem, so that a file moved or renamed is not checked unnoticed.
    pub fn exclude(mut self, path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        let relative = path.strip_prefix(&self.sources).unwrap_or(path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|part| part == std::path::Component::ParentDir)
        {
            self.declared.push(format!(
                "exclude {}: not a path under the sources, {}",
                path.display(),
                self.sources.display()
            ));
        } else {
            self.excluded.push(relative.to_owned());
        }
        self
    }

    /// Panics with every problem found, one a line.
    pub fn assert_ok(&self) {
        let problems = self.problems();
        assert!(
            problems.is_empty(),
            "the catalogs and the sources differ:\n{}",
            problems.join("\n")
        );
    }

    /// What is wrong, one line each; nothing when the catalogs match the sources.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = self.declared.clone();
        for excluded in &self.excluded {
            if !self.sources.join(excluded).exists() {
                problems.push(format!(
                    "exclude {}: no such file or directory under {}",
                    excluded.display(),
                    self.sources.display()
                ));
            }
        }
        let mut used = BTreeMap::new();
        let mut files = Vec::new();
        self.rust_files(&self.sources, &mut files);
        if files.is_empty() {
            problems.push(format!("no Rust sources under {}", self.sources.display()));
        }
        for file in files {
            let name = file.strip_prefix(&self.sources).unwrap_or(&file).display().to_string();
            match std::fs::read_to_string(&file) {
                Ok(code) => scan(&code, &name, &self.functions, &mut used, &mut problems),
                Err(error) => problems.push(format!("{name}: {error}")),
            }
        }
        let english = match read_catalog(&self.catalogs.join("en.json")) {
            Ok(english) => english,
            Err(error) => {
                problems.push(format!("en.json: {error}"));
                return problems;
            }
        };
        for (text, counted) in &used {
            match (english.get(text), counted) {
                (None, _) => problems.push(format!("missing from en.json: {text:?}")),
                (Some(Value::Object(forms)), true) => {
                    let categories: Vec<&str> = forms.iter().map(|(category, _)| category.as_str()).collect();
                    let other = forms.iter().find(|(category, _)| category == "other");
                    if categories != ["one", "other"] || other.and_then(|(_, form)| form.as_str()) != Some(text) {
                        problems.push(format!(
                            "en.json: counted, so needs its one form and itself as its other form: {text:?}"
                        ));
                    }
                }
                (Some(_), true) => {
                    problems.push(format!("en.json: counted, so needs its one and other forms: {text:?}"));
                }
                (Some(value), false) if value.as_str() != Some(text) => {
                    problems.push(format!("en.json: should be itself: {text:?}"));
                }
                (Some(_), false) => {}
            }
        }
        for text in english.keys().filter(|text| !used.contains_key(*text)) {
            problems.push(format!("in en.json but no source translates it: {text:?}"));
        }

        // An app may translate omui's own strings in its catalogs: they are checked against omui's English.
        let omui: BTreeMap<String, Value> = json::parse(OMUI_ENGLISH)
            .map(|catalog| catalog.members().iter().cloned().collect())
            .unwrap_or_default();
        let mut catalogs: Vec<PathBuf> = std::fs::read_dir(&self.catalogs)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
            .collect();
        catalogs.sort();
        for path in catalogs {
            let code = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            if code == "en" {
                continue;
            }
            if valid_code(&code) != code {
                problems.push(format!("{code}.json: not named by one of Omarchy's language codes"));
            }
            let catalog = match read_catalog(&path) {
                Ok(catalog) if !catalog.is_empty() => catalog,
                Ok(_) => {
                    problems.push(format!("{code}.json: empty"));
                    continue;
                }
                Err(error) => {
                    problems.push(format!("{code}.json: {error}"));
                    continue;
                }
            };
            // Some languages reach a form only at a million ("many" in French).
            let categories: BTreeSet<&str> = (0..=1000)
                .chain([1_000_000])
                .map(|count| plural_category(&code, count))
                .chain(["other"])
                .collect();
            for (text, value) in &catalog {
                let at = format!("{code}.json: {text:?}");
                match (english.get(text).or_else(|| omui.get(text)), value) {
                    (None, _) => problems.push(format!("{at} is not in en.json")),
                    (Some(Value::String(_)), Value::String(translation)) if !translation.is_empty() => {
                        if placeholders(translation) != placeholders(text) {
                            problems.push(format!("{at} has other placeholders than the English"));
                        }
                    }
                    (Some(Value::String(_)), _) => problems.push(format!("{at} needs a translation")),
                    (Some(source), Value::Object(forms)) => {
                        let form = |category: &str| {
                            forms
                                .iter()
                                .find(|(name, _)| name == category)
                                .and_then(|(_, form)| form.as_str())
                                .filter(|form| !form.is_empty())
                        };
                        for category in &categories {
                            if form(category).is_none() {
                                problems.push(format!("{at} needs its {category} form"));
                            }
                        }
                        let wanted = placeholders(source.get("other").and_then(Value::as_str).unwrap_or(text));
                        for (category, translation) in forms {
                            let has = placeholders(translation.as_str().unwrap_or_default());
                            let mut needed = wanted.clone();
                            // A form other than "other" may say its number in words.
                            if category != "other" && !has.iter().any(|name| name == "n") {
                                needed.retain(|name| name != "n");
                            }
                            if has != needed {
                                problems.push(format!("{at}: {category} has other placeholders than the English"));
                            }
                        }
                    }
                    (Some(_), _) => problems.push(format!("{at} is counted, so needs its plural forms")),
                }
            }
        }
        problems
    }

    fn rust_files(&self, dir: &Path, files: &mut Vec<PathBuf>) {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        for path in entries {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let relative = path.strip_prefix(&self.sources).unwrap_or(&path);
            if self.skipped.iter().any(|skipped| *skipped == name) || self.excluded.iter().any(|e| e == relative) {
                continue;
            }
            if path.is_dir() {
                self.rust_files(&path, files);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }
}

/// A catalog's members by their English text.
fn read_catalog(path: &Path) -> Result<BTreeMap<String, Value>, String> {
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    match json::parse(&source)? {
        Value::Object(members) => Ok(members.into_iter().collect()),
        _ => Err("not a JSON object".to_owned()),
    }
}

/// One piece of Rust source, as far as finding calls needs telling them apart.
#[derive(Debug, PartialEq)]
enum Token {
    Word(String),
    /// A string literal, with its escapes read.
    Text(String),
    Mark(u8),
    /// A character, a lifetime, a number: nothing a call is made of.
    Other,
    End,
}

/// Walks Rust source a token at a time, so that a string never hides a comment or the other way round, a raw
/// string's quotes are its own, and a `'"'` does not open a string.
struct Source<'a> {
    code: &'a [u8],
    at: usize,
}

impl Source<'_> {
    fn peek(&self, ahead: usize) -> u8 {
        self.code.get(self.at + ahead).copied().unwrap_or(0)
    }

    fn skip_blank(&mut self) {
        loop {
            match (self.peek(0), self.peek(1)) {
                (b' ' | b'\t' | b'\n' | b'\r', _) => self.at += 1,
                (b'/', b'/') => {
                    while !matches!(self.peek(0), b'\n' | 0) {
                        self.at += 1;
                    }
                }
                (b'/', b'*') => {
                    // Block comments nest.
                    let mut depth = 0;
                    loop {
                        match (self.peek(0), self.peek(1)) {
                            (b'/', b'*') => {
                                depth += 1;
                                self.at += 2;
                            }
                            (b'*', b'/') => {
                                depth -= 1;
                                self.at += 2;
                                if depth == 0 {
                                    break;
                                }
                            }
                            (0, _) => break,
                            _ => self.at += 1,
                        }
                    }
                }
                _ => break,
            }
        }
    }

    fn next(&mut self) -> Token {
        self.skip_blank();
        let byte = self.peek(0);
        match byte {
            0 if self.at >= self.code.len() => Token::End,
            b'"' => Token::Text(self.text()),
            b'\'' => {
                // A character ends in a quote one or two places on; a lifetime has none.
                if self.peek(1) == b'\\' {
                    self.at += 2;
                    while !matches!(self.peek(0), b'\'' | b'\n' | 0) {
                        self.at += 1;
                    }
                    self.at += 1;
                } else {
                    let width = match self.peek(1) {
                        0xf0.. => 4,
                        0xe0.. => 3,
                        0xc0.. => 2,
                        _ => 1,
                    };
                    self.at += if self.peek(1 + width) == b'\'' { width + 2 } else { 1 };
                }
                Token::Other
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                let start = self.at;
                while self.peek(0).is_ascii_alphanumeric() || self.peek(0) == b'_' {
                    self.at += 1;
                }
                let word = String::from_utf8_lossy(&self.code[start..self.at]).into_owned();
                // The letters before a string literal that say what kind it is.
                let raw = matches!(word.as_str(), "r" | "br" | "cr");
                let hashes = (0..).take_while(|ahead| self.peek(*ahead) == b'#').count();
                if raw && self.peek(hashes) == b'"' {
                    self.at += hashes + 1;
                    return Token::Text(self.raw_text(hashes));
                }
                if matches!(word.as_str(), "b" | "c") && self.peek(0) == b'"' {
                    return Token::Text(self.text());
                }
                Token::Word(word)
            }
            b'0'..=b'9' => {
                while self.peek(0).is_ascii_alphanumeric() || matches!(self.peek(0), b'_' | b'.') {
                    self.at += 1;
                }
                Token::Other
            }
            _ => {
                self.at += 1;
                Token::Mark(byte)
            }
        }
    }

    /// The token after this one, without taking it.
    fn upcoming(&mut self) -> Token {
        let at = self.at;
        let token = self.next();
        self.at = at;
        token
    }

    /// A string literal from its opening quote, with its escapes read.
    fn text(&mut self) -> String {
        self.at += 1;
        let mut text = Vec::new();
        loop {
            let byte = self.peek(0);
            self.at += 1;
            match byte {
                b'"' => break,
                0 if self.at > self.code.len() => break,
                b'\\' => {
                    let escaped = self.peek(0);
                    self.at += 1;
                    let character = match escaped {
                        b'n' => '\n',
                        b't' => '\t',
                        b'r' => '\r',
                        b'0' => '\0',
                        b'x' => {
                            let digits =
                                String::from_utf8_lossy(&self.code[self.at..(self.at + 2).min(self.code.len())])
                                    .into_owned();
                            self.at += 2;
                            u8::from_str_radix(&digits, 16).map_or('?', char::from)
                        }
                        b'u' => {
                            // \u{1F600}
                            let end = self.code[self.at..].iter().position(|b| *b == b'}').unwrap_or(0);
                            let digits =
                                String::from_utf8_lossy(&self.code[(self.at + 1).min(self.at + end)..self.at + end])
                                    .into_owned();
                            self.at += end + 1;
                            u32::from_str_radix(&digits.replace('_', ""), 16)
                                .ok()
                                .and_then(char::from_u32)
                                .unwrap_or('?')
                        }
                        b'\n' => {
                            // A backslash at the end of a line joins the next one, less its indentation.
                            while matches!(self.peek(0), b' ' | b'\t' | b'\n' | b'\r') {
                                self.at += 1;
                            }
                            continue;
                        }
                        other => char::from(other),
                    };
                    text.extend_from_slice(character.encode_utf8(&mut [0; 4]).as_bytes());
                }
                _ => text.push(byte),
            }
        }
        String::from_utf8_lossy(&text).into_owned()
    }

    /// A raw string from after its opening quote, to the quote followed by as many `#` as opened it.
    fn raw_text(&mut self, hashes: usize) -> String {
        let start = self.at;
        loop {
            if self.at >= self.code.len() {
                return String::from_utf8_lossy(&self.code[start..]).into_owned();
            }
            if self.peek(0) == b'"' && (1..=hashes).all(|ahead| self.peek(ahead) == b'#') {
                let text = String::from_utf8_lossy(&self.code[start..self.at]).into_owned();
                self.at += hashes + 1;
                return text;
            }
            self.at += 1;
        }
    }

    /// Passes what `open` (just taken) opened, to the bracket that closes it.
    fn skip_group(&mut self, open: u8) {
        let close = match open {
            b'(' => b')',
            b'[' => b']',
            _ => b'}',
        };
        let mut depth = 1;
        while depth > 0 {
            match self.next() {
                Token::Mark(mark) if mark == open => depth += 1,
                Token::Mark(mark) if mark == close => depth -= 1,
                Token::End => return,
                _ => {}
            }
        }
    }

    /// Passes one item: to the semicolon that ends it, or through the braces that hold it.
    fn skip_item(&mut self) {
        loop {
            match self.next() {
                Token::Mark(b';') | Token::End => return,
                Token::Mark(b'{') => return self.skip_group(b'{'),
                Token::Mark(open @ (b'(' | b'[')) => self.skip_group(open),
                _ => {}
            }
        }
    }
}

/// Finds every text `code` hands to one of `functions`, and whether a call counts it.
fn scan(code: &str, file: &str, functions: &[Function], used: &mut BTreeMap<String, bool>, problems: &mut Vec<String>) {
    let mut source = Source {
        code: code.as_bytes(),
        at: 0,
    };
    let mut defining = false;
    loop {
        let token = source.next();
        let was_defining = std::mem::replace(&mut defining, false);
        match token {
            Token::End => return,
            Token::Mark(b'#') => {
                // An attribute: `#[cfg(test)]` marks an item that is no part of the app.
                if source.upcoming() == Token::Mark(b'!') {
                    source.next();
                }
                if source.upcoming() != Token::Mark(b'[') {
                    continue;
                }
                source.next();
                let mut attribute = String::new();
                let mut depth = 1;
                while depth > 0 {
                    match source.next() {
                        Token::Mark(b'[') => depth += 1,
                        Token::Mark(b']') => depth -= 1,
                        Token::Word(word) => attribute.push_str(&word),
                        Token::Mark(mark) => attribute.push(char::from(mark)),
                        Token::End => return,
                        _ => {}
                    }
                }
                if attribute == "cfg(test)" {
                    source.skip_item();
                }
            }
            Token::Word(word) if word == "fn" => defining = true,
            // A macro's own definition hands on what it was given, which is no text.
            Token::Word(word) if word == "macro_rules" => source.skip_item(),
            Token::Word(word) if !was_defining => {
                let wanted: Vec<&Function> = functions.iter().filter(|function| function.name == word).collect();
                if wanted.is_empty() {
                    continue;
                }
                let called_by = qualifier(source.code, source.at - word.len());
                let wanted: Vec<&Function> = wanted
                    .into_iter()
                    .filter(|function| called_by.ends_with(&function.path))
                    .collect();
                if wanted.is_empty() {
                    continue;
                }
                let after_name = source.at;
                if source.upcoming() == Token::Mark(b'!') {
                    source.next();
                }
                if source.upcoming() != Token::Mark(b'(') {
                    source.at = after_name;
                    continue;
                }
                source.next();
                let call = source.at;
                for function in wanted {
                    source.at = call;
                    // The arguments before the text.
                    let mut passed = 0;
                    let mut ended = false;
                    while passed < function.argument && !ended {
                        match source.next() {
                            Token::Mark(b',') => passed += 1,
                            Token::Mark(b')') | Token::End => ended = true,
                            Token::Mark(open @ (b'(' | b'[' | b'{')) => source.skip_group(open),
                            _ => {}
                        }
                    }
                    if ended {
                        // Too few arguments to be the function meant: another of the same name.
                        continue;
                    }
                    if source.upcoming() == Token::Mark(b'&') {
                        source.next();
                    }
                    match source.next() {
                        Token::Text(text) => {
                            let counted = used.entry(text).or_insert(false);
                            *counted |= function.counted;
                        }
                        _ => {
                            let line = code[..call].lines().count();
                            let shown: String = code[after_name..].chars().take(40).collect();
                            problems.push(format!(
                                "{file}:{line}: not a literal, so it cannot be in a catalog: {word}{}",
                                shown.split('\n').next().unwrap_or_default()
                            ));
                        }
                    }
                }
                // On from the arguments, where another call may be.
                source.at = call;
            }
            _ => {}
        }
    }
}

/// The path a name at `start` of `code` is written with, outermost first: `["omui", "dialog"]` for
/// `omui::dialog::confirm`, nothing for a bare name or a method's.
fn qualifier(code: &[u8], start: usize) -> Vec<String> {
    let mut path = Vec::new();
    let mut at = start;
    let blank = |at: &mut usize| {
        while *at > 0 && code[*at - 1].is_ascii_whitespace() {
            *at -= 1;
        }
    };
    loop {
        blank(&mut at);
        if at < 2 || &code[at - 2..at] != b"::" {
            break;
        }
        at -= 2;
        blank(&mut at);
        let end = at;
        while at > 0 && (code[at - 1].is_ascii_alphanumeric() || code[at - 1] == b'_') {
            at -= 1;
        }
        if at == end {
            // `<T as Trait>::` or `::` at the root: no name to read.
            break;
        }
        path.push(String::from_utf8_lossy(&code[at..end]).into_owned());
    }
    path.reverse();
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing;

    fn found(code: &str) -> (Vec<(String, bool)>, Vec<String>) {
        let check = CatalogCheck::new("", "");
        let (mut used, mut problems) = (BTreeMap::new(), Vec::new());
        scan(code, "app.rs", &check.functions, &mut used, &mut problems);
        (used.into_iter().collect(), problems)
    }

    fn texts(code: &str) -> Vec<String> {
        let (used, problems) = found(code);
        assert!(problems.is_empty(), "{problems:?}");
        used.into_iter().map(|(text, _)| text).collect()
    }

    /// A function with English text in several arguments, and an app's function named as one of omui's.
    #[test]
    fn a_function_may_take_its_text_in_several_arguments() {
        let check = CatalogCheck::new("", "")
            .arguments("confirm", &[1, 2, 3], false)
            .function("bind_title", 1, false)
            .function("label", 0, false)
            .function("label", 2, true)
            .function("set_title_translated", 1, false);
        let scan = |code: &str| {
            let (mut used, mut problems) = (BTreeMap::new(), Vec::new());
            super::scan(code, "app.rs", &check.functions, &mut used, &mut problems);
            (used.into_iter().collect::<Vec<_>>(), problems)
        };
        let (used, problems) = scan(
            r#"
            confirm(&window, "Delete it?", "It cannot be undone.", "_Delete", Appearance::Destructive, || {});
            bind_title(&window, "People");
            label("Saved");
            label("{n} saved", x, "{n} files", 3);
            row.set_title_translated(&group, "Accounts");
            "#,
        );
        assert!(problems.is_empty(), "{problems:?}");
        let found = |text: &str| {
            used.iter()
                .find(|(found, _)| found == text)
                .map(|(_, counted)| *counted)
        };
        for text in [
            "Delete it?",
            "It cannot be undone.",
            "_Delete",
            "People",
            "Saved",
            "{n} saved",
            "Accounts",
        ] {
            assert_eq!(found(text), Some(false), "{text}");
        }
        assert_eq!(found("{n} files"), Some(true), "the argument declared counted");
        // Each argument named is a literal, or a problem: one problem for the one that is not.
        let (_, problems) = scan(r#"confirm(&window, &heading, "Body", "_Go", a, b);"#);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("not a literal"));
        // omui's own name, replaced by the app's: its argument 0 is the app's to use as it likes.
        let (_, problems) = scan(r#"set_title_translated(&group, "Accounts");"#);
        assert!(problems.is_empty(), "{problems:?}");
    }

    /// A declaration by path reads only the calls written with it, so an app's own function of the same bare name
    /// is not taken for omui's; `confirmf` is known without a declaration.
    #[test]
    fn a_function_declared_by_path_leaves_others_of_its_name_alone() {
        let check = CatalogCheck::new("", "").arguments("omui::confirm", &[1, 2, 3], false);
        let scan = |code: &str| {
            let (mut used, mut problems) = (BTreeMap::new(), Vec::new());
            super::scan(code, "app.rs", &check.functions, &mut used, &mut problems);
            (used.into_keys().collect::<Vec<_>>(), problems)
        };
        let (used, problems) = scan(
            r#"
            omui::confirm(&window, "Delete it?", "Gone.", "_Delete", Appearance::Destructive, || {});
            crate::omui :: confirm(&window, "Nested", "Path", "_Ok", a, b);
            confirm(&prompt, &question, &default, &answer, a, b);
            cli::confirm(&prompt, &question, &default, &answer);
            self.confirm(&prompt, &question, &default, &answer);
            omui::confirmf(&window, "Delete {}?", "{} is gone.", "_Delete", &[&name], a, || {});
            "#,
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(
            used,
            [
                "Delete it?",
                "Delete {}?",
                "Gone.",
                "Nested",
                "Path",
                "_Delete",
                "_Ok",
                "{} is gone."
            ]
        );
        assert_eq!(qualifier(b"a::b :: c(", 8), ["a", "b"]);
        assert!(qualifier(b"x.c(", 2).is_empty() && qualifier(b"<T>::c(", 5).is_empty());

        // A declaration that is no path is the check's own problem, told plainly.
        let problems = CatalogCheck::new("nowhere", "nowhere")
            .function("omui::", 1, false)
            .function("my tr", 0, false)
            .exclude("../outside.rs")
            .exclude("gone.rs")
            .problems();
        for expected in [
            "function \"omui::\": not a function's name or path (as \"confirm\" or \"omui::confirm\")",
            "function \"my tr\": not a function's name or path (as \"confirm\" or \"omui::confirm\")",
            "exclude ../outside.rs: not a path under the sources, nowhere",
            "exclude gone.rs: no such file or directory under nowhere",
        ] {
            assert!(
                problems.iter().any(|problem| problem == expected),
                "{expected}\nin {problems:#?}"
            );
        }
    }

    #[test]
    fn the_strings_a_source_translates_are_found() {
        let code = r####"
            use omui::{tr, trn};
            // tr("in a comment") /* tr("in another /* nested */ one") */
            /// Docs say tr("documented").
            pub fn tr(text: &str) -> String { translate(text) }
            fn build(count: usize, name: &str) {
                let a = tr("Save");
                let b = omui::tr!("Moved {} to {}", name, tr("Trash"));
                let c = omui::i18n::trn( "{n} clips" , count as i64);
                let d = trn!("{n} files", count);
                let e = trnf("{n} of {}", 3, &[&name]);
                let f = tr(&"Borrowed");
                let quote = '"'; let lifetime: &'static str = "tr(\"in a string\")";
                let g = tr("Quote \"it\"\n\u{e9} \x41 \
                            joined");
                let h = tr(r#"Raw "text" \n"#);
                let i = trf(r"C:\path {}", &[&name]);
                omui::i18n::bind_label(&button, "Bound");
                bind_tooltip(row.widget(), "Tip");
                bind_label(&make(a, b), &"After a call");
                let tr = 5; let x = tr + 1; other.attr("Not ours");
                let e = 'é'; let j = tr("After a wide character");
            }
            macro_rules! mine { ($text:expr) => { tr($text) }; }
            #[cfg(test)]
            mod tests {
                fn inner() { let _ = tr("Only in a test"); }
            }
            #[cfg(test)] fn helper() -> String { tr("Nor this") }
            #[derive(Debug)] struct After; fn after() -> String { tr("After the tests") }
        "####;
        let (used, problems) = found(code);
        assert!(problems.is_empty(), "{problems:?}");
        let counted: Vec<&str> = used
            .iter()
            .filter(|(_, counted)| *counted)
            .map(|(text, _)| text.as_str())
            .collect();
        assert_eq!(counted, ["{n} clips", "{n} files", "{n} of {}"]);
        let plain: Vec<&str> = used
            .iter()
            .filter(|(_, counted)| !counted)
            .map(|(text, _)| text.as_str())
            .collect();
        assert_eq!(
            plain,
            [
                "After a call",
                "After a wide character",
                "After the tests",
                "Borrowed",
                "Bound",
                "C:\\path {}",
                "Moved {} to {}",
                "Quote \"it\"\né A joined",
                "Raw \"text\" \\n",
                "Save",
                "Tip",
                "Trash",
            ]
        );
    }

    #[test]
    fn a_text_that_is_not_a_literal_is_a_problem() {
        let (used, problems) = found("fn f(name: &str) {\n    let a = tr(name);\n    let b = tr(\"Fine\");\n}");
        assert_eq!(used, [("Fine".to_owned(), false)]);
        assert_eq!(problems.len(), 1);
        assert!(
            problems[0].starts_with("app.rs:2: not a literal") && problems[0].contains("tr(name)"),
            "{problems:?}"
        );
        // A text counted in one place is counted.
        assert_eq!(
            found("fn f() { tr(\"{n} x\"); trn(\"{n} x\", 2); }").0,
            [("{n} x".to_owned(), true)]
        );
        assert!(texts("fn f() { translate(name); my_tr(name); }").is_empty());
    }

    /// An app as its test would see it: sources and catalogs in a directory.
    #[test]
    fn an_apps_catalogs_are_checked_against_its_sources() {
        let _main = testing::main_loop();
        let dir = testing::temp_dir("catalogs");
        let write = |name: &str, contents: &str| {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        };
        write(
            "src/main.rs",
            "mod ui;\n#[cfg(test)]\nmod tests;\nfn main() { let _ = (tr(\"Save\"), trn(\"{n} clips\", 2), \
             my_tr(1, \"Mine\")); }\n",
        );
        write(
            "src/ui/window.rs",
            "fn build() { bind_label(&b, \"Moved {} to {}\"); }\n",
        );
        write("src/tests.rs", "fn t() { tr(\"Only a test's\"); }\n");
        write("src/ui/tests/more.rs", "fn t() { tr(\"Nor this\"); }\n");
        write("src/generated.rs", "fn g() { tr(\"Generated\"); }\n");
        // Left out by path, under the sources or as a path of its own: a function of the app's named as omui's.
        write("src/ui/cli/ask.rs", "fn a() { tr(\"Excluded\"); }\n");
        write("src/ui/prompt.rs", "fn a() { tr(name); }\n");
        write(
            "src/i18n/en.json",
            r#"{"Save": "Save", "Mine": "Mine", "Moved {} to {}": "Moved {} to {}",
                "{n} clips": {"one": "{n} clip", "other": "{n} clips"}}"#,
        );
        write(
            "src/i18n/da.json",
            r#"{"Save": "Gem", "Moved {} to {}": "Flyttede {1} fra {0}", "_Cancel": "_Annuller",
                "Keyboard shortcuts": "Tastaturgenveje", "{n} clips": {"one": "ét klip", "other": "{n} klip"}}"#,
        );
        write(
            "src/i18n/ru.json",
            r#"{"{n} clips": {"one": "{n} клип", "few": "{n} клипа", "many": "{n} клипов", "other": "{n} клипа"}}"#,
        );
        let check = || {
            CatalogCheck::new(dir.join("src"), dir.join("src/i18n"))
                .function("my_tr", 1, false)
                .skip("generated.rs")
                .exclude("ui/cli")
                .exclude(dir.join("src/ui/prompt.rs"))
        };
        assert_eq!(check().problems(), Vec::<String>::new());
        check().assert_ok();

        // Each way a catalog goes wrong is told apart.
        write(
            "src/ui/window.rs",
            "fn build() { bind_label(&b, \"Moved {} to {}\"); tr(\"New\"); tr(\"{n} clips\"); }\n",
        );
        write(
            "src/i18n/en.json",
            r#"{"Save": "Gem", "Mine": "Mine", "Moved {} to {}": "Moved {} to {}", "Old": "Old",
                "{n} clips": "{n} clips"}"#,
        );
        write(
            "src/i18n/da.json",
            r#"{"Save": "", "Moved {} to {}": "Flyttede {}", "Stray": "Løs", "{n} clips": {"one": "{n} klip"},
                "Mine": {"other": "Mine"}}"#,
        );
        write(
            "src/i18n/ru.json",
            r#"{"{n} clips": {"one": "{n}", "few": "{n}", "many": "{n} {}", "other": "клипа"}}"#,
        );
        write("src/i18n/Danish.json", r#"{"Save": "Gem"}"#);
        write("src/i18n/fr.json", "{");
        write("src/i18n/sv.json", "{}");
        let problems = check().problems();
        for expected in [
            "missing from en.json: \"New\"",
            "en.json: should be itself: \"Save\"",
            "en.json: counted, so needs its one and other forms: \"{n} clips\"",
            "in en.json but no source translates it: \"Old\"",
            "Danish.json: not named by one of Omarchy's language codes",
            "da.json: \"Save\" needs a translation",
            "da.json: \"Moved {} to {}\" has other placeholders than the English",
            "da.json: \"Stray\" is not in en.json",
            "da.json: \"{n} clips\" needs a translation",
            "da.json: \"Mine\" needs a translation",
            "ru.json: \"{n} clips\" needs a translation",
            "sv.json: empty",
        ] {
            assert!(
                problems.iter().any(|problem| problem == expected),
                "{expected}\nin {problems:#?}"
            );
        }
        assert!(
            problems.iter().any(|problem| problem.starts_with("fr.json: JSON:")),
            "{problems:#?}"
        );
        assert_eq!(problems.len(), 13, "{problems:#?}");

        // With the English a counted string again, each language owes its own forms and the count in `other`.
        write(
            "src/i18n/en.json",
            r#"{"Save": "Save", "Mine": "Mine", "Moved {} to {}": "Moved {} to {}", "New": "New",
                "{n} clips": {"one": "{n} clip", "other": "{n} clips"}}"#,
        );
        for name in ["Danish.json", "fr.json", "sv.json"] {
            std::fs::remove_file(dir.join("src/i18n").join(name)).unwrap();
        }
        write("src/i18n/da.json", r#"{"{n} clips": {"one": "{n} klip"}}"#);
        write("src/i18n/nl.json", r#"{"{n} clips": "{n} clips"}"#);
        assert_eq!(
            check().problems(),
            [
                "da.json: \"{n} clips\" needs its other form",
                "nl.json: \"{n} clips\" is counted, so needs its plural forms",
                "ru.json: \"{n} clips\": many has other placeholders than the English",
                "ru.json: \"{n} clips\": other has other placeholders than the English",
            ]
        );
        assert_eq!(
            CatalogCheck::new(dir.join("nowhere"), dir.join("nowhere"))
                .problems()
                .len(),
            2,
            "no sources and no en.json are problems, not a pass"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
