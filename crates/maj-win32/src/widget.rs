//! The zones: what is where, what it does, and what the pointer is over.
//!
//! The painter builds this table as it draws, in painting order, so a zone added later sits on
//! top of one added earlier. Hit testing is then a search through that table rather than a
//! second set of arithmetic that could disagree with what is on screen — the class of bug where
//! a button moves and its click target does not.
//!
//! A zone carries the *intent* it stands for: a binding, an action, or a reason it cannot be
//! used. Nothing here decides anything about the document.

use maj_core::ClipId;
use maj_ui::{AnimChannel, Binding, Key, Page};

use crate::glyphs::Glyph;
use crate::win32::{CursorKind, Rect};

/// A button in the window's own frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FrameButton {
    /// Minimise.
    Minimize,
    /// Maximise, or restore when already maximised.
    Maximize,
    /// Close.
    Close,
}

/// A button on the toolbar, with the intent it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Tool {
    /// Undo the last command.
    Undo,
    /// Redo it.
    Redo,
    /// Split at the playhead.
    Split,
    /// Remove the selection.
    Remove,
    /// Duplicate the selection.
    Duplicate,
    /// Drop a marker at the playhead, in the first of the eight colours.
    Marker,
    /// Trim the head to the playhead.
    TrimIn,
    /// Trim the tail to the playhead.
    TrimOut,
    /// Zoom in.
    ZoomIn,
    /// Zoom out.
    ZoomOut,
    /// Fit the whole sequence.
    ZoomFit,
    /// Snap to clip edges and to the playhead.
    Snap,
    /// Keep the selection linked.
    Link,
    /// Switch the preview resolution.
    Draft,
    /// Switch the rendering path.
    Tiled,
    /// Open the command palette.
    Palette,
    /// Build the example project.
    Example,
    /// Write a still.
    Export,
    /// Add a video track.
    AddVideoTrack,
    /// Add an audio track.
    AddAudioTrack,
}

impl Tool {
    /// Every tool, for the test that walks the set. Built only for the tests, like the
    /// glyph table it is checked against.
    #[cfg(test)]
    pub(crate) const ALL: &'static [Tool] = &[
        Tool::Undo,
        Tool::Redo,
        Tool::Split,
        Tool::Remove,
        Tool::Duplicate,
        Tool::Marker,
        Tool::TrimIn,
        Tool::TrimOut,
        Tool::ZoomIn,
        Tool::ZoomOut,
        Tool::ZoomFit,
        Tool::Snap,
        Tool::Link,
        Tool::Draft,
        Tool::Tiled,
        Tool::Palette,
        Tool::Example,
        Tool::Export,
        Tool::AddVideoTrack,
        Tool::AddAudioTrack,
    ];

    /// The picture.
    #[must_use]
    pub(crate) const fn glyph(self) -> Glyph {
        match self {
            Self::Undo => Glyph::Undo,
            Self::Redo => Glyph::Redo,
            Self::Split => Glyph::Split,
            Self::Remove => Glyph::Remove,
            Self::Duplicate => Glyph::Duplicate,
            Self::Marker => Glyph::Marker,
            Self::TrimIn => Glyph::TrimIn,
            Self::TrimOut => Glyph::TrimOut,
            Self::ZoomIn => Glyph::ZoomIn,
            Self::ZoomOut => Glyph::ZoomOut,
            Self::ZoomFit => Glyph::ZoomFit,
            Self::Snap => Glyph::Snap,
            Self::Link => Glyph::Link,
            Self::Draft => Glyph::Film,
            Self::Tiled => Glyph::GridView,
            Self::Palette => Glyph::Wand,
            Self::Example => Glyph::Plus,
            Self::Export => Glyph::Marker,
            Self::AddVideoTrack => Glyph::ArrowDown,
            Self::AddAudioTrack => Glyph::Wave,
        }
    }

    /// What the button says, in the shell's own words.
    #[must_use]
    pub(crate) const fn label(self) -> Key {
        match self {
            Self::Undo => Key::EditUndo,
            Self::Redo => Key::EditRedo,
            Self::Split => Key::EditSplit,
            Self::Remove => Key::EditRemove,
            Self::Duplicate => Key::EditDuplicate,
            Self::Marker => Key::MarkerTool,
            Self::TrimIn => Key::ViewTrimIn,
            Self::TrimOut => Key::ViewTrimOut,
            Self::ZoomIn => Key::ViewZoomIn,
            Self::ZoomOut => Key::ViewZoomOut,
            Self::ZoomFit => Key::ViewZoomFit,
            Self::Snap => Key::ViewSnap,
            Self::Link => Key::ViewLink,
            Self::Draft => Key::ViewDraft,
            Self::Tiled => Key::MonitorTiled,
            Self::Palette => Key::ViewPalette,
            Self::Example => Key::FileExample,
            Self::Export => Key::FileExportStill,
            Self::AddVideoTrack => Key::EditAddVideoTrack,
            Self::AddAudioTrack => Key::EditAddAudioTrack,
        }
    }

    /// The binding a key can reach the same intent with, where one exists.
    #[must_use]
    pub(crate) const fn binding(self) -> Option<Binding> {
        match self {
            Self::Undo => Some(Binding::Undo),
            Self::Redo => Some(Binding::Redo),
            Self::Split => Some(Binding::Split),
            Self::Remove => Some(Binding::Remove),
            Self::Duplicate => Some(Binding::Duplicate),
            Self::TrimIn => Some(Binding::TrimStartAtPlayhead),
            Self::TrimOut => Some(Binding::TrimEndAtPlayhead),
            Self::ZoomIn => Some(Binding::ZoomIn),
            Self::ZoomOut => Some(Binding::ZoomOut),
            Self::Snap => Some(Binding::ToggleSnap),
            Self::Link => Some(Binding::ToggleLink),
            Self::Draft => Some(Binding::Draft(maj_ui::Draft::Half)),
            Self::Tiled => Some(Binding::ToggleTiled),
            Self::Palette => Some(Binding::TogglePalette),
            Self::Example => Some(Binding::Example),
            Self::Export => Some(Binding::ExportStill),
            Self::AddVideoTrack => Some(Binding::AddVideoTrack),
            Self::AddAudioTrack => Some(Binding::AddAudioTrack),
            // Fit has no stroke of its own; it is one press of the zoom-out ladder away, and it
            // is in the View menu, which the keyboard can open.
            Self::ZoomFit => None,
            // The marker tool used to be the one button with no stroke, because the model had
            // no marker entity for it to place; it has one now, and `m` is the key Premiere
            // and Resolve both use for it.
            Self::Marker => Some(Binding::AddMarker),
        }
    }

    /// The stroke, written the way the keyboard documentation writes it.
    #[must_use]
    pub(crate) const fn hint(self) -> Option<&'static str> {
        match self {
            Self::Undo => Some("ctrl+z"),
            Self::Redo => Some("ctrl+shift+z"),
            Self::Split => Some("s"),
            Self::Remove => Some("supr"),
            Self::Duplicate => Some("ctrl+d"),
            Self::Marker => Some("m"),
            Self::TrimIn => Some("["),
            Self::TrimOut => Some("]"),
            Self::ZoomIn => Some("+"),
            Self::ZoomOut => Some("-"),
            Self::Snap => Some("alt+i"),
            Self::Link => Some("alt+l"),
            Self::Tiled => Some("ctrl+t"),
            Self::Palette => Some("ctrl+k"),
            Self::AddVideoTrack => Some("n"),
            Self::AddAudioTrack => Some("shift+n"),
            _ => None,
        }
    }
}

/// A panel tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Tab {
    /// The media pool.
    Media,
    /// The title studio: the words, the face and the size a title would be set with.
    Titles,
    /// The command log.
    History,
    /// The selected clip's own controls.
    Inspector,
    /// Every effect the engine has, to add to the selection.
    EffectList,
    /// Every transition the engine has, to add at the playhead.
    TransitionList,
    /// The mixer, which has no engine behind it yet.
    Mixer,
}

impl Tab {
    /// Which column the tab belongs to: the left one or the right one.
    ///
    /// Three tabs are the left column's — the pool, the title studio and the log — and four
    /// are the right column's. Writing it down once means a zone cannot switch the wrong
    /// column, which is what a click on a tab used to decide from the tab alone.
    #[must_use]
    pub(crate) const fn is_left(self) -> bool {
        matches!(self, Self::Media | Self::Titles | Self::History)
    }
}

/// A divider that can be dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Splitter {
    /// Between the left column and the middle.
    Left,
    /// Between the middle and the right column.
    Right,
    /// Between the upper half and the timeline.
    Timeline,
    /// Between two panels inside one column, on the cut page.
    Inline,
}

/// Which part of a clip a point is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ClipPart {
    /// The body: dragging it moves the clip.
    Body,
    /// The head: dragging it trims.
    Head,
    /// The tail: dragging it trims.
    Tail,
}

/// A button in the transport bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TransportButton {
    /// The first frame.
    Start,
    /// One frame back.
    PrevFrame,
    /// Play, or pause when already playing.
    Play,
    /// One frame on.
    NextFrame,
    /// Past the last frame.
    End,
    /// Play the sequence again when it ends.
    Loop,
}

impl TransportButton {
    /// The picture.
    #[must_use]
    pub(crate) const fn glyph(self, playing: bool) -> Glyph {
        match self {
            Self::Start => Glyph::Start,
            Self::PrevFrame => Glyph::PrevFrame,
            Self::Play => {
                if playing {
                    Glyph::Pause
                } else {
                    Glyph::Play
                }
            }
            Self::NextFrame => Glyph::NextFrame,
            Self::End => Glyph::End,
            Self::Loop => Glyph::Loop,
        }
    }

    /// What it says.
    #[must_use]
    pub(crate) const fn label(self, playing: bool) -> Key {
        match self {
            Self::Start => Key::TransportStart,
            Self::PrevFrame => Key::ViewStepBack,
            Self::Play => {
                if playing {
                    Key::TransportPause
                } else {
                    Key::TransportPlay
                }
            }
            Self::NextFrame => Key::ViewStepForward,
            Self::End => Key::TransportEnd,
            Self::Loop => Key::TransportLoop,
        }
    }

    /// The stroke that does the same thing.
    #[must_use]
    pub(crate) const fn hint(self) -> Option<&'static str> {
        match self {
            Self::Start => Some("inicio"),
            Self::PrevFrame => Some("←"),
            Self::Play => Some("espacio"),
            Self::NextFrame => Some("→"),
            Self::End => Some("fin"),
            Self::Loop => Some("alt+b"),
        }
    }
}

/// What a control on a track header does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TrackControl {
    /// Whether the track is rendered at all.
    Enable,
    /// Whether it is heard. No audio model yet, so this one is drawn disabled.
    Mute,
    /// Whether it can be edited. No editorial enforcement yet, so this too.
    Lock,
}

/// Something the pointer can be over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Zone {
    /// One of the frame's own buttons.
    FrameButton(FrameButton),
    /// A menu in the menu bar.
    Menu(usize),
    /// An item in the menu that is open.
    MenuItem(usize),
    /// A toolbar button.
    Tool(Tool),
    /// A button in the timeline's own tool strip.
    TimelineTool(Tool),
    /// A page tab.
    Page(Page),
    /// A panel tab.
    Tab(Tab),
    /// A divider.
    Splitter(Splitter),
    /// A tile in the media pool.
    MediaTile(usize),
    /// A row of the command log.
    LogRow(usize),
    /// An index in the inspector.
    InspectorRow(usize),
    /// A clip, and which part of it.
    Clip {
        /// Which clip.
        clip: ClipId,
        /// Which part.
        part: ClipPart,
    },
    /// A track's header.
    /// A diamond in the inspector: one channel's keyframe switch.
    Keyframe(AnimChannel),
    TrackHeader(usize),
    /// A control on a track's header.
    TrackControl {
        /// Which track.
        track: usize,
        /// Which control.
        control: TrackControl,
    },
    /// The ruler, where scrubbing happens.
    Ruler,
    /// Empty timeline background.
    Timeline,
    /// The audio meters, which say why they are empty.
    Meter,
    /// A button in the transport bar.
    Transport(TransportButton),
    /// The transport's editable timecode field.
    TimecodeField,
    /// The playhead itself.
    Playhead,
    /// The deliver page's render button.
    Render,
    /// The palette's search field.
    PaletteField,
    /// A row in the palette.
    PaletteRow(usize),
    /// The palette's backdrop, which a click closes.
    PaletteBackdrop,
    /// The pool's button for bringing a still in from disk.
    ImportMedia,
    /// The title studio's words field, which takes the keyboard when it is clicked.
    TitleField,
    /// One of the title studio's faces.
    TitleFace(usize),
    /// One of the title studio's alignments.
    TitleAlign(usize),
    /// One of the title studio's size steps: `-1` smaller, `1` larger.
    TitleSize(i32),
    /// The title studio's button: set the words and put them on the timeline.
    TitleAdd,
    /// One effect in the browser, added to the selection when it is clicked.
    EffectRow(usize),
    /// One transition in the browser, added at the playhead when it is clicked.
    TransitionRow(usize),
}

impl From<Splitter> for maj_ui::Split {
    /// The shell's own name for the same divider.
    fn from(splitter: Splitter) -> Self {
        match splitter {
            Splitter::Left => Self::Left,
            Splitter::Right => Self::Right,
            Splitter::Timeline => Self::Timeline,
            Splitter::Inline => Self::Inline,
        }
    }
}

/// One interactive rectangle.
#[derive(Debug, Clone)]
pub(crate) struct Region {
    /// What it is.
    pub(crate) zone: Zone,
    /// Where it is.
    pub(crate) rect: Rect,
    /// What the pointer should look like over it.
    pub(crate) cursor: CursorKind,
    /// What a tooltip says.
    pub(crate) label: Key,
    /// The stroke that does the same thing, when there is one.
    pub(crate) hint: Option<&'static str>,
    /// Whether it can be used at all.
    pub(crate) usable: bool,
    /// Why it cannot, when it cannot: the phase that owns it.
    pub(crate) reason: Option<Key>,
}

impl Region {
    /// A usable region.
    pub(crate) const fn new(zone: Zone, rect: Rect, cursor: CursorKind, label: Key) -> Self {
        Self {
            zone,
            rect,
            cursor,
            label,
            hint: None,
            usable: true,
            reason: None,
        }
    }

    /// The same region, with the stroke that reaches it.
    #[must_use]
    pub(crate) const fn hinted(mut self, hint: Option<&'static str>) -> Self {
        self.hint = hint;
        self
    }

    /// The same region, refused, with the reason.
    #[must_use]
    pub(crate) const fn refused(mut self, reason: Key) -> Self {
        self.usable = false;
        self.reason = Some(reason);
        self
    }

    /// Whether a point is inside it.
    #[must_use]
    pub(crate) const fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.rect.left && x < self.rect.right && y >= self.rect.top && y < self.rect.bottom
    }
}

/// The table of regions the last paint produced.
#[derive(Debug, Default)]
pub(crate) struct Zones {
    regions: Vec<Region>,
}

impl Zones {
    /// An empty table.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            regions: Vec::new(),
        }
    }

    /// Forgets everything, which is what a new layout does.
    pub(crate) fn clear(&mut self) {
        self.regions.clear();
    }

    /// Adds a region, on top of everything already there.
    pub(crate) fn push(&mut self, region: Region) {
        self.regions.push(region);
    }

    /// The region a point is over: the last one added, which is the one drawn on top.
    #[must_use]
    pub(crate) fn at(&self, x: i32, y: i32) -> Option<&Region> {
        self.regions
            .iter()
            .rev()
            .find(|region| region.contains(x, y))
    }

    /// The region for a zone, if it was drawn.
    #[must_use]
    pub(crate) fn get(&self, zone: Zone) -> Option<&Region> {
        self.regions.iter().find(|region| region.zone == zone)
    }

    /// How many regions there are, for the report.
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.regions.len()
    }

    /// Every region, for the tests that walk the table.
    #[must_use]
    pub(crate) fn regions(&self) -> &[Region] {
        &self.regions
    }
}
