//! Where everything is: the frame, the seven pages, their panels, and the dividers between them.
//!
//! This module is the only place that knows how the client area is divided. The painter reads
//! the rectangles it produces and the hit-test table is built from the same rectangles, so a
//! button cannot move without its click target moving with it.
//!
//! Every field is filled on every page. A panel that a page does not show is an empty rectangle
//! rather than an `Option`, because the paint code then has one shape to handle and cannot
//! forget a case.

use maj_ui::{Page, Panels};

use crate::theme::Metrics;
use crate::win32::Rect;

/// The view state the geometry depends on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct View {
    /// Which page is showing.
    pub(crate) page: Page,
    /// Which of the cut page's panels are open.
    pub(crate) panels: Panels,
    /// How wide the left column was left.
    pub(crate) left_w: i32,
    /// How wide the right column was left.
    pub(crate) right_w: i32,
    /// How tall the timeline was left.
    pub(crate) timeline_h: i32,
    /// How much of the left column the media pool takes.
    pub(crate) media_split: i32,
    /// How many tracks the active sequence has.
    pub(crate) tracks: usize,
    /// The sizes everything is built from.
    pub(crate) metrics: Metrics,
}

/// Every rectangle the interface draws in, for one paint.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Frame {
    /// The menu bar, which is also the strip that moves a frameless window.
    pub(crate) menu: Rect,
    /// The toolbar.
    pub(crate) toolbar: Rect,
    /// The status bar.
    pub(crate) status: Rect,
    /// The page strip along the bottom.
    pub(crate) pages: Rect,
    /// Everything between the toolbar and the status bar.
    pub(crate) body: Rect,
    /// The left column.
    pub(crate) left: Rect,
    /// The middle.
    pub(crate) centre: Rect,
    /// The right column.
    pub(crate) right: Rect,
    /// The media pool.
    pub(crate) media: Rect,
    /// The command log.
    pub(crate) history: Rect,
    /// The divider between the media pool and the log.
    pub(crate) media_split: Rect,
    /// The viewer, above its transport bar.
    pub(crate) viewer: Rect,
    /// The audio meters.
    pub(crate) meter: Rect,
    /// The transport bar.
    pub(crate) transport: Rect,
    /// The clip inspector.
    pub(crate) inspector: Rect,
    /// The mixer, which has no engine behind it.
    pub(crate) mixer: Rect,
    /// The timeline.
    pub(crate) timeline: Rect,
    /// The timeline's own tool strip.
    pub(crate) timeline_tools: Rect,
    /// The ruler.
    pub(crate) ruler: Rect,
    /// The track-name gutter.
    pub(crate) gutter: Rect,
    /// The lane the clips are drawn in.
    pub(crate) track_area: Rect,
    /// The media page's grid.
    pub(crate) grid: Rect,
    /// The deliver page's settings column.
    pub(crate) export: Rect,
    /// The divider between the left column and the middle.
    pub(crate) split_left: Rect,
    /// The divider between the middle and the right column.
    pub(crate) split_right: Rect,
    /// The divider between the upper half and the timeline.
    pub(crate) split_timeline: Rect,
}

impl Frame {
    /// Splits the client area for one page.
    #[must_use]
    pub(crate) fn build(rect: Rect, view: &View) -> Self {
        let m = view.metrics;
        let corner = |x: i32, y: i32| Rect::new(x, y, x, y);
        let mut frame = Self {
            menu: corner(rect.left, rect.top),
            toolbar: corner(rect.left, rect.top),
            status: corner(rect.left, rect.top),
            pages: corner(rect.left, rect.top),
            body: corner(rect.left, rect.top),
            left: corner(rect.left, rect.top),
            centre: corner(rect.left, rect.top),
            right: corner(rect.left, rect.top),
            media: corner(rect.left, rect.top),
            history: corner(rect.left, rect.top),
            media_split: corner(rect.left, rect.top),
            viewer: corner(rect.left, rect.top),
            meter: corner(rect.left, rect.top),
            transport: corner(rect.left, rect.top),
            inspector: corner(rect.left, rect.top),
            mixer: corner(rect.left, rect.top),
            timeline: corner(rect.left, rect.top),
            timeline_tools: corner(rect.left, rect.top),
            ruler: corner(rect.left, rect.top),
            gutter: corner(rect.left, rect.top),
            track_area: corner(rect.left, rect.top),
            grid: corner(rect.left, rect.top),
            export: corner(rect.left, rect.top),
            split_left: corner(rect.left, rect.top),
            split_right: corner(rect.left, rect.top),
            split_timeline: corner(rect.left, rect.top),
        };
        if rect.width() < m.px(240) || rect.height() < m.px(160) {
            return frame;
        }

        frame.menu = Rect::new(rect.left, rect.top, rect.right, rect.top + m.menu_h);
        frame.toolbar = Rect::new(
            rect.left,
            frame.menu.bottom,
            rect.right,
            frame.menu.bottom + m.toolbar_h,
        );
        frame.pages = Rect::new(rect.left, rect.bottom - m.pages_h, rect.right, rect.bottom);
        frame.status = Rect::new(
            rect.left,
            frame.pages.top - m.status_h,
            rect.right,
            frame.pages.top,
        );
        frame.body = Rect::new(
            rect.left,
            frame.toolbar.bottom,
            rect.right,
            frame.status.top,
        );
        if frame.body.is_empty() {
            return frame;
        }

        match view.page {
            Page::Media => frame.build_media(view),
            Page::Deliver => frame.build_deliver(view),
            Page::Cut => frame.build_cut(view),
            // The pages that are not built yet show the cut layout with a note over it, so the
            // window is never a blank rectangle with a tab at the bottom.
            _ => frame.build_cut(view),
        }
        frame
    }

    /// The cut page: pool and log on the left, viewer in the middle, inspector on the right,
    /// timeline across the bottom.
    fn build_cut(&mut self, view: &View) {
        let m = view.metrics;
        let body = self.body;
        let least = m.tools_h + m.ruler_h + m.min_track_h;
        let tallest = (body.height() / 2).max(least);
        let timeline_h = view.timeline_h.clamp(least, tallest);
        let timeline_h = if view.panels.timeline { timeline_h } else { 0 };
        self.timeline = Rect::new(body.left, body.bottom - timeline_h, body.right, body.bottom);
        let split = if timeline_h > 0 { m.split } else { 0 };
        self.split_timeline = Rect::new(
            body.left,
            self.timeline.top - split,
            body.right,
            self.timeline.top,
        );
        let upper = Rect::new(body.left, body.top, body.right, self.split_timeline.top);
        if upper.is_empty() {
            return;
        }

        let ceiling = (upper.width() / 3).max(m.px(140));
        let left_w = if view.panels.media || view.panels.history {
            view.left_w.clamp(m.px(170), ceiling)
        } else {
            0
        };
        let right_w = if view.panels.effects {
            view.right_w.clamp(m.px(170), ceiling)
        } else {
            0
        };
        self.left = Rect::new(upper.left, upper.top, upper.left + left_w, upper.bottom);
        let left_split = if left_w > 0 { m.split } else { 0 };
        self.split_left = Rect::new(
            self.left.right,
            upper.top,
            self.left.right + left_split,
            upper.bottom,
        );
        self.right = Rect::new(upper.right - right_w, upper.top, upper.right, upper.bottom);
        let right_split = if right_w > 0 { m.split } else { 0 };
        self.split_right = Rect::new(
            self.right.left - right_split,
            upper.top,
            self.right.left,
            upper.bottom,
        );
        self.centre = Rect::new(
            self.split_left.right,
            upper.top,
            self.split_right.left,
            upper.bottom,
        );

        // The left column's two panels, and the divider between them.
        match (view.panels.media, view.panels.history) {
            (true, true) => {
                let wanted = view
                    .media_split
                    .clamp(m.px(80), (self.left.height() - m.px(80)).max(m.px(80)));
                self.media = Rect::new(
                    self.left.left,
                    self.left.top,
                    self.left.right,
                    self.left.top + wanted,
                );
                self.media_split = Rect::new(
                    self.left.left,
                    self.media.bottom,
                    self.left.right,
                    self.media.bottom + m.split,
                );
                self.history = Rect::new(
                    self.left.left,
                    self.media_split.bottom,
                    self.left.right,
                    self.left.bottom,
                );
            }
            (true, false) => {
                self.media = self.left;
            }
            (false, true) => {
                self.history = self.left;
            }
            (false, false) => {}
        }

        // The right column's two panels.
        if view.panels.effects {
            let half = self.right.height() / 2;
            self.inspector = Rect::new(
                self.right.left,
                self.right.top,
                self.right.right,
                self.right.top + half,
            );
            self.mixer = Rect::new(
                self.right.left,
                self.right.top + half + m.split,
                self.right.right,
                self.right.bottom,
            );
        }

        self.split_viewer(view);
        self.split_timeline_rows(view);
    }

    /// The viewer column: the frame, the meters to its right, the transport beneath.
    fn split_viewer(&mut self, view: &View) {
        let m = view.metrics;
        let centre = self.centre;
        if centre.is_empty() {
            return;
        }
        let meter_w = if view.panels.monitors { m.px(54) } else { 0 };
        let transport_h = if view.panels.monitors { m.px(46) } else { 0 };
        let column = Rect::new(
            centre.left,
            centre.top,
            centre.right - meter_w,
            centre.bottom,
        );
        self.meter = Rect::new(column.right, centre.top, centre.right, centre.bottom);
        self.viewer = Rect::new(
            column.left,
            column.top,
            column.right,
            column.bottom - transport_h,
        );
        self.transport = Rect::new(column.left, self.viewer.bottom, column.right, column.bottom);
    }

    /// The timeline's own rows: the tool strip, the ruler, the gutter and the lane.
    fn split_timeline_rows(&mut self, view: &View) {
        let m = view.metrics;
        let timeline = self.timeline;
        if timeline.is_empty() {
            return;
        }
        let tools_h = m.tools_h.min(timeline.height() / 3);
        self.timeline_tools = Rect::new(
            timeline.left,
            timeline.top,
            timeline.right,
            timeline.top + tools_h,
        );
        let ruler_h = m.ruler_h.min(timeline.bottom - self.timeline_tools.bottom);
        self.ruler = Rect::new(
            timeline.left,
            self.timeline_tools.bottom,
            timeline.right,
            self.timeline_tools.bottom + ruler_h,
        );
        let gutter = m.gutter.min(timeline.width() / 3);
        self.gutter = Rect::new(
            timeline.left,
            self.ruler.bottom,
            timeline.left + gutter,
            timeline.bottom,
        );
        self.track_area = Rect::new(
            self.gutter.right,
            self.ruler.bottom,
            timeline.right,
            timeline.bottom,
        );
    }

    /// The media page: the pool across most of the width, the viewer beside it, the inspector
    /// underneath.
    fn build_media(&mut self, view: &View) {
        let m = view.metrics;
        let body = self.body;
        let grid_w = (body.width() * 62 / 100).max(m.px(300)).min(body.width());
        self.grid = Rect::new(body.left, body.top, body.left + grid_w, body.bottom);
        self.split_left = Rect::new(
            self.grid.right,
            body.top,
            self.grid.right + m.split,
            body.bottom,
        );
        let rest = Rect::new(self.split_left.right, body.top, body.right, body.bottom);
        if rest.is_empty() {
            return;
        }
        let viewer_h = (rest.height() * 58 / 100).max(m.px(160));
        let meter_w = m.px(54);
        self.viewer = Rect::new(
            rest.left,
            rest.top,
            rest.right - meter_w,
            rest.top + viewer_h,
        );
        self.meter = Rect::new(self.viewer.right, rest.top, rest.right, rest.top + viewer_h);
        self.transport = Rect::new(
            self.viewer.left,
            self.viewer.bottom,
            self.viewer.right,
            self.viewer.bottom + m.px(46),
        );
        self.inspector = Rect::new(
            rest.left,
            self.transport.bottom + m.split,
            rest.right,
            rest.bottom,
        );
        self.centre = self.viewer;
        self.media = self.grid;
    }

    /// The deliver page: the settings, the preview of what would be written, and the log.
    fn build_deliver(&mut self, view: &View) {
        let m = view.metrics;
        let body = self.body;
        let export_w = (body.width() * 44 / 100).max(m.px(260)).min(body.width());
        self.export = Rect::new(body.left, body.top, body.left + export_w, body.bottom);
        self.split_left = Rect::new(
            self.export.right,
            body.top,
            self.export.right + m.split,
            body.bottom,
        );
        let rest = Rect::new(self.split_left.right, body.top, body.right, body.bottom);
        if rest.is_empty() {
            return;
        }
        let right_w = (rest.width() / 3)
            .max(m.px(180))
            .min(rest.width() - m.px(160));
        let right_w = right_w.max(0);
        let record_w = if right_w > 0 { m.split } else { 0 };
        self.right = Rect::new(rest.right - right_w, rest.top, rest.right, rest.bottom);
        self.split_right = Rect::new(
            self.right.left - record_w,
            rest.top,
            self.right.left,
            rest.bottom,
        );
        self.centre = Rect::new(rest.left, rest.top, self.split_right.left, rest.bottom);
        self.viewer = self.centre;
        self.history = self.right;
        self.inspector = self.right;
    }
}

/// A page's own summary, for the layout test.
#[cfg(test)]
#[must_use]
pub(crate) fn summary(view: &View, panels: Panels) -> String {
    format!(
        "{} · left {} right {} timeline {} · media {} history {} effects {} timeline {} monitors {}",
        view.page.key(),
        view.left_w,
        view.right_w,
        view.timeline_h,
        panels.media,
        panels.history,
        panels.effects,
        panels.timeline,
        panels.monitors
    )
}
