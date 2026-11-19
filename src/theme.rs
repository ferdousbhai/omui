// SPDX-License-Identifier: MIT
//! The look: Omarchy's current theme turned into eleven semantic colours, applied as GTK CSS, and followed live.
//!
//! The approach, the token set and the Omarchy mapping follow Strata (github.com/lgse/strata, MIT, src/ui/theme*), so an
//! omui app sits beside it on an Omarchy desktop: the palette comes from `colors.toml` of the current theme, content
//! sits on the theme's darker background, JetBrains Mono is the interface font, sizes scale from a 13 px root with the
//! desktop's text size, and GTK's own theme is reset underneath so neither the desktop's GTK theme nor a user's
//! `gtk.css` can reshape the widgets. Without Omarchy, Tokyo Night.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    time::Duration,
};

use gtk::{gdk, gio, glib, prelude::*};

/// The colours every stylesheet rule is written against, each `#rrggbb`. In CSS they are `@om_bg`, `@om_surface`,
/// `@om_text`, `@om_accent`, `@om_danger`, `@om_success`, `@om_warning`, `@om_muted`, `@om_highlight`, `@om_border` and
/// `@om_dim_text`.
#[derive(Clone, Debug, PartialEq)]
pub struct Tokens {
    pub name: String,
    pub background: String,
    pub surface: String,
    pub text: String,
    pub accent: String,
    pub danger: String,
    pub success: String,
    pub warning: String,
    pub muted: String,
    pub highlight: String,
    pub border: String,
    pub dim_text: String,
    pub light: bool,
}

const TOKYO_NIGHT: &str = r##"mode = "dark"
accent = "#7aa2f7"
selection = "#292e42"
muted = "#414868"
background = "#1a1b26"
dark_background = "#13141c"
lighter_background = "#24283b"
foreground = "#a9b1d6"
red = "#f7768e"
green = "#9ece6a"
yellow = "#e0af68""##;

impl Tokens {
    /// Tokyo Night, Strata's default, for a desktop without Omarchy.
    pub fn fallback() -> Tokens {
        Tokens::from_colors("tokyo-night", TOKYO_NIGHT).expect("the bundled palette parses")
    }

    /// Strata's `tokens_from_quattro` in its default "darker" form: the keys of an Omarchy `colors.toml` to semantic
    /// colours. None when background, foreground or accent is missing or not a hex colour.
    ///
    /// Unlike Strata's, the colours that are drawn as text are kept readable on the background: several light themes
    /// have a soft foreground and pastel status colours, and their darker background takes the rest.
    pub fn from_colors(name: &str, colors_toml: &str) -> Option<Tokens> {
        let values = parse_lines(colors_toml);
        let get = |key: &str| values.get(key).copied().and_then(canonical);
        let source_background = get("background")?;
        let text = get("foreground")?;
        let accent = get("accent")?;
        let background = get("dark_background").unwrap_or_else(|| source_background.clone());
        let light = match values.get("mode").copied() {
            Some("light") => true,
            Some("dark") => false,
            _ => luminance(&source_background) > 0.45,
        };
        // A theme without its own green or yellow gets a fixed hue pulled a quarter of the way to its text colour, so
        // the two sit in the palette instead of on top of it.
        let toned = |hue: &str| blend(hue, &text, 0.25);
        let status = |color: String| readable(&color, &background, &text, STATUS_CONTRAST);
        Some(Tokens {
            name: name.to_owned(),
            danger: status(get("red").or_else(|| get("color1")).unwrap_or_else(|| "#e5484d".into())),
            success: status(
                get("green")
                    .or_else(|| get("color2"))
                    .unwrap_or_else(|| toned("#46a758")),
            ),
            warning: status(
                get("yellow")
                    .or_else(|| get("color3"))
                    .unwrap_or_else(|| toned("#ffb224")),
            ),
            // Some themes set lighter_background equal to background, which would hide hover.
            muted: get("lighter_background")
                .filter(|l| *l != source_background)
                .unwrap_or_else(|| blend(&source_background, &text, 0.10)),
            highlight: get("selection").unwrap_or_else(|| blend(&source_background, &accent, 0.25)),
            border: get("muted").unwrap_or_else(|| blend(&source_background, &text, 0.36)),
            dim_text: readable(
                &blend(&source_background, &text, 0.62),
                &background,
                &text,
                TEXT_CONTRAST,
            ),
            surface: background.clone(),
            background,
            text,
            accent,
            light,
        })
    }
}

/// The `key = "value"` lines of an Omarchy `colors.toml`, which is all that file holds: no tables, no escapes. Reading
/// them here spares every app a TOML parser.
fn parse_lines(source: &str) -> HashMap<&str, &str> {
    source
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            let key = key.trim();
            let value = value.trim();
            if key.is_empty() || key.starts_with('#') {
                return None;
            }
            let value = match value.chars().next() {
                // A quoted value ends at its quote: colours start with `#`, so a comment cannot be cut off first.
                Some(quote @ ('"' | '\'')) => value[1..].split_once(quote)?.0,
                _ => value.split('#').next().unwrap_or_default().trim_end(),
            };
            Some((key, value))
        })
        .collect()
}

/// `#rgb`, `#rrggbb` or `#rrggbbaa` (alpha dropped) as channels.
fn channels(color: &str) -> Option<[u8; 3]> {
    let hex = color.trim().strip_prefix('#')?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let pair = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    match hex.len() {
        3 => {
            let digit = |i: usize| u8::from_str_radix(&hex[i..=i], 16).ok().map(|v| v * 17);
            Some([digit(0)?, digit(1)?, digit(2)?])
        }
        6 | 8 => Some([pair(0)?, pair(2)?, pair(4)?]),
        _ => None,
    }
}

fn canonical(value: &str) -> Option<String> {
    channels(value).map(|[r, g, b]| format!("#{r:02x}{g:02x}{b:02x}"))
}

/// The colour `t` of the way from `a` to `b` (both hex), as `#rrggbb`. When either cannot be read, `b`.
pub fn blend(a: &str, b: &str, t: f64) -> String {
    let (Some(l), Some(r)) = (channels(a), channels(b)) else {
        return b.to_owned();
    };
    let ch = |i: usize| (f64::from(l[i]) + (f64::from(r[i]) - f64::from(l[i])) * t).round() as u8;
    format!("#{:02x}{:02x}{:02x}", ch(0), ch(1), ch(2))
}

fn luminance(color: &str) -> f64 {
    let c = channels(color).unwrap_or([0, 0, 0]).map(|v| {
        let v = f64::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    });
    c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722
}

/// WCAG's floor for body text, which secondary text is.
const TEXT_CONTRAST: f64 = 4.5;
/// WCAG's floor for large text and for graphics: a status colour is a word or two, an icon, or a border.
const STATUS_CONTRAST: f64 = 3.0;

/// The WCAG contrast ratio of two colours, 1 to 21.
fn contrast(a: &str, b: &str) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// `color`, moved toward `text` a tenth at a time until it reads at `ratio` on `background`. A colour that already
/// does is returned as it is, and none ends up past `text`.
fn readable(color: &str, background: &str, text: &str, ratio: f64) -> String {
    (0..=10)
        .map(|step| blend(color, text, f64::from(step) / 10.0))
        .find(|candidate| contrast(candidate, background) >= ratio)
        .unwrap_or_else(|| text.to_owned())
}

/// `$HOME`, read from the environment each time: glib caches its own answer.
fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(glib::home_dir)
}

/// Where Omarchy keeps its current theme: `theme.name` and the `theme` directory. Omarchy's scripts spell the path
/// `$HOME/.local/state/omarchy/current` whatever `XDG_STATE_HOME` says, so this does too.
pub fn omarchy_state_dir() -> PathBuf {
    home().join(".local/state/omarchy/current")
}

fn read_omarchy() -> Option<Tokens> {
    let state = omarchy_state_dir();
    let name = std::fs::read_to_string(state.join("theme.name")).unwrap_or_else(|_| "omarchy".into());
    let colors = std::fs::read_to_string(state.join("theme/colors.toml")).ok()?;
    Tokens::from_colors(name.trim(), &colors)
}

/// Strata waits 75 ms; a theme switch here is several renames and a rewrite of theme.name.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// Omarchy's palette, followed live. It needs a main loop but no display, so the reload path is tested without one.
struct Palette {
    tokens: RefCell<Tokens>,
    monitors: RefCell<Vec<gio::FileMonitor>>,
    pending: RefCell<Option<glib::SourceId>>,
    changed: Box<dyn Fn(&Tokens)>,
}

impl Palette {
    /// `changed` runs after a switch, and only when the tokens differ.
    fn follow(changed: impl Fn(&Tokens) + 'static) -> Rc<Self> {
        let palette = Rc::new(Self {
            tokens: RefCell::new(read_omarchy().unwrap_or_else(Tokens::fallback)),
            monitors: RefCell::new(Vec::new()),
            pending: RefCell::new(None),
            changed: Box::new(changed),
        });
        palette.watch();
        palette
    }

    fn reload(&self) {
        // Omarchy's state can be unreadable mid-switch: the palette stays until it can be read again. Only a desktop
        // with no Omarchy state at all gets the fallback.
        let Some(next) = read_omarchy().or_else(|| (!omarchy_state_dir().exists()).then(Tokens::fallback)) else {
            return;
        };
        if next != *self.tokens.borrow() {
            self.tokens.replace(next.clone());
            (self.changed)(&next);
        }
    }

    /// Omarchy replaces the theme directory by `mv` and rewrites theme.name on a switch. Watched: the directory the
    /// theme path resolves to (its identity, not the stable spelling), the state directory, and its parents below the
    /// home directory, so a state directory that appears later is seen too.
    fn watch(self: &Rc<Self>) {
        let state = omarchy_state_dir();
        let home = home();
        let mut dirs: Vec<PathBuf> = std::fs::canonicalize(state.join("theme")).ok().into_iter().collect();
        dirs.extend(state.ancestors().take_while(|p| *p != home).map(PathBuf::from));
        let mut monitors = Vec::new();
        for dir in dirs.into_iter().filter(|d| d.is_dir()) {
            let file = gio::File::for_path(&dir);
            let Ok(monitor) = file.monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) else {
                continue;
            };
            let weak = Rc::downgrade(self);
            monitor.connect_changed(move |_, _, _, _| {
                if let Some(palette) = weak.upgrade() {
                    palette.schedule();
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
            if let Some(palette) = weak.upgrade() {
                palette.pending.take();
                palette.reload();
                palette.watch(); // the theme directory a switch replaced is a new directory
            }
        });
        self.pending.replace(Some(id));
    }
}

// ------------------------------------------------------------------------------------------------ applying it to GTK --

/// Above GTK's user priority every property is unset and GTK's built-in theme restated, so `~/.config/gtk-4.0/gtk.css`
/// cannot decide any of them (Strata's layering). USER+6 is kept free for a user stylesheet of omui's own.
const RESET_PRIORITY: u32 = gtk::STYLE_PROVIDER_PRIORITY_USER + 1;
const BASE_PRIORITY: u32 = RESET_PRIORITY + 1;
const OMUI_PRIORITY: u32 = BASE_PRIORITY + 1;
const APP_PRIORITY: u32 = OMUI_PRIORITY + 1;
const TOKEN_PRIORITY: u32 = APP_PRIORITY + 1;
const OMUI_CSS: &str = include_str!("omui.css");
const ROOT_FONT_PX: f64 = 13.0;
/// An empty theme in omui's resources (data/omui.gresource.xml). GTK's own theme provider sits below the reset, so a
/// real theme there would only cost selector matching.
const EMPTY_GTK_THEME: &str = "OmEmpty";

thread_local! {
    static CURRENT: RefCell<Option<Rc<Theme>>> = const { RefCell::new(None) };
}

type Listener = Rc<dyn Fn(&Tokens)>;

/// The installed look of this process. [`crate::application`] installs it; the window reads it through [`Theme::get`].
pub struct Theme {
    base: gtk::CssProvider,
    tokens_css: gtk::CssProvider,
    palette: Rc<Palette>,
    /// Each with the id its anchor's destroy handler removes it by.
    listeners: RefCell<Vec<(u64, Listener)>>,
    next_listener: Cell<u64>,
}

impl Theme {
    /// Styles every window of `display`: the reset, GTK's built-in theme, omui's stylesheet, `app_css`, then the
    /// tokens, each above the one before. The reset is display-wide, so a process is either omui's or libadwaita's.
    /// One theme per process: a later call returns the first.
    ///
    /// `GTK_THEME` in the environment loads that theme whatever is pinned here. [`crate::application`] removes it; a
    /// caller that builds its own application removes it before GTK starts.
    pub fn install(display: &gdk::Display, app_css: &str) -> Rc<Theme> {
        if let Some(theme) = Theme::get() {
            return theme;
        }
        crate::icons::register();
        gtk::IconTheme::for_display(display).add_resource_path(crate::icons::RESOURCE_PATH);
        let theme = Rc::new_cyclic(|weak: &std::rc::Weak<Theme>| {
            let weak = weak.clone();
            Theme {
                base: gtk::CssProvider::new(),
                tokens_css: gtk::CssProvider::new(),
                palette: Palette::follow(move |tokens| {
                    if let Some(theme) = weak.upgrade() {
                        theme.apply();
                        // Cloned out first: a listener may add or drop listeners.
                        let listeners: Vec<_> = theme.listeners.borrow().iter().map(|(_, f)| f.clone()).collect();
                        for listener in listeners {
                            listener(tokens);
                        }
                    }
                }),
                listeners: RefCell::new(Vec::new()),
                next_listener: Cell::new(0),
            }
        });
        let fixed = |css: &str, priority: u32| {
            let provider = gtk::CssProvider::new();
            provider.load_from_string(css);
            gtk::style_context_add_provider_for_display(display, &provider, priority);
        };
        fixed("* { all: unset; }", RESET_PRIORITY);
        gtk::style_context_add_provider_for_display(display, &theme.base, BASE_PRIORITY);
        fixed(OMUI_CSS, OMUI_PRIORITY);
        fixed(app_css, APP_PRIORITY);
        gtk::style_context_add_provider_for_display(display, &theme.tokens_css, TOKEN_PRIORITY);
        if let Some(settings) = gtk::Settings::default() {
            // Values an application sets outrank the settings portal and settings.ini for the whole process. They are
            // set even when they already match, because only application values survive a later portal update.
            settings.set_gtk_theme_name(Some(EMPTY_GTK_THEME));
            set_enum(&settings, "gtk-interface-contrast", "no-preference");
            // Automatic rendering hints glyph outlines but not their metrics, which costs glyphs their top pixel row at
            // some sizes (GNOME/gtk#8395).
            set_enum(&settings, "gtk-font-rendering", "manual");
            let weak = Rc::downgrade(&theme);
            settings.connect_gtk_xft_dpi_notify(move |_| {
                // the desktop's text size changed
                if let Some(theme) = weak.upgrade() {
                    theme.apply();
                }
            });
        }
        theme.apply();
        CURRENT.with(|current| current.replace(Some(theme.clone())));
        theme
    }

    /// The installed theme, on the main thread.
    pub fn get() -> Option<Rc<Theme>> {
        CURRENT.with(|current| current.borrow().clone())
    }

    pub fn tokens(&self) -> Tokens {
        self.palette.tokens.borrow().clone()
    }

    /// Runs `f` after each theme switch that changes the tokens, for what CSS cannot reach (a web view, a Cairo
    /// drawing). The listener lives as long as `anchor`; `f` must not hold `anchor` strongly, or neither is freed.
    pub fn on_change<W: IsA<gtk::Widget>>(self: &Rc<Self>, anchor: &W, f: impl Fn(&Tokens) + 'static) {
        let id = self.next_listener.replace(self.next_listener.get() + 1);
        self.listeners.borrow_mut().push((id, Rc::new(f)));
        let weak = Rc::downgrade(self);
        anchor.connect_destroy(move |_| {
            if let Some(theme) = weak.upgrade() {
                theme.listeners.borrow_mut().retain(|(listener, _)| *listener != id);
            }
        });
    }

    /// The desktop's text size, as GTK reads it from the settings portal (gtk-xft-dpi, 96 dpi = 1.0).
    pub fn text_scale() -> f64 {
        gtk::Settings::default()
            .map(|s| s.gtk_xft_dpi())
            .filter(|dpi| *dpi > 0)
            .map(|dpi| (f64::from(dpi) / 1024.0 / 96.0).clamp(0.5, 3.0))
            .unwrap_or(1.0)
    }

    /// The size every `em` in the stylesheets resolves against.
    pub fn root_font_px() -> f64 {
        (ROOT_FONT_PX * Self::text_scale()).round()
    }

    fn apply(&self) {
        let t = self.tokens();
        let root = Self::root_font_px();
        let scheme = if t.light { "light" } else { "dark" };
        // The built-in theme's variant follows the palette, not the desktop. Before GTK 4.20 the variants are two
        // files. Since then both names hold one sheet that chooses by media query, and a provider answers the query
        // from its own properties, which nothing sets for a provider an application made.
        set_enum(&self.base, "prefers-color-scheme", scheme);
        set_enum(&self.base, "prefers-contrast", "no-preference");
        self.base.load_from_string(&format!(
            "@import url(\"resource:///org/gtk/libgtk/theme/Default/{}\");",
            if t.light { "gtk.css" } else { "gtk-dark.css" }
        ));
        if let Some(settings) = gtk::Settings::default() {
            // Whatever else asks GTK for the colour scheme gets the palette's too: the setting since GTK 4.20, the
            // dark-theme flag before.
            if settings.find_property("gtk-interface-color-scheme").is_some() {
                set_enum(&settings, "gtk-interface-color-scheme", scheme);
            } else {
                settings.set_gtk_application_prefer_dark_theme(!t.light);
            }
            settings.set_gtk_font_name(Some(&format!("JetBrains Mono {root}px")));
        }
        let colors = [
            ("bg", &t.background),
            ("surface", &t.surface),
            ("text", &t.text),
            ("accent", &t.accent),
            ("danger", &t.danger),
            ("success", &t.success),
            ("warning", &t.warning),
            ("muted", &t.muted),
            ("highlight", &t.highlight),
            ("border", &t.border),
            ("dim_text", &t.dim_text),
        ];
        let mut css: String = colors
            .iter()
            .map(|(name, value)| format!("@define-color om_{name} {value};\n"))
            .collect();
        css.push_str(&format!(
            "window, popover, popover.background, tooltip {{ font-size: {root}px; }}\n"
        ));
        self.tokens_css.load_from_string(&css);
    }
}

/// Sets an enum property by its nick, skipping properties this GTK lacks (Strata's helper).
fn set_enum(object: &impl IsA<glib::Object>, property: &str, nick: &str) {
    let Some(spec) = object.find_property(property) else {
        return;
    };
    if let Some(value) = glib::EnumClass::with_type(spec.value_type()).and_then(|class| class.to_value_by_nick(nick)) {
        object.set_property(property, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_sits_on_the_dark_background() {
        let t = Tokens::from_colors("tokyo-night", TOKYO_NIGHT).unwrap();
        assert_eq!((t.background.as_str(), t.surface.as_str()), ("#13141c", "#13141c"));
        assert_eq!(
            (t.text.as_str(), t.accent.as_str(), t.border.as_str()),
            ("#a9b1d6", "#7aa2f7", "#414868")
        );
        assert_eq!((t.muted.as_str(), t.highlight.as_str()), ("#24283b", "#292e42"));
        assert_eq!(
            (t.danger.as_str(), t.success.as_str(), t.warning.as_str()),
            ("#f7768e", "#9ece6a", "#e0af68")
        );
        assert_eq!(t.name, "tokyo-night");
        assert!(!t.light);
    }

    #[test]
    fn colours_drawn_as_text_are_raised_until_they_read() {
        // Catppuccin Latte: a soft foreground, pastel status colours, and a background darker than the theme's own.
        let latte = "mode = \"light\"\nbackground = \"#eff1f5\"\ndark_background = \"#e3e4e8\"\n\
                     foreground = \"#4c4f69\"\naccent = \"#1e66f5\"\nred = \"#d20f39\"\ngreen = \"#40a02b\"\n\
                     yellow = \"#df8e1d\"";
        let t = Tokens::from_colors("catppuccin-latte", latte).unwrap();
        assert!(contrast(&blend("#eff1f5", "#4c4f69", 0.62), &t.background) < 3.0);
        assert!(contrast(&t.dim_text, &t.background) >= TEXT_CONTRAST, "{}", t.dim_text);
        assert!(contrast(&t.dim_text, &t.background) < contrast(&t.text, &t.background));
        assert_eq!(t.danger, "#d20f39", "a colour that reads is the theme's own");
        for status in [&t.success, &t.warning] {
            assert!(contrast(status, &t.background) >= STATUS_CONTRAST, "{status}");
        }
        // A foreground that cannot reach the ratio itself is as far as a colour goes.
        assert_eq!(readable("#999999", "#888888", "#777777", TEXT_CONTRAST), "#777777");
    }

    #[test]
    fn missing_keys_are_derived() {
        let t = Tokens::from_colors(
            "x",
            "background = \"#ffffff\"\nforeground = \"#000000\"\naccent = \"#0000ff\"",
        )
        .unwrap();
        assert!(t.light, "a light background with no mode reads as light");
        assert_eq!((t.background.as_str(), t.surface.as_str()), ("#ffffff", "#ffffff"));
        assert_eq!(t.muted, "#e6e6e6");
        assert_eq!(t.highlight, "#bfbfff");
        assert_eq!(t.border, "#a3a3a3");
        assert_eq!(t.dim_text, "#616161");
        assert_eq!(t.danger, "#e5484d");
        assert_eq!(
            (t.success.as_str(), t.warning.as_str()),
            (blend("#46a758", "#000000", 0.25).as_str(), "#bf861b")
        );
    }

    #[test]
    fn ansi_colours_stand_in_and_mode_outranks_luminance() {
        let source = "mode = \"dark\"\nbackground = \"#fff\"\nforeground = \"#000\"\naccent = \"#00f\"\n\
                      color1 = \"#aa0000\"\ncolor2 = \"#00aa00\"\ncolor3 = \"#777700\"\nlighter_background = \"#ffffff\"";
        let t = Tokens::from_colors("x", source).unwrap();
        assert!(!t.light);
        assert_eq!(
            (t.danger.as_str(), t.success.as_str(), t.warning.as_str()),
            ("#aa0000", "#00aa00", "#777700")
        );
        assert_eq!(
            t.muted, "#e6e6e6",
            "a lighter_background equal to the background is ignored"
        );
    }

    #[test]
    fn a_palette_without_its_required_colours_is_refused() {
        assert!(
            Tokens::from_colors(
                "x",
                "background = \"nonsense\"\nforeground = \"#000\"\naccent = \"#00f\""
            )
            .is_none()
        );
        assert!(Tokens::from_colors("x", "background = \"#000\"\nforeground = \"#fff\"").is_none());
        assert!(Tokens::from_colors("x", "").is_none());
    }

    #[test]
    fn blend_mixes_channels_and_falls_back_to_the_second_colour() {
        assert_eq!(blend("#000000", "#ffffff", 0.5), "#808080");
        assert_eq!(blend("#102030", "#fff", 0.0), "#102030");
        assert_eq!(blend("#102030", "#fff", 1.0), "#ffffff");
        assert_eq!(blend("#ff000080", "#0000ff", 0.5), "#800080");
        assert_eq!(blend("red", "#0000ff", 0.5), "#0000ff");
    }

    #[test]
    fn lines_are_read_as_keys_and_values() {
        let values = parse_lines(
            "# a comment\n\naccent = \"#7aa2f7\"  # trailing\n  foreground='#a9b1d6'\nmode = dark # bare\n\
             broken\n= \"#000\"\nopen = \"#123\ncolor0 = \"#1a1b26\"",
        );
        assert_eq!(values.get("accent"), Some(&"#7aa2f7"));
        assert_eq!(values.get("foreground"), Some(&"#a9b1d6"));
        assert_eq!(values.get("mode"), Some(&"dark"));
        assert_eq!(values.get("color0"), Some(&"#1a1b26"));
        assert_eq!(
            values.len(),
            4,
            "a line without a key, a value or a closing quote is skipped"
        );
    }

    /// GTK draws nothing for a file it cannot find, and says so only on stderr.
    #[test]
    fn every_icon_the_stylesheet_names_is_bundled() {
        crate::icons::register();
        let named: Vec<&str> = OMUI_CSS
            .split("url(\"resource://")
            .skip(1)
            .map(|rest| rest.split('"').next().unwrap())
            .collect();
        assert!(!named.is_empty());
        for path in named {
            assert!(
                gio::resources_lookup_data(path, gio::ResourceLookupFlags::NONE).is_ok(),
                "{path}"
            );
        }
    }

    fn write_theme(dir: &std::path::Path, background: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let colors = format!("background = \"{background}\"\nforeground = \"#808080\"\naccent = \"#3366cc\"\n");
        std::fs::write(dir.join("colors.toml"), colors).unwrap();
    }

    /// The one test that touches the environment and the file system: Omarchy's switch, replayed in a scratch home.
    /// No display is involved.
    #[test]
    fn a_theme_swapped_by_mv_is_followed() {
        let scratch = std::env::temp_dir().join(format!("omui-theme-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        // SAFETY: no other test in this binary reads or writes the environment, and this runs before this test starts
        // any glib thread.
        unsafe { std::env::set_var("HOME", &scratch) };
        let state = omarchy_state_dir();
        assert_eq!(state, scratch.join(".local/state/omarchy/current"));
        write_theme(&state.join("theme"), "#101010");
        std::fs::write(state.join("theme.name"), "night\n").unwrap();

        // glib's timeouts and file monitors report on the default context; no other test runs it.
        let context = glib::MainContext::default();
        let _guard = context.acquire().unwrap();
        let seen = Rc::new(RefCell::new(Vec::<String>::new()));
        let palette = Palette::follow({
            let seen = seen.clone();
            move |tokens| seen.borrow_mut().push(format!("{} {}", tokens.name, tokens.background))
        });
        assert_eq!(palette.tokens.borrow().background, "#101010");
        // Runs the loop until `changes` switches have been reported, or for `quiet` when none is expected.
        let settle = |changes: usize, quiet: Duration| {
            let deadline = std::time::Instant::now()
                + if changes > seen.borrow().len() {
                    Duration::from_secs(10)
                } else {
                    quiet
                };
            while std::time::Instant::now() < deadline && (seen.borrow().len() < changes || changes == 0) {
                context.iteration(false);
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        let swap = |background: &str, name: &str| {
            write_theme(&state.join("next"), background);
            std::fs::rename(state.join("theme"), state.join("previous")).unwrap();
            std::fs::rename(state.join("next"), state.join("theme")).unwrap();
            std::fs::remove_dir_all(state.join("previous")).unwrap();
            std::fs::write(state.join("theme.name"), name).unwrap();
        };

        swap("#f0f0f0", "day");
        settle(1, Duration::ZERO);
        assert_eq!(*seen.borrow(), ["day #f0f0f0"]);
        assert!(palette.tokens.borrow().light);

        // And again: after a switch the monitors are re-armed, so the next one is seen too.
        swap("#202020", "dusk");
        settle(2, Duration::ZERO);
        assert_eq!(seen.borrow().last().unwrap(), "dusk #202020");

        // Mid-switch the theme is gone: the palette stays.
        std::fs::rename(state.join("theme"), state.join("previous")).unwrap();
        settle(0, DEBOUNCE * 4);
        assert_eq!(seen.borrow().len(), 2);
        assert_eq!(palette.tokens.borrow().background, "#202020");

        // No Omarchy state at all: Tokyo Night.
        std::fs::remove_dir_all(scratch.join(".local/state/omarchy")).unwrap();
        settle(3, Duration::ZERO);
        assert_eq!(*palette.tokens.borrow(), Tokens::fallback());
        std::fs::remove_dir_all(&scratch).unwrap();
    }
}
