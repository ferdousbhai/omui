// SPDX-License-Identifier: MIT
//! How a window's panes are arranged and how they follow its width: what `adw::NavigationSplitView`,
//! `OverlaySplitView`, `NavigationView`, `ViewSwitcher`, `Clamp`, `PreferencesPage` and `Breakpoint` do.
//!
//! GTK has no signal for a widget's width, and a widget cannot be shown or hidden while the window is being laid
//! out. So whatever here follows a width acts once a layout has given the widget one: a split on the frame after
//! its layout was handed a width that calls for it, `on_narrow` and `tabs` after each layout of their window, which
//! the window's frame clock announces. Nothing looks once a frame: a window that is not being resized or redrawn
//! costs nothing.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use gtk::{gdk, glib, graphene, gsk, prelude::*, subclass::prelude::*};

/// Below this window width [`tabs`] shows icons alone.
const TABS_ICON_ONLY_BELOW: i32 = 700;

/// The share of a wide [`Split`] its sidebar takes, within the limits it was given (libadwaita's default).
const SIDEBAR_FRACTION: f64 = 0.25;

/// How far Left and Right move a [`Split`]'s divider.
const DIVIDER_STEP: i32 = 16;

/// Tells `f` whether `widget` is narrower than `below_px`: once when it first has a width, then each time it crosses.
/// `f` lives as long as the widget does, so it holds the widget itself, and whatever holds it, weakly.
///
/// The widget must be able to get that narrow: a child that cannot shrink keeps it, and the window, wide.
pub fn on_narrow(widget: &impl IsA<gtk::Widget>, below_px: i32, f: impl Fn(bool) + 'static) {
    watch_width(widget.as_ref(), |widget| widget.width(), below_px, f);
}

/// Follows `width` of `widget` across `below_px` without a timer or a tick callback, either of which would wake the
/// app sixty times a second for as long as the widget lived. A width only changes in a layout, and the frame clock
/// of the widget's window says when one has been done: it runs that phase when something asked for it and not
/// otherwise. The clock is the window's, so it is followed from when the widget is realized until it no longer is.
fn watch_width(widget: &gtk::Widget, width: fn(&gtk::Widget) -> i32, below_px: i32, f: impl Fn(bool) + 'static) {
    let was = Cell::new(None);
    let look = Rc::new(move |widget: &gtk::Widget| {
        let width = width(widget);
        // No width yet: the widget has not been laid out.
        if width > 0 {
            let narrow = width < below_px;
            if was.replace(Some(narrow)) != Some(narrow) {
                f(narrow);
            }
        }
    });
    let following: Rc<RefCell<Option<(gdk::FrameClock, glib::SignalHandlerId)>>> = Rc::default();
    let follow = {
        let (look, following) = (look.clone(), following.clone());
        move |widget: &gtk::Widget| {
            let Some(clock) = widget.frame_clock() else { return };
            let weak = widget.downgrade();
            let look = look.clone();
            // After GTK's own handler, which is the one that lays the window out, whichever was connected first. By
            // then the layout is over, so `f` may show and hide widgets: that asks for another layout, and the
            // clock runs one.
            let handler = clock.connect_local("layout", true, move |_| {
                if let Some(widget) = weak.upgrade() {
                    look(&widget);
                }
                None
            });
            if let Some((clock, handler)) = following.replace(Some((clock, handler))) {
                clock.disconnect(handler);
            }
        }
    };
    widget.connect_realize(follow.clone());
    widget.connect_unrealize(move |_| {
        if let Some((clock, handler)) = following.take() {
            clock.disconnect(handler);
        }
    });
    if widget.is_realized() {
        follow(widget);
        // Laid out already, and nothing may ask for a layout again for a while: looked at once, as soon as the
        // caller has finished setting up what `f` reaches.
        let weak = widget.downgrade();
        glib::idle_add_local_once(move || {
            if let Some(widget) = weak.upgrade() {
                look(&widget);
            }
        });
    }
}

/// What has the keyboard when it is inside `widget`.
fn focus_inside(widget: &gtk::Widget) -> Option<gtk::Widget> {
    let focus = widget.root()?.focus()?;
    (focus == *widget || focus.is_ancestor(widget)).then_some(focus)
}

/// Gives the keyboard to `returns_to` when that is on screen, else to the first widget in `page` that takes it.
fn give_focus(page: &gtk::Widget, returns_to: &glib::WeakRef<gtk::Widget>) {
    match returns_to.upgrade().filter(|widget| widget.is_mapped()) {
        Some(widget) => {
            widget.grab_focus();
        }
        None => {
            page.child_focus(gtk::DirectionType::TabForward);
        }
    }
}

/// Escape, or Alt and the left arrow: the keys that go back a page.
fn is_back(key: gdk::Key, modifiers: gdk::ModifierType) -> bool {
    let held =
        modifiers & (gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK | gdk::ModifierType::SHIFT_MASK);
    (key == gdk::Key::Escape && held.is_empty())
        || (matches!(key, gdk::Key::Left | gdk::Key::KP_Left) && held == gdk::ModifierType::ALT_MASK)
}

mod imp {
    use super::*;

    /// What a split layout tells its split: whether the width calls for one pane.
    pub type Told = Rc<dyn Fn(bool)>;

    /// Gives its widget's one child at most `maximum` pixels of width, centred in whatever more there is.
    #[derive(Default)]
    pub struct ClampLayout {
        pub maximum: Cell<i32>,
    }

    impl ClampLayout {
        fn child_width(&self, child: &gtk::Widget, available: i32) -> i32 {
            let (minimum, ..) = child.measure(gtk::Orientation::Horizontal, -1);
            available.min(self.maximum.get()).max(minimum)
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ClampLayout {
        const NAME: &'static str = "OmuiClampLayout";
        type Type = super::ClampLayout;
        type ParentType = gtk::LayoutManager;
    }

    impl ObjectImpl for ClampLayout {}

    impl LayoutManagerImpl for ClampLayout {
        fn request_mode(&self, _widget: &gtk::Widget) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, widget: &gtk::Widget, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(child) = widget.first_child().filter(|child| child.should_layout()) else {
                return (0, 0, -1, -1);
            };
            if orientation == gtk::Orientation::Horizontal {
                // Whatever height is asked about: the width comes first here, and the height follows from it.
                let (minimum, natural, ..) = child.measure(orientation, -1);
                (minimum, natural.min(self.maximum.get()).max(minimum), -1, -1)
            } else {
                // As tall as the child is at the width it will get.
                let available = if for_size < 0 {
                    child.measure(gtk::Orientation::Horizontal, -1).1
                } else {
                    for_size
                };
                child.measure(orientation, self.child_width(&child, available))
            }
        }

        fn allocate(&self, widget: &gtk::Widget, width: i32, height: i32, baseline: i32) {
            let Some(child) = widget.first_child().filter(|child| child.should_layout()) else {
                return;
            };
            let child_width = self.child_width(&child, width);
            place(
                &child,
                ((width - child_width) / 2).max(0),
                child_width,
                height,
                baseline,
            );
        }
    }

    /// Gives its widget's one child the whole of it, and asks for `size` pixels each way (-1: what the child asks
    /// for): the size the child has where there is room, and no less than the child can be.
    pub struct PreferredLayout {
        pub size: Cell<(i32, i32)>,
    }

    impl Default for PreferredLayout {
        fn default() -> Self {
            Self {
                size: Cell::new((-1, -1)),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PreferredLayout {
        const NAME: &'static str = "OmuiPreferredLayout";
        type Type = super::PreferredLayout;
        type ParentType = gtk::LayoutManager;
    }

    impl ObjectImpl for PreferredLayout {}

    impl LayoutManagerImpl for PreferredLayout {
        fn measure(&self, widget: &gtk::Widget, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(child) = widget.first_child().filter(|child| child.should_layout()) else {
                return (0, 0, -1, -1);
            };
            let (width, height) = self.size.get();
            let (preferred, for_size) = if orientation == gtk::Orientation::Horizontal {
                (width, for_size)
            } else if for_size < 0 && width >= 0 {
                // Asked for no width in particular: as tall as the child is at the preferred one.
                (height, width.max(minimum_width(&child)))
            } else {
                (height, for_size)
            };
            let (minimum, natural, ..) = child.measure(orientation, for_size);
            let natural = if preferred < 0 { natural } else { preferred.max(minimum) };
            (minimum, natural, -1, -1)
        }

        fn allocate(&self, widget: &gtk::Widget, width: i32, height: i32, baseline: i32) {
            if let Some(child) = widget.first_child().filter(|child| child.should_layout()) {
                child.allocate(width, height, baseline, None);
            }
        }
    }

    /// Lays out the two panes of a [`Split`](super::Split): its widget's first child holds the sidebar and its second
    /// the content. A third, when there is one, is the divider, over the edge between them. Which of them are on
    /// screen is the split's to say, by their child visibility; this gives them their places, and tells the split
    /// when the width it was given calls for the other arrangement.
    #[derive(Default)]
    pub struct SplitLayout {
        /// The least and the most the sidebar gets beside the content.
        pub sidebar_width: Cell<(i32, i32)>,
        /// The sidebar's width as the app set it or the divider was dragged to, in place of a share of the split.
        pub chosen: Cell<Option<i32>>,
        pub collapse_below: Cell<Option<i32>>,
        /// Whether the last width called for one pane, and what to tell when that changes.
        pub collapsed: Cell<bool>,
        pub on_collapsed: RefCell<Option<Told>>,
        /// The least, the most and the width a screen reader was last told of the divider; none for a new divider.
        pub divider_told: Cell<Option<(i32, i32, i32)>>,
    }

    impl SplitLayout {
        fn panes(widget: &gtk::Widget) -> Option<(gtk::Widget, gtk::Widget)> {
            let sidebar = widget.first_child()?;
            let content = sidebar.next_sibling()?;
            Some((sidebar, content))
        }

        /// The least the sidebar is beside the content: the app's limit, or what the sidebar itself needs.
        fn least_sidebar(&self, sidebar: &gtk::Widget) -> i32 {
            self.sidebar_width.get().0.max(minimum_width(sidebar))
        }

        /// The least and the most the sidebar can be in a split `width` wide: the limits, giving way to what the
        /// content needs.
        pub fn sidebar_limits(&self, sidebar: &gtk::Widget, content: &gtk::Widget, width: i32) -> (i32, i32) {
            let least = self.least_sidebar(sidebar);
            let most = self.sidebar_width.get().1.min(width - minimum_width(content));
            (least, most.max(least))
        }

        /// The sidebar's width in a split `width` wide: the width chosen for it, else a share of the split, within
        /// its limits.
        pub fn sidebar(&self, sidebar: &gtk::Widget, content: &gtk::Widget, width: i32) -> i32 {
            let (least, most) = self.sidebar_limits(sidebar, content, width);
            let share = (f64::from(width) * SIDEBAR_FRACTION) as i32;
            self.chosen.get().unwrap_or(share).clamp(least, most)
        }

        /// Where each pane on screen goes in a split `width` wide: its left edge and its width, which is never
        /// less than the pane can be. For the frame before the split collapses, the content hangs over the edge.
        ///
        /// The sidebar is where reading starts: on the left, or on the right where text runs right to left (`rtl`).
        fn places(
            &self,
            sidebar: &gtk::Widget,
            content: &gtk::Widget,
            width: i32,
            rtl: bool,
        ) -> Vec<(gtk::Widget, i32, i32)> {
            let whole = |pane: &gtk::Widget| (pane.clone(), 0, width.max(minimum_width(pane)));
            // Child visibility is the split's own switch, which `should_layout` does not look at.
            let on_screen = |pane: &gtk::Widget| in_use(pane) && pane.is_child_visible();
            match (on_screen(sidebar), on_screen(content)) {
                (true, true) => {
                    let beside = self.sidebar(sidebar, content, width);
                    let rest = (width - beside).max(minimum_width(content));
                    if rtl {
                        let edge = width - beside;
                        vec![(sidebar.clone(), edge, beside), (content.clone(), edge - rest, rest)]
                    } else {
                        vec![(sidebar.clone(), 0, beside), (content.clone(), beside, rest)]
                    }
                }
                (true, false) => vec![whole(sidebar)],
                (false, true) => vec![whole(content)],
                (false, false) => Vec::new(),
            }
        }

        /// Whether `width` is too little for both panes: under the app's threshold, or under what they need.
        fn collapses(&self, sidebar: &gtk::Widget, content: &gtk::Widget, width: i32) -> bool {
            self.collapse_below
                .get()
                .is_some_and(|below| width < below || width < self.least_sidebar(sidebar) + minimum_width(content))
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SplitLayout {
        const NAME: &'static str = "OmuiSplitLayout";
        type Type = super::SplitLayout;
        type ParentType = gtk::LayoutManager;
    }

    impl ObjectImpl for SplitLayout {}

    impl LayoutManagerImpl for SplitLayout {
        fn request_mode(&self, _widget: &gtk::Widget) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, widget: &gtk::Widget, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some((sidebar, content)) = Self::panes(widget) else {
                return (0, 0, -1, -1);
            };
            if orientation == gtk::Orientation::Vertical {
                // As tall as the taller pane on screen: naturally, at the width it gets; at the least, what it needs
                // at no width in particular, and so the same at every width. When a window's least height changes
                // with its width, GTK looks for its least size along the shape the window had before it was tiled
                // (gtk_window_compute_min_size), and a pane whose text wraps one line more in a narrow tile made the
                // window wider than the tile. So a narrow split may be given less height than its panes need: see
                // `allocate`.
                let mut tallest = (0, 0);
                let width = if for_size < 0 { widget.width() } else { for_size };
                for (pane, _, pane_width) in self.places(&sidebar, &content, width, false) {
                    let minimum = pane.measure(orientation, -1).0;
                    let natural = pane.measure(orientation, if for_size < 0 { -1 } else { pane_width }).1;
                    tallest = (tallest.0.max(minimum), tallest.1.max(natural.max(minimum)));
                }
                return (tallest.0, tallest.1, -1, -1);
            }
            // Whatever height is asked about: the width comes first here, and the height follows from it.
            let (content_minimum, content_natural, ..) = content.measure(orientation, -1);
            // Off screen while the split is wide, the sidebar is put away. Off screen in a collapsed split it
            // still counts: the split may be given the width to show it.
            if !in_use(&sidebar) || !sidebar.is_child_visible() && !self.collapsed.get() {
                return (content_minimum, content_natural, -1, -1);
            }
            let (sidebar_minimum, ..) = sidebar.measure(orientation, -1);
            let beside = self.least_sidebar(&sidebar);
            // One pane at a time is the least a split that collapses needs.
            let minimum = if self.collapse_below.get().is_some() {
                sidebar_minimum.max(content_minimum)
            } else {
                beside + content_minimum
            };
            (minimum, (beside + content_natural).max(minimum), -1, -1)
        }

        fn allocate(&self, widget: &gtk::Widget, width: i32, height: i32, baseline: i32) {
            let Some((sidebar, content)) = Self::panes(widget) else {
                return;
            };
            let rtl = widget.direction() == gtk::TextDirection::Rtl;
            let places = self.places(&sidebar, &content, width, rtl);
            if let Some(divider) = content.next_sibling() {
                // Centred on the edge between the panes, over both; off the split while one pane is on screen. The
                // edge is where the pane on the right starts.
                let divider_width = minimum_width(&divider);
                let edge = if places.len() == 2 {
                    places[usize::from(!rtl)].1
                } else {
                    -divider_width
                };
                divider.measure(gtk::Orientation::Vertical, divider_width);
                place(&divider, edge - divider_width / 2, divider_width, height, baseline);
                // Here, where the width they depend on is known: a width set before the window is first laid out,
                // or a window resized, changes them as surely as the divider moving does.
                let (least, most) = self.sidebar_limits(&sidebar, &content, width);
                let told = (least, most, self.sidebar(&sidebar, &content, width));
                if self.divider_told.replace(Some(told)) != Some(told) {
                    divider.update_property(&[
                        gtk::accessible::Property::ValueMin(f64::from(told.0)),
                        gtk::accessible::Property::ValueMax(f64::from(told.1)),
                        gtk::accessible::Property::ValueNow(f64::from(told.2)),
                    ]);
                }
            }
            for (pane, x, pane_width) in places {
                // Never less than the pane needs at its width, for what does not fit to be cut off at the bottom
                // rather than drawn over itself.
                let least = pane.measure(gtk::Orientation::Vertical, pane_width).0;
                place(&pane, x, pane_width, height.max(least), baseline);
            }
            let collapsed = self.collapses(&sidebar, &content, width);
            if self.collapsed.replace(collapsed) != collapsed
                && let Some(tell) = self.on_collapsed.borrow().clone()
            {
                // Told on the next frame: a pane cannot be shown or hidden while the window is being laid out.
                widget.add_tick_callback(move |_, _| {
                    tell(collapsed);
                    glib::ControlFlow::Break
                });
            }
        }
    }

    /// Whether the app shows the pane a holder holds.
    fn in_use(holder: &gtk::Widget) -> bool {
        holder.first_child().is_some_and(|pane| pane.is_visible())
    }

    fn minimum_width(widget: &gtk::Widget) -> i32 {
        widget.measure(gtk::Orientation::Horizontal, -1).0
    }

    fn place(child: &gtk::Widget, x: i32, width: i32, height: i32, baseline: i32) {
        let shift = gsk::Transform::new().translate(&graphene::Point::new(x as f32, 0.0));
        child.allocate(width, height, baseline, Some(shift));
    }
}

glib::wrapper! {
    pub struct ClampLayout(ObjectSubclass<imp::ClampLayout>) @extends gtk::LayoutManager;
}

glib::wrapper! {
    pub struct PreferredLayout(ObjectSubclass<imp::PreferredLayout>) @extends gtk::LayoutManager;
}

impl PreferredLayout {
    pub fn set_size(&self, width: i32, height: i32) {
        self.imp().size.set((width, height));
        self.layout_changed();
    }
}

glib::wrapper! {
    pub struct SplitLayout(ObjectSubclass<imp::SplitLayout>) @extends gtk::LayoutManager;
}

/// `child` at most `max_width` pixels wide, centred in a wider space (`adw::Clamp`): a column of text or a form
/// that would be hard to read across a whole window.
pub fn clamp(child: &impl IsA<gtk::Widget>, max_width: i32) -> gtk::Widget {
    let layout: ClampLayout = glib::Object::new();
    layout.imp().maximum.set(max_width);
    // A box for its `append` alone: the layout is the clamp's.
    let holder = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    holder.set_layout_manager(Some(layout));
    holder.append(child);
    holder.upcast()
}

/// A page that scrolls: a column (class `page`) at most `max_width` pixels wide, centred (`adw::PreferencesPage`).
/// Append [`crate::Group`]s and anything else to the column; the scrolled window is what goes into the window.
pub fn page(max_width: i32) -> (gtk::ScrolledWindow, gtk::Box) {
    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["page"])
        .build();
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&clamp(&column, max_width))
        .vexpand(true)
        .build();
    (scroll, column)
}

type Changed<T> = RefCell<Vec<Rc<dyn Fn(&T)>>>;

fn emit<T>(handlers: &Changed<T>, this: &T) {
    let handlers = handlers.borrow().clone();
    for handler in handlers {
        handler(this);
    }
}

/// The weak handle of a [`Split`] or a [`Nav`], and what lets `glib::clone!` make one for `#[weak]`.
macro_rules! weak_handle {
    ($handle:ident, $weak:ident, $state:ident) => {
        /// A handle that does not keep the widget alive: what a handler on a widget inside it holds, since the
        /// widget owns the handler. `glib::clone!` makes one for `#[weak]`.
        #[derive(Clone)]
        pub struct $weak(Weak<$state>);

        impl $weak {
            /// The handle, while the widget is alive.
            pub fn upgrade(&self) -> Option<$handle> {
                $handle::of(&self.0.upgrade()?)
            }
        }

        impl $handle {
            /// For a handler on a widget inside this one.
            pub fn downgrade(&self) -> $weak {
                $weak(Rc::downgrade(&self.state))
            }

            fn of(state: &Rc<$state>) -> Option<Self> {
                let widget = state.widget.upgrade()?;
                Some(Self {
                    widget,
                    state: state.clone(),
                })
            }
        }

        impl glib::clone::Downgrade for $handle {
            type Weak = $weak;

            fn downgrade(&self) -> $weak {
                $handle::downgrade(self)
            }
        }

        impl glib::clone::Upgrade for $weak {
            type Strong = $handle;

            fn upgrade(&self) -> Option<$handle> {
                $weak::upgrade(self)
            }
        }
    };
}

/// A sidebar beside the content (`adw::NavigationSplitView` and `adw::OverlaySplitView`). The sidebar is a quarter
/// of the width, within the limits given. Narrower than `collapse_below`, or than the two panes need, the split is
/// collapsed and shows one pane, which fills it: the sidebar, until the content is asked for or the sidebar is put
/// away. Escape and Alt+Left then go back from content that was asked for to the sidebar.
///
/// A list and its detail call [`Split::set_show_content`] when a row is chosen, and put a back button on the
/// content that is visible while [`Split::is_collapsed`]. A sidebar with a button that shows and hides it calls
/// [`Split::set_show_sidebar`], and keeps the button in step from [`Split::connect_changed`].
///
/// [`Split::set_resizable`] puts a divider between the panes that the pointer drags and the arrows move, within the
/// sidebar's limits; [`Split::sidebar_width`] is the width to save, and [`Split::set_sidebar_width`] restores it.
///
/// The least height it asks for is the same at every width, so that a window keeps to a narrow tile: it is what the
/// panes need with their text unwrapped, which is less than a narrow pane with wrapping text needs. A pane given less
/// than it needs is cut off at the bottom, so text that must stay within reach in a short window goes in a
/// `gtk::ScrolledWindow`, as a list does already.
///
/// This is a handle: the widget is the split and works without it. A handler on a widget inside the split holds a
/// [`WeakSplit`], or the two would keep each other alive.
#[derive(Clone)]
pub struct Split {
    pub widget: gtk::Box,
    state: Rc<SplitState>,
}

struct SplitState {
    /// Weak: the widget owns this, by its layout and its key handler.
    widget: glib::WeakRef<gtk::Box>,
    sidebar: gtk::Widget,
    content: gtk::Widget,
    collapsed: Cell<bool>,
    show_sidebar: Cell<bool>,
    show_content: Cell<bool>,
    /// What had the keyboard in the sidebar and in the content when each was last taken off screen with it.
    left_focus: [glib::WeakRef<gtk::Widget>; 2],
    /// The divider of [`Split::set_resizable`], while there is one.
    divider: RefCell<Option<gtk::Separator>>,
    /// While the handlers hear that the split collapsed or widened: what they ask for is put on screen once, after
    /// the last of them, with what the new width calls for.
    settling: Cell<bool>,
    changed: Changed<Split>,
}

weak_handle!(Split, WeakSplit, SplitState);

impl Split {
    /// `sidebar_width` is the least and the most the sidebar gets, in pixels. With no `collapse_below` the split
    /// never collapses.
    pub fn new(
        sidebar: &impl IsA<gtk::Widget>,
        content: &impl IsA<gtk::Widget>,
        sidebar_width: (i32, i32),
        collapse_below: Option<i32>,
    ) -> Self {
        let layout: SplitLayout = glib::Object::new();
        layout.imp().sidebar_width.set(sidebar_width);
        layout.imp().collapse_below.set(collapse_below);
        // A box for its `append` alone: the layout is the split's. The panes hang over its edge for the one frame
        // before it collapses, and are cut off there.
        let widget = gtk::Box::builder()
            .css_classes(["om-split"])
            .overflow(gtk::Overflow::Hidden)
            .build();
        widget.set_layout_manager(Some(layout.clone()));
        // Each pane sits in a bare holder. Before a window is first laid out GTK orders Tab by where the panes'
        // CSS boxes are centred, and a border on one side of the sidebar would put the content first.
        let hold = |pane: &gtk::Widget| {
            // Homogeneous, so its one child gets the whole width whether or not it asks to expand.
            let holder = gtk::Box::builder().homogeneous(true).build();
            holder.append(pane);
            widget.append(&holder);
            holder.upcast::<gtk::Widget>()
        };
        let (sidebar, content) = (hold(sidebar.as_ref()), hold(content.as_ref()));
        let state = Rc::new(SplitState {
            widget: widget.downgrade(),
            sidebar,
            content,
            collapsed: Cell::new(false),
            show_sidebar: Cell::new(true),
            show_content: Cell::new(false),
            left_focus: Default::default(),
            divider: RefCell::new(None),
            settling: Cell::new(false),
            changed: RefCell::default(),
        });

        layout.imp().on_collapsed.replace(Some(Rc::new({
            let state = state.clone();
            move |collapsed| {
                if let Some(split) = Split::of(&state)
                    && state.collapsed.replace(collapsed) != collapsed
                {
                    state.settling.set(true);
                    emit(&state.changed, &split);
                    state.settling.set(false);
                    split.apply();
                }
            }
        })));
        // Back from the content to the sidebar, where the content was a step forward from it.
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed({
            let state = state.clone();
            move |_, key, _, modifiers| match Split::of(&state) {
                Some(split)
                    if is_back(key, modifiers)
                        && split.is_collapsed()
                        && split.shows_sidebar()
                        && split.shows_content() =>
                {
                    split.set_show_content(false);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        widget.add_controller(keys);
        widget.add_controller(divider_drag(&state));
        Self { widget, state }
    }

    /// Whether the split is too narrow for both panes and shows one.
    pub fn is_collapsed(&self) -> bool {
        self.state.collapsed.get()
    }

    /// When collapsed: the content in place of the sidebar, or the sidebar again. Wide, both are there whatever
    /// this says, and it is remembered for when the split collapses.
    pub fn set_show_content(&self, on: bool) {
        if self.state.show_content.replace(on) != on {
            self.apply();
            emit(&self.state.changed, self);
        }
    }

    pub fn shows_content(&self) -> bool {
        self.state.show_content.get()
    }

    /// Puts the sidebar away, or brings it back. Collapsed, the content is shown while the sidebar is away.
    pub fn set_show_sidebar(&self, on: bool) {
        if self.state.show_sidebar.replace(on) != on {
            self.apply();
            emit(&self.state.changed, self);
        }
    }

    pub fn shows_sidebar(&self) -> bool {
        self.state.show_sidebar.get()
    }

    /// Runs `f` when the split collapses or widens, when either pane is shown or put away, and when the divider has
    /// been moved.
    pub fn connect_changed(&self, f: impl Fn(&Split) + 'static) {
        self.state.changed.borrow_mut().push(Rc::new(f));
    }

    /// A divider between the panes while both are on screen: dragged, or moved with Left and Right (Home and End go
    /// to the limits), it sets the sidebar's width within the least and the most [`Split::new`] or
    /// [`Split::set_sidebar_limits`] gave. A screen reader hears the width and the limits.
    pub fn set_resizable(&self, on: bool) {
        let state = &self.state;
        if state.divider.borrow().is_some() == on {
            return;
        }
        if let Some(divider) = state.divider.take() {
            divider.unparent();
            return;
        }
        let divider = gtk::Separator::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["om-divider"])
            .focusable(true)
            .cursor(&gdk::Cursor::from_name("col-resize", None).expect("GTK's cursor names"))
            .build();
        crate::i18n::bind(&divider, |divider| {
            divider.update_property(&[gtk::accessible::Property::Label(&crate::i18n::tr("Sidebar width"))]);
        });
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed({
            let state = Rc::downgrade(state);
            move |_, key, _, modifiers| {
                let Some(split) = state.upgrade().as_ref().and_then(Split::of) else {
                    return glib::Propagation::Proceed;
                };
                if modifiers.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK) {
                    return glib::Propagation::Proceed;
                }
                let (least, most) = split.sidebar_limits();
                let width = split.shown_sidebar_width();
                // The arrows move the divider the way they point, which widens a sidebar on the right the other way.
                let step = if split.widget.direction() == gtk::TextDirection::Rtl {
                    -DIVIDER_STEP
                } else {
                    DIVIDER_STEP
                };
                let to = match key {
                    gdk::Key::Left | gdk::Key::KP_Left => width - step,
                    gdk::Key::Right | gdk::Key::KP_Right => width + step,
                    gdk::Key::Home | gdk::Key::KP_Home => least,
                    gdk::Key::End | gdk::Key::KP_End => most,
                    _ => return glib::Propagation::Proceed,
                };
                split.resize(to);
                emit(&split.state.changed, &split);
                glib::Propagation::Stop
            }
        });
        divider.add_controller(keys);
        // Last, so that it is drawn over both panes and the pointer finds it there.
        divider.insert_before(&self.widget, gtk::Widget::NONE);
        self.layout().imp().divider_told.set(None);
        state.divider.replace(Some(divider));
        self.apply();
    }

    /// The sidebar's width to save: what the divider was moved to or [`Split::set_sidebar_width`] set, even while a
    /// narrower window gives it less; else the share of the split it has.
    pub fn sidebar_width(&self) -> i32 {
        self.layout()
            .imp()
            .chosen
            .get()
            .unwrap_or_else(|| self.shown_sidebar_width())
    }

    /// The sidebar's width beside the content at the split's width now.
    fn shown_sidebar_width(&self) -> i32 {
        let (sidebar, content) = (&self.state.sidebar, &self.state.content);
        self.layout().imp().sidebar(sidebar, content, self.widget.width())
    }

    /// New limits for the sidebar, in place of those [`Split::new`] was given: for a sidebar that changes what it
    /// shows, as Notes' does between its folder and note panes and its tree. The width chosen is kept, and is
    /// within the new limits on screen; [`Split::set_sidebar_width`] after this restores one saved for them.
    pub fn set_sidebar_limits(&self, sidebar_width: (i32, i32)) {
        self.layout().imp().sidebar_width.set(sidebar_width);
        self.widget.queue_resize();
    }

    /// The sidebar's width, kept within the sidebar's limits: a width saved from [`Split::sidebar_width`] when the
    /// window last closed.
    pub fn set_sidebar_width(&self, width: i32) {
        let (least, most) = self.layout().imp().sidebar_width.get();
        self.layout()
            .imp()
            .chosen
            .set(Some(width.clamp(least, most.max(least))));
        self.widget.queue_resize();
    }

    fn layout(&self) -> SplitLayout {
        self.widget
            .layout_manager()
            .and_downcast()
            .expect("a split's layout is its own")
    }

    fn sidebar_limits(&self) -> (i32, i32) {
        let (sidebar, content) = (&self.state.sidebar, &self.state.content);
        self.layout()
            .imp()
            .sidebar_limits(sidebar, content, self.widget.width())
    }

    /// Sets the sidebar's width from the divider, within what the split's width allows now: a drag past a limit then
    /// turns back at once.
    fn resize(&self, width: i32) {
        let (least, most) = self.sidebar_limits();
        self.layout().imp().chosen.set(Some(width.clamp(least, most)));
        self.widget.queue_resize();
    }

    /// Puts on screen the panes the state calls for, by their holders' child visibility: a pane taken off still
    /// counts toward the split's size.
    fn apply(&self) {
        let state = &self.state;
        if state.settling.get() {
            return;
        }
        let collapsed = state.collapsed.get();
        let sidebar = state.show_sidebar.get() && !(collapsed && state.show_content.get());
        let content = !(collapsed && sidebar);
        crate::set_class(&self.widget, "collapsed", collapsed);
        // A pane that goes with the keyboard in it hands the keyboard to the other, and gets it back where it was:
        // GTK would drop the focus, and the next Tab would start from the top of the window.
        let panes = [(&state.sidebar, sidebar), (&state.content, content)];
        let leaving = panes
            .iter()
            .position(|(pane, stays)| !stays && focus_inside(pane).is_some());
        if let Some(divider) = state.divider.borrow().as_ref() {
            // The divider leaves with either pane; the keyboard on it goes to the pane that stays.
            let staying = sidebar && content;
            if !staying && focus_inside(divider.upcast_ref()).is_some() {
                let stays = if sidebar { &state.sidebar } else { &state.content };
                stays.child_focus(gtk::DirectionType::TabForward);
            }
            divider.set_child_visible(staying);
        }
        if let Some(leaving) = leaving {
            state.left_focus[leaving].set(focus_inside(panes[leaving].0).as_ref());
        }
        state.sidebar.set_child_visible(sidebar);
        state.content.set_child_visible(content);
        self.widget.queue_resize();
        if let Some(leaving) = leaving {
            let other = 1 - leaving;
            give_focus(panes[other].0, &state.left_focus[other]);
        }
    }
}

/// Dragging the divider of [`Split::set_resizable`]. It is the split's, not the divider's: a drag's offsets are in
/// its widget's coordinates, and the divider moves under the pointer.
fn divider_drag(state: &Rc<SplitState>) -> gtk::GestureDrag {
    let drag = gtk::GestureDrag::new();
    // The sidebar's width as the drag began; none while the pointer went down elsewhere.
    let from = Rc::new(Cell::new(None));
    drag.connect_drag_begin({
        let (state, from) = (Rc::downgrade(state), from.clone());
        move |drag, x, y| {
            let split = state.upgrade().as_ref().and_then(Split::of);
            let divider = split.as_ref().and_then(|split| split.state.divider.borrow().clone());
            let (Some(split), Some(divider)) = (split, divider) else {
                return;
            };
            if split.widget.pick(x, y, gtk::PickFlags::DEFAULT).as_ref() != Some(divider.upcast_ref()) {
                drag.set_state(gtk::EventSequenceState::Denied);
                return;
            }
            drag.set_state(gtk::EventSequenceState::Claimed);
            from.set(Some(split.shown_sidebar_width()));
            divider.add_css_class("dragging");
        }
    });
    drag.connect_drag_update({
        let (state, from) = (Rc::downgrade(state), from.clone());
        move |_, dx, _| {
            if let (Some(split), Some(from)) = (state.upgrade().as_ref().and_then(Split::of), from.get()) {
                // Dragged left, a sidebar on the right grows.
                let rtl = split.widget.direction() == gtk::TextDirection::Rtl;
                split.resize(if rtl { from - dx as i32 } else { from + dx as i32 });
            }
        }
    });
    drag.connect_drag_end({
        let state = Rc::downgrade(state);
        move |_, _, _| {
            let Some(split) = state.upgrade().as_ref().and_then(Split::of) else {
                return;
            };
            if from.take().is_some() {
                if let Some(divider) = split.state.divider.borrow().as_ref() {
                    divider.remove_css_class("dragging");
                }
                emit(&split.state.changed, &split);
            }
        }
    });
    drag
}

/// Pages one over another, the last pushed on top (`adw::NavigationView`). Escape and Alt+Left go back a page,
/// wherever the keyboard is in the window while the pages are on screen and no dialog is open: a page pushed from a
/// menu or an accelerator leaves the keyboard where it was. A page's own back button is the app's, built with
/// [`crate::icon_button`] and calling [`Nav::pop`]. The keyboard goes with the page shown, and comes back to where
/// it was on the page under it: the stack sees to that.
///
/// This is a handle: the widget is the stack of pages and works without it. A handler on a widget in one of the
/// pages holds a [`WeakNav`], or the two would keep each other alive.
#[derive(Clone)]
pub struct Nav {
    pub widget: gtk::Stack,
    state: Rc<NavState>,
}

struct NavState {
    /// Weak: the widget owns this, by its key handler.
    widget: glib::WeakRef<gtk::Stack>,
    root: gtk::Widget,
    /// The pages over the root, with their tags.
    pushed: RefCell<Vec<(String, gtk::Widget)>>,
    changed: Changed<Nav>,
}

weak_handle!(Nav, WeakNav, NavState);

impl Nav {
    pub fn new(root: &impl IsA<gtk::Widget>) -> Self {
        let widget = gtk::Stack::new();
        let state = Rc::new(NavState {
            widget: widget.downgrade(),
            root: root.as_ref().clone(),
            pushed: RefCell::default(),
            changed: RefCell::default(),
        });
        widget.add_child(&state.root);
        let back = {
            let state = state.clone();
            move || match Nav::of(&state) {
                Some(nav) if nav.visible_tag().is_some() => {
                    nav.pop();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        };
        // With the keyboard inside, ahead of whatever holds the pages: a split around them goes back only once they
        // are at their root.
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed({
            let back = back.clone();
            move |_, key, _, modifiers| {
                if is_back(key, modifiers) {
                    back()
                } else {
                    glib::Propagation::Proceed
                }
            }
        });
        widget.add_controller(keys);
        // With the keyboard anywhere else in the window: GTK runs a managed shortcut from the window while its widget
        // is on screen, and a dialog over the window holds it back.
        let anywhere = gtk::ShortcutController::new();
        anywhere.set_scope(gtk::ShortcutScope::Managed);
        anywhere.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("Escape|<Alt>Left|<Alt>KP_Left"),
            Some(gtk::CallbackAction::new(move |_, _| back())),
        ));
        widget.add_controller(anywhere);
        Self { widget, state }
    }

    /// Shows `page` over what is there.
    pub fn push(&self, tag: &str, page: &impl IsA<gtk::Widget>) {
        let page: gtk::Widget = page.as_ref().clone();
        self.widget.add_child(&page);
        self.widget.set_visible_child(&page);
        self.state.pushed.borrow_mut().push((tag.to_owned(), page));
        emit(&self.state.changed, self);
    }

    /// Goes back one page. The page that was on top is dropped. At the root this does nothing.
    pub fn pop(&self) {
        let depth = self.state.pushed.borrow().len();
        self.leave(depth.saturating_sub(1));
    }

    pub fn pop_to_root(&self) {
        self.leave(0);
    }

    /// The tag of the page on top; None at the root.
    pub fn visible_tag(&self) -> Option<String> {
        self.state.pushed.borrow().last().map(|(tag, _)| tag.clone())
    }

    /// Runs `f` after a page was pushed or popped, by the app or by a key.
    pub fn connect_changed(&self, f: impl Fn(&Nav) + 'static) {
        self.state.changed.borrow_mut().push(Rc::new(f));
    }

    /// Drops pages until `depth` are left.
    fn leave(&self, depth: usize) {
        let gone: Vec<(String, gtk::Widget)> = {
            let mut pushed = self.state.pushed.borrow_mut();
            if depth >= pushed.len() {
                return;
            }
            pushed.drain(depth..).collect()
        };
        let under = self.state.pushed.borrow().last().map(|(_, page)| page.clone());
        self.widget
            .set_visible_child(&under.unwrap_or_else(|| self.state.root.clone()));
        for (_, page) in &gone {
            self.widget.remove(page);
        }
        emit(&self.state.changed, self);
    }
}

/// A row of tabs for the pages of `stack` (`adw::ViewSwitcher`): each page's icon and title, the one shown marked.
/// In a window narrower than 700 pixels a tab that has an icon shows the icon alone. Pages are those added with
/// `add_titled`, with the icon set on the `gtk::StackPage`; the row follows pages added and removed later.
pub fn tabs(stack: &gtk::Stack) -> gtk::Box {
    let bar = gtk::Box::builder()
        .css_classes(["tabs"])
        .accessible_role(gtk::AccessibleRole::Group)
        .build();
    let pages = stack.pages();
    let fill = {
        let (bar, stack) = (bar.downgrade(), stack.downgrade());
        move |pages: &gtk::SelectionModel| {
            let (Some(bar), Some(stack)) = (bar.upgrade(), stack.upgrade()) else {
                return;
            };
            crate::clear(&bar);
            let mut first: Option<gtk::ToggleButton> = None;
            for page in pages.iter::<gtk::StackPage>().flatten() {
                let tab = tab(&stack, &page);
                tab.set_group(first.as_ref());
                first.get_or_insert_with(|| tab.clone());
                bar.append(&tab);
                set_icon_only(&tab, bar.has_css_class("narrow"));
            }
        }
    };
    fill(&pages);
    pages.connect_items_changed(move |pages, _, _, _| fill(pages));
    stack.connect_visible_child_notify({
        let bar = bar.downgrade();
        move |stack| {
            let (Some(bar), Some(shown)) = (bar.upgrade(), stack.visible_child()) else {
                return;
            };
            // The tabs are in the pages' order.
            let mut tab = bar.first_child();
            for page in stack.pages().iter::<gtk::StackPage>().flatten() {
                let Some(widget) = tab else { break };
                if page.child() == shown
                    && let Some(button) = widget.downcast_ref::<gtk::ToggleButton>()
                {
                    button.set_active(true);
                }
                tab = widget.next_sibling();
            }
        }
    });
    // The window's width, not the row's: the row is as wide as its tabs.
    let window_width = |bar: &gtk::Widget| bar.root().map_or(0, |root| root.width());
    watch_width(bar.upcast_ref(), window_width, TABS_ICON_ONLY_BELOW, {
        let bar = bar.downgrade();
        move |narrow| {
            // The stack does not keep its list of pages alive, and the row follows it for as long as the row lives.
            let _ = &pages;
            let Some(bar) = bar.upgrade() else { return };
            crate::set_class(&bar, "narrow", narrow);
            let mut tab = bar.first_child();
            while let Some(widget) = tab {
                if let Some(button) = widget.downcast_ref::<gtk::ToggleButton>() {
                    set_icon_only(button, narrow);
                }
                tab = widget.next_sibling();
            }
        }
    });
    bar
}

/// One tab: the page's icon and title, which it follows when the app changes them.
fn tab(stack: &gtk::Stack, page: &gtk::StackPage) -> gtk::ToggleButton {
    let image = crate::icon("");
    let label = gtk::Label::new(None);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    content.append(&image);
    content.append(&label);
    let child = page.child();
    let tab = gtk::ToggleButton::builder()
        .css_classes(["tab"])
        .child(&content)
        .active(stack.visible_child().as_ref() == Some(&child))
        .build();
    let fit = {
        let tab = tab.downgrade();
        move || {
            if let Some(tab) = tab.upgrade() {
                let narrow = tab.parent().is_some_and(|bar| bar.has_css_class("narrow"));
                set_icon_only(&tab, narrow);
            }
        }
    };
    image.connect_icon_name_notify({
        let fit = fit.clone();
        move |_| fit()
    });
    label.connect_label_notify(move |_| fit());
    page.bind_property("icon-name", &image, "icon-name")
        .sync_create()
        .build();
    page.bind_property("title", &label, "label").sync_create().build();
    let stack = stack.downgrade();
    tab.connect_toggled(move |tab| {
        if tab.is_active()
            && let Some(stack) = stack.upgrade()
        {
            stack.set_visible_child(&child);
        }
    });
    tab
}

/// Shows a tab's icon when it has one, and hides its title when `on` and the icon can stand for it. The title is
/// then the tooltip, and always the accessible name.
fn set_icon_only(tab: &gtk::ToggleButton, on: bool) {
    let Some(content) = tab.child() else { return };
    let image = content.first_child().and_downcast::<gtk::Image>();
    let label = content.last_child().and_downcast::<gtk::Label>();
    let (Some(image), Some(label)) = (image, label) else {
        return;
    };
    let has_icon = image.icon_name().is_some_and(|name| !name.is_empty());
    let alone = on && has_icon;
    image.set_visible(has_icon);
    label.set_visible(!alone);
    let title = label.label();
    tab.set_tooltip_text(alone.then_some(title.as_str()));
    tab.update_property(&[gtk::accessible::Property::Label(&title)]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_is_escape_or_alt_left_and_nothing_else() {
        let none = gdk::ModifierType::empty();
        let alt = gdk::ModifierType::ALT_MASK;
        assert!(is_back(gdk::Key::Escape, none));
        assert!(is_back(gdk::Key::Left, alt));
        assert!(!is_back(gdk::Key::Left, none));
        assert!(!is_back(gdk::Key::Escape, alt));
        assert!(!is_back(gdk::Key::Left, alt | gdk::ModifierType::SHIFT_MASK));
        // Caps Lock and Num Lock are held on many keyboards.
        assert!(is_back(gdk::Key::Escape, gdk::ModifierType::LOCK_MASK));
    }
}
