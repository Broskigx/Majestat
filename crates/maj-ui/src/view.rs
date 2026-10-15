//! The interface's own view state: which page is showing, how the panels are divided, and the
//! switches that change how editing feels rather than what the document holds.
//!
//! None of this is document state. A page is not a command, a divider is not an edit, and
//! snapping does not change a single frame of output — which is exactly why it lives here, in
//! the shell, and not in [`maj_core`]: the model holds the film, and this holds the desk.
//!
//! It is stored in the preferences file beside the language and the zoom, so a person who
//! arranges the window finds it arranged the next morning. Every field is clamped on the way in,
//! because a preferences file is a text file a person can edit by hand and a divider of nine
//! thousand pixels is not a state the interface can honour.

use serde::{Deserialize, Serialize};

use crate::action::Panels;

/// A page of the interface.
///
/// The reference interface has seven; three of them have an engine behind them here, and the
/// other four are drawn, refused, and told whose work they are. A page that could be selected
/// and then showed nothing would be a lie with a tab on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Page {
    /// The pool and the viewer: finding the material.
    Media,
    /// Editing: the pool, the viewer, the timeline and the inspector.
    #[default]
    Cut,
    /// The editorial page, which is M6's operations made visible.
    Edit,
    /// Fusion, which is M4's graph made visible.
    Fusion,
    /// Colour, which is M5.
    Color,
    /// Fairlight, which is M9.
    Fairlight,
    /// Delivery: what can be written today, and what the exporter cannot.
    Deliver,
}

/// Every page, in the order the strip lists them.
pub const PAGES: [Page; 7] = [
    Page::Media,
    Page::Cut,
    Page::Edit,
    Page::Fusion,
    Page::Color,
    Page::Fairlight,
    Page::Deliver,
];

impl Page {
    /// The short stable name, used in preferences and in the report.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Media => "media",
            Self::Cut => "cut",
            Self::Edit => "edit",
            Self::Fusion => "fusion",
            Self::Color => "color",
            Self::Fairlight => "fairlight",
            Self::Deliver => "deliver",
        }
    }

    /// Reads a page back from its short name.
    #[must_use]
    pub fn from_key(value: &str) -> Option<Self> {
        PAGES.into_iter().find(|page| page.key() == value)
    }

    /// Whether this page has an engine behind it.
    #[must_use]
    pub const fn built(self) -> bool {
        matches!(self, Self::Media | Self::Cut | Self::Deliver)
    }

    /// The phase that owns the pages that are not built, in one word.
    #[must_use]
    pub const fn owner(self) -> &'static str {
        match self {
            Self::Edit => "M6",
            Self::Fusion => "M4",
            Self::Color => "M5",
            Self::Fairlight => "M9",
            _ => "",
        }
    }
}

/// A divider between two parts of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Split {
    /// Between the left column and the middle.
    Left,
    /// Between the middle and the right column.
    Right,
    /// Between the upper half and the timeline.
    Timeline,
    /// Between two panels inside one column.
    Inline,
}

impl Split {
    /// The short stable name.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Timeline => "timeline",
            Self::Inline => "inline",
        }
    }
}

/// How the window is arranged, and which of its switches are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct View {
    /// The page that is showing.
    pub page: Page,
    /// Which panels are open.
    pub panels: Panels,
    /// Whether a dragged edge snaps to the playhead and to other clips.
    pub snap: bool,
    /// Whether selecting a clip selects everything linked to it.
    pub link: bool,
    /// Whether the transport starts again when it reaches the end.
    pub loops: bool,
    /// The left column's width, in pixels at the interface's own scale.
    pub left_w: i32,
    /// The right column's width.
    pub right_w: i32,
    /// The timeline's height.
    pub timeline_h: i32,
    /// How much of a column its first panel takes.
    pub media_split: i32,
}

/// The range a divider can be dragged into, before the layout is asked for anything.
pub const SPLIT_RANGE: (i32, i32) = (120, 900);

impl Default for View {
    fn default() -> Self {
        Self {
            page: Page::Cut,
            panels: Panels::default(),
            snap: true,
            link: true,
            loops: false,
            left_w: 264,
            right_w: 288,
            timeline_h: 300,
            media_split: 220,
        }
    }
}

impl View {
    /// Repairs a preferences file's numbers into what the layout can honour.
    #[must_use]
    pub fn sanitised(mut self) -> Self {
        let fix = |value: &mut i32| {
            *value = (*value).clamp(SPLIT_RANGE.0, SPLIT_RANGE.1);
        };
        fix(&mut self.left_w);
        fix(&mut self.right_w);
        fix(&mut self.timeline_h);
        fix(&mut self.media_split);
        if !self.page.built() {
            // A page whose engine does not exist is a page a person cannot have been on; a
            // preferences file that says otherwise is repaired rather than honoured.
            self.page = Page::Cut;
        }
        self
    }

    /// The width a divider holds, by name.
    #[must_use]
    pub const fn width(self, split: Split) -> i32 {
        match split {
            Split::Left => self.left_w,
            Split::Right => self.right_w,
            Split::Timeline => self.timeline_h,
            Split::Inline => self.media_split,
        }
    }

    /// The same view, with one divider moved.
    #[must_use]
    pub const fn with_width(mut self, split: Split, value: i32) -> Self {
        match split {
            Split::Left => self.left_w = value,
            Split::Right => self.right_w = value,
            Split::Timeline => self.timeline_h = value,
            Split::Inline => self.media_split = value,
        }
        self
    }

    /// Puts every divider back where it started.
    #[must_use]
    pub fn reset(self) -> Self {
        Self {
            page: self.page,
            panels: self.panels,
            snap: self.snap,
            link: self.link,
            loops: self.loops,
            ..Self::default()
        }
    }

    /// The same view, showing a page.
    #[must_use]
    pub const fn with_page(mut self, page: Page) -> Self {
        self.page = page;
        self
    }

    /// The same view, with one switch set.
    #[must_use]
    pub const fn with_switch(mut self, switch: Switch, value: bool) -> Self {
        match switch {
            Switch::Snap => self.snap = value,
            Switch::Link => self.link = value,
            Switch::Loops => self.loops = value,
        }
        self
    }

    /// The same view, with one panel opened or closed.
    #[must_use]
    pub const fn with_panels(mut self, panels: Panels) -> Self {
        self.panels = panels;
        self
    }

    /// Reads one switch.
    #[must_use]
    pub const fn switch(self, switch: Switch) -> bool {
        match switch {
            Switch::Snap => self.snap,
            Switch::Link => self.link,
            Switch::Loops => self.loops,
        }
    }
}

/// One of the interface's three switches: they change how editing feels rather than what the
/// document holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Switch {
    /// A dragged edge lands on the playhead and on other clips' edges.
    Snap,
    /// Selecting a clip selects everything linked to it.
    Link,
    /// The transport starts again when it reaches the end.
    Loops,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_round_trips_through_its_own_name() {
        for page in PAGES {
            assert_eq!(Page::from_key(page.key()), Some(page));
        }
        assert_eq!(Page::from_key("cutting"), None);
    }

    #[test]
    fn the_pages_without_an_engine_name_their_phase() {
        for page in PAGES {
            assert_eq!(
                page.built(),
                page.owner().is_empty(),
                "{page:?} disagrees about whether it is built"
            );
        }
    }

    #[test]
    fn a_divider_out_of_range_is_repaired_rather_than_honoured() {
        let view = View {
            left_w: 90_000,
            right_w: -4,
            timeline_h: 0,
            media_split: 12_000,
            ..View::default()
        }
        .sanitised();
        for value in [view.left_w, view.right_w, view.timeline_h, view.media_split] {
            assert!(
                (SPLIT_RANGE.0..=SPLIT_RANGE.1).contains(&value),
                "{value} is outside {SPLIT_RANGE:?}"
            );
        }
    }

    #[test]
    fn a_page_that_is_not_built_is_not_where_a_person_was() {
        let view = View {
            page: Page::Color,
            ..View::default()
        }
        .sanitised();
        assert_eq!(view.page, Page::Cut);
    }

    #[test]
    fn moving_a_divider_leaves_the_others_alone() {
        let view = View::default().with_width(Split::Left, 300);
        assert_eq!(view.width(Split::Left), 300);
        assert_eq!(view.width(Split::Right), View::default().right_w);
        assert_eq!(view.reset(), View::default());
    }

    #[test]
    fn a_switch_round_trips_through_its_own_reader() {
        for (switch, other) in [
            (Switch::Snap, Switch::Link),
            (Switch::Link, Switch::Loops),
            (Switch::Loops, Switch::Snap),
        ] {
            let view = View::default().with_switch(switch, !View::default().switch(switch));
            assert_ne!(view.switch(switch), View::default().switch(switch));
            assert_eq!(
                view.switch(other),
                View::default().switch(other),
                "{switch:?} moved {other:?} as well"
            );
        }
    }

    #[test]
    fn resetting_the_dividers_keeps_the_page_and_the_switches() {
        let view = View::default()
            .with_page(Page::Media)
            .with_switch(Switch::Loops, true)
            .with_width(Split::Left, 400);
        let reset = view.reset();
        assert_eq!(reset.page, Page::Media);
        assert!(reset.loops);
        assert_eq!(reset.left_w, View::default().left_w);
    }
}
