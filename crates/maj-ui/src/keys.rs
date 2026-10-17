//! The keyboard, as a table rather than as a pile of conditionals.
//!
//! A binding is an *intent that needs the shell's state*: "trim the head of the selection to
//! wherever the playhead is" cannot be an [`Action`](crate::Action), because the action
//! carries the frame and only the shell knows it. So the table maps a stroke to a
//! [`Binding`], and [`Shell::perform`](crate::Shell::perform) resolves it against the state
//! it actually has.
//!
//! The table is data. A test walks it to prove no two strokes collide and that the letters
//! this document promises actually do what it says, which is the only way a keymap stays
//! honest as it grows.
//!
//! Where a choice was free it follows Premiere, because a person arriving from there should
//! not have to relearn the arrows: `←` and `→` step a frame, `Shift` widens the step, `Alt`
//! moves the clip instead of the playhead, `Up` and `Down` change track, `Ctrl+Z` and
//! `Ctrl+Shift+Z` are undo and redo, `Ctrl+K` opens the palette, `Delete` removes, `S`
//! splits, `Space` plays and `J`/`K`/`L` shuttle.
//!
//! The transport was the one deliberate departure, and it was reversed by a measurement. The
//! table used to leave `space` and `J`/`K`/`L` unbound, because there was no playback engine
//! and a transport key that did nothing visible would have been worse than an absent one; the
//! test `the_transport_keys_exist_because_the_clock_does` asserted exactly that, so the day a
//! clock existed the test was the thing that had to change. It does now: the window drives a
//! rational clock that moves the playhead and redraws the preview. What it does not do is
//! decode media (M2) or make a sound (M9), and `docs/UI.md` says which those are.

use crate::action::{Draft, Panel};
use crate::view::Page;

/// A physical press, without its modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Press {
    /// A character key, lowercased so `S` and `s` are the same stroke.
    Character(char),
    /// An arrow, in the direction of travel along the timeline or across tracks.
    Arrow(Direction),
    /// The first frame.
    Home,
    /// The frame after the last one.
    End,
    /// Ten seconds earlier.
    PageUp,
    /// Ten seconds later.
    PageDown,
    /// Remove what is selected.
    Delete,
    /// Close what is open.
    Escape,
    /// Confirm what is offered.
    Enter,
    /// A wider step.
    Tab,
}

/// Which way an arrow points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Earlier on the timeline.
    Left,
    /// Later on the timeline.
    Right,
    /// Toward the first track.
    Up,
    /// Toward the last track.
    Down,
}

/// A press and its modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Stroke {
    /// What was pressed.
    pub press: Press,
    /// Control, or Command.
    pub ctrl: bool,
    /// Shift.
    pub shift: bool,
    /// Alt, or Option.
    pub alt: bool,
}

impl Stroke {
    /// A bare press.
    #[must_use]
    pub const fn new(press: Press) -> Self {
        Self {
            press,
            ctrl: false,
            shift: false,
            alt: false,
        }
    }

    /// The same press with Control held.
    #[must_use]
    pub const fn ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    /// The same press with Shift held.
    #[must_use]
    pub const fn shift(mut self) -> Self {
        self.shift = true;
        self
    }

    /// The same press with Alt held.
    #[must_use]
    pub const fn alt(mut self) -> Self {
        self.alt = true;
        self
    }

    /// A character, lowercased.
    #[must_use]
    pub const fn character(value: char) -> Self {
        Self::new(Press::Character(value))
    }
}

/// Something a stroke asks for, before the shell resolves it against its own state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    /// Move the playhead by whole frames.
    StepFrames(i64),
    /// Move the playhead by whole seconds at the sequence's rate.
    StepSeconds(i64),
    /// Move the playhead to the first frame.
    GoToStart,
    /// Move the playhead past the last frame.
    GoToEnd,
    /// Step the playhead and bring the clip under it into the selection.
    StepFramesAndSelect(i64),
    /// Zoom the timeline in.
    ZoomIn,
    /// Zoom the timeline out.
    ZoomOut,
    /// Trim the selected clip's head to the playhead.
    TrimStartAtPlayhead,
    /// Trim the selected clip's tail to the playhead.
    TrimEndAtPlayhead,
    /// Move the selected clip by whole frames.
    MoveSelected(i64),
    /// Move the selected clip to an adjacent track.
    MoveTrack(i64),
    /// Split the selected clip at the playhead.
    Split,
    /// Remove the selected clip.
    Remove,
    /// Duplicate the selected clip.
    Duplicate,
    /// Enable or disable the selected clip.
    ToggleEnabled,
    /// Undo the last command.
    Undo,
    /// Redo the last undone command.
    Redo,
    /// Write the document to the path it came from.
    Save,
    /// Build the demo project out of real commands.
    Example,
    /// Show or hide the command palette.
    TogglePalette,
    /// Choose the preview resolution.
    Draft(Draft),
    /// Render the preview through tiles, or stop doing that.
    ToggleTiled,
    /// Switch the interface's language.
    ToggleLanguage,
    /// Add a video track to the active sequence.
    AddVideoTrack,
    /// Add an audio track to the active sequence.
    AddAudioTrack,
    /// Place the media under the cursor at the playhead.
    PlaceSelection,
    /// Fit the whole sequence into the window.
    ZoomFit,
    /// Write the frame under the playhead as a still, with its manifest.
    ExportStill,
    /// Read a document from a path the person chooses.
    Open,
    /// Write the document to a new path.
    SaveAs,
    /// Start the transport clock, or stop it.
    PlayPause,
    /// Run the clock forwards or backwards, faster with each press. Zero pauses it.
    Shuttle(i64),
    /// Start again when the clock reaches the end.
    ToggleLoop,
    /// Move to the next page, or the previous one when the step is negative.
    CyclePage(i64),
    /// Go to a page.
    Page(Page),
    /// Open or close one panel.
    TogglePanel(Panel),
    /// Snap a dragged edge to the playhead and to other clips.
    ToggleSnap,
    /// Keep the selection linked while editing.
    ToggleLink,
    /// Put the playhead on the next or previous edit point: a clip's head or its tail.
    StepEditPoint(i64),
    /// Select the next or previous clip and put the playhead on its head.
    SelectAdjacent(i64),
    /// Close the gap in front of the selected clip.
    CloseGap,
    /// Remove the selected clip and close the gap it leaves.
    RippleDelete,
    /// Open a one-second gap at the playhead on the selected clip's track.
    InsertBlank,
    /// Anchor a note to the active sequence at the playhead.
    AddMarker,
    /// Take the marker under the playhead away.
    RemoveMarker,
    /// Move the marker under the playhead to the next of the eight colours.
    CycleMarkerColour,
    /// Put the playhead on the next or previous keyframe of the selected clip.
    StepKeyframe(i64),
    /// Put a keyframe on every channel of the selected clip that is already animated, or take
    /// the keys under the playhead away again.
    ToggleKeyframes,
    /// Take every curve off the selected clip and return it to its static values.
    ClearKeyframes,
}

/// Every stroke the interface answers to.
///
/// A test reads this table, so a binding can only be added by also adding its stroke here.
pub const TABLE: &[(Stroke, Binding)] = &[
    // The marker keys follow the two conventions an editor arrives with: `m` places a mark
    // (Premiere and Resolve both use it), `shift+m` takes it away rather than placing a
    // second one on the same frame, and `alt+m` walks the colour, which is what every other
    // cycling key in this table does with `alt`.
    (Stroke::character('m'), Binding::AddMarker),
    (Stroke::character('m').shift(), Binding::RemoveMarker),
    (Stroke::character('m').alt(), Binding::CycleMarkerColour),
    (
        Stroke::new(Press::Arrow(Direction::Left)),
        Binding::StepFrames(-1),
    ),
    (
        Stroke::new(Press::Arrow(Direction::Right)),
        Binding::StepFrames(1),
    ),
    (
        Stroke::new(Press::Arrow(Direction::Left)).shift(),
        Binding::StepFrames(-10),
    ),
    (
        Stroke::new(Press::Arrow(Direction::Right)).shift(),
        Binding::StepFrames(10),
    ),
    (
        Stroke::new(Press::Arrow(Direction::Left)).ctrl(),
        Binding::StepSeconds(-1),
    ),
    (
        Stroke::new(Press::Arrow(Direction::Right)).ctrl(),
        Binding::StepSeconds(1),
    ),
    (
        Stroke::new(Press::Arrow(Direction::Left)).alt(),
        Binding::MoveSelected(-1),
    ),
    (
        Stroke::new(Press::Arrow(Direction::Right)).alt(),
        Binding::MoveSelected(1),
    ),
    (
        Stroke::new(Press::Arrow(Direction::Left)).alt().shift(),
        Binding::MoveSelected(-10),
    ),
    (
        Stroke::new(Press::Arrow(Direction::Right)).alt().shift(),
        Binding::MoveSelected(10),
    ),
    (
        Stroke::new(Press::Arrow(Direction::Up)),
        Binding::MoveTrack(-1),
    ),
    (
        Stroke::new(Press::Arrow(Direction::Down)),
        Binding::MoveTrack(1),
    ),
    (
        Stroke::new(Press::Arrow(Direction::Up)).shift(),
        Binding::StepFramesAndSelect(-1),
    ),
    (
        Stroke::new(Press::Arrow(Direction::Down)).shift(),
        Binding::StepFramesAndSelect(1),
    ),
    (Stroke::new(Press::Home), Binding::GoToStart),
    (Stroke::new(Press::End), Binding::GoToEnd),
    (Stroke::new(Press::PageUp), Binding::StepSeconds(-10)),
    (Stroke::new(Press::PageDown), Binding::StepSeconds(10)),
    (Stroke::new(Press::Delete), Binding::Remove),
    (Stroke::character('s'), Binding::Split),
    (Stroke::character('['), Binding::TrimStartAtPlayhead),
    (Stroke::character(']'), Binding::TrimEndAtPlayhead),
    (Stroke::character('e'), Binding::ToggleEnabled),
    (Stroke::character('i'), Binding::PlaceSelection),
    (Stroke::character('+'), Binding::ZoomIn),
    (Stroke::character('='), Binding::ZoomIn),
    (Stroke::character('-'), Binding::ZoomOut),
    (Stroke::character('n'), Binding::AddVideoTrack),
    (Stroke::character('n').shift(), Binding::AddAudioTrack),
    (Stroke::character('z').ctrl(), Binding::Undo),
    (Stroke::character('z').ctrl().shift(), Binding::Redo),
    (Stroke::character('y').ctrl(), Binding::Redo),
    (Stroke::character('d').ctrl(), Binding::Duplicate),
    (Stroke::character('k').ctrl(), Binding::TogglePalette),
    (Stroke::character('s').ctrl(), Binding::Save),
    (Stroke::character('l').ctrl(), Binding::ToggleLanguage),
    (Stroke::character('t').ctrl(), Binding::ToggleTiled),
    (Stroke::character('1').ctrl(), Binding::Draft(Draft::Full)),
    (Stroke::character('2').ctrl(), Binding::Draft(Draft::Half)),
    (
        Stroke::character('3').ctrl(),
        Binding::Draft(Draft::Quarter),
    ),
    // The transport. `space` starts the clock and `J`/`K`/`L` shuttle it, in the arrangement
    // every editor's hands already know. They are bound here — and were deliberately not bound
    // before — because a clock now exists: pressing one moves the picture and says so. What
    // they do *not* do is decode media or make a sound, and `docs/UI.md` says which phases
    // those are.
    (Stroke::character(' '), Binding::PlayPause),
    (Stroke::character('j'), Binding::Shuttle(-1)),
    (Stroke::character('k'), Binding::Shuttle(0)),
    (Stroke::character('l'), Binding::Shuttle(1)),
    (Stroke::character('b').alt(), Binding::ToggleLoop),
    // Pages, the dividers and the interface's switches.
    (Stroke::new(Press::Tab).ctrl(), Binding::CyclePage(1)),
    (
        Stroke::new(Press::Tab).ctrl().shift(),
        Binding::CyclePage(-1),
    ),
    (Stroke::character('1').alt(), Binding::Page(Page::Media)),
    (Stroke::character('2').alt(), Binding::Page(Page::Cut)),
    (Stroke::character('3').alt(), Binding::Page(Page::Deliver)),
    (Stroke::character('0').alt(), Binding::ZoomFit),
    (Stroke::character('e').alt(), Binding::ToggleSnap),
    (Stroke::character('i').alt(), Binding::ToggleLink),
    (Stroke::character('x').alt(), Binding::ExportStill),
    (Stroke::character('o').alt(), Binding::Open),
    (Stroke::character('s').ctrl().shift(), Binding::SaveAs),
    // Navigation along the cuts, and the three edits that move a whole track. The commas are
    // where a person's hand is already: `.` is the cut after this one and `,` the cut before it,
    // and the same strokes with `shift` take the clip at that cut into the selection. `g` closes
    // a gap and `shift+g` opens one — the two halves of the same idea, on the same key.
    (Stroke::character(','), Binding::StepEditPoint(-1)),
    (Stroke::character('.'), Binding::StepEditPoint(1)),
    (Stroke::character(',').shift(), Binding::SelectAdjacent(-1)),
    (Stroke::character('.').shift(), Binding::SelectAdjacent(1)),
    (Stroke::character('g'), Binding::CloseGap),
    (Stroke::character('g').shift(), Binding::InsertBlank),
    (Stroke::new(Press::Delete).shift(), Binding::RippleDelete),
    // Keyframes. The two commas with `alt` walk from key to key the way `,` and `.` walk from cut
    // to cut, and `alt+k` writes a key on every channel that is already animated — the gesture
    // Resolve spells as a stopwatch, on the key whose letter is the word. A channel with no curve
    // is not invented by it: a key is put on a curve, not on a parameter nobody animated.
    (Stroke::character(',').alt(), Binding::StepKeyframe(-1)),
    (Stroke::character('.').alt(), Binding::StepKeyframe(1)),
    (Stroke::character('k').alt(), Binding::ToggleKeyframes),
    (
        Stroke::character('k').alt().shift(),
        Binding::ClearKeyframes,
    ),
];

/// What a stroke asks for, if anything.
#[must_use]
pub fn binding(stroke: Stroke) -> Option<Binding> {
    TABLE
        .iter()
        .find(|(candidate, _)| *candidate == stroke)
        .map(|(_, binding)| *binding)
}

/// Every stroke in the table, for a document or a help screen to list.
#[must_use]
pub fn strokes() -> Vec<Stroke> {
    TABLE.iter().map(|(stroke, _)| *stroke).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_two_strokes_collide() {
        let mut seen = std::collections::HashSet::new();
        for (stroke, binding) in TABLE {
            assert!(
                seen.insert(*stroke),
                "{stroke:?} is bound twice, and the second is {binding:?}"
            );
        }
    }

    #[test]
    fn the_arrows_step_and_the_wider_modifiers_widen_it() {
        assert_eq!(
            binding(Stroke::new(Press::Arrow(Direction::Right))),
            Some(Binding::StepFrames(1))
        );
        assert_eq!(
            binding(Stroke::new(Press::Arrow(Direction::Right)).shift()),
            Some(Binding::StepFrames(10))
        );
        assert_eq!(
            binding(Stroke::new(Press::Arrow(Direction::Right)).ctrl()),
            Some(Binding::StepSeconds(1))
        );
    }

    #[test]
    fn alt_moves_the_clip_rather_than_the_playhead() {
        assert_eq!(
            binding(Stroke::new(Press::Arrow(Direction::Left)).alt()),
            Some(Binding::MoveSelected(-1))
        );
        assert_eq!(
            binding(Stroke::new(Press::Arrow(Direction::Up))),
            Some(Binding::MoveTrack(-1))
        );
    }

    #[test]
    fn undo_and_redo_are_where_a_premiere_editor_expects_them() {
        assert_eq!(binding(Stroke::character('z').ctrl()), Some(Binding::Undo));
        assert_eq!(
            binding(Stroke::character('z').ctrl().shift()),
            Some(Binding::Redo)
        );
        assert_eq!(binding(Stroke::character('y').ctrl()), Some(Binding::Redo));
    }

    #[test]
    fn the_transport_keys_exist_because_the_clock_does() {
        // This test used to assert the opposite — that `space`, `J`, `K` and `L` were unbound,
        // because there was no playback engine and a transport key that did nothing would have
        // been a lie. The clock changed that: these strokes now move the playhead in real time
        // and repaint the preview, so they do something visible. What they do not do is decode
        // media or make a sound, which is M2, M3 and M9, and which the status bar says.
        assert_eq!(binding(Stroke::character(' ')), Some(Binding::PlayPause));
        assert_eq!(binding(Stroke::character('j')), Some(Binding::Shuttle(-1)));
        assert_eq!(binding(Stroke::character('k')), Some(Binding::Shuttle(0)));
        assert_eq!(binding(Stroke::character('l')), Some(Binding::Shuttle(1)));
    }

    #[test]
    fn the_pages_have_a_keyboard_of_their_own() {
        assert_eq!(
            binding(Stroke::new(Press::Tab).ctrl()),
            Some(Binding::CyclePage(1))
        );
        assert_eq!(
            binding(Stroke::character('2').alt()),
            Some(Binding::Page(Page::Cut))
        );
        // The draft resolutions keep the unmodified control digits.
        assert_eq!(
            binding(Stroke::character('1').ctrl()),
            Some(Binding::Draft(Draft::Full))
        );
    }

    #[test]
    fn an_unbound_stroke_is_silent_rather_than_wrong() {
        assert_eq!(binding(Stroke::character('q')), None);
        assert_eq!(binding(Stroke::character('q').ctrl().shift().alt()), None);
    }

    #[test]
    fn the_table_is_not_empty_and_lists_its_strokes() {
        assert!(TABLE.len() > 20);
        assert_eq!(strokes().len(), TABLE.len());
    }
}
