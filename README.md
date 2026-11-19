# omui

The shared look and widgets of a family of Omarchy desktop apps. Plain GTK 4 in Rust: no libadwaita, and no
dependency beyond `gtk4`.

An omui window takes its colours from Omarchy's current theme and follows a theme switch while it runs. It looks the
same whatever GTK theme or `gtk.css` the desktop has, and sits beside [Strata](https://github.com/lgse/strata), whose
theming approach it follows.

## Using it

```toml
[dependencies]
omui = { git = "https://github.com/ferdousbhai/omui", rev = "<40-hex>" } # v0.2.0
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

## What is in 0.2

| | |
|---|---|
| `application`, `main`, `background` | One-window application with the theme installed; a binary that is a window with no arguments and a command line with some; work on a thread with the result on the main loop |
| `Theme`, `Tokens`, `blend`, `omarchy_state_dir` | The installed look, its colours, and a listener for what CSS cannot reach |
| `icons::name::*`, `icon`, `icon_button`, `data_uri` | 38 bundled Lucide icons as GTK symbolic icons |
| `Shell`, `header`, `Title`, `claim_titlebar` | The window's overlay with toasts, and its header bar |
| `Dialog`, `WeakDialog`, `confirm`, `Appearance` | A dialog drawn inside its window, and the question most of them are |
| `label`, `badge`, `sidebar_row`, `clear`, `set_class` | Small builders |
| `Banner`, `EmptyState` | A message bar; what an empty page or list shows |
| `dev::shot` (feature `dev`) | A widget written to a PNG, for before-and-after pictures |

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

## GTK's own widgets

`omui.css` has rules for the stock widgets an app is likely to use, so they need no classes: button, entry, search
and password entry, spin button, text view, check and radio button, switch, scale, drop-down and its list, menu
button, popover and popover menu, notebook, calendar, frame, expander, progress bar, tooltip, lists and their
selection, and the disabled state of each. A text view is bare, so that an editor can fill its pane; inside a `Frame`
it is a field of several lines.

The arrows, checks and steppers GTK draws for them are the bundled icons, whatever icon theme the desktop has. One
is not: the eye of a password entry is the icon theme's. In a drop-down the check is the one GTK's own rows end with;
a row from the app's factory keeps its images.

## Style classes

GTK and Adwaita names are kept where one exists.

- Buttons: `suggested-action` `destructive-action` `flat` `pill` `circular` `osd` `header-action` `chip`
- Text: `dim-label` `title-1` `title-2` `title-4` `heading` `caption` `numeric` `monospace` `error` `warning`
  `success` `accent` `page-title` `section-title` `meta` `status-line`
- Containers: `sidebar` `sidebar-row` `sidebar-footer` `toolbar` `result` `result-title` `snippet` `badge`
  `empty-state` `compact` `banner` `toast` `om-shell` `search-large` `dialog` `dialog-backdrop` `dialog-heading`
  `dialog-body` `dialog-extra` `dialog-actions`

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
```

The fourth form switches the theme while the gallery runs and writes `/tmp/live-switched.png` as well. The last
opens one of the gallery's dialogs, its drop-down or a menu for the picture; `examples/gallery.rs` lists them.

`dev/keys.sh` presses Tab, Enter, Escape and mnemonics in the gallery's dialogs, in the same nested desktop, and checks where
the focus went and what was answered. It needs `wtype`.

The icons under `data/icons` are generated from Lucide by `dev/outline` and committed; its source says how to run it.

## License

MIT. The bundled icons are Lucide's, under ISC, and MIT for those Lucide derives from Feather. See
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for Strata and Lucide.
