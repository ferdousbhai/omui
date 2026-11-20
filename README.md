# omui

The shared look and widgets of a family of Omarchy desktop apps. Plain GTK 4 in Rust: no libadwaita, and no
dependency beyond `gtk4`.

An omui window takes its colours from Omarchy's current theme and follows a theme switch while it runs. It looks the
same whatever GTK theme or `gtk.css` the desktop has, and sits beside [Strata](https://github.com/lgse/strata), whose
theming approach it follows.

## Using it

```toml
[dependencies]
omui = { git = "https://github.com/ferdousbhai/omui", rev = "<40-hex>" } # v0.5.2
```

Building needs `glib-compile-resources` (on Arch it is part of `glib2`).

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

`omui::application` goes first in `main`: it takes `GTK_THEME` out of the environment, because that variable loads a
desktop theme under omui's whatever else is set. An app that builds its own `gtk::Application` and calls
`Theme::install` removes the variable itself, before GTK starts.

A packaged app carries the Strata and Lucide notices with it, since its binary holds code adapted from Strata and
the Lucide icons: ship [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)'s two entries and the texts under
`data/licenses/` among the app's own notices.

## What is in 0.5

| | |
|---|---|
| `application`, `main`, `background` | One-window application with the theme installed; a binary that is a window with no arguments and a command line with some; work on a thread with the result on the main loop |
| `Theme`, `Tokens`, `blend`, `omarchy_state_dir`, `APP_PRIORITY` | The installed look, its colours, a listener for what CSS cannot reach, and where an app's own stylesheets go |
| `icons::name::*`, `icon`, `icon_button`, `data_uri` | 74 bundled Lucide icons as GTK symbolic icons |
| `icon_menu_button` | A menu button drawn as an icon button: a header bar's main menu |
| `Shell`, `Toast`, `header`, `header_without_title`, `Title`, `claim_titlebar` | The window's overlay with toasts, and its header bar |
| `Dialog`, `WeakDialog`, `confirm`, `about`, `dialog_open`, `Appearance` | A dialog drawn inside its window, the question most of them are, an About dialog, and whether one is open |
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
Omarchy the palette is Tokyo Night. The interface font is JetBrains Mono at 13 px times the desktop's text scale; an
app's CSS may set another `font-family` on a widget or a pane.

The colours are the theme's own, with one adjustment: secondary text and the status colours are moved toward the
text colour when they would not read on the background (4.5:1 and 3:1), which several light themes need.

In CSS the colours are GTK named colours, tinted only with `alpha()`:

`@om_bg` `@om_surface` `@om_text` `@om_dim_text` `@om_accent` `@om_danger` `@om_success` `@om_warning` `@om_muted`
`@om_highlight` `@om_border`

App stylesheets use these and no hex, so they follow every theme. Stylesheets are layered above GTK's user priority:
a reset, GTK's built-in theme, `omui.css`, the app's CSS, then the tokens. A `gtk::CssProvider` the app adds as it runs
(a zoom, a font from its settings) goes at `omui::APP_PRIORITY`, beside the app's CSS: at GTK's
`STYLE_PROVIDER_PRIORITY_APPLICATION` the reset unsets all it says. The reset covers the whole display, so
omui and libadwaita cannot share a process. A widget `omui.css` has no rule for keeps the built-in theme's look, dark
or light as the palette is.

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
`set_can_close(false)` makes Escape do nothing until `force_close`. `set_response_closes(id, false)` keeps the dialog
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
`on_narrow(&widget, 640, |narrow| …)` is for whatever else changes with the width.

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
  `expanded` `avatar`

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
| `{"do":"shot", "path":"x.png"}` | Writes the window to a PNG through `dev::shot`, as it is drawn next | |
| `{"do":"list", role}` | | The `widgets` on screen with a name, as `{role, name}`: for writing a script |
| `{"do":"quit"}` | Closes the windows and ends the run; a script must end with it | |

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
