# Third-party notices

omui includes or derives from the following projects.

## Strata

- Project: <https://github.com/lgse/strata>
- Copyright: (c) 2026 LGSE Ltd.
- License: MIT
- Full license: [`data/licenses/Strata-MIT.txt`](data/licenses/Strata-MIT.txt)

omui follows Strata's way of theming GTK 4 on an Omarchy desktop, and parts of it are adapted from Strata's source:

- `src/theme.rs`: the stylesheet layering above GTK's user priority (reset, built-in theme restated, application
  styles, tokens), the pinned empty GTK theme and the pinned contrast and font-rendering settings, the choice of the
  built-in theme's variant, the `set_enum_setting` helper, the mapping from an Omarchy `colors.toml` to semantic
  colours (`tokens_from_quattro`, in its "darker" form), and watching the theme directory's identity for a switch.
- `data/omui.gresource.xml`, `data/theme-empty.css`: the empty theme registered under GTK's theme resource path.
- `src/shell.rs`: `claim_titlebar`.
- `src/omui.css`: the visual conventions (surface chrome with hairline borders, accent tints for hover and selection,
  accent focus outlines, thin accent scrollbars), and pointing GTK's own arrows at bundled icons with
  `-gtk-icon-source`. The rules themselves are omui's.
- `src/dialog.rs`: a dialog as a layer over the window's content with a backdrop, in place of a second window. The
  code is omui's.
- The icons are bundled the way Strata bundles its `-symbolic` ones: Lucide strokes outlined as fills.

## Lucide

- Project: <https://github.com/lucide-icons/lucide>
- Version: 1.35.0 (`lucide-static`)
- Copyright: Lucide Icons and Contributors; for the icons derived from Feather, Cole Bemis
- License: ISC, and MIT for the icons derived from Feather
- Included assets: the icons under `data/icons/scalable/actions/`
- Full license: [`data/licenses/Lucide-ISC.txt`](data/licenses/Lucide-ISC.txt)

The files keep Lucide's geometry. Each 2px round stroke is outlined as a filled path by `dev/outline`, because GTK
before 4.20 recolours only the fill of a symbolic icon, and the foreground colour is GTK's symbolic foreground in
place of `currentColor`. `om-star-filled-symbolic.svg` also fills the inside of Lucide's star. The files are renamed
with an `om-` prefix and a `-symbolic` suffix.
