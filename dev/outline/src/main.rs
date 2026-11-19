// SPDX-License-Identifier: MIT
//! Turns Lucide's stroked icons into the filled `-symbolic` files under `data/icons`.
//!
//! GTK before 4.20 recolours only the fill of a symbolic icon, so a Lucide file (`fill="none" stroke="currentColor"`)
//! draws as a blob there. Each 2 px round stroke is expanded to its outline here, by the Skia stroker usvg carries, and
//! written as plain filled paths.
//!
//!   cargo run --manifest-path dev/outline/Cargo.toml -- <lucide>/icons data/icons/scalable/actions
//!
//! `<lucide>` is an unpacked `lucide-static` npm package of the version THIRD_PARTY_NOTICES.md names.

use std::{fmt::Write, fs, path::Path};

use usvg::tiny_skia_path::{self as skia, PathSegment};

/// Lucide name, then how it is drawn. Bundled as `om-<name>[-filled]-symbolic.svg`; `src/icons.rs` names each one.
const ICONS: &[(&str, Draw)] = &[
    ("book-open", Draw::Outline),
    ("calendar", Draw::Outline),
    ("check", Draw::Outline),
    ("chevron-down", Draw::Outline),
    ("chevron-left", Draw::Outline),
    ("chevron-right", Draw::Outline),
    ("chevron-up", Draw::Outline),
    ("circle-check", Draw::Outline),
    ("clock", Draw::Outline),
    ("cloud", Draw::Outline),
    ("cloud-off", Draw::Outline),
    ("copy", Draw::Outline),
    ("external-link", Draw::Outline),
    ("file-text", Draw::Outline),
    ("folder", Draw::Outline),
    ("image", Draw::Outline),
    ("info", Draw::Outline),
    ("mail", Draw::Outline),
    ("map-pin", Draw::Outline),
    ("maximize", Draw::Outline),
    ("minus", Draw::Outline),
    ("panel-left", Draw::Outline),
    ("pencil", Draw::Outline),
    ("plus", Draw::Outline),
    ("refresh-cw", Draw::Outline),
    ("search", Draw::Outline),
    ("settings", Draw::Outline),
    ("square-check", Draw::Outline),
    ("star", Draw::Outline),
    ("star", Draw::Filled),
    ("trash-2", Draw::Outline),
    ("triangle-alert", Draw::Outline),
    ("user", Draw::Outline),
    ("video", Draw::Outline),
    ("x", Draw::Outline),
    ("zoom-in", Draw::Outline),
    ("zoom-out", Draw::Outline),
];

#[derive(Clone, Copy, PartialEq)]
enum Draw {
    Outline,
    /// The shape's inside as well as its stroke (a starred star).
    Filled,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [source, out] = args.as_slice() else {
        eprintln!("usage: omui-outline <lucide icons directory> <output directory>");
        std::process::exit(2);
    };
    for (name, draw) in ICONS {
        let svg =
            fs::read_to_string(Path::new(source).join(format!("{name}.svg"))).unwrap_or_else(|e| panic!("{name}: {e}"));
        let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let mut paths = Vec::new();
        outline(tree.root(), *draw, &mut paths);
        let suffix = if *draw == Draw::Filled { "-filled" } else { "" };
        let mut file = format!(
            "<!-- Lucide {name}, ISC license; its 2px round stroke outlined as a fill for GTK symbolic recolouring. -->\n\
             <svg xmlns=\"http://www.w3.org/2000/svg\" width=\"24\" height=\"24\" viewBox=\"0 0 24 24\" fill=\"#2e3436\">\n"
        );
        for d in paths {
            writeln!(file, "  <path d=\"{d}\"/>").unwrap();
        }
        file.push_str("</svg>\n");
        fs::write(Path::new(out).join(format!("om-{name}{suffix}-symbolic.svg")), file).unwrap();
    }
}

fn outline(group: &usvg::Group, draw: Draw, out: &mut Vec<String>) {
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => outline(g, draw, out),
            usvg::Node::Path(p) => {
                let Some(data) = p.data().clone().transform(p.abs_transform()) else {
                    continue;
                };
                if draw == Draw::Filled {
                    out.push(path_data(&data));
                }
                let Some(s) = p.stroke() else { continue };
                let stroke = skia::Stroke {
                    width: s.width().get(),
                    miter_limit: s.miterlimit().get(),
                    line_cap: match s.linecap() {
                        usvg::LineCap::Butt => skia::LineCap::Butt,
                        usvg::LineCap::Round => skia::LineCap::Round,
                        usvg::LineCap::Square => skia::LineCap::Square,
                    },
                    line_join: match s.linejoin() {
                        usvg::LineJoin::Round => skia::LineJoin::Round,
                        usvg::LineJoin::Bevel => skia::LineJoin::Bevel,
                        _ => skia::LineJoin::Miter,
                    },
                    dash: None,
                };
                // The resolution scale sets how finely round caps and joins are approximated; icons draw at 16 to 96 px.
                if let Some(stroked) = data.stroke(&stroke, 8.0) {
                    out.push(path_data(&stroked));
                }
            }
            _ => {}
        }
    }
}

fn path_data(path: &skia::Path) -> String {
    let mut d = String::new();
    let n = |v: f32| {
        let s = format!("{v:.3}");
        let s = s.trim_end_matches('0').trim_end_matches('.');
        if s == "-0" { "0".to_owned() } else { s.to_owned() }
    };
    for segment in path.segments() {
        match segment {
            PathSegment::MoveTo(p) => write!(d, "M{} {}", n(p.x), n(p.y)),
            PathSegment::LineTo(p) => write!(d, "L{} {}", n(p.x), n(p.y)),
            PathSegment::QuadTo(c, p) => write!(d, "Q{} {} {} {}", n(c.x), n(c.y), n(p.x), n(p.y)),
            PathSegment::CubicTo(a, b, p) => {
                write!(d, "C{} {} {} {} {} {}", n(a.x), n(a.y), n(b.x), n(b.y), n(p.x), n(p.y))
            }
            PathSegment::Close => write!(d, "Z"),
        }
        .unwrap();
    }
    d
}
