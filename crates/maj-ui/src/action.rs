//! Intentions, and the commands they become.
//!
//! This module is the whole contract between the interface and the engine. A person does
//! something; it becomes one of these; [`Action::commands`] turns it into the engine's own
//! [`Command`] values and nothing else. There is no second path: the shell applies what this
//! returns through the same session the Model Context Protocol server holds, so an edit made
//! by hand and an edit made by a model are the same kind of object in the same log.
//!
//! Three things are deliberate.
//!
//! * **One intention, one or more commands, never a mutation.** Duplicating a clip is an
//!   insert plus the commands that restore its look; splitting one is a trim plus an insert.
//!   Both are expressed as sequences of ordinary commands rather than as a special
//!   operation, because "everything is a command" has to survive contact with the interface
//!   or it was never true.
//! * **Refusal is a value.** An intention that does not apply to the state it arrived in —
//!   trimming nothing, splitting where there is no cut — returns [`UiError`] rather than
//!   doing something approximately right.
//! * **The engine validates.** This module computes what the person meant; whether that is
//!   *legal* (a trim inside the media, a positive speed, a unique identifier) is the model's
//!   decision, and its refusal is carried through word for word.

use std::collections::BTreeMap;
use std::path::PathBuf;

use maj_core::{
    AnimChannel, BlendMode, Clip, ClipId, Command, EffectId, EffectInstance, EffectKind,
    Interpolation, Keyframe, Marker, MarkerColour, MarkerId, Mask, MaskId, MaskShape, MediaAsset,
    MediaAssetId, ParamValue, Project, Sequence, SequenceId, Track, TrackId, TrackKind, Transform,
    Transition, TransitionId, TransitionKind,
};
use maj_render::{TitleAlign, TitleFace};
use maj_time::{FrameRange, Rational, Timebase};
use serde::{Deserialize, Serialize};

use crate::UiError;
use crate::i18n::Lang;
use crate::view::{Page, Split};

/// A panel the person can show or hide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Panel {
    /// The media pool.
    Media,
    /// The command log.
    History,
    /// The effect controls.
    Effects,
    /// The timeline.
    Timeline,
    /// The two monitors.
    Monitors,
}

/// Which panels are open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Panels {
    /// The media pool.
    pub media: bool,
    /// The command log.
    pub history: bool,
    /// The effect controls.
    pub effects: bool,
    /// The timeline.
    pub timeline: bool,
    /// The two monitors.
    pub monitors: bool,
}

impl Default for Panels {
    /// Every panel open. The interface starts with everything on.
    fn default() -> Self {
        Self {
            media: true,
            history: true,
            effects: true,
            timeline: true,
            monitors: true,
        }
    }
}

impl Panels {
    /// Whether one panel is open.
    #[must_use]
    pub const fn is_open(self, panel: Panel) -> bool {
        match panel {
            Panel::Media => self.media,
            Panel::History => self.history,
            Panel::Effects => self.effects,
            Panel::Timeline => self.timeline,
            Panel::Monitors => self.monitors,
        }
    }

    /// Opens or closes one panel, returning the new state.
    #[must_use]
    pub const fn toggled(mut self, panel: Panel) -> Self {
        match panel {
            Panel::Media => self.media = !self.media,
            Panel::History => self.history = !self.history,
            Panel::Effects => self.effects = !self.effects,
            Panel::Timeline => self.timeline = !self.timeline,
            Panel::Monitors => self.monitors = !self.monitors,
        }
        self
    }
}

/// How much of the sequence's resolution the viewer asks the renderer for.
///
/// A draft is not an optimisation to be hidden: it changes the pixels, so the interface says
/// which one is in play, in the monitor and in the status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Draft {
    /// The sequence's own resolution.
    Full,
    /// Half of it, rounded up.
    Half,
    /// A quarter of it, rounded up.
    Quarter,
}

impl Draft {
    /// Every draft, from the sharpest to the fastest.
    pub const ALL: [Self; 3] = [Self::Full, Self::Half, Self::Quarter];

    /// The divisor this draft applies to a dimension.
    #[must_use]
    pub const fn divisor(self) -> u32 {
        match self {
            Self::Full => 1,
            Self::Half => 2,
            Self::Quarter => 4,
        }
    }

    /// Scales a dimension, never to zero.
    ///
    /// Rounded up, so a half-resolution draft of an odd width is one pixel wider rather than
    /// one narrower: a preview that quietly loses a column of the frame is a preview that
    /// lies about the composition.
    #[must_use]
    pub const fn scale(self, value: u32) -> u32 {
        let divisor = self.divisor();
        let scaled = value / divisor + if value % divisor == 0 { 0 } else { 1 };
        if scaled == 0 { 1 } else { scaled }
    }

    /// Short stable name, for preferences and the manifest.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Half => "half",
            Self::Quarter => "quarter",
        }
    }
}

/// One field of a clip's placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransformField {
    /// Horizontal translation, in pixels.
    X,
    /// Vertical translation, in pixels.
    Y,
    /// Horizontal scale.
    ScaleX,
    /// Vertical scale.
    ScaleY,
    /// Rotation in degrees.
    Rotation,
}

/// What an intention needs to know about the shell to become a command.
///
/// Passing this rather than reading the shell keeps every translation testable without a
/// screen: a test builds a project, a selection and a playhead, and asserts the command.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The document.
    pub project: &'a Project,
    /// The sequence the person is working in.
    pub sequence: Option<SequenceId>,
    /// The selected clip, if any.
    pub selection: Option<ClipId>,
    /// The frame the playhead sits on.
    pub playhead: i64,
}

impl<'a> Context<'a> {
    /// Gathers what an action needs.
    #[must_use]
    pub const fn new(
        project: &'a Project,
        sequence: Option<SequenceId>,
        selection: Option<ClipId>,
        playhead: i64,
    ) -> Self {
        Self {
            project,
            sequence,
            selection,
            playhead,
        }
    }

    /// The active sequence, or a refusal.
    pub fn sequence(&self) -> Result<&'a Sequence, UiError> {
        self.sequence
            .and_then(|id| self.project.sequence(id))
            .ok_or(UiError::no_sequence())
    }

    /// The selected clip, or a refusal.
    pub fn selected(&self) -> Result<&'a Clip, UiError> {
        self.selection
            .and_then(|id| self.project.clip(id))
            .ok_or(UiError::no_selection())
    }

    /// The sequence that holds a clip, a refusal when it is gone.
    pub fn owning_sequence(&self, clip: ClipId) -> Result<SequenceId, UiError> {
        self.project
            .locate_clip(clip)
            .map(|(sequence, _, _)| sequence)
            .ok_or(UiError::no_selection())
    }

    /// The track that holds a clip, with the clip's position within it.
    pub fn track_of(&self, clip: ClipId) -> Result<(&'a Track, usize), UiError> {
        let (sequence, track, _) = self
            .project
            .locate_clip(clip)
            .ok_or(UiError::no_selection())?;
        let sequence = self
            .project
            .sequence(sequence)
            .ok_or(UiError::no_sequence())?;
        let track = sequence.track_at(track).ok_or(UiError::no_selection())?;
        let index = track.clip_position(clip).ok_or(UiError::no_selection())?;
        Ok((track, index))
    }

    /// The first video track of the active sequence.
    pub fn first_video_track(&self) -> Result<&'a Track, UiError> {
        self.sequence()?
            .tracks()
            .iter()
            .find(|track| track.kind() == TrackKind::Video)
            .ok_or(UiError::NotApplicable(
                "the sequence has no video track to place a clip on",
            ))
    }
}

/// Something a person asked for.
///
/// View intentions produce no command; editing intentions produce one or more. The variants
/// that name an entity carry it explicitly when the caller already knows one — a palette row
/// naming an effect — and omit it when the shell should supply the selection.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Move the playhead by a whole number of frames, clamped to the sequence.
    StepFrames(i64),
    /// Move the playhead by a number of seconds, at the sequence's rate.
    StepSeconds(i64),
    /// Put the playhead at a frame.
    SeekTo(i64),
    /// Put the playhead on the first frame of the sequence's content.
    GoToStart,
    /// Put the playhead on the frame after the last one.
    GoToEnd,
    /// Multiply the timeline zoom.
    ZoomBy(f64),
    /// Show or hide a panel.
    TogglePanel(Panel),
    /// Go to a page of the interface.
    SetPage(Page),
    /// Move to the next page, or the previous one when the step is negative.
    CyclePage(i64),
    /// Move one divider.
    SetSplit(Split, i32),
    /// Put every divider back where it started.
    ResetSplits,
    /// Snap a dragged edge to the playhead and to other clips, or stop doing that.
    SetSnap(bool),
    /// Keep the selection linked while editing, or stop doing that.
    SetLink(bool),
    /// Start the transport again when it reaches the end, or stop doing that.
    SetLoops(bool),
    /// Choose the preview resolution.
    SetDraft(Draft),
    /// Render the preview through tiles, which costs more and proves the tiling agrees.
    SetTiled(bool),
    /// Choose the curve the viewer's code values are drawn through.
    SetTransfer(maj_color::TransferFunction),
    /// Switch the interface's language.
    SetLang(Lang),
    /// Work in a different sequence.
    SelectSequence(SequenceId),
    /// Select a clip.
    SelectClip(ClipId),
    /// Select nothing.
    ClearSelection,
    /// Show the command palette.
    OpenPalette,
    /// Hide the command palette.
    ClosePalette,
    /// Replace the palette's search text.
    SetPaletteQuery(String),
    /// Move the palette's selection to a row.
    SetPaletteRow(usize),
    /// Set one argument field of the palette's selected row, as the text that was typed.
    SetPaletteArg {
        /// The field's JSON key, from the tool's catalogue entry.
        field: String,
        /// What was typed. Decoded by the tool, not here.
        value: String,
    },
    /// Run one of the agent's tools against this session.
    RunTool {
        /// The tool's name, as the catalogue advertises it.
        name: String,
        /// Its arguments, as text keyed by field name.
        args: BTreeMap<String, String>,
    },
    /// Select media in the pool.
    SelectMedia(MediaAssetId),
    /// Undo the last command.
    Undo,
    /// Redo the last undone command.
    Redo,
    /// Write the document where it came from.
    Save,
    /// Write the document to a path, and remember it.
    SaveAs(PathBuf),
    /// Read a document from a path, replacing what is open.
    Open(PathBuf),
    /// Replace the document with the example, built out of ordinary commands.
    OpenExample,
    /// Write the frame under the playhead as a still, with its manifest.
    ExportStill(PathBuf),

    /// Split the selected clip at the playhead.
    SplitAtPlayhead,
    /// Remove the selected clip.
    RemoveSelected,
    /// Duplicate the selected clip after itself, with its look and its effects.
    DuplicateSelected,
    /// Move the selected clip along the timeline by a number of frames.
    MoveSelected(i64),
    /// Move the selected clip to an adjacent track: -1 is toward the first track, 1 the other
    /// way.
    MoveSelectedToTrack(i64),
    /// Trim the selected clip's head to a timeline frame.
    TrimSelectedStart(i64),
    /// Trim the selected clip's tail to a timeline frame.
    TrimSelectedEnd(i64),
    /// Enable or disable the selected clip.
    ToggleSelectedEnabled,
    /// Set the selected clip's opacity.
    SetSelectedOpacity(Rational),
    /// Set the selected clip's blend mode.
    SetSelectedBlend(BlendMode),
    /// Set one field of the selected clip's placement.
    SetSelectedTransform(TransformField, Rational),
    /// Set the selected clip's speed, in source frames per timeline frame.
    SetSelectedSpeed(Rational),
    /// Put a keyframe on one of the selected clip's channels at the playhead, or take away the
    /// one that is already there.
    ///
    /// The value recorded is the one the channel has *now*: the sample of its curve, or the
    /// clip's own field when nothing is animated. That is what makes the first press on a
    /// diamond leave the picture exactly where it was — a keyframe records, it does not move.
    ToggleKeyframe(AnimChannel),
    /// Take away a channel's whole curve, returning the channel to the clip's static value.
    ///
    /// This is the one intention that shortens a clip's animation to nothing. It is deliberately
    /// a separate intention from [`Self::ToggleKeyframe`], because "the key under the playhead"
    /// and "every key on this channel" are different things to be sure about.
    ClearChannel(AnimChannel),
    /// Close the gap in front of the selected clip: the clip and everything after it on its
    /// track move left until they touch what is before them.
    ///
    /// Composed out of [`Command::MoveClip`]s rather than being an engine command of its own,
    /// which is what makes each step of it appear in the log as the move it is, in a fixed
    /// order, and be undone one move at a time.
    CloseGap,
    /// Remove the selected clip and close the gap it leaves, pulling the rest of its track after
    /// it.
    RippleDelete,
    /// Open a gap at the playhead on the selected clip's track, one second long, by pushing the
    /// clips that follow to the right.
    ///
    /// The hole opens *before the clip the playhead is inside* rather than at the playhead's
    /// exact frame: a cut through the middle of a clip would be a split, and this is not one.
    InsertBlank,
    /// Add an effect to the end of the selected clip's chain.
    AddEffect(EffectKind),
    /// Remove an effect from the selected clip.
    RemoveEffect(EffectId),
    /// Bypass or restore an effect without removing it.
    ToggleEffect(EffectId),
    /// Set one parameter of an effect.
    SetEffectParam {
        /// The effect to change.
        effect: EffectId,
        /// The parameter's declared key.
        key: String,
        /// Its new value.
        value: ParamValue,
    },
    /// Add a mask to the end of the selected clip's list.
    AddMask(MaskShape),
    /// Remove a mask from the selected clip.
    RemoveMask(MaskId),
    /// Swap a mask between selecting the inside and the outside.
    InvertMask(MaskId),
    /// Add a transition over the head of the selected clip, lasting this many frames.
    AddTransition(TransitionKind, i64),
    /// Remove a transition from a track.
    RemoveTransition(TransitionId),
    /// Add a track to the active sequence.
    AddTrack(TrackKind),
    /// Remove a track, with its clips.
    RemoveTrack(TrackId),
    /// Enable or disable a track: the renderer composites only the enabled ones, so this is an
    /// edit of the picture and not a preference about the window.
    SetTrackEnabled {
        /// The track to switch.
        track: TrackId,
        /// Whether it should contribute.
        enabled: bool,
    },
    /// Register media.
    CreateMediaAsset {
        /// Display name.
        name: String,
        /// Where the media is.
        uri: String,
        /// The rate it was captured at.
        timebase: Timebase,
        /// Its length in its own frames.
        duration_frames: i64,
        /// Whether it carries picture.
        has_video: bool,
        /// Whether it carries sound.
        has_audio: bool,
    },
    /// Unregister media.
    RemoveMediaAsset(MediaAssetId),
    /// Place media on the first video track at the playhead.
    InsertMediaAtPlayhead(MediaAssetId),
    /// Place media where it was dropped, on the track and at the frame the pointer named.
    ///
    /// The difference between this and [`Self::InsertMediaAtPlayhead`] is the whole point of
    /// dragging a tile: the pointer says *where*, rather than the playhead standing in for it.
    /// The track is named rather than assumed, because a drop lands on the row under the
    /// pointer and a person who drops a shot on the second track means the second track.
    DropMedia {
        /// The media to place.
        asset: MediaAssetId,
        /// The track the pointer was over.
        track: TrackId,
        /// The frame the pointer named, clamped at the head of the sequence.
        frame: i64,
    },
    /// Place media on the first video track after everything already there.
    AppendMedia(MediaAssetId),
    /// Register a still image from a file, so the pool holds what is on disk.
    ///
    /// A still is held for [`STILL_SECONDS`] seconds at the active sequence's own rate, which
    /// is what an editor means by dropping a picture on a timeline, and it is placed by the
    /// pointer or by [`Self::InsertMediaAtPlayhead`] afterwards rather than here: importing
    /// is finding the material, and placing it is an edit.
    ImportMedia {
        /// Where the file is.
        path: PathBuf,
    },
    /// Set the words the title studio will set.
    ///
    /// View state: what is typed is not in the document until [`Self::AddTitle`] is asked for.
    SetTitleText(String),
    /// Choose the face the title studio sets them in.
    SetTitleFace(TitleFace),
    /// Choose the cap height the studio sets them at, in pixels. Zero means the sequence's own
    /// tenth, which is what a title for an unknown frame should be.
    SetTitleSize(f32),
    /// Choose where the studio's line sits between the frame's margins.
    SetTitleAlign(TitleAlign),
    /// Set the studio's words as a title, and put it on the first video track at the playhead.
    ///
    /// A title is registered as media and placed, in that order, because everything is a
    /// command: the pool holds a still the renderer sets type into, and the clip is an
    /// ordinary clip that can be trimmed, moved, faded and keyframed like any other.
    AddTitle {
        /// The words.
        text: String,
        /// The built-in face to set them in.
        face: TitleFace,
        /// The cap height in pixels.
        size: f32,
        /// Where the line sits between the margins.
        align: TitleAlign,
    },
    /// Anchor a note to the active sequence at the playhead.
    ///
    /// The marker's name is the playhead's own timecode, which is the one thing the interface
    /// can call it without inventing language: it says *where* the note is, it reads the same
    /// in both languages, and a person who cares can rename it when the model grows a marker
    /// editor. The note is what the caller has to say, and an empty one is a bare point.
    AddMarker {
        /// The body of the note. May be empty.
        note: String,
        /// Which of the eight colours to draw it in.
        colour: MarkerColour,
        /// Who is placing it, in the log's own vocabulary.
        ///
        /// The marker stores its own author, and it outlives the command that made it, so a
        /// review can read whose note it is without replaying anything. That means the hand
        /// has to be carried to the intention rather than inferred from the log later.
        author: String,
    },
    /// Remove the marker covering the playhead.
    RemoveMarkerAtPlayhead,
    /// Move the marker covering the playhead to the next of the eight colours.
    CycleMarkerColourAtPlayhead,
    /// Create a sequence.
    CreateSequence {
        /// Its name.
        name: String,
        /// Its rate.
        timebase: Timebase,
        /// Its width in pixels.
        width: u32,
        /// Its height in pixels.
        height: u32,
        /// Its audio sample rate.
        sample_rate: u32,
    },
    /// Delete a sequence.
    RemoveSequence(SequenceId),
}

impl Action {
    /// The commands this intention becomes, in the order they must be applied.
    ///
    /// An empty list means the intention is a view: it changes what is displayed and nothing
    /// about the document. An error means the intention does not apply to the state it
    /// arrived in.
    ///
    /// # Errors
    ///
    /// Returns [`UiError::NotApplicable`] when the intention needs a sequence or a selection
    /// that is not there, or when the numbers it was given cannot form a range at all;
    /// whether the command is *legal* is the model's decision, taken when it is applied.
    pub fn commands(&self, context: &Context) -> Result<Vec<Command>, UiError> {
        let project = context.project;
        match self {
            Self::StepFrames(_)
            | Self::StepSeconds(_)
            | Self::SeekTo(_)
            | Self::GoToStart
            | Self::GoToEnd
            | Self::ZoomBy(_)
            | Self::TogglePanel(_)
            | Self::SetPage(_)
            | Self::CyclePage(_)
            | Self::SetSplit(_, _)
            | Self::ResetSplits
            | Self::SetSnap(_)
            | Self::SetLink(_)
            | Self::SetLoops(_)
            | Self::SetDraft(_)
            | Self::SetTiled(_)
            | Self::SetTransfer(_)
            | Self::SetLang(_)
            | Self::SelectSequence(_)
            | Self::SelectClip(_)
            | Self::ClearSelection
            | Self::OpenPalette
            | Self::ClosePalette
            | Self::SetPaletteQuery(_)
            | Self::SetPaletteRow(_)
            | Self::SetPaletteArg { .. }
            | Self::RunTool { .. }
            | Self::SelectMedia(_)
            | Self::Undo
            | Self::Redo
            | Self::Save
            | Self::SaveAs(_)
            | Self::Open(_)
            | Self::OpenExample
            | Self::ExportStill(_) => Ok(Vec::new()),

            Self::SetTrackEnabled { track, enabled } => {
                let sequence = context.sequence.ok_or_else(UiError::no_sequence)?;
                let project = context.project;
                if project
                    .sequence(sequence)
                    .and_then(|sequence| sequence.track(*track))
                    .is_none()
                {
                    return Err(UiError::NotApplicable("that track does not exist"));
                }
                Ok(vec![Command::SetTrackEnabled {
                    sequence,
                    track: *track,
                    enabled: *enabled,
                }])
            }

            Self::SplitAtPlayhead => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                let (track, index) = context.track_of(clip.id())?;
                let at = context.playhead;
                let range = clip.source_range();
                if !clip.covers(at) || at <= clip.timeline_start() {
                    return Err(UiError::NotApplicable(
                        "the playhead is not inside the clip",
                    ));
                }
                let source_at = clip.source_frame_at(at).ok_or(UiError::NotApplicable(
                    "the clip has no source at that frame",
                ))?;
                if source_at <= range.start() || source_at >= range.end() {
                    return Err(UiError::NotApplicable(
                        "the split would leave one half empty",
                    ));
                }
                let head = Command::TrimClip {
                    sequence,
                    clip: clip.id(),
                    source_range: range_of(range.start(), source_at)?,
                    timeline_start: clip.timeline_start(),
                };
                let mut allocator = Allocator::new(project);
                // The second half is a clip in the engine's default state, and the commands
                // that follow give it the first half's look. That is louder in the log than
                // one hand-built clip and it is the only way that does not write into the
                // model from the interface.
                let tail = Clip::new(
                    allocator.next_clip(),
                    clip.asset_id(),
                    clip.name(),
                    range_of(source_at, range.end())?,
                    at,
                    clip.speed(),
                )
                .map_err(refused)?;
                let tail_id = tail.id();
                let mut commands = vec![
                    head,
                    Command::InsertClip {
                        sequence,
                        track: track.id(),
                        index: index + 1,
                        clip: tail,
                    },
                ];
                commands.extend(restore_of(clip, sequence, tail_id, &mut allocator)?);
                Ok(commands)
            }

            Self::RemoveSelected => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                Ok(vec![Command::RemoveClip {
                    sequence,
                    clip: clip.id(),
                }])
            }

            Self::AddMarker {
                note,
                colour,
                author,
            } => {
                let sequence = context.sequence()?;
                let frame = context.playhead.max(0);
                let name = sequence
                    .timecode_at(frame)
                    .map(|timecode| timecode.to_string())
                    .map_err(|_| {
                        UiError::NotApplicable(
                            "this sequence's rate cannot name a frame, so a marker here \
                             would have no name and no note",
                        )
                    })?;
                let mut allocator = Allocator::new(project);
                let marker = Marker::new(allocator.next_marker(), name, note.clone(), frame, 0)
                    .map_err(refused)?
                    .with_colour(*colour)
                    .with_author(author.clone());
                Ok(vec![Command::AddMarker {
                    sequence: sequence.id(),
                    index: sequence.markers().len(),
                    marker,
                }])
            }

            Self::RemoveMarkerAtPlayhead => {
                let sequence = context.sequence()?;
                let marker = marker_at(sequence, context.playhead)?;
                Ok(vec![Command::RemoveMarker {
                    sequence: sequence.id(),
                    marker: marker.id(),
                }])
            }

            Self::CycleMarkerColourAtPlayhead => {
                let sequence = context.sequence()?;
                let marker = marker_at(sequence, context.playhead)?;
                let colours = MarkerColour::ALL;
                let next = colours
                    .iter()
                    .position(|colour| *colour == marker.colour())
                    .map_or(colours[0], |index| colours[(index + 1) % colours.len()]);
                Ok(vec![Command::SetMarker {
                    sequence: sequence.id(),
                    marker: marker.clone().with_colour(next),
                }])
            }

            Self::CloseGap => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                let (track, _) = context.track_of(clip.id())?;
                let start = clip.timeline_start();
                let before = track
                    .clips()
                    .iter()
                    .filter(|other| other.id() != clip.id() && other.timeline_start() < start)
                    .map(|other| {
                        other
                            .timeline_start()
                            .saturating_add(other.timeline_length())
                    })
                    .max();
                let Some(end) = before else {
                    return Err(UiError::NotApplicable(
                        "nothing is before the clip, so there is no gap to close",
                    ));
                };
                if end >= start {
                    return Err(UiError::NotApplicable(
                        "the clip already touches what is before it",
                    ));
                }
                // Everything from the clip on moves left by the gap that was in front of it, in
                // order from the head of the track, and in that order because the log is a
                // document: the same layout has to produce the same commands in the same order.
                let shift = start - end;
                let mut moving: Vec<&Clip> = track
                    .clips()
                    .iter()
                    .filter(|other| other.timeline_start() >= start)
                    .collect();
                moving.sort_by_key(|other| other.timeline_start());
                Ok(moving
                    .into_iter()
                    .map(|other| Command::MoveClip {
                        sequence,
                        clip: other.id(),
                        timeline_start: other.timeline_start().saturating_sub(shift),
                    })
                    .collect())
            }

            Self::RippleDelete => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                let (track, _) = context.track_of(clip.id())?;
                let start = clip.timeline_start();
                let length = clip.timeline_length();
                let mut commands = vec![Command::RemoveClip {
                    sequence,
                    clip: clip.id(),
                }];
                // Only what begins after the clip ended moves: a clip that merely overlaps the
                // tail of this one is left where it is, because sliding it would silently change
                // an edit nobody asked about.
                let after = start.saturating_add(length);
                let mut pulling: Vec<&Clip> = track
                    .clips()
                    .iter()
                    .filter(|other| other.timeline_start() >= after)
                    .collect();
                pulling.sort_by_key(|other| other.timeline_start());
                commands.extend(pulling.into_iter().map(|other| Command::MoveClip {
                    sequence,
                    clip: other.id(),
                    timeline_start: other.timeline_start().saturating_sub(length),
                }));
                Ok(commands)
            }

            Self::InsertBlank => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                let (track, _) = context.track_of(clip.id())?;
                let rate = i64::from(context.sequence()?.timebase().nominal_fps());
                let at = context.playhead;
                let edge = track
                    .clips()
                    .iter()
                    .filter(|other| other.covers(at))
                    .map(|other| other.timeline_start())
                    .min()
                    .unwrap_or(at);
                // From the far end backwards, so no intermediate state has two clips in the same
                // frames as the ones still to move.
                let mut moving: Vec<&Clip> = track
                    .clips()
                    .iter()
                    .filter(|other| other.timeline_start() >= edge)
                    .collect();
                moving.sort_by_key(|other| std::cmp::Reverse(other.timeline_start()));
                let commands: Vec<Command> = moving
                    .into_iter()
                    .map(|other| Command::MoveClip {
                        sequence,
                        clip: other.id(),
                        timeline_start: other.timeline_start().saturating_add(rate),
                    })
                    .collect();
                if commands.is_empty() {
                    return Err(UiError::NotApplicable(
                        "there is nothing after the playhead to move",
                    ));
                }
                Ok(commands)
            }

            Self::DuplicateSelected => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                let (track, index) = context.track_of(clip.id())?;
                let start = clip.timeline_start().saturating_add(clip.timeline_length());
                let mut allocator = Allocator::new(project);
                let copy = Clip::new(
                    allocator.next_clip(),
                    clip.asset_id(),
                    clip.name(),
                    clip.source_range(),
                    start,
                    clip.speed(),
                )
                .map_err(refused)?;
                let copy_id = copy.id();
                let mut commands = vec![Command::InsertClip {
                    sequence,
                    track: track.id(),
                    index: index + 1,
                    clip: copy,
                }];
                commands.extend(restore_of(clip, sequence, copy_id, &mut allocator)?);
                Ok(commands)
            }

            Self::MoveSelected(frames) => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                Ok(vec![Command::MoveClip {
                    sequence,
                    clip: clip.id(),
                    timeline_start: clip.timeline_start().saturating_add(*frames),
                }])
            }

            Self::MoveSelectedToTrack(delta) => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                let (track, _) = context.track_of(clip.id())?;
                let sequence_entity = context
                    .project
                    .sequence(sequence)
                    .ok_or(UiError::no_sequence())?;
                let position = sequence_entity
                    .track_position(track.id())
                    .ok_or(UiError::no_selection())?;
                let target = position as i64 + delta;
                if target < 0 || target as usize >= sequence_entity.tracks().len() {
                    return Err(UiError::NotApplicable("there is no track that way"));
                }
                let target = sequence_entity
                    .track_at(target as usize)
                    .ok_or(UiError::no_selection())?;
                if target.kind() != track.kind() {
                    return Err(UiError::NotApplicable(
                        "a clip cannot move between a video track and an audio track",
                    ));
                }
                Ok(vec![Command::ReparentClip {
                    sequence,
                    clip: clip.id(),
                    track: target.id(),
                    index: insertion_index(target, clip.timeline_start()),
                }])
            }

            Self::TrimSelectedStart(to) => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                if *to >= clip.timeline_start() + clip.timeline_length() {
                    return Err(UiError::NotApplicable("a trim cannot leave the clip empty"));
                }
                // The unbounded retime map, not `source_frame_at`: the interface's job is
                // to say which source frame the new first timeline frame shows, and the
                // engine's job to refuse when the media does not hold it. Using the bounded
                // accessor here would refuse every trim *away* from the current range, and
                // would hide the media-bound check behind an interface message.
                let source = clip.retime().source_at(*to);
                if source >= clip.source_range().end() {
                    return Err(UiError::NotApplicable("a trim cannot leave the clip empty"));
                }
                Ok(vec![Command::TrimClip {
                    sequence,
                    clip: clip.id(),
                    source_range: range_of(source, clip.source_range().end())?,
                    timeline_start: *to,
                }])
            }

            Self::TrimSelectedEnd(to) => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                if *to <= clip.timeline_start() {
                    return Err(UiError::NotApplicable("a trim cannot leave the clip empty"));
                }
                // Unbounded for the same reason as the head trim: the engine owns the
                // judgement about what the media holds.
                let source = clip.retime().source_at(*to);
                if source <= clip.source_range().start() {
                    return Err(UiError::NotApplicable("a trim cannot leave the clip empty"));
                }
                Ok(vec![Command::TrimClip {
                    sequence,
                    clip: clip.id(),
                    source_range: range_of(clip.source_range().start(), source)?,
                    timeline_start: clip.timeline_start(),
                }])
            }

            Self::ToggleSelectedEnabled => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                Ok(vec![Command::SetClipEnabled {
                    sequence,
                    clip: clip.id(),
                    enabled: !clip.is_enabled(),
                }])
            }

            // A channel that is animated is written through its curve rather than through the
            // clip's own field, because that is what the compositor reads back: an animation
            // always wins over the static value, so writing the field while a curve existed
            // would be a control that visibly did nothing. This is the rule that makes a value
            // change at a later playhead position a moving picture rather than a still one.
            Self::SetSelectedOpacity(value) => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                match curve_of(clip, AnimChannel::Opacity) {
                    Some(keys) => Ok(vec![Command::SetChannelKeys {
                        sequence,
                        clip: clip.id(),
                        channel: AnimChannel::Opacity,
                        keys: keyed(&keys, keyframe_offset(clip, context.playhead)?, *value),
                    }]),
                    None => Ok(vec![Command::SetClipOpacity {
                        sequence,
                        clip: clip.id(),
                        opacity: *value,
                    }]),
                }
            }

            Self::ToggleKeyframe(channel) => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                let offset = keyframe_offset(clip, context.playhead)?;
                let mut keys = curve_of(clip, *channel).unwrap_or_default();
                match keys.iter().position(|key| key.offset() == offset) {
                    Some(existing) => {
                        keys.remove(existing);
                    }
                    None => keys.push(Keyframe::new(
                        offset,
                        channel_value(clip, *channel, offset)?,
                        Interpolation::Linear,
                    )),
                }
                // An empty list is the model's own way of saying "this channel has no curve any
                // more", so removing the last key is not a special case here.
                Ok(vec![Command::SetChannelKeys {
                    sequence,
                    clip: clip.id(),
                    channel: *channel,
                    keys,
                }])
            }

            Self::ClearChannel(channel) => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                if curve_of(clip, *channel).is_none() {
                    return Err(UiError::NotApplicable(
                        "that channel has no keyframes to clear",
                    ));
                }
                Ok(vec![Command::SetChannelKeys {
                    sequence,
                    clip: clip.id(),
                    channel: *channel,
                    keys: Vec::new(),
                }])
            }

            Self::SetSelectedBlend(mode) => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                Ok(vec![Command::SetClipBlend {
                    sequence,
                    clip: clip.id(),
                    blend: *mode,
                }])
            }

            Self::SetSelectedTransform(field, value) => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                let channel = field.channel();
                if let Some(keys) = curve_of(clip, channel) {
                    return Ok(vec![Command::SetChannelKeys {
                        sequence,
                        clip: clip.id(),
                        channel,
                        keys: keyed(&keys, keyframe_offset(clip, context.playhead)?, *value),
                    }]);
                }
                let current = clip.transform();
                let mut next = [
                    current.x(),
                    current.y(),
                    current.scale_x(),
                    current.scale_y(),
                    current.rotation(),
                ];
                next[match field {
                    TransformField::X => 0,
                    TransformField::Y => 1,
                    TransformField::ScaleX => 2,
                    TransformField::ScaleY => 3,
                    TransformField::Rotation => 4,
                }] = *value;
                let transform = Transform::new(
                    next[0],
                    next[1],
                    next[2],
                    next[3],
                    next[4],
                    current.anchor_x(),
                    current.anchor_y(),
                )
                .map_err(refused)?;
                Ok(vec![Command::SetClipTransform {
                    sequence,
                    clip: clip.id(),
                    transform,
                }])
            }

            Self::SetSelectedSpeed(value) => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                Ok(vec![Command::SetClipSpeed {
                    sequence,
                    clip: clip.id(),
                    speed: *value,
                }])
            }

            Self::AddEffect(kind) => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                let mut allocator = Allocator::new(project);
                Ok(vec![Command::AddEffect {
                    sequence,
                    clip: clip.id(),
                    index: clip.effects().len(),
                    effect: EffectInstance::new(allocator.next_effect(), *kind),
                }])
            }

            Self::RemoveEffect(effect) => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                Ok(vec![Command::RemoveEffect {
                    sequence,
                    clip: clip.id(),
                    effect: *effect,
                }])
            }

            Self::ToggleEffect(effect) => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                let instance = clip
                    .effect(*effect)
                    .ok_or(UiError::NotApplicable("that effect is not on the clip"))?;
                Ok(vec![Command::SetEffectEnabled {
                    sequence,
                    clip: clip.id(),
                    effect: *effect,
                    enabled: !instance.is_enabled(),
                }])
            }

            Self::SetEffectParam { effect, key, value } => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                Ok(vec![Command::SetEffectParam {
                    sequence,
                    clip: clip.id(),
                    effect: *effect,
                    key: key.clone(),
                    value: value.clone(),
                }])
            }

            Self::AddMask(shape) => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                let mut allocator = Allocator::new(project);
                let mask = Mask::new(allocator.next_mask(), *shape).map_err(refused)?;
                Ok(vec![Command::AddMask {
                    sequence,
                    clip: clip.id(),
                    index: clip.masks().len(),
                    mask,
                }])
            }

            Self::RemoveMask(mask) => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                Ok(vec![Command::RemoveMask {
                    sequence,
                    clip: clip.id(),
                    mask: *mask,
                }])
            }

            Self::InvertMask(mask) => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                let current = clip
                    .mask(*mask)
                    .ok_or(UiError::NotApplicable("that mask is not on the clip"))?;
                let (center_x, center_y) = current.center();
                let (width, height) = current.size();
                let flipped = Mask::with_geometry(
                    current.id(),
                    current.shape(),
                    center_x,
                    center_y,
                    width,
                    height,
                    current.rotation(),
                    current.feather(),
                    current.opacity(),
                    !current.is_inverted(),
                )
                .map_err(refused)?;
                Ok(vec![Command::SetMask {
                    sequence,
                    clip: clip.id(),
                    mask: *mask,
                    value: flipped,
                }])
            }

            Self::AddTransition(kind, duration) => {
                let clip = context.selected()?;
                let sequence = context.owning_sequence(clip.id())?;
                let (track, _) = context.track_of(clip.id())?;
                let at = clip.timeline_start();
                let from = track
                    .clips()
                    .iter()
                    .find(|other| {
                        other.id() != clip.id()
                            && other
                                .timeline_range()
                                .is_some_and(|range| range.end() == at)
                    })
                    .map(Clip::id);
                let mut allocator = Allocator::new(project);
                let transition = Transition::new(
                    allocator.next_transition(),
                    *kind,
                    at,
                    *duration,
                    from,
                    Some(clip.id()),
                )
                .map_err(refused)?;
                Ok(vec![Command::AddTransition {
                    sequence,
                    track: track.id(),
                    index: track.transitions().len(),
                    transition,
                }])
            }

            Self::RemoveTransition(transition) => {
                let sequence = context.sequence()?;
                let track = sequence
                    .tracks()
                    .iter()
                    .find(|track| track.transition(*transition).is_some())
                    .ok_or(UiError::NotApplicable("that transition is not on a track"))?;
                Ok(vec![Command::RemoveTransition {
                    sequence: sequence.id(),
                    track: track.id(),
                    transition: *transition,
                }])
            }

            Self::AddTrack(kind) => {
                let sequence = context.sequence()?;
                let mut allocator = Allocator::new(project);
                let ordinal = sequence
                    .tracks()
                    .iter()
                    .filter(|track| track.kind() == *kind)
                    .count()
                    + 1;
                let label = match kind {
                    TrackKind::Video => format!("V{ordinal}"),
                    TrackKind::Audio => format!("A{ordinal}"),
                };
                Ok(vec![Command::CreateTrack {
                    sequence: sequence.id(),
                    index: sequence.tracks().len(),
                    track: Track::new(allocator.next_track(), *kind, label),
                }])
            }

            Self::RemoveTrack(track) => {
                let sequence = context.sequence()?;
                Ok(vec![Command::RemoveTrack {
                    sequence: sequence.id(),
                    track: *track,
                }])
            }

            Self::CreateMediaAsset {
                name,
                uri,
                timebase,
                duration_frames,
                has_video,
                has_audio,
            } => {
                let mut allocator = Allocator::new(project);
                let asset = MediaAsset::new(
                    allocator.next_asset(),
                    name.clone(),
                    uri.clone(),
                    *timebase,
                    *duration_frames,
                )
                .map_err(refused)?
                .with_streams(*has_video, *has_audio);
                Ok(vec![Command::CreateMediaAsset {
                    index: project.assets().len(),
                    asset,
                }])
            }

            Self::RemoveMediaAsset(asset) => Ok(vec![Command::RemoveMediaAsset { id: *asset }]),

            Self::InsertMediaAtPlayhead(asset) => {
                let asset = project
                    .asset(*asset)
                    .ok_or(UiError::NotApplicable("that media is not in the pool"))?;
                let sequence = context.sequence()?;
                let track = context.first_video_track()?;
                let range = asset.full_range();
                if range.is_empty() {
                    return Err(UiError::NotApplicable("that media has no frames to place"));
                }
                let mut allocator = Allocator::new(project);
                let clip = Clip::new(
                    allocator.next_clip(),
                    asset.id(),
                    asset.name(),
                    range,
                    context.playhead.max(0),
                    Rational::from_integer(1),
                )
                .map_err(refused)?;
                Ok(vec![Command::InsertClip {
                    sequence: sequence.id(),
                    track: track.id(),
                    index: insertion_index(track, clip.timeline_start()),
                    clip,
                }])
            }

            Self::DropMedia {
                asset,
                track,
                frame,
            } => {
                let asset = project
                    .asset(*asset)
                    .ok_or(UiError::NotApplicable("that media is not in the pool"))?;
                let sequence = context.sequence()?;
                let track = sequence.track(*track).ok_or(UiError::NotApplicable(
                    "that track is not on the active sequence",
                ))?;
                if track.kind() != TrackKind::Video {
                    return Err(UiError::NotApplicable(
                        "media is dropped on a picture track",
                    ));
                }
                let range = asset.full_range();
                if range.is_empty() {
                    return Err(UiError::NotApplicable("that media has no frames to place"));
                }
                let at = (*frame).max(0);
                let mut allocator = Allocator::new(project);
                let clip = Clip::new(
                    allocator.next_clip(),
                    asset.id(),
                    asset.name(),
                    range,
                    at,
                    Rational::from_integer(1),
                )
                .map_err(refused)?;
                Ok(vec![Command::InsertClip {
                    sequence: sequence.id(),
                    track: track.id(),
                    index: insertion_index(track, at),
                    clip,
                }])
            }

            Self::AppendMedia(asset) => {
                let asset = project
                    .asset(*asset)
                    .ok_or(UiError::NotApplicable("that media is not in the pool"))?;
                let sequence = context.sequence()?;
                let track = context.first_video_track()?;
                let range = asset.full_range();
                if range.is_empty() {
                    return Err(UiError::NotApplicable("that media has no frames to place"));
                }
                let start = track.end_frame().unwrap_or(0);
                let mut allocator = Allocator::new(project);
                let clip = Clip::new(
                    allocator.next_clip(),
                    asset.id(),
                    asset.name(),
                    range,
                    start,
                    Rational::from_integer(1),
                )
                .map_err(refused)?;
                Ok(vec![Command::AppendClip {
                    sequence: sequence.id(),
                    track: track.id(),
                    clip,
                }])
            }

            Self::ImportMedia { path } => {
                let timebase = context
                    .sequence()
                    .map_or(Timebase::FPS_24, |sequence| sequence.timebase());
                let name = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                );
                let mut allocator = Allocator::new(project);
                let asset = MediaAsset::new(
                    allocator.next_asset(),
                    name,
                    path.display().to_string(),
                    timebase,
                    still_frames(timebase),
                )
                .map_err(refused)?
                .with_streams(true, false);
                Ok(vec![Command::CreateMediaAsset {
                    index: project.assets().len(),
                    asset,
                }])
            }

            Self::SetTitleText(_)
            | Self::SetTitleFace(_)
            | Self::SetTitleSize(_)
            | Self::SetTitleAlign(_) => Ok(Vec::new()),

            Self::AddTitle {
                text,
                face,
                size,
                align,
            } => {
                let sequence = context.sequence()?;
                let track = context.first_video_track()?;
                let words = clean_title(text);
                if words.is_empty() {
                    return Err(UiError::NotApplicable("a title needs words to set"));
                }
                let cap = if size.is_finite() && *size > 0.0 {
                    *size
                } else {
                    f64::from(sequence.height()) as f32 / TITLE_DEFAULT_DIVISOR
                };
                let timebase = sequence.timebase();
                let mut allocator = Allocator::new(project);
                let asset = MediaAsset::new(
                    allocator.next_asset(),
                    title_name(&words),
                    title_uri(&words, *face, cap, *align),
                    timebase,
                    still_frames(timebase),
                )
                .map_err(refused)?
                .with_streams(true, false);
                let clip = Clip::new(
                    allocator.next_clip(),
                    asset.id(),
                    asset.name(),
                    asset.full_range(),
                    context.playhead.max(0),
                    Rational::from_integer(1),
                )
                .map_err(refused)?;
                Ok(vec![
                    Command::CreateMediaAsset {
                        index: project.assets().len(),
                        asset,
                    },
                    Command::InsertClip {
                        sequence: sequence.id(),
                        track: track.id(),
                        index: insertion_index(track, clip.timeline_start()),
                        clip,
                    },
                ])
            }

            Self::CreateSequence {
                name,
                timebase,
                width,
                height,
                sample_rate,
            } => {
                let mut allocator = Allocator::new(project);
                let sequence = Sequence::new(
                    allocator.next_sequence(),
                    name.clone(),
                    *timebase,
                    *width,
                    *height,
                    *sample_rate,
                )
                .map_err(refused)?;
                Ok(vec![Command::CreateSequence {
                    index: project.sequences().len(),
                    sequence,
                }])
            }

            Self::RemoveSequence(sequence) => Ok(vec![Command::RemoveSequence { id: *sequence }]),
        }
    }
}

/// Hands out identifiers the way the engine does: from the model's own counter.
///
/// A command that creates an entity carries the entity, so the identifier has to be chosen
/// before the command exists. Taking the first value from
/// [`Project::next_unused_id`](maj_core::Project::next_unused_id) and then counting upward
/// keeps that choice in the model's hands rather than the interface's, and keeps a session
/// that makes the same moves producing the same identifiers — which is what lets two
/// interfaces be compared by hash at all.
/// How long a still is held on a timeline when it is registered.
///
/// Five seconds is the duration an editor reaches for when dropping a picture or a title onto
/// a sequence. It is a default rather than a measurement: the clip is an ordinary clip and is
/// trimmed to what the shot wants.
pub const STILL_SECONDS: i64 = 5;

/// What a title's cap height is when the studio has not been given one: a tenth of the frame,
/// which is a title card's proportion rather than a caption's.
pub const TITLE_DEFAULT_DIVISOR: f32 = 10.0;

/// The white a title is set in until the studio can be told otherwise.
const TITLE_WHITE: u32 = 0x00ff_ffff;

/// How many frames a still lasts at a rate, never fewer than one.
#[must_use]
pub fn still_frames(timebase: Timebase) -> i64 {
    let numerator = timebase.numerator().saturating_mul(STILL_SECONDS);
    (numerator / timebase.denominator().max(1)).max(1)
}

/// The URI that names a title to the renderer.
///
/// This is the whole of the seam between the interface and the alphabet: a title is a media
/// asset whose URI says which face, cap height, colour, alignment and words, and the renderer
/// sets them. Nothing about a title is stored anywhere else, so a project is still a function
/// of its command log.
#[must_use]
pub fn title_uri(text: &str, face: TitleFace, size: f32, align: TitleAlign) -> String {
    format!(
        "title:{}:{}:{:06x}:{}:{}",
        face.key(),
        size.round().max(1.0) as i64,
        TITLE_WHITE,
        align.key(),
        text
    )
}

/// The name a title's media asset carries: the words, so the pool reads like a shot list.
fn title_name(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "Title".to_owned();
    }
    let words: String = trimmed.chars().take(28).collect();
    if words.chars().count() < trimmed.chars().count() {
        format!("Title: {words}…")
    } else {
        format!("Title: {words}")
    }
}

/// A title's words, with the characters that would break a URI or a log line folded to spaces.
///
/// A newline in a title is a second line, and a second line is a layout this alphabet does not
/// set; folding it to a space sets one legible line instead of a broken one.
fn clean_title(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .trim()
        .to_owned()
}

/// The identifiers the interface hands out, taken from the same counter the model uses so a
/// command the interface builds carries the identifier the model would have chosen.
struct Allocator {
    next: u64,
}

impl Allocator {
    /// Starts from the identifier the model would hand out next.
    fn new(project: &Project) -> Self {
        Self {
            next: project.next_unused_id(),
        }
    }

    /// Takes the next identifier.
    fn take(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        id
    }

    /// An identifier for an asset.
    fn next_asset(&mut self) -> MediaAssetId {
        MediaAssetId::new(self.take())
    }

    /// An identifier for a sequence.
    fn next_sequence(&mut self) -> SequenceId {
        SequenceId::new(self.take())
    }

    /// An identifier for a track.
    fn next_track(&mut self) -> TrackId {
        TrackId::new(self.take())
    }

    /// An identifier for a clip.
    fn next_clip(&mut self) -> ClipId {
        ClipId::new(self.take())
    }

    /// An identifier for a marker.
    fn next_marker(&mut self) -> MarkerId {
        MarkerId::new(self.take())
    }

    /// An identifier for an effect.
    fn next_effect(&mut self) -> EffectId {
        EffectId::new(self.take())
    }

    /// An identifier for a mask.
    fn next_mask(&mut self) -> MaskId {
        MaskId::new(self.take())
    }

    /// An identifier for a transition.
    fn next_transition(&mut self) -> TransitionId {
        TransitionId::new(self.take())
    }
}

/// A half-open range, with the engine's refusal carried through.
/// The channel a placement field is animated through.
impl TransformField {
    /// The channel this field writes to.
    #[must_use]
    pub const fn channel(self) -> AnimChannel {
        match self {
            Self::X => AnimChannel::X,
            Self::Y => AnimChannel::Y,
            Self::ScaleX => AnimChannel::ScaleX,
            Self::ScaleY => AnimChannel::ScaleY,
            Self::Rotation => AnimChannel::Rotation,
        }
    }
}

/// The keys a channel's curve holds, when it has one.
fn curve_of(clip: &Clip, channel: AnimChannel) -> Option<Vec<Keyframe>> {
    clip.animation()
        .iter()
        .find(|animation| animation.channel() == channel)
        .map(|animation| animation.keys().to_vec())
}

/// The same keys, with a value written at an offset: the key there is replaced, and a new one is
/// added otherwise. Both cases keep the curve sorted, because the model sorts it on the way in.
fn keyed(keys: &[Keyframe], offset: i64, value: Rational) -> Vec<Keyframe> {
    let mut out = keys.to_vec();
    match out.iter().position(|key| key.offset() == offset) {
        Some(existing) => {
            let interpolation = out[existing].interpolation();
            out[existing] = Keyframe::new(offset, ParamValue::Rational(value), interpolation);
        }
        None => out.push(Keyframe::new(
            offset,
            ParamValue::Rational(value),
            Interpolation::Linear,
        )),
    }
    out
}

/// The value a channel has at an offset: its curve's sample, or the clip's own field.
///
/// The order is the compositor's order, and it has to be: a keyframe may only be recorded at the
/// value the picture is actually showing, or pressing a diamond would move the picture.
fn channel_value(clip: &Clip, channel: AnimChannel, offset: i64) -> Result<ParamValue, UiError> {
    if let Some(animation) = clip
        .animation()
        .iter()
        .find(|animation| animation.channel() == channel)
    {
        return animation.sample(offset).map_err(refused);
    }
    let transform = clip.transform();
    Ok(ParamValue::Rational(match channel {
        AnimChannel::Opacity => clip.opacity(),
        AnimChannel::X => transform.x(),
        AnimChannel::Y => transform.y(),
        AnimChannel::ScaleX => transform.scale_x(),
        AnimChannel::ScaleY => transform.scale_y(),
        AnimChannel::Rotation => transform.rotation(),
    }))
}

/// Where a keyframe would sit, as the clip measures its own time.
fn keyframe_offset(clip: &Clip, playhead: i64) -> Result<i64, UiError> {
    let offset = playhead.saturating_sub(clip.timeline_start());
    if offset < 0 || offset >= clip.timeline_length() {
        return Err(UiError::NotApplicable(
            "the playhead is not inside the clip, so there is no frame to key",
        ));
    }
    Ok(offset)
}

/// The frames a range covers, or a refusal when it covers none.
/// The marker covering a frame, or a refusal that says there is none.
///
/// A marker of zero length covers exactly the frame it sits on, so this is also how the
/// interface finds "the marker under the playhead" for a point.
fn marker_at(sequence: &Sequence, frame: i64) -> Result<&Marker, UiError> {
    sequence
        .markers()
        .iter()
        .find(|marker| marker.covers(frame))
        .ok_or(UiError::NotApplicable(
            "no marker is under the playhead: put one there first",
        ))
}

fn range_of(start: i64, end: i64) -> Result<FrameRange, UiError> {
    FrameRange::new(start, end).map_err(refused)
}

/// Turns any error that can explain itself into a refusal with its own words.
fn refused(error: impl std::fmt::Display) -> UiError {
    UiError::Refused(error.to_string())
}

/// How many clips in a track begin at or before a frame.
///
/// This is the insertion position that keeps a track in timeline order, which is the order
/// the renderer composes within a track.
fn insertion_index(track: &Track, at: i64) -> usize {
    track
        .clips()
        .iter()
        .filter(|clip| clip.timeline_start() <= at)
        .count()
}

/// The commands that give a newly inserted copy the look, the effects and the masks of the
/// clip it was copied from.
///
/// Assigning a clip's appearance through `Command`s rather than by constructing a clip that
/// already has it is deliberate: the engine's public surface cannot place a transform on a
/// clip before that clip exists, and going around that would mean the interface writing into
/// the model directly, which is the one thing this architecture forbids. The log runs a few
/// lines longer, and every line is an ordinary invertible edit that a reviewer can read.
///
/// # Errors
///
/// Returns a refusal when an effect parameter the original holds is not one its own kind
/// declares, which would mean the document and the effect catalogue already disagreed.
fn restore_of(
    original: &Clip,
    sequence: SequenceId,
    target: ClipId,
    allocator: &mut Allocator,
) -> Result<Vec<Command>, UiError> {
    let mut commands = Vec::new();
    if !original.transform().is_identity() {
        commands.push(Command::SetClipTransform {
            sequence,
            clip: target,
            transform: original.transform(),
        });
    }
    if original.opacity() != Rational::from_integer(1) {
        commands.push(Command::SetClipOpacity {
            sequence,
            clip: target,
            opacity: original.opacity(),
        });
    }
    if original.blend() != BlendMode::Normal {
        commands.push(Command::SetClipBlend {
            sequence,
            clip: target,
            blend: original.blend(),
        });
    }
    for (index, effect) in original.effects().iter().enumerate() {
        let id = allocator.next_effect();
        let mut copy = EffectInstance::new(id, effect.kind());
        for param in effect.params() {
            copy.set_param(param.key(), param.value().clone())
                .map_err(refused)?;
        }
        commands.push(Command::AddEffect {
            sequence,
            clip: target,
            index,
            effect: copy,
        });
        if !effect.is_enabled() {
            commands.push(Command::SetEffectEnabled {
                sequence,
                clip: target,
                effect: id,
                enabled: false,
            });
        }
    }
    for (index, mask) in original.masks().iter().enumerate() {
        let (center_x, center_y) = mask.center();
        let (width, height) = mask.size();
        let copy = Mask::with_geometry(
            allocator.next_mask(),
            mask.shape(),
            center_x,
            center_y,
            width,
            height,
            mask.rotation(),
            mask.feather(),
            mask.opacity(),
            mask.is_inverted(),
        )
        .map_err(refused)?;
        commands.push(Command::AddMask {
            sequence,
            clip: target,
            index,
            mask: copy,
        });
    }
    if !original.is_enabled() {
        commands.push(Command::SetClipEnabled {
            sequence,
            clip: target,
            enabled: false,
        });
    }
    Ok(commands)
}

/// Every command kind the interface can name, in one place.
///
/// A test constructs one variant of each and compares the two lists, so this cannot quietly
/// fall behind the model; another checks that the agent's catalogue covers every one of them,
/// which is the interface-level statement of "there is one door".
pub const KINDS: [&str; 36] = [
    "set_sequence_start_timecode",
    "relink_asset",
    "set_asset_fingerprint",
    "add_marker",
    "remove_marker",
    "set_marker",
    "set_channel_keys",
    "add_mask",
    "remove_mask",
    "set_mask",
    "add_transition",
    "remove_transition",
    "set_clip_transform",
    "set_clip_opacity",
    "set_clip_blend",
    "add_effect",
    "remove_effect",
    "set_effect_enabled",
    "set_effect_param",
    "create_media_asset",
    "remove_media_asset",
    "insert_media_asset",
    "create_sequence",
    "remove_sequence",
    "insert_sequence",
    "create_track",
    "remove_track",
    "append_clip",
    "insert_clip",
    "remove_clip",
    "trim_clip",
    "move_clip",
    "reparent_clip",
    "set_clip_speed",
    "set_clip_enabled",
    "set_track_enabled",
];

/// The snake_case name of a command, exactly as the log stores it.
///
/// The names are checked against the model's own serialisation by a test, so the interface
/// cannot invent a vocabulary that drifts from the document: if the model renames a variant,
/// this function stops agreeing with it and the suite fails.
#[must_use]
pub fn kind_of(command: &Command) -> &'static str {
    match command {
        Command::SetSequenceStartTimecode { .. } => "set_sequence_start_timecode",
        Command::RelinkAsset { .. } => "relink_asset",
        Command::SetAssetFingerprint { .. } => "set_asset_fingerprint",
        Command::AddMarker { .. } => "add_marker",
        Command::RemoveMarker { .. } => "remove_marker",
        Command::SetMarker { .. } => "set_marker",
        Command::SetChannelKeys { .. } => "set_channel_keys",
        Command::AddMask { .. } => "add_mask",
        Command::RemoveMask { .. } => "remove_mask",
        Command::SetMask { .. } => "set_mask",
        Command::AddTransition { .. } => "add_transition",
        Command::RemoveTransition { .. } => "remove_transition",
        Command::SetClipTransform { .. } => "set_clip_transform",
        Command::SetClipOpacity { .. } => "set_clip_opacity",
        Command::SetClipBlend { .. } => "set_clip_blend",
        Command::AddEffect { .. } => "add_effect",
        Command::RemoveEffect { .. } => "remove_effect",
        Command::SetEffectEnabled { .. } => "set_effect_enabled",
        Command::SetEffectParam { .. } => "set_effect_param",
        Command::CreateMediaAsset { .. } => "create_media_asset",
        Command::RemoveMediaAsset { .. } => "remove_media_asset",
        Command::InsertMediaAsset { .. } => "insert_media_asset",
        Command::CreateSequence { .. } => "create_sequence",
        Command::RemoveSequence { .. } => "remove_sequence",
        Command::InsertSequence { .. } => "insert_sequence",
        Command::CreateTrack { .. } => "create_track",
        Command::RemoveTrack { .. } => "remove_track",
        Command::AppendClip { .. } => "append_clip",
        Command::InsertClip { .. } => "insert_clip",
        Command::RemoveClip { .. } => "remove_clip",
        Command::TrimClip { .. } => "trim_clip",
        Command::MoveClip { .. } => "move_clip",
        Command::ReparentClip { .. } => "reparent_clip",
        Command::SetClipSpeed { .. } => "set_clip_speed",
        Command::SetClipEnabled { .. } => "set_clip_enabled",
        Command::SetTrackEnabled { .. } => "set_track_enabled",
    }
}
