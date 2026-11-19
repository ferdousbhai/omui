// SPDX-License-Identifier: MIT
//! How a window's panes are arranged and how they follow its width: what `adw::NavigationSplitView`,
//! `OverlaySplitView`, `NavigationView`, `ViewSwitcher`, `Clamp`, `PreferencesPage` and `Breakpoint` do.
//!
//! GTK has no signal for a widget's width, and a widget cannot be shown or hidden while the window is being laid
//! out. So whatever here follows a width acts on the frame after the width changed, from a tick callback: a split
//! when its layout was given a width that calls for it, `on_narrow` and `tabs` by looking once a frame.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{gdk, glib, graphene, gsk, prelude::*, subclass::prelude::*};

/// Below this window width [`tabs`] shows icons alone.
const TABS_ICON_ONLY_BELOW: i32 = 700;

/// The share of a wide [`Split`] its sidebar takes, within the limits it was given (libadwaita's default).
const SIDEBAR_FRACTION: f64 = 0.25;

/// Tells `f` whether `widget` is narrower than `below_px`: once when it first has a width, then each time it crosses.
/// `f` lives as long as the widget does, so it holds the widget itself, and whatever holds it, weakly.
///
/// The widget must be able to get that narrow: a child that cannot shrink keeps it, and the window, wide.
pub fn on_narrow(widget: &impl IsA<gtk::Widget>, below_px: i32, f: impl Fn(bool) + 'static) {
    watch_width(widget.as_ref(), |widget| widget.width(), below_px, f);
}

fn watch_width(widget: &gtk::Widget, width: fn(&gtk::Widget) -> i32, below_px: i32, f: impl Fn(bool) + 'static) {
    let was = Cell::new(None);
    widget.add_tick_callback(move |widget, _| {
        let width = width(widget);
        // No width yet: the first frame comes before the first layout.
        if width > 0 {
            let narrow = width < below_px;
            if was.replace(Some(narrow)) != Some(narrow) {
                f(narrow);
            }
        }
        glib::ControlFlow::Continue
    });
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

    /// Lays out the two panes of a [`Split`](super::Split): its widget's first child holds the sidebar and its last
    /// the content. Which of them are on screen is the split's to say, by their child visibility; this gives them
    /// their places, and tells the split when the width it was given calls for the other arrangement.
    #[derive(Default)]
    pub struct SplitLayout {
        /// The least and the most the sidebar gets beside the content.
        pub sidebar_width: Cell<(i32, i32)>,
        pub collapse_below: Cell<Option<i32>>,
        /// Whether the last width called for one pane, and what to tell when that changes.
        pub collapsed: Cell<bool>,
        pub on_collapsed: RefCell<Option<Told>>,
    }

    impl SplitLayout {
        fn panes(widget: &gtk::Widget) -> Option<(gtk::Widget, gtk::Widget)> {
            widget.first_child().zip(widget.last_child())
        }

        /// The least the sidebar is beside the content: the app's limit, or what the sidebar itself needs.
        fn least_sidebar(&self, sidebar: &gtk::Widget) -> i32 {
            self.sidebar_width.get().0.max(minimum_width(sidebar))
        }

        /// The sidebar's width in a split `width` wide: a share of it, within the limits, giving way to what the
        /// content needs.
        fn sidebar(&self, sidebar: &gtk::Widget, content: &gtk::Widget, width: i32) -> i32 {
            let least = self.least_sidebar(sidebar);
            let most = self.sidebar_width.get().1.max(least);
            let share = (f64::from(width) * SIDEBAR_FRACTION) as i32;
            share.clamp(least, most).min(width - minimum_width(content)).max(least)
        }

        /// Where each pane on screen goes in a split `width` wide: its left edge and its width, which is never
        /// less than the pane can be. For the frame before the split collapses, the content hangs over the edge.
        fn places(&self, sidebar: &gtk::Widget, content: &gtk::Widget, width: i32) -> Vec<(gtk::Widget, i32, i32)> {
            let whole = |pane: &gtk::Widget| (pane.clone(), 0, width.max(minimum_width(pane)));
            // Child visibility is the split's own switch, which `should_layout` does not look at.
            let on_screen = |pane: &gtk::Widget| in_use(pane) && pane.is_child_visible();
            match (on_screen(sidebar), on_screen(content)) {
                (true, true) => {
                    let beside = self.sidebar(sidebar, content, width);
                    let rest = (width - beside).max(minimum_width(content));
                    vec![(sidebar.clone(), 0, beside), (content.clone(), beside, rest)]
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
                // As tall as the taller pane on screen, at the width it has.
                let mut tallest = (0, 0);
                let width = if for_size < 0 { widget.width() } else { for_size };
                for (pane, _, pane_width) in self.places(&sidebar, &content, width) {
                    let (minimum, natural, ..) = pane.measure(orientation, if for_size < 0 { -1 } else { pane_width });
                    tallest = (tallest.0.max(minimum), tallest.1.max(natural));
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
            for (pane, x, pane_width) in self.places(&sidebar, &content, width) {
                place(&pane, x, pane_width, height, baseline);
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

/// A sidebar beside the content (`adw::NavigationSplitView` and `adw::OverlaySplitView`). The sidebar is a quarter
/// of the width, within the limits given. Narrower than `collapse_below`, or than the two panes need, the split is
/// collapsed and shows one pane, which fills it: the sidebar, until the content is asked for or the sidebar is put
/// away. Escape and Alt+Left then go back from content that was asked for to the sidebar.
///
/// A list and its detail call [`Split::set_show_content`] when a row is chosen, and put a back button on the
/// content that is visible while [`Split::is_collapsed`]. A sidebar with a button that shows and hides it calls
/// [`Split::set_show_sidebar`], and keeps the button in step from [`Split::connect_changed`].
///
/// The `Rc` is the split: its widget follows the width for as long as the app holds it.
pub struct Split {
    pub widget: gtk::Box,
    sidebar: gtk::Widget,
    content: gtk::Widget,
    collapsed: Cell<bool>,
    show_sidebar: Cell<bool>,
    show_content: Cell<bool>,
    /// What had the keyboard in the sidebar and in the content when each was last taken off screen with it.
    left_focus: [glib::WeakRef<gtk::Widget>; 2],
    /// While the handlers hear that the split collapsed or widened: what they ask for is put on screen once, after
    /// the last of them, with what the new width calls for.
    settling: Cell<bool>,
    changed: Changed<Split>,
}

impl Split {
    /// `sidebar_width` is the least and the most the sidebar gets, in pixels. With no `collapse_below` the split
    /// never collapses.
    pub fn new(
        sidebar: &impl IsA<gtk::Widget>,
        content: &impl IsA<gtk::Widget>,
        sidebar_width: (i32, i32),
        collapse_below: Option<i32>,
    ) -> Rc<Self> {
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
        let split = Rc::new(Self {
            widget,
            sidebar,
            content,
            collapsed: Cell::new(false),
            show_sidebar: Cell::new(true),
            show_content: Cell::new(false),
            left_focus: Default::default(),
            settling: Cell::new(false),
            changed: RefCell::default(),
        });

        let weak = Rc::downgrade(&split);
        layout.imp().on_collapsed.replace(Some(Rc::new(move |collapsed| {
            if let Some(split) = weak.upgrade()
                && split.collapsed.replace(collapsed) != collapsed
            {
                split.settling.set(true);
                emit(&split.changed, &split);
                split.settling.set(false);
                split.apply();
            }
        })));
        // Back from the content to the sidebar, where the content was a step forward from it.
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(&split);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let Some(split) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if is_back(key, modifiers) && split.is_collapsed() && split.show_sidebar.get() && split.show_content.get() {
                split.set_show_content(false);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        split.widget.add_controller(keys);
        split
    }

    /// Whether the split is too narrow for both panes and shows one.
    pub fn is_collapsed(&self) -> bool {
        self.collapsed.get()
    }

    /// When collapsed: the content in place of the sidebar, or the sidebar again. Wide, both are there whatever
    /// this says, and it is remembered for when the split collapses.
    pub fn set_show_content(&self, on: bool) {
        if self.show_content.replace(on) != on {
            self.apply();
            emit(&self.changed, self);
        }
    }

    pub fn shows_content(&self) -> bool {
        self.show_content.get()
    }

    /// Puts the sidebar away, or brings it back. Collapsed, the content is shown while the sidebar is away.
    pub fn set_show_sidebar(&self, on: bool) {
        if self.show_sidebar.replace(on) != on {
            self.apply();
            emit(&self.changed, self);
        }
    }

    pub fn shows_sidebar(&self) -> bool {
        self.show_sidebar.get()
    }

    /// Runs `f` when the split collapses or widens, and when either pane is shown or put away.
    pub fn connect_changed(&self, f: impl Fn(&Split) + 'static) {
        self.changed.borrow_mut().push(Rc::new(f));
    }

    /// Puts on screen the panes the state calls for, by their holders' child visibility: a pane taken off still
    /// counts toward the split's size.
    fn apply(&self) {
        if self.settling.get() {
            return;
        }
        let collapsed = self.collapsed.get();
        let sidebar = self.show_sidebar.get() && !(collapsed && self.show_content.get());
        let content = !(collapsed && sidebar);
        crate::set_class(&self.widget, "collapsed", collapsed);
        // A pane that goes with the keyboard in it hands the keyboard to the other, and gets it back where it was:
        // GTK would drop the focus, and the next Tab would start from the top of the window.
        let panes = [(&self.sidebar, sidebar), (&self.content, content)];
        let leaving = panes
            .iter()
            .position(|(pane, stays)| !stays && focus_inside(pane).is_some());
        if let Some(leaving) = leaving {
            self.left_focus[leaving].set(focus_inside(panes[leaving].0).as_ref());
        }
        self.sidebar.set_child_visible(sidebar);
        self.content.set_child_visible(content);
        self.widget.queue_resize();
        if let Some(leaving) = leaving {
            let other = 1 - leaving;
            give_focus(panes[other].0, &self.left_focus[other]);
        }
    }
}

/// Pages one over another, the last pushed on top (`adw::NavigationView`). Escape and Alt+Left go back a page; a
/// page's own back button is the app's, built with [`crate::icon_button`] and calling [`Nav::pop`]. The keyboard
/// goes with the page shown, and comes back to where it was on the page under it: the stack sees to that.
///
/// The `Rc` is the stack of pages: its widget answers those keys for as long as the app holds it.
pub struct Nav {
    pub widget: gtk::Stack,
    root: gtk::Widget,
    /// The pages over the root, with their tags.
    pushed: RefCell<Vec<(String, gtk::Widget)>>,
    changed: Changed<Nav>,
}

impl Nav {
    pub fn new(root: &impl IsA<gtk::Widget>) -> Rc<Self> {
        let nav = Rc::new(Self {
            widget: gtk::Stack::new(),
            root: root.as_ref().clone(),
            pushed: RefCell::default(),
            changed: RefCell::default(),
        });
        nav.widget.add_child(&nav.root);
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(&nav);
        keys.connect_key_pressed(move |_, key, _, modifiers| match weak.upgrade() {
            Some(nav) if is_back(key, modifiers) && nav.visible_tag().is_some() => {
                nav.pop();
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        });
        nav.widget.add_controller(keys);
        nav
    }

    /// Shows `page` over what is there.
    pub fn push(&self, tag: &str, page: &impl IsA<gtk::Widget>) {
        let page: gtk::Widget = page.as_ref().clone();
        self.widget.add_child(&page);
        self.widget.set_visible_child(&page);
        self.pushed.borrow_mut().push((tag.to_owned(), page));
        emit(&self.changed, self);
    }

    /// Goes back one page. The page that was on top is dropped. At the root this does nothing.
    pub fn pop(&self) {
        let depth = self.pushed.borrow().len();
        self.leave(depth.saturating_sub(1));
    }

    pub fn pop_to_root(&self) {
        self.leave(0);
    }

    /// The tag of the page on top; None at the root.
    pub fn visible_tag(&self) -> Option<String> {
        self.pushed.borrow().last().map(|(tag, _)| tag.clone())
    }

    /// Runs `f` after a page was pushed or popped, by the app or by a key.
    pub fn connect_changed(&self, f: impl Fn(&Nav) + 'static) {
        self.changed.borrow_mut().push(Rc::new(f));
    }

    /// Drops pages until `depth` are left.
    fn leave(&self, depth: usize) {
        let gone: Vec<(String, gtk::Widget)> = {
            let mut pushed = self.pushed.borrow_mut();
            if depth >= pushed.len() {
                return;
            }
            pushed.drain(depth..).collect()
        };
        let under = self.pushed.borrow().last().map(|(_, page)| page.clone());
        self.widget
            .set_visible_child(&under.unwrap_or_else(|| self.root.clone()));
        for (_, page) in &gone {
            self.widget.remove(page);
        }
        emit(&self.changed, self);
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
