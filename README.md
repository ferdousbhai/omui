# omui

The shared look and widgets of a family of Omarchy desktop apps. Plain GTK 4 in Rust: no libadwaita, and no
dependency beyond `gtk4`.

An omui window takes its colours from Omarchy's current theme and follows a theme switch while it runs. It looks the
same whatever GTK theme or `gtk.css` the desktop has, and sits beside [Strata](https://github.com/lgse/strata), whose
theming approach it follows.

## Using it

```toml
[dependencies]
omui = { git = "https://github.com/ferdousbhai/omui", rev = "<40-hex>" } # v0.3.0
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

## What is in 0.3

| | |
|---|---|
| `application`, `main`, `background` | One-window application with the theme installed; a binary that is a window with no arguments and a command line with some; work on a thread with the result on the main loop |
| `Theme`, `Tokens`, `blend`, `omarchy_state_dir` | The installed look, its colours, and a listener for what CSS cannot reach |
| `icons::name::*`, `icon`, `icon_button`, `data_uri` | 67 bundled Lucide icons as GTK symbolic icons |
| `Shell`, `header`, `Title`, `claim_titlebar` | The window's overlay with toasts, and its header bar |
| `Dialog`, `WeakDialog`, `confirm`, `Appearance` | A dialog drawn inside its window, and the question most of them are |
| `Split`, `Nav`, `tabs`, `on_narrow` | A sidebar beside content that becomes one pane in a narrow window; pages pushed one over another; the pages of a stack as tabs; a widget's width crossing a threshold |
| `clamp`, `page` | A child kept to a readable width; a scrolling column of groups |
| `Group`, `Row`, `entry_row`, `password_row`, `switch_row`, `combo_row`, `ExpanderRow` | A titled, bordered list and the rows of a settings page or a form |
| `label`, `badge`, `avatar`, `spinner`, `sidebar_row`, `clear`, `set_class` | Small builders |
| `Banner`, `EmptyState` | A message bar; what an empty page or list shows |
| `dev::shot` (feature `dev`) | A widget written to a PNG, for before-and-after pictures |

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
a reset, GTK's built-in theme, `omui.css`, the app's CSS, then the tokens. The reset covers the whole display, so
omui and libadwaita cannot share a process. A widget `omui.css` has no rule for keeps the built-in theme's look, dark
or light as the palette is.

## Dialogs

```rust
omui::confirm(&button, "Discard this draft?", "It cannot be brought back.", "Discard",
    omui::Appearance::Destructive, move || discard());
```

`Dialog` has the methods of `adw::AlertDialog` and `adw::Dialog`: a heading, a body, a widget of the app's
(`set_extra_child`, `set_content_size`), and responses (`add_response`, `set_response_appearance`,
`set_response_enabled`, `set_default_response`, `set_close_response`, `connect_response`). It is drawn over the
window's content, on the `Shell` overlay, because a second window would be tiled beside the first. While it is open
the content and the title bar take neither keys nor the pointer. Tab stays inside it, Escape gives the close response,
and Enter the default one: from an entry too, when the entry has `set_activates_default(true)`. `set_can_close(false)`
makes Escape do nothing until `force_close`. A window that has no `Shell` gets a modal window instead.

In a window too small for the dialog the app's widget scrolls, and the heading and the responses stay in view. A
dialog with no heading takes its name for a screen reader from `set_title`.

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

`Nav` is a `gtk::Stack` with `push`, `pop` and `pop_to_root`; Escape and Alt+Left pop. `tabs(&stack)` is a row of
tabs for a stack's pages, for a header bar; in a window under 700 pixels a tab with an icon shows the icon alone.
`on_narrow(&widget, 640, |narrow| …)` is for whatever else changes with the width.

A `Split` and a `Nav` are `Rc`s that the app keeps, as it keeps its `Shell`: the widget alone does not follow the
width or answer the keys. A split can only collapse if the window can get that narrow, so content that will not
shrink (a wide row of buttons outside a scrolled window) keeps it wide.

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

Tab goes from row to row. A row that carries a control is one stop, the control, and a click anywhere on the row
goes to it. `ExpanderRow` opens to show more rows under it.

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
  `empty-state` `compact` `banner` `toast` `om-shell` `search-large` `dialog` `dialog-backdrop` `dialog-heading`
  `dialog-body` `dialog-extra` `dialog-actions` `om-split` `collapsed` `page` `tabs` `narrow` `group` `group-title`
  `group-description` `boxed-list` `om-row` `row-title` `row-subtitle` `property` `entry-row` `expander-row`
  `expanded` `avatar`

Three of them mean something narrower than the same name in an app's old stylesheet:

- `sidebar` goes on the pane that holds the list and its footer, not on the list: it draws the pane's surface and
  its border. Rows come from `sidebar_row`.
- `empty-state` is `EmptyState`'s own box. One line of status where content will be ("Loading…", "Nothing found.",
  an error) is a label with `status-line`.
- Every list row gets omui's hover and selection tint. Padding belongs to the row's kind (`sidebar-row`, `result`),
  so a plain row keeps GTK's.

## Working on it

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
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
mnemonics in its dialogs, rows, tabs, pages and splits, wide and narrow. It checks where the focus went, what was
answered, and how each split and stack of pages changed. It needs `wtype`.

The icons under `data/icons` are generated from Lucide by `dev/outline` and committed; its source says how to run it.

## License

MIT. The bundled icons are Lucide's, under ISC, and MIT for those Lucide derives from Feather. See
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for Strata and Lucide.
