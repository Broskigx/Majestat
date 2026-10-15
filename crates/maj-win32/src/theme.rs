//! The palette, the fonts and the metrics: every colour and every size, in one place.
//!
//! The palette is **measured** rather than invented, and its hue is a decision. The measurements
//! are the reference frame's own structure: a floor, a body, raised chrome, separators one pixel
//! wide, and a ladder of greys between them that a capture of the reference interface produced at
//! `#17181A` (floor), `#1E1E22` (body) and `#28282E` (chrome). The hue is the owner's: the ladder
//! is carried onto **navy** rather than left neutral, which is the colour of the frame this
//! interface follows. Two families therefore do the work:
//!
//! * **Navy** is material and state — the ground, the panels, the raised chrome, the separators,
//!   a clip's body, the wash over what is selected. The ground is the brand's ink `#0B0E14`
//!   climbed a step into `#10141C`, so the navy reads as one field rather than as a tint.
//! * **Gold** is the brand's `#E8B44A` and it is reserved for what the interface is *doing*: the
//!   active page, the playhead, a focused control, a hovered button. Nothing else is gold, so
//!   that gold keeps meaning something — a second accent would make neither of them mean anything.
//!
//! The error colour is the one value that is neither: it is red because red is what a refusal
//! looks like, and it is the only colour here that a person has to notice rather than look for.

use std::cell::RefCell;

use crate::win32::{Brush, Colour, Font, Pen, Scratch};

/// The sizes every panel is built from, in pixels at the interface's own scale.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Metrics {
    /// The menu bar's height.
    pub(crate) menu_h: i32,
    /// The toolbar's height.
    pub(crate) toolbar_h: i32,
    /// The page strip along the bottom.
    pub(crate) pages_h: i32,
    /// The status bar's height.
    pub(crate) status_h: i32,
    /// A panel's heading.
    pub(crate) heading_h: i32,
    /// The timeline's ruler.
    pub(crate) ruler_h: i32,
    /// The tool strip above the timeline.
    pub(crate) tools_h: i32,
    /// One track row.
    pub(crate) track_h: i32,
    /// One audio or video track that is too narrow to show anything is still this tall.
    pub(crate) min_track_h: i32,
    /// The track-name gutter.
    pub(crate) gutter: i32,
    /// One line of a list.
    pub(crate) row_h: i32,
    /// One tile in the media pool.
    pub(crate) tile_h: i32,
    /// The width of one media tile.
    pub(crate) tile_w: i32,
    /// The thickness of a divider that can be dragged.
    pub(crate) split: i32,
    /// The horizontal padding inside a panel.
    pub(crate) pad: i32,
    /// The radius of a rounded card.
    pub(crate) radius: i32,
    /// How far the corner buttons sit from the frame's edge.
    pub(crate) frame_button_w: i32,
    /// How much bigger a pixel is than at 96 dots per inch.
    pub(crate) scale: f64,
}

impl Metrics {
    /// The metrics at a scale: 1.0 is 96 dots per inch.
    #[must_use]
    pub(crate) fn new(scale: f64) -> Self {
        let scale = if scale.is_finite() && scale > 0.5 && scale < 4.0 {
            scale
        } else {
            1.0
        };
        let at = |value: i32| (f64::from(value) * scale).round() as i32;
        Self {
            menu_h: at(26),
            toolbar_h: at(34),
            pages_h: at(34),
            status_h: at(24),
            heading_h: at(24),
            ruler_h: at(28),
            tools_h: at(30),
            track_h: at(38),
            min_track_h: at(22),
            gutter: at(168),
            row_h: at(20),
            tile_h: at(84),
            tile_w: at(132),
            split: at(4),
            pad: at(6),
            radius: at(4),
            frame_button_w: at(46),
            scale,
        }
    }

    /// A length in pixels at this scale.
    #[must_use]
    pub(crate) fn px(&self, value: i32) -> i32 {
        (f64::from(value) * self.scale).round() as i32
    }

    /// A font height in pixels at this scale, for `CreateFontW`.
    #[must_use]
    pub(crate) fn font(&self, points: i32) -> i32 {
        (f64::from(points) * self.scale).round() as i32
    }
}

/// The colours and the fonts the whole interface is drawn with.
#[derive(Debug)]
pub(crate) struct Theme {
    /// The floor: the viewer's matte, the timeline's background.
    pub(crate) ground: Colour,
    /// A panel's body.
    pub(crate) panel: Colour,
    /// A panel's alternating body, for lists.
    pub(crate) panel_alt: Colour,
    /// Raised chrome: the menu bar, the toolbar, headings.
    pub(crate) chrome: Colour,
    /// Raised chrome under the pointer.
    pub(crate) chrome_high: Colour,
    /// A pressed control.
    pub(crate) chrome_low: Colour,
    /// A panel's border.
    pub(crate) border: Colour,
    /// The one-pixel lines the reference uses between panels.
    pub(crate) separator: Colour,
    /// Ordinary text.
    pub(crate) ink: Colour,
    /// Quiet text: labels, counts, the ruler.
    pub(crate) dim: Colour,
    /// Text that is only just there.
    pub(crate) faint: Colour,
    /// The active element: the brand's gold.
    pub(crate) accent: Colour,
    /// The active element, darkened, for a body that must still be readable.
    pub(crate) accent_deep: Colour,
    /// A refusal.
    pub(crate) error: Colour,
    /// A video clip's body, and the lighter band along its top.
    pub(crate) video: Colour,
    pub(crate) video_top: Colour,
    /// An audio clip's body, and its band.
    pub(crate) audio: Colour,
    pub(crate) audio_top: Colour,
    /// A clip whose track or effect is switched off.
    pub(crate) off: Colour,
    /// A media tile's thumbnail field.
    pub(crate) well: Colour,
    /// The selection's own tint, laid over whatever it is over.
    pub(crate) selection: Colour,
    /// The dark that a soft shadow is made of.
    pub(crate) shadow: Colour,
    /// The metrics at this display's scale.
    pub(crate) metrics: Metrics,
    /// Fonts.
    pub(crate) heading_font: Font,
    pub(crate) body_font: Font,
    pub(crate) small_font: Font,
    pub(crate) mono_font: Font,
    /// A one-pixel field of colour, for the blends: a wash of the accent over a selection, the
    /// dim behind a modal, and nothing else.
    pub(crate) field: RefCell<Scratch>,
}

impl Theme {
    /// Builds the palette. The scale is the display's, so the metrics come out in the pixels
    /// that are actually on the screen.
    #[must_use]
    pub(crate) fn new(scale: f64) -> Self {
        let metrics = Metrics::new(scale);
        Self {
            ground: Colour::hex(0x10_14_1C),
            panel: Colour::hex(0x18_20_29),
            panel_alt: Colour::hex(0x1C_25_31),
            chrome: Colour::hex(0x20_2B_39),
            chrome_high: Colour::hex(0x2A_37_48),
            chrome_low: Colour::hex(0x33_42_5A),
            border: Colour::hex(0x33_40_4F),
            separator: Colour::hex(0x3A_4A_5E),
            ink: Colour::hex(0xE8_ED_F4),
            dim: Colour::hex(0xA3_B0_C0),
            faint: Colour::hex(0x6E_7C_8E),
            accent: Colour::hex(0xE8_B4_4A),
            accent_deep: Colour::hex(0x8A_6A_24),
            error: Colour::hex(0xE8_73_5A),
            video: Colour::hex(0x2C_4A_72),
            video_top: Colour::hex(0x3E_67_9B),
            audio: Colour::hex(0x24_51_4A),
            audio_top: Colour::hex(0x2F_6B_60),
            off: Colour::hex(0x1F_27_33),
            well: Colour::hex(0x0B0F16),
            selection: Colour::hex(0x3D_7B_C8),
            shadow: Colour::hex(0x06_08_10),
            metrics,
            heading_font: Font::new("Segoe UI Semibold", metrics.font(14), true),
            body_font: Font::new("Segoe UI", metrics.font(14), false),
            small_font: Font::new("Segoe UI", metrics.font(12), false),
            mono_font: Font::new("Consolas", metrics.font(13), false),
            field: RefCell::new(Scratch::detached(1, 1)),
        }
    }

    /// The brush a panel's one-pixel border is drawn with.
    #[must_use]
    pub(crate) fn border_brush(&self) -> Brush {
        Brush::solid(self.border)
    }

    /// The pen the playhead is drawn with: the accent, one pixel wide at this scale.
    #[must_use]
    pub(crate) fn playhead_pen(&self) -> Pen {
        Pen::solid(self.accent, self.metrics.px(1).max(1))
    }

    /// A wash of one colour over a rectangle, for selections and modal dimming.
    pub(crate) fn wash(
        &self,
        hdc: crate::win32::Hdc,
        rect: crate::win32::Rect,
        colour: Colour,
        alpha: u8,
    ) {
        let field = match self.field.try_borrow() {
            Ok(field) => field,
            // Re-entering a wash while one is in progress cannot happen in one thread; if it
            // ever did, drawing nothing is better than borrowing twice.
            Err(_) => return,
        };
        crate::win32::blend(hdc, &field, rect, colour, alpha);
    }
}
