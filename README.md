# omui

The shared look and widgets of a family of Omarchy desktop apps. Plain GTK 4 in Rust: no libadwaita, and no
dependency beyond `gtk4`.

An omui window takes its colours from Omarchy's current theme and follows a theme switch while it runs. It looks the
same whatever GTK theme or `gtk.css` the desktop has, and sits beside [Strata](https://github.com/lgse/strata), whose
theming approach it follows.

## Using it

```toml
[dependencies]
omui = { git = "https://github.com/ferdousbhai/omui", rev = "<40-hex>" } # v0.1.0
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

## What is in 0.1

| | |
|---|---|
| `application`, `main`, `background` | One-window application with the theme installed; a binary that is a window with no arguments and a command line with some; work on a thread with the result on the main loop |
| `Theme`, `Tokens`, `blend`, `omarchy_state_dir` | The installed look, its colours, and a listener for what CSS cannot reach |
| `icons::name::*`, `icon`, `icon_button`, `data_uri` | 37 bundled Lucide icons as GTK symbolic icons |
| `Shell`, `header`, `Title`, `claim_titlebar` | The window's overlay with toasts, and its header bar |
| `label`, `badge`, `sidebar_row`, `clear`, `set_class` | Small builders |
| `Banner`, `EmptyState` | A message bar; what an empty page or list shows |
| `dev::shot` (feature `dev`) | A widget written to a PNG, for before-and-after pictures |

## Theme

Colours come from `$XDG_STATE_HOME/omarchy/current/theme/colors.toml` (`~/.local/state` by default). Without Omarchy
the palette is Tokyo Night. The interface font is JetBrains Mono at 13 px times the desktop's text scale.

In CSS the colours are GTK named colours, tinted only with `alpha()`:

`@om_bg` `@om_surface` `@om_text` `@om_dim_text` `@om_accent` `@om_danger` `@om_success` `@om_warning` `@om_muted`
`@om_highlight` `@om_border`

App stylesheets use these and no hex, so they follow every theme. Stylesheets are layered above GTK's user priority:
a reset, GTK's built-in theme, `omui.css`, the app's CSS, then the tokens. The reset covers the whole display, so
omui and libadwaita cannot share a process.

## Style classes

GTK and Adwaita names are kept where one exists.

- Buttons: `suggested-action` `destructive-action` `flat` `pill` `circular` `osd` `header-action` `chip`
- Text: `dim-label` `title-1` `title-2` `title-4` `heading` `caption` `numeric` `monospace` `error` `warning`
  `success` `accent` `page-title` `section-title` `meta`
- Containers: `sidebar` `sidebar-row` `sidebar-footer` `toolbar` `result` `result-title` `snippet` `badge`
  `empty-state` `compact` `banner` `toast` `om-shell` `search-large`

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
```

The last form switches the theme while the gallery runs and writes `/tmp/live-switched.png` as well.

The icons under `data/icons` are generated from Lucide by `dev/outline` and committed; its source says how to run it.

## License

MIT. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for Strata and Lucide.
