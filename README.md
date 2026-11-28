# omui

The shared look and widgets of a family of Omarchy desktop apps. Plain GTK 4 in Rust: no libadwaita, and no
dependency beyond `gtk4` but one, `gdk4-wayland` of the same gtk-rs release, which names a window for the compositor
([One name](#one-name)).

An omui window takes its colours from Omarchy's current theme and follows a theme switch while it runs. It looks the
same whatever GTK theme or `gtk.css` the desktop has, and sits beside [Strata](https://github.com/lgse/strata), whose
theming approach it follows. Its corners are Hyprland's window rounding and its language is Omarchy's, both followed
live, and it reads right to left in the languages that do.

## Using it

```toml
[dependencies]
omui = { git = "https://github.com/ferdousbhai/omui", rev = "<40-hex>" } # v0.5.2
```

Building needs `glib-compile-resources` (on Arch it is part of `glib2`). Running needs GTK 4.16 or later, for the
CSS variables the corners are written with.

```rust
use omui::gtk::{self, prelude::*};

fn main() -> gtk::glib::ExitCode {
    omui::application("com.example.Notes", "notes", "Notes", include_str!("style.css"), |app| {
        let window = gtk::ApplicationWindow::builder().application(app).title("Notes").build();
        window.set_titlebar(Some(&omui::header(None)));
        let empty = omui::EmptyState::new(Some(omui::icons::name::DOCUMENT), "No notes", None);
        let shell = omui::Shell::new(&window, &empty.widget);
        shell.toast("Welcome");
        window.present();
    })
    .run()
}
```

`omui::gtk` is the `gtk4` crate omui was built with. An app that depends on `gtk4` itself, for a newer feature level,
gets the same one.

### One name

An Omarchy app has one short lowercase name (`notes`): its binary's, its `notes.desktop`'s, its icon's, and its
windows' Wayland app id, which is how Hyprland and the launcher match a window to its desktop entry. A GApplication id
needs a dot, so `omui::application` takes both: the dotted id for D-Bus (`com.example.Notes`) and the name. It sets the
program's name to it, and gives every window added to the application the name as its Wayland app id on each map,
where GTK would give the dotted id. omui's own windows (a dialog over a window without a `Shell`, and so the shortcut
overlay there) get it too. A window an app keeps out of the application is named with `omui::name_window(&window)`.
On X11 and broadway it does nothing; the program's name is X11's `WM_CLASS` already.

So the second argument of `omui::application(id, name, …)` is the Wayland app id of every window the app shows, and
an app needs no `gdk4-wayland` of its own: a window made with `.application(app)`, `set_application` or
`add_window` is named as it joins, and any other window (a preview, a window kept out of the application so that
closing it does not end the app) is one call away, `omui::name_window(&window)`, before or after it is shown.

An app that is handed files or URIs to open (a mail app given a `mailto:` link, a viewer given a file) starts with
`omui::application_opening` instead: the same arguments and the same application, flagged `HANDLES_OPEN`, with a
second handler for what it is asked to open. No activation comes with an open, so that handler makes the window when
there is none yet.

```rust
omui::application_opening(APP_ID, "mail", "Mail", CSS, |app| window(app).present(), |app, files| {
    let window = window(app);
    window.present();
    files.iter().filter(|file| file.uri().starts_with("mailto:")).for_each(|file| compose(&window, &file.uri()));
})
```

GTK sets the app id itself as it maps a window, after `realize`, so it is set from `map`, as `name_window` does. The
offscreen tests can only see that each window was seen to; on a real session, with the app running, Hyprland shows
what it got:

```sh
hyprctl clients -j | jq -r '.[] | select(.pid == '"$(pgrep -x notes)"') | .class'   # notes, for every window
```

`omui::application` goes first in `main`: it takes `GTK_THEME` out of the environment, because that variable loads a
desktop theme under omui's whatever else is set. An app that builds its own `gtk::Application` and calls
`Theme::install` removes the variable itself, before GTK starts.

A packaged app carries the Strata and Lucide notices with it, since its binary holds code adapted from Strata and
the Lucide icons: ship [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)'s two entries and the texts under
`data/licenses/` among the app's own notices.

## What is in 0.5

| | |
|---|---|
| `application`, `name_window`, `main`, `background`, `hold` | One-window application with the theme and the language installed, its windows named by the app's one name; a binary that is a window with no arguments and a command line with some; work on a thread with the result on the main loop, and the application kept running until it is done |
| `Theme`, `Tokens`, `Colors`, `Rounding`, `blend`, `omarchy_state_dir`, `APP_PRIORITY` | The installed look, its colours, the theme's whole palette by name, Hyprland's rounding, a listener for what CSS cannot reach, and where an app's own stylesheets go |
| `tr`, `trf`, `trn`, `trnf`, `tr!`, `trn!`, `i18n::*`, `Translator` | Every string in Omarchy's language, followed live: catalogs, plurals, text bound to a widget, numbers and dates, and the test that keeps the catalogs in step with the sources |
| `Keys`, `on_key`, `quit_keys`, `text_has_focus`, `ShortcutOverlay`, `ShortcutSection` | Single keys that are characters while a field has the keyboard, the keys that quit, and the overlay that lists an app's shortcuts |
| `FilePicker`, `PortalFilePicker`, `FakeFilePicker`, `PickOptions`, `FileFilter`, `PickError`, `PickDetail` | The desktop portal's open and save dialogs, behind a trait a test can fake |
| `icons::name::*`, `icon`, `icon_button`, `data_uri` | 75 bundled Lucide icons as GTK symbolic icons |
| `icon_menu_button` | A menu button drawn as an icon button: a header bar's main menu |
| `Shell`, `Toast`, `header`, `header_without_title`, `Title`, `claim_titlebar` | The window's overlay with toasts, and its header bar |
| `Dialog`, `WeakDialog`, `confirm`, `confirmf`, `about`, `dialog_open`, `Appearance` | A dialog drawn inside its window, the question most of them are, an About dialog, and whether one is open |
| `Split`, `Nav`, `tabs`, `on_narrow` | A sidebar beside content that becomes one pane in a narrow window, with a divider to drag if the app wants one; pages pushed one over another; the pages of a stack as tabs; a widget's width crossing a threshold |
| `clamp`, `page` | A child kept to a readable width; a scrolling column of groups |
| `Group`, `Row`, `entry_row`, `password_row`, `switch_row`, `combo_row`, `ExpanderRow` | A titled, bordered list and the rows of a settings page or a form |
| `label`, `badge`, `avatar`, `spinner`, `sidebar_row`, `single_tab_stop`, `clear`, `set_class` | Small builders; a list that Tab passes in one step |
| `Banner`, `EmptyState` | A message bar with an icon and buttons; what an empty page or list shows |
| `dev::shot` (feature `dev`) | A widget written to a PNG, for before-and-after pictures |
| Scripted QA (feature `qa`), `qa::attach` | With `OMUI_QA=1`, the app is driven by JSON lines on stdin and answers on a descriptor of their own: widgets found by role and name, and any GLib warning fails the run |

With these an app that was built on libadwaita needs none of it: each has the methods of the libadwaita widget it
stands for, which its documentation names.

## Theme

Colours come from `~/.local/state/omarchy/current/theme/colors.toml`, where Omarchy writes its current theme. Without
Omarchy the palette is the Omarchy app template's: a dark surface (`#0e0e10`) with Omarchy's fallback yellow
(`#FFD60A`) for the accent. The interface font is JetBrains Mono at 13 px times the desktop's text scale; an
app's CSS may set another `font-family` on a widget or a pane.

The colours are the theme's own, with one adjustment: secondary text and the status colours are moved toward the
text colour when they would not read on the background (4.5:1 and 3:1), which several light themes need.

In CSS the colours are GTK named colours, tinted only with `alpha()`:

`@om_bg` `@om_surface` `@om_text` `@om_dim_text` `@om_accent` `@om_danger` `@om_success` `@om_warning` `@om_muted`
`@om_highlight` `@om_border`

`@om_accent_fg` is the colour for text and icons drawn on the accent: black or white, whichever reads there
(`Tokens::accent_foreground`). An accent is as often lighter than the background as darker, so neither `@om_bg` nor a
fixed white will do.

The rest of the theme's palette is there by name: every key of `colors.toml` is the named colour `@om_color_<key>`
(`@om_color_blue`, `@om_color_selection`, `@om_color_color4`), and `Theme::get().unwrap().color("blue")` in Rust
(`Theme::colors` for all of them). The names `accent` `background` `foreground` `selection` `muted` `red` `green`
`yellow` `orange` `blue` `magenta` `cyan` are always defined: a theme that lacks one answers with its other name for
the colour (`color1` for `red`), and failing that with the fallback palette's. A colour in `colors.toml` may be
quoted, bare, or followed by a comment.

Corners are Hyprland's `decoration:rounding`, 0 on Omarchy's default, so they are square unless the theme or the user
rounds windows. omui asks `hyprctl` as it starts and again when Hyprland's event socket announces `configreloaded`;
nothing polls, and away from Hyprland the corners stay square. An app's stylesheet takes its corners from the same
variables, never in pixels:

| | |
|---|---|
| `var(--om-radius)` | A panel, a dialog, a card, a button, a field |
| `var(--om-radius-inner)` | One pixel less, for what sits inside a border |
| `var(--om-radius-small)` | 4 px at most: a row, a menu item, a badge, a check |
| `var(--om-radius-tiny)` | 2 px at most: a track, a thin bar |

Only what is round by nature keeps a radius of its own (`999px` or `100%`): an avatar, a knob, a dot. `Theme::radius()`
is the rounding in pixels for what is drawn by hand, and `Theme::on_change` runs when it changes, as when the colours
do. `Rounding::follow(hyprctl, socket, f)` follows a `hyprctl` and a socket of a test's own, and
`Theme::install_following` installs the theme with them.

App stylesheets use these and no hex, so they follow every theme. Stylesheets are layered above GTK's user priority:
a reset, GTK's built-in theme, `omui.css`, the app's CSS, then the tokens. A `gtk::CssProvider` the app adds as it runs
(a zoom, a font from its settings) goes at `omui::APP_PRIORITY`, beside the app's CSS: at GTK's
`STYLE_PROVIDER_PRIORITY_APPLICATION` the reset unsets all it says. The reset covers the whole display, so
omui and libadwaita cannot share a process. A widget `omui.css` has no rule for keeps the built-in theme's look, dark
or light as the palette is.

## Language

Omarchy runs in 50 languages, and an app follows the one picked under Setup > Region > Language, live, like the theme.
The English text is the key: every visible string is written `tr("Save")`, and the app embeds one catalog per language,
a flat JSON object from the English to its translation, named by Omarchy's code (`src/i18n/da.json`, `pt-BR.json`).
`en.json` lists every string the app translates, each as itself, which is what translators and the test work from.

```rust
fn main() -> glib::ExitCode {
    omui::i18n::register_all(&[
        ("en", include_str!("i18n/en.json")),
        ("da", include_str!("i18n/da.json")),
    ]);
    omui::application(/* … */).run()
}

let title = omui::tr("Save");
let moved = omui::tr!("Moved {} to {}", name, folder);         // or omui::trf("…", &[&name, &folder])
let count = omui::trn!("{n} clips", clips.len());              // or omui::trn("{n} clips", n as i64)
omui::i18n::bind_label(&button, "Save");                       // translated now and after every switch
omui::i18n::bind_tooltip(&icon_button, "New folder");          // tooltip and accessible name
omui::i18n::bind(&status, move |status| status.set_label(&omui::trn!("{n} clips", count.get())));
omui::i18n::on_change(&window, move || rebuild_the_sidebar()); // for whatever else was built from text
```

```json
{
  "Save": "Gem",
  "Moved {} to {}": "Flyttede {0} til {1}",
  "{n} clips": { "one": "{n} klip", "other": "{n} klip" }
}
```

- **Placeholders** are Rust's: `{}` takes the next argument, `{0}` and `{1}` name one by its place so a translation
  can reorder them, and `{{` is a brace. `{n}` is the count of a counted string, written with the language's digit
  grouping. `tr` returns its text as it is; `trf`, `trn`, `trnf` and the macros fill placeholders.
- **A counted string** is an object of plural forms by CLDR category. English gives `one` and `other`, with the text
  itself as `other`; each language gives the categories it uses (Russian `one`, `few`, `many`, `other`; Arabic six;
  Japanese `other` alone), always with `other`. The forms are picked by the rules of the catalog that supplied them,
  so an English fallback in Russian takes English's.
- **Fallback** is per string: `pt-BR`, then `pt`, then English.
- **A switch** swaps the catalogs, turns every window to read right to left (Arabic, Hebrew, Persian, Urdu) or back,
  and runs what `bind`, `bind_label`, `bind_tooltip` and `on_change` registered. A string translated once and kept (a
  status line, an error) stays in the old language: build it again in `on_change`, or keep the English and translate
  where it is shown. `i18n::language()` and `i18n::is_rtl()` say where things stand.
- A text that is not a literal (read from a file, built at run time) goes through `i18n::translate`.
- Brand names, units and keyboard keys stay as they are. A label with a mnemonic (`_Cancel`) keeps one in every
  translation.

`omui::application` installs the `Translator`, which reads `~/.config/omarchy/language` and watches it. An app that
builds its own `gtk::Application` calls `Translator::install()` as it starts. A test never depends on the desktop it
runs on: without a translator the language is English, `i18n::set_language("da")` switches it, and
`Translator::follow(path)` follows a language file in a temporary directory.

The language is one for the whole process, and cargo runs tests on several threads at once. A test that switches it,
or reads translated text and needs a known language, takes `i18n::lock_language()` first: the tests that do wait
for each other, and each finds the language as the one before found it, since the lock puts it back as it is
dropped.

```rust
#[test]
fn the_toolbar_follows_the_language() {
    let _language = omui::i18n::lock_language();
    omui::i18n::register("da", r#"{"Save": "Gem"}"#);
    omui::i18n::set_language("da");
    assert_eq!(omui::tr("Save"), "Gem");
}
```

Every app keeps the catalog test, which fails when a string was added without a catalog entry, a catalog translates
something `en.json` does not have, a translation has other placeholders than the English, or a counted string lacks
a form its language uses:

```rust
#[test]
fn the_catalogs_match_the_sources() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    omui::i18n::CatalogCheck::new(root.join("src"), root.join("src/i18n")).assert_ok();
}
```

It reads every `.rs` file under the sources but test code (`#[cfg(test)]` items, `tests.rs`, `tests/`), and finds the
literals handed to `tr`, `trf`, `trn`, `trnf`, `tr!`, `trn!`, `bind_label` and `bind_tooltip`, and to the `_translated`
setters of dialogs, groups and rows (`set_heading_translated`, `set_body_translated`, `set_response_translated`'s
label, `set_title_translated`, `set_subtitle_translated`, `set_description_translated`). `.function("my_tr", 0,
false)` adds a wrapper of the app's own and `.skip("generated.rs")` leaves a file out. A name given more than once is
checked at each argument given, for a helper with English in several (`.arguments("confirm", &[1, 2, 3], false)`, as
`omui::confirm` takes its heading, body and button), and a name of omui's that an app declares is read as the app
says. `omui::confirmf`'s heading, body and button are counted without being told.

A declaration may name a path, `.arguments("omui::confirm", &[1, 2, 3], false)`: then only calls written with that
path (or one ending in it) are read, and a function of the same bare name elsewhere, a command line's own `confirm`,
is left alone. A call by bare name after a `use` needs the bare declaration. `.exclude("cli/prompt.rs")` leaves out
one file or directory by its path under the sources, where `.skip` leaves out every one of a name; a path that is
not there, or a declaration that is not a Rust path, is a problem the check reports.

omui's own strings (Cancel, Close, the shortcut overlay's "Keyboard shortcuts", the file picker's errors) are in its
`src/i18n/en.json`, which omui's own catalog test keeps in step with its sources. omui ships them in English only. An
app translates them in its own catalogs, beside its strings, and leaves them out of its `en.json`: omui registers its
English before the app registers anything, a later catalog wins where both have a string, and the check accepts a
translation of any of omui's strings in a catalog other than `en.json`.

```json
{ "Save": "Gem", "Keyboard shortcuts": "Tastaturgenveje", "_Cancel": "_Annuller" }
```

Numbers and dates follow the language too:

```rust
omui::i18n::format_int(1234567);                  // 1,234,567   da: 1.234.567
omui::i18n::format_number(1234.5, 1);             // 1,234.5     da: 1.234,5
omui::i18n::format_date(&glib::DateTime::now_local()?);          // the language's short date
omui::i18n::format_time_short(&when);             // 02:05 PM    da: 14:05, the language's clock without seconds
omui::i18n::format_day_month(&when);              // March 14    en-GB: 14 March, da: 14. marts
omui::i18n::format_datetime(&when, "%A %e %B");   // Saturday 21 November, in the language
```

Which to use:

| For | Use | Gives (American English) |
|---|---|---|
| A date with its year: a file's date, a due date, a list's column | `format_date` | `03/14/2026` |
| What comes round every year, or a date this year where the year is noise: a birthday, an anniversary, "Mar 14" in an inbox | `format_day_month` | `March 14` |
| A time of day on a clock: an event's start, a message received today | `format_time_short` | `02:05 PM` |
| A time to the second: a log line, a timestamp to compare | `format_time` | `02:05:09 PM` |
| Anything else: a weekday, a month alone, a long date | `format_datetime` with a `strftime` format | `Saturday, March 14` for `"%A, %B %e"` |

A format given to `format_datetime` fixes the order, which is not every language's: it is for what has no order
(`"%A"`, `"%B"`), and the others are for whole dates and times.

Their limits: the separators, names and orders are the system locale's for the language (glibc's), which Omarchy
generates when the language is picked; `$OMARCHY_PATH/default/i18n/locales.json` says which locale that is. Where the
locale is missing, a number is written with the registry's decimal mark and no grouping, and a date as the locale the
app was started in writes it. Digits are grouped in threes (not India's lakhs) and are always Western, as Omarchy's
own are. A widget that shows a number or a date formats it inside `bind`, so that it is written anew on a switch.

`omui.css` mirrors what it writes with a left or a right under `:dir(rtl)`; GTK mirrors the order of a box's children,
a label's alignment and `start`/`end` itself, and a `Split` puts its sidebar where reading starts. An app's stylesheet
does the same for its own `margin-left`, `border-right` and the like: `.pane:dir(rtl) { … }`.

## Keyboard

```rust
let shortcuts = omui::ShortcutOverlay::new(|| vec![
    omui::ShortcutSection::new(&tr("Playback"), &[("Space", &tr("Play or pause")), ("Ctrl+Shift+Z", &tr("Redo"))]),
    omui::ShortcutSection::new(&tr("General"), &[("?", &tr("Show or hide the shortcuts")), ("Q", &tr("Quit"))]),
]);
shortcuts.install(&window, false);      // `?` and F1; `true` for an app that is typed into: F1 and Ctrl+?

let keys = omui::Keys::new(&window);
keys.on("space", move || play_or_pause());                              // not while a field has the keyboard
keys.on("q", glib::clone!(#[weak] window, move || window.close()));
keys.on_always("<Control>s", move || save());                           // in a field too
omui::quit_keys(&window);               // Ctrl+Q closes every window, Ctrl+W this one
```

`?` shows every shortcut of an app in an overlay, a dialog over the window that lists each section's keys; Escape or
the same key closes it. The sections are asked for each time it opens, so they are in the language of the moment.
Keys that do the same share a row: `("? / F1", &tr("Show or hide the shortcuts"))`, or
`section.alternatives(&["Ctrl+Q", "Ctrl+W"], &tr("Close"))`, each shown as its own keys with a `/` between.

A single-key shortcut (`Keys::on`, or `omui::on_key(&window, "q", …)` for one) does nothing while an entry, a search
or password entry, a spin button or a text view has the keyboard: there the key is a character.
`omui::text_has_focus(&widget)` is the same test for an app's own handlers. `Keys::on_always` is for a key with Ctrl or
Alt and for function keys. Neither acts while a dialog is open over the window, and the widget under the keyboard has
each key first, so Space still presses a focused button. A key is one character (`"q"`, `"?"`) or GTK's accelerator
syntax (`"space"`, `"<Control>s"`, `"F1"`). A key acts only with exactly the modifiers it names: `"r"` is R alone,
not Shift+R or Ctrl+R, and `"R"` or `"<Shift>r"` is Shift+R. A symbol is whatever the layout presses to type it, so
`"?"` works on every layout, and Caps Lock changes nothing. `keys.activate("q")` does what the key would and says
whether anything ran, for an app's tests.

An app that is typed into (an editor, anything whose text field has the keyboard most of the time) cannot give bare
keys away: it shows the shortcuts on F1 and Ctrl+?, quits on Ctrl+Q and closes on Ctrl+W (`quit_keys`), and puts its
actions on Ctrl and a letter. `quit_keys` closes windows as their close button would, so a window with unsaved work
asks first, in its `close-request` handler.

## Work in the background

```rust
omui::background(move || upload(&file), move |result| show(result));
```

`background` runs its work on a thread and its `done` on the main loop, and holds the application until `done` has
run: GTK ends an application when its last window closes, whatever is still running, and a save or an upload must
finish. The app stays, windowless, until the work is done. Work started another way (`gio::spawn_blocking`, a
subprocess, a future) takes the same hold: `let held = omui::hold();` as it starts, and `drop(held)` on the main
thread when it has ended.

## Files

```rust
let picker: Rc<dyn omui::FilePicker> = Rc::new(omui::PortalFilePicker::new());
let options = omui::PickOptions {
    filters: vec![omui::FileFilter::new(&tr("Images"), &["*.png", "*.jpg"]).mime_types(&["image/*"])],
    multiple: true,
    ..omui::PickOptions::new(&tr("Import photos"))
};
picker.open(options, Box::new(move |picked| match picked {
    Ok(files) => import(files),
    Err(omui::PickError::Cancelled) => {}
    Err(failed) => shell.toast(&failed.to_string()),
}));
```

Open and save dialogs go through the desktop portal (`org.freedesktop.portal.FileChooser` over D-Bus), never
`gtk::FileDialog`, so they are the desktop's own. `save` takes the same options with `name` as the file name
suggested and answers with one path. Every request ends: with the files, with `Cancelled` when the user closed the
dialog or the app asked for another before it was answered, or with a failure whose text (`to_string()`) is for the
window, in the user's language:

| `PickError` | When | Says |
|---|---|---|
| `Unavailable(detail)` | No session bus, or no portal on it | No file chooser is available on this desktop. |
| `Refused(detail)` | The portal would not show the dialog, or ended it with nothing | The desktop's file chooser would not open. |
| `NoAnswer` | The portal did not take the request within ten seconds (`set_timeout`) | The desktop's file chooser did not answer. |
| `Lost` | The portal, or the bus, went away while its dialog was open | The desktop's file chooser stopped while it was open. |
| `Failed(text)` | An app's own text, for a fake or a picker of its own | The text |

What the system said (GIO's `Cannot autolaunch D-Bus without X11 $DISPLAY`) is for a log: `error.detail()`, and the
error's `source()`. A QA script answers the next request with a failure or with files (`picker`, under Scripted QA). The app's code takes the trait, so its tests hand it a `FakeFilePicker`, which shows nothing,
answers what it was given (`fake.answer(Ok(vec![path]))`) and remembers what it was asked (`fake.asked()`). The
dialog is not tied to the app's window: that takes the window's Wayland handle, which is outside GTK's own crate.

## Toasts

```rust
shell.toast("Copied");
shell.add_toast(omui::Toast::new("Moved to Recently Deleted").seconds(10)
    .action("Undo", move || restore()).on_dismissed(move || forget_undo()));
```

Toasts stack at the bottom of the window, newest lowest, three at most: the rest wait their turn, and their time
starts when they appear. A toast without a button takes the place of another without one; a toast with a button stays
for as long as it said, whatever comes after it. `on_dismissed` runs once a toast
has gone, however it went: its time ran out, it was closed, a later toast replaced it, or its button was pressed
(after the button's action). A toast lasts five seconds unless `seconds` says otherwise.

## Banners

```rust
let banner = omui::Banner::new("Your iCloud sign-in expired.", Some("Sign in"));
banner.set_icon(Some(omui::icons::name::WARNING), "warning");
banner.set_button_suggested(true);
banner.connect_button_clicked(move || sign_in());
banner.set_revealed(true);
```

A `Banner` slides down across the top of its pane. Its icon is tinted by a class (`accent`, `warning`, `error`,
`success`); `set_button_enabled(false)` holds its button while what it starts is running, and `add_button` adds more.

## Dialogs

```rust
omui::confirm(&button, "Discard this draft?", "It cannot be brought back.", "Discard",
    omui::Appearance::Destructive, move || discard());
```

`Dialog` has the methods of `adw::AlertDialog` and `adw::Dialog`: a heading, a body, a widget of the app's
(`set_extra_child`, `set_content_size`), and responses (`add_response`, `set_response_appearance`,
`set_response_enabled`, `set_default_response`, `set_close_response`, `connect_response`). It is drawn over the
window's content, on the `Shell` overlay, because a second window would be tiled beside the first. While it is open
the content and the title bar take neither keys nor the pointer, and the window's shortcuts (a `gtk::ShortcutController`
in managed or global scope anywhere under the dialog, or any on the window) and the application's accelerators do
nothing; they act again once the last dialog has closed. `omui::dialog_open(&widget)` and `Shell::dialog_open` say
whether one is open, for whatever else an app starts from outside the window. Tab stays inside it, Escape gives the
close response, and Enter the default one: from an entry too, when the entry has `set_activates_default(true)`.
However a dialog is dismissed (Escape, its close button, `close()`, the close button of a dialog's own window, or
the window under it closing while it is open), its
handlers hear of it: with the response `set_close_response` named, else with `"cancel"` when the dialog has a response
of that id, else with `Dialog::CLOSE` (`"close"`). A handler acts on the responses it knows by name and treats any
other as the dialog having been put away, which is the answer that loses nothing.
`set_can_close(false)` makes Escape do nothing until `force_close`, which runs no handler. `set_response_closes(id, false)` keeps the dialog
open when that response is given, for an action on what it shows (Clear in a log): its handlers run with the dialog on
screen. The close response always closes, from its button as from Escape. A window that has no `Shell` gets a modal window
instead.

A dialog with a heading or a body is 420 pixels wide, margins included, so its text wraps at a readable width; it is
narrower in a narrower window. `set_content_size` with a width sizes it by the app's widget instead, and a dialog with
neither heading nor body is as wide as its widget. `set_close_button(true)` puts a close button beside the heading, as
a preferences or About dialog has; `omui::about(&parent, name, version, text, Some(website))` is such a dialog.
`confirm`'s Cancel is Alt+C.

`set_content_size` is the size the app's widget has where there is room, as libadwaita's content size is. In a
narrower window the widget is narrower; in one too low for the dialog it scrolls, and the heading and the responses
stay in view. A dialog with no heading takes its name for a screen reader from `set_title`.

A dialog owns its widgets, so a handler on one of them holds it weakly: `dialog.downgrade()`, or `#[weak] dialog` in
`glib::clone!`.

An open dialog follows a switch of language as the window under it does. `set_heading`, `set_body` and
`set_response_label` change what it says, and the `_translated` setters take English and translate it now and after
each switch; `bind` runs a closure now and after each switch, for text built with arguments. `confirm` and `about` do
this themselves: `confirm` translates the heading, body and button it is given (text already translated, or no
catalog's, shows as it is), and its Cancel. `confirmf` is `confirm` for words with a name in them: each is
translated and then filled from the arguments, again on each switch, with `confirm`'s Escape, close button and Enter.

```rust
omui::confirmf(&button, "Delete {}?", "{} will be gone from every device.", "_Delete", &[&name],
    omui::Appearance::Destructive, move || delete(id));
```

A `_translated` setter binds one thing: called again, it replaces its earlier text rather than adding to it, and the
plain setter (`set_heading`, `set_title`, `set_label` after `bind_label`) ends it, so what shows after a switch is
the newest text given.

```rust
let dialog = omui::Dialog::new(Some("Delete the album?"), Some(""));
dialog.set_heading_translated("Delete the album?");
dialog.bind(move |dialog| dialog.set_body(&omui::trn!("{n} photos stay in the library.", count)));
dialog.add_response("delete", "_Delete");
dialog.set_response_translated("delete", "_Delete");
```

Escape closes the dialog on top wherever the keyboard is: GTK leaves the focus with no widget when the focused one is
disabled or removed (a row's Put Back button that takes its row away), and the dialog has Escape, Enter and Tab from
the window until it has the keyboard back.

## Panes and pages

```rust
let split = omui::Split::new(&sidebar, &content, (240, 320), Some(600));
list.connect_row_activated(glib::clone!(#[weak] split, move |_, _| split.set_show_content(true)));
split.connect_changed(glib::clone!(#[weak] back, move |split| back.set_visible(split.is_collapsed())));
let shell = omui::Shell::new(&window, &split.widget);
```

`Split` puts a sidebar beside the content: a quarter of the width, within the two limits. Narrower than the
threshold, or than the two panes need, it is collapsed and shows one pane. It serves both ways libadwaita has of
using one:

- A list and its detail (`adw::NavigationSplitView`). Collapsed, it shows the list; `set_show_content(true)` shows
  the detail in its place, and Escape, Alt+Left or the app's back button (`set_show_content(false)`) the list again.
- A sidebar that is put away (`adw::OverlaySplitView`): `set_show_sidebar`, from a toggle button the app keeps in
  step in `connect_changed`. Collapsed, the content is shown while the sidebar is away.

The keyboard goes with the pane that is shown, and comes back to where it was. Tab starts in the sidebar.

`set_resizable(true)` puts a divider between the two panes while both are on screen. It is dragged, or takes the
keyboard in its turn after the sidebar, where Left and Right move it 16 pixels and Home and End to the sidebar's least
and most: the limits given to `Split::new`, and no more than leaves the content what it needs. A screen reader hears
the width and the limits. `sidebar_width()` is the width to save when the window closes, `set_sidebar_width(saved)`
restores it, and `connect_changed` runs after each move. A sidebar that changes what it shows (Notes' folder and note
panes, and its tree) takes new limits with `set_sidebar_limits((least, most))`, then `set_sidebar_width` with the width
saved for them: one divider serves both.

```rust
let split = omui::Split::new(&lists, &editor, (200, 500), Some(560));
split.set_resizable(true);
split.set_sidebar_width(saved.sidebar);
// and as the window closes:
saved.sidebar = split.sidebar_width();
```

`Nav` is a `gtk::Stack` with `push`, `pop` and `pop_to_root`; Escape and Alt+Left pop, wherever the keyboard is in
the window while a pushed page is on screen and no dialog is open. `tabs(&stack)` is a row of
tabs for a stack's pages, for a header bar; in a window under 700 pixels a tab with an icon shows the icon alone.
`on_narrow(&widget, 640, |narrow| …)` is for whatever else changes with the width. Both hear of a width from the
layouts that change it, so a window that is left alone runs nothing.

A `Split` and a `Nav` are handles: the widget does the work, and goes on doing it when the handle is dropped. A
handler on a widget inside one holds it weakly (`#[weak]` in `glib::clone!`, or `downgrade()`). A split can only
collapse if the window can get that narrow, so content that will not shrink (a wide row of buttons outside a
scrolled window) keeps it wide. A split's least height is the same at every width, what its panes need with their text
on one line, so wrapping text in a pane does not make GTK give a narrow window a least width wider than its tile. A
narrow pane given less height than its wrapped text needs is cut off at the bottom: text that must stay within reach
in a short window goes in a `gtk::ScrolledWindow`.

## Groups and rows

```rust
let (scroll, column) = omui::page(640);
let account = omui::Group::new("Account", "Where your files are kept.");
let (name_row, name) = omui::entry_row("Name", "Ada");
let (secret_row, secret) = omui::password_row("Secret access key");
let (sync_row, sync) = omui::switch_row("Sync settings too", "Themes and key bindings", true);
for row in [&name_row, &secret_row, &sync_row] {
    account.add(&row.widget);
}
column.append(&account.widget);
```

A `Group` is a title, a description, and a bordered list (`boxed-list`) of rows; an app's own `gtk::ListBox` with
that class takes the same rows. `Row` has a title, a subtitle, and widgets before and after them, and
`connect_activated` for a click or Enter. The rows that carry a control return it, a plain GTK widget, beside the row:
`gtk::Entry`, `gtk::PasswordEntry`, `gtk::Switch`, `gtk::DropDown`. Titles and subtitles are plain text, not markup.

Each changes in place: `set_title` and `set_description` on a `Group`, `set_title` and `set_subtitle` on a `Row` and an
`ExpanderRow`. Their `_translated` kin take the English and follow the language, as `bind_label` does, so a settings
page relabels on a switch and is not built again: `account.set_title_translated("Account")`.

Tab goes from row to row, and so do Up and Down: through a group, on to the next, and into an open `ExpanderRow`,
which shows more rows under it. A row that carries a control is one stop, the control, and a click anywhere on the
row goes to it. In a list that selects, the arrows are GTK's own and move the selection.

In a dialog, rows go in a `Group` with no title, and a whole page of groups is the dialog's widget:
`dialog.set_extra_child(Some(scroll.upcast_ref()))`.

## GTK's own widgets

`omui.css` has rules for the stock widgets an app is likely to use, so they need no classes: button, entry, search
and password entry, spin button, text view, check and radio button, switch, scale, drop-down and its list, menu
button, popover and popover menu, notebook, calendar, frame, expander, progress bar, tooltip, lists and their
selection, and the disabled state of each. A text view is bare, so that an editor can fill its pane; inside a `Frame`
it is a field of several lines.

The arrows, checks and steppers GTK draws for them are the bundled icons, whatever icon theme the desktop has. The
eye of a `gtk::PasswordEntry` is the bundled one too, and the same whether the text is shown or hidden, since
nothing tells CSS which; `password_row` has a button whose icon changes. In a drop-down the check is the one GTK's
own rows end with; a row from the app's factory keeps its images.

A menu button that is an icon (`gtk::MenuButton` with `icon_name`) is as wide as an icon button, and with
`has_frame(false)` is one in a header bar.

## Style classes

GTK and Adwaita names are kept where one exists.

- Buttons: `suggested-action` `destructive-action` `flat` `pill` `circular` `osd` `header-action` `chip` `tab`
- Text: `dim-label` `title-1` `title-2` `title-4` `heading` `caption` `numeric` `monospace` `error` `warning`
  `success` `accent` `page-title` `section-title` `meta` `status-line`
- Containers: `sidebar` `sidebar-row` `sidebar-footer` `toolbar` `result` `result-title` `snippet` `badge`
  `empty-state` `compact` `banner` `toasts` `toast` `om-shell` `search-large` `dialog` `dialog-backdrop`
  `dialog-header` `dialog-heading` `dialog-close` `dialog-body` `dialog-extra` `dialog-actions` `om-split` `collapsed`
  `om-divider` `dragging` `page` `tabs` `narrow` `group` `group-title`
  `group-description` `boxed-list` `om-row` `row-title` `row-subtitle` `property` `entry-row` `expander-row`
  `expanded` `avatar` `keys` `keycap`

Three of them mean something narrower than the same name in an app's old stylesheet:

- `sidebar` goes on the pane that holds the list and its footer, not on the list: it draws the pane's surface and
  its border. Rows come from `sidebar_row`.
- `empty-state` is `EmptyState`'s own box. One line of status where content will be ("Loading…", "Nothing found.",
  an error) is a label with `status-line`.
- Every list row gets omui's hover and selection tint. Padding belongs to the row's kind (`sidebar-row`, `result`),
  so a plain row keeps GTK's.

A sidebar list of `sidebar_row`s is one Tab stop with `omui::single_tab_stop(&list)`: Tab passes it in one step and
the arrows move inside it, so a selection that follows the keyboard does not load each page Tab goes by.

## Working on it

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

The test that needs GTK running (sizes, toasts, what a dialog holds back) is listed as ignored there: it runs on a
broadway display, so that it neither shows anything nor reads the settings of the desktop it is run from.
`dev/gtk-tests.sh` starts one with a private home and runs it:

```sh
dev/gtk-tests.sh
```

The tests that need no display use a temporary directory for everything they read: a theme, a language file, a
`hyprctl` that is a script, an event socket and a portal of their own. `.github/workflows/test.yml` runs all of it on
every push, in an Arch container: the format check, clippy, both sets of tests, and the gallery driven by the QA
scripts below.

`examples/gallery.rs` shows every widget. `dev/preview.sh` runs it in a nested Hyprland with a private home and
session bus, so nothing appears on your desktop, and writes a screenshot:

```sh
cargo build --example gallery
dev/preview.sh /tmp/dark.png
dev/preview.sh /tmp/light.png /usr/share/omarchy/themes/catppuccin-latte
OMUI_GALLERY_PAGE=icons dev/preview.sh /tmp/live.png /usr/share/omarchy/themes/gruvbox /usr/share/omarchy/themes/white
OMUI_GALLERY_OPEN=rename dev/preview.sh /tmp/dialog.png
OMUI_PREVIEW_SIZE=480x820 OMUI_GALLERY_PAGE=split dev/preview.sh /tmp/narrow.png
```

The fourth form switches the theme while the gallery runs and writes `/tmp/live-switched.png` as well. The fifth
opens one of the gallery's dialogs, its drop-down or a menu for the picture; `examples/gallery.rs` lists them. The
last is a narrow window, where the splits show one pane and the tabs their icons.

`dev/keys.sh` walks the gallery with the keyboard alone, in the same nested desktop: Tab, Enter, Escape, Alt+Left and
mnemonics in its dialogs, rows, tabs, pages and splits, wide and narrow, and the window's shortcuts under a dialog. It
checks where the focus went, what was answered, which shortcuts acted, how each split and stack of pages changed, and
that a narrow window keeps its width. It needs `wtype`.

The icons under `data/icons` are generated from Lucide by `dev/outline` and committed; its source says how to run it.

## Scripted QA

An app built with feature `qa` and run with `OMUI_QA=1` takes commands on stdin, one JSON object a line, and answers
each, in order, on the open file descriptor `OMUI_QA_FD` names (above 2; dev/qa.sh opens 3), so what the app prints
on stdout or stderr itself never mixes with them: `{"do": …, "ok": true, …}`, or `"ok": false` with an `error`. Without
`OMUI_QA_FD` it exits with 2. The first failure ends the
run, and the exit status is 1; so does a script that ends without `quit`, and a panic ends it at once with 101. Any
GLib warning or critical fails the command it came in (`logged`), even one logged as the windows close after `quit`.
Two are let pass: GTK's complaint that there is no accessibility bus, and broadway's one warning of a duplicate frame
timing (below). `omui::application` starts it, so an app needs nothing but the feature, which a release build leaves
off: it brings `serde_json`. It runs on a broadway display alone, and exits with 2 on any other.

An app that builds its own `gtk::Application` (to handle `open`, say, for `mailto:` links) starts it itself, before
`run`, with what `omui::application` does: `OMUI_QA` read once, any display but broadway refused as the application
starts, and commands read from the window the first `activate` or `open` opened, after the app's own handlers:

```rust
#[cfg(feature = "qa")] // the app's own feature, `qa = ["omui/qa"]`
omui::qa::attach(&app);
```

```sh
cargo build --example gallery --features qa
dev/qa.sh target/debug/examples/gallery dev/qa/gallery.jsonl
dev/qa.sh target/debug/examples/gallery dev/qa/icons.jsonl
```

`dev/qa.sh` runs the app on a broadway display of its own with a private home and no session bus, so no window
opens anywhere, lets the app's stdout and stderr through, prints each command with its answer, and exits with the
app's status. Lines starting with `#` are
comments. `dev/qa/critical.jsonl` must fail: it has the gallery make GTK log a critical.

A widget is named by its accessible `role` (GTK's nick: `button`, `toggle-button`, `checkbox`, `switch`, `text-box`,
`search-box`, `list-item`, `dialog`, `alert`, `separator`, …) and `name`, and optionally `in` an ancestor named the
same way: `{"role": "dialog", "name": "Rename the folder"}`. Its name is the text of the labels it is labelled by
(`Relation::LabelledBy`, as the control of `entry_row`, `password_row`, `switch_row` and `combo_row` is by the row's
title), its accessible label, its label, title or placeholder, its tooltip, or for a button, row, list item, grid
cell, group, dialog or toast any label shown inside it. It is looked for in every
window, a dialog's own modal window too (over a window with no `Shell`). Of several, the innermost on screen is meant; more than one is an error once it has stayed so for the command's `timeout_ms` (a page going while the next comes is waited out). A command that acts waits up to `timeout_ms` (5000) for its widget to
be on screen, and every command waits for the window to be drawn after the last.

| Command | Does | Answers |
|---|---|---|
| `{"do":"click", role, name}` | Clicks a button (a menu button or drop-down on the button inside it, so that its menu opens at once), chooses an item of a list that takes one click for a choice (a drop-down's), or activates a row, switch or other widget as Enter would; fails on one that is hidden, disabled or under a dialog, in its window or a modal window | |
| `{"do":"focus", role, name}` | Gives it the keyboard | |
| `{"do":"type", "text":"…"}` | Types the text as key presses, into the widget named if one is | |
| `{"do":"key", "keys":"<Control>u"}` | Presses a key with its modifiers (GTK accelerator syntax), as a keyboard would: a letter in upper case with Shift held, for `R` or `<Shift>r` alike and with other modifiers too (`<Control>U` is Ctrl+Shift+U, where GTK reads Ctrl+u), and once a window on screen has the keyboard | |
| `{"do":"action", "name":"win.save", "parameter":"…"}` | Activates a `win.` or `app.` action; the parameter is GVariant text | |
| `{"do":"set", role, name, "checked":true}` | Sets a switch, check box or toggle button | |
| `{"do":"select", role, name}` | Selects what it is or is in: a `GtkListBox` row, or an item of a `GtkListView` or `GtkGridView` (`list-item`, `grid-cell`) through the view's selection model, as a click would; fails if that model leaves it unselected (`GtkNoSelection`) | |
| `{"do":"query", role, name}` | | `role` `name` `visible` `sensitive` `focused` `text` (a drop-down's: the string it shows as chosen) `checked` `selected` (list box rows, view items) `width` `height` |
| `{"do":"state"}` | | `size` of the app's window; of all windows the `dialog` on top (its name, or null), the `toasts` showing, the `focus` in the window that has the keyboard (a drop-down or menu button for the button inside it, as GTK names that button) |
| `{"do":"wait", …}` | Waits until each other key equals what `query` (with a role or name) or `state` answers (a number may be asked for as a range, `"height": {"min": 30, "max": 60}`, with either bound alone too), or a widget is on screen; one that is not there is `"visible": false` | The last answer |
| `{"do":"resize", "width":560, "height":700}` | Sets the window's size | |
| `{"do":"shot", "path":"x.png"}` | Writes the window to a PNG through `dev::shot`, as it is drawn next, with the menus and lists open in it; a spinner on screen is first given the 0.6 s it takes to fade in | |
| `{"do":"list", role}` | | The `widgets` on screen with a name, as `{role, name}`: for writing a script |
| `{"do":"language", "code":"da"}` | Switches the language as Omarchy would, open windows and dialogs with it; with `"catalog":"dev/qa/da.json"` first registers that file as the language's catalog, and with `"made_up":true` a made-up one, every English string registered (the app's and omui's) as `[da] Save`, placeholders and mnemonics kept | `language` |
| `{"do":"picker", "fail":"unavailable"}` | Answers the next request of any of omui's file pickers in place of the portal: with a failure (`unavailable`, `refused`, `no-answer`, `lost`, `cancel`), or with `"files":["/tmp/a.txt"]`; once each, in order | |
| `{"do":"close"}` | Closes every window of the application as its close button would, once the answer is out, without ending the run: a window that asks first asks, and the app ends with its last window as it would (once the work it holds on for is done), with its own exit status. What GLib logs as it goes still fails the run. The commands after it run only if the app is still there | |
| `{"do":"quit"}` | Closes the windows and ends the run; a script must end with it, or with a `close` the app ends on | |

A relative path a script gives, a `language` command's `catalog` or a `picker` command's `files`, is read beside
the script first, where it is written, whatever directory the app is run from (an app's runner often runs it from
the one its shots go to): the script is the file `OMUI_QA_SCRIPT` names, which `dev/qa.sh` sets, or else the file
the app's stdin is read from. A runner that hands the app a rewritten copy of the script sets `OMUI_QA_SCRIPT` to
the original. When there is no such file beside the script, the path is read from the working directory as before,
so an absolute path, or a file copied there, still works. A `shot`'s path is the working directory's, as ever.

Keys reach the app through the broadway display's input, as its web page sends them, so they go through GTK's own
handling: shortcuts, mnemonics, a dialog holding the window's keys back. `dev/qa.sh` sets `OMUI_QA_INPUT` to it. A
key is sent once a window on screen has the keyboard: when a window closes, broadway gives it back to the one under
it a moment later, and when the window that had it was only hidden, the topmost window is given it, as a click would.
A key's release goes to whichever window has the keyboard then, one the key opened too, or to none when the key
closed its window. A menu that is open has the keyboard, as on a desktop: Down moves in it, and Escape closes it.

Broadway draws each window as one picture in QA mode (a CSS `filter` on each window, popover and tooltip): GTK 4.24's gtk4-broadwayd
aborts, its page connected, on a frame that draws anew one icon twice, as a list built again or a resized window does
(`append_node_ops: assertion failed: (!reused_node->consumed)`), since the images of one icon share one node and
broadway sends a node it sent before by reference. The pixels are the same. `dev/qa/icons.jsonl` is such a frame, and
`dev/qa.sh` prints the display's log when the display dies under the app.

GDK's broadway backend warns once a run of a duplicate frame timing, and once of a frame submitted twice (a popover
shares its window's frame clock), when a page answers its frames, as this one does (`gdk_frame_timings_submitted()`,
domain Gdk); those warnings are let pass while the page is connected. So is GTK's warning that a spinner's
animation was handed a frame time before its own (`gtk_svg_advance: time went backward`, domain Gtk).

## License

MIT. The bundled icons are Lucide's, under ISC, and MIT for those Lucide derives from Feather. See
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for Strata and Lucide.
