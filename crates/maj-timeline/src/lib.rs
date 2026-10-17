//! `maj-timeline` — the editorial operations, as compositions of commands.
//!
//! **Layer 5.** See `docs/ARCHITECTURE.md` for the layering rule.
//!
//! # The one decision this crate rests on
//!
//! **It adds no commands.** Ripple, roll, slip, slide, closing a gap and inserting one are
//! each a handful of [`TrimClip`](maj_core::Command::TrimClip) and
//! [`MoveClip`](maj_core::Command::MoveClip) applied together, and
//! [`History::apply_group`](maj_core::History::apply_group) already makes a handful of
//! commands one undo step that rolls back entire if any of them is refused.
//!
//! A `CloseGap` command would have been easy and wrong. The log would gain a second way to
//! say something it could already say, so two projects that did the same edit could hold
//! different commands; a diff would show a shape rather than a change; and replay would
//! have to know both. The vocabulary of the document is worth more than the convenience of
//! a verb, so every function here is a **pure function from a project to a list of commands
//! it has not applied**. Nothing in this crate touches a `Project`.
//!
//! That also makes an operation inspectable before it happens, which is what the agent
//! surface needs: a model can be shown the four commands a ripple would append and asked to
//! approve them.
//!
//! # What the operations mean
//!
//! All four move an edge or a clip by a number of **timeline** frames. Positive is later.
//!
//! | Operation | What moves | What does not |
//! |---|---|---|
//! | [`ripple`] | One edge, and every clip after it on that track | The rest of the clip's source |
//! | [`roll`] | The cut between two clips: one gains what the other loses | The sequence's duration, and every other clip |
//! | [`slip`] | Which part of the source a clip shows | Where the clip sits, and how long it is |
//! | [`slide`] | Where a clip sits; its neighbours' edges absorb the move | The sequence's duration, and the clip's own source |
//!
//! # Speed, and what is refused rather than rounded
//!
//! An edge moved by `n` timeline frames moves `n × speed` source frames. When the clip's
//! speed is not one, that product can fall between two source frames — and there is no
//! honest answer to "which one", so [`EditError::NotOnAFrame`] says so instead of rounding.
//! It names the speed, so the refusal is actionable.
//!
//! This is a real limit and it is the project's own discipline applied consistently: a
//! retimed clip can still be rippled by a multiple of its denominator. Rounding silently is
//! how an edit drifts one frame per adjustment, which is the defect this engine exists to
//! make impossible.

#![forbid(unsafe_code)]

use std::fmt;

use maj_core::{Clip, ClipId, Command, FrameRange, Project, SequenceId, Track, TrackId};

/// Human-readable statement of this crate's responsibility.
pub const ROLE: &str = "editorial operations: trim, ripple, roll, slip, slide, gaps";

/// Layer in the Majestat architecture, from `docs/ARCHITECTURE.md`.
pub const LAYER: u8 = 5;

/// Which end of a clip an operation works on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    /// The first frame: the clip's in point.
    Head,
    /// Past the last frame: the clip's out point.
    Tail,
}

impl Edge {
    /// The short stable name, for a tool argument or a status line.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Head => "head",
            Self::Tail => "tail",
        }
    }

    /// The edge with this name, if there is one.
    #[must_use]
    pub fn from_key(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "head" | "in" | "start" => Some(Self::Head),
            "tail" | "out" | "end" => Some(Self::Tail),
            _ => None,
        }
    }
}

/// Why an editorial operation could not be composed.
///
/// Every variant is a refusal *before* anything is applied: these functions return commands
/// or an error, and never half a list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// The sequence is not in the project.
    NoSequence(SequenceId),
    /// The clip is not on any track of that sequence.
    NoClip(ClipId),
    /// The operation needs a clip on the other side of the cut, and there is none.
    NoNeighbour {
        /// Which side was missing.
        edge: Edge,
    },
    /// The move would run past the end of the clip's source media.
    PastSource {
        /// The edge that ran out.
        edge: Edge,
        /// How many source frames are actually available in that direction.
        available: i64,
    },
    /// The move would leave the clip with no frames at all.
    ///
    /// A clip of zero length is not an edit; it is a deletion written as one, and the two
    /// should not be spelled the same way.
    WouldEmpty,
    /// The amount is not a whole number of source frames at this clip's speed.
    NotOnAFrame {
        /// The clip's speed, as it is stored.
        speed: maj_core::Rational,
        /// The timeline amount that does not divide.
        by: i64,
    },
    /// There is no gap at that point to close.
    NoGap {
        /// Where the caller said the gap was.
        at: i64,
    },
    /// The arithmetic left the addressable range.
    OutOfRange,
}

impl fmt::Display for EditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSequence(id) => write!(formatter, "there is no sequence#{}", id.raw()),
            Self::NoClip(id) => write!(formatter, "there is no clip#{} on that sequence", id.raw()),
            Self::NoNeighbour { edge } => write!(
                formatter,
                "there is no clip on the {} side of that cut",
                edge.key()
            ),
            Self::PastSource { edge, available } => write!(
                formatter,
                "the {} cannot move that far: {available} source frame(s) are available",
                edge.key()
            ),
            Self::WouldEmpty => {
                formatter.write_str("that would leave the clip with no frames; remove it instead")
            }
            Self::NotOnAFrame { speed, by } => write!(
                formatter,
                "{by} timeline frame(s) at speed {}/{} is not a whole number of source \
                 frames, and rounding it would drift",
                speed.num(),
                speed.den()
            ),
            Self::NoGap { at } => write!(formatter, "there is no gap at frame {at} to close"),
            Self::OutOfRange => formatter.write_str("the result falls outside addressable time"),
        }
    }
}

impl std::error::Error for EditError {}

/// Moves one edge of a clip, and everything after it on the same track, by `by` frames.
///
/// This is the trim an editor means by "ripple": the sequence gets longer or shorter and no
/// gap appears. A head ripple moves the clip's start *and* its in point, so the frames at
/// the front are added or taken away and the clip does not slide over its source.
///
/// # Errors
///
/// [`EditError`] when the clip or its sequence is missing, when the move runs past the
/// source, when it would empty the clip, or when the amount is not a whole number of source
/// frames at this clip's speed.
pub fn ripple(
    project: &Project,
    sequence: SequenceId,
    clip: ClipId,
    edge: Edge,
    by: i64,
) -> Result<Vec<Command>, EditError> {
    let (track, target) = locate(project, sequence, clip)?;
    if by == 0 {
        return Ok(Vec::new());
    }
    let source_by = in_source_frames(target, by)?;
    let trimmed = move_edge(target, edge, source_by)?;

    let mut commands = vec![Command::TrimClip {
        sequence,
        clip,
        source_range: trimmed.source_range,
        timeline_start: trimmed.timeline_start,
    }];

    // Everything that began after this clip began moves with it. Measured from the clip's
    // own start rather than from the cut, so a head ripple does not drag the clip it just
    // moved.
    let shift = match edge {
        Edge::Head => -by,
        Edge::Tail => by,
    };
    let after = target.timeline_start();
    for other in track.clips_in_timeline_order() {
        if other.id() == clip || other.timeline_start() <= after {
            continue;
        }
        let moved = other
            .timeline_start()
            .checked_add(shift)
            .ok_or(EditError::OutOfRange)?;
        if moved < 0 {
            return Err(EditError::OutOfRange);
        }
        commands.push(Command::MoveClip {
            sequence,
            clip: other.id(),
            timeline_start: moved,
        });
    }
    Ok(commands)
}

/// Moves the cut between a clip and its neighbour, without moving anything else.
///
/// One clip gains exactly what the other loses, so the sequence's duration does not change
/// and no clip after the pair moves. This is the trim that leaves the rest of the cut alone,
/// and it is two [`TrimClip`](maj_core::Command::TrimClip)s.
///
/// # Errors
///
/// [`EditError`] when either side is missing, when either side would run past its source or
/// be emptied, or when the amount is not a whole number of source frames for both clips.
pub fn roll(
    project: &Project,
    sequence: SequenceId,
    clip: ClipId,
    edge: Edge,
    by: i64,
) -> Result<Vec<Command>, EditError> {
    let (track, target) = locate(project, sequence, clip)?;
    if by == 0 {
        return Ok(Vec::new());
    }
    let partner = neighbour(track, target, edge).ok_or(EditError::NoNeighbour { edge })?;

    // The clip's own edge moves by `by`; the partner's facing edge moves the same way, so
    // the two stay adjacent and the pair's total length is unchanged.
    let target_moved = move_edge(target, edge, in_source_frames(target, by)?)?;
    let partner_edge = match edge {
        Edge::Head => Edge::Tail,
        Edge::Tail => Edge::Head,
    };
    let partner_moved = move_edge(partner, partner_edge, in_source_frames(partner, by)?)?;

    Ok(vec![
        Command::TrimClip {
            sequence,
            clip: target.id(),
            source_range: target_moved.source_range,
            timeline_start: target_moved.timeline_start,
        },
        Command::TrimClip {
            sequence,
            clip: partner.id(),
            source_range: partner_moved.source_range,
            timeline_start: partner_moved.timeline_start,
        },
    ])
}

/// Changes which part of the source a clip shows, without moving it or changing its length.
///
/// Both edges move together through the media: the window stays the same size and slides.
/// Nothing else on the timeline is touched, which is why slipping is the safe way to fix a
/// performance inside a cut that is already timed.
///
/// # Errors
///
/// [`EditError`] when the clip is missing, when the window would leave the media, or when
/// the amount is not a whole number of source frames at this clip's speed.
pub fn slip(
    project: &Project,
    sequence: SequenceId,
    clip: ClipId,
    by: i64,
) -> Result<Vec<Command>, EditError> {
    let (_, target) = locate(project, sequence, clip)?;
    if by == 0 {
        return Ok(Vec::new());
    }
    let source_by = in_source_frames(target, by)?;
    let range = target.source_range();
    let start = range
        .start()
        .checked_add(source_by)
        .ok_or(EditError::OutOfRange)?;
    if start < 0 {
        return Err(EditError::PastSource {
            edge: Edge::Head,
            available: range.start(),
        });
    }
    let available = source_available(project, target).ok_or(EditError::OutOfRange)?;
    let end = start
        .checked_add(range.len())
        .ok_or(EditError::OutOfRange)?;
    if end > available {
        return Err(EditError::PastSource {
            edge: Edge::Tail,
            available: available - range.end(),
        });
    }
    let slipped = FrameRange::new(start, end).map_err(|_| EditError::OutOfRange)?;
    Ok(vec![Command::TrimClip {
        sequence,
        clip,
        source_range: slipped,
        timeline_start: target.timeline_start(),
    }])
}

/// Moves a clip along the timeline, with its neighbours absorbing the move.
///
/// The clip keeps its source and its length; the clip before it gives up or gains frames at
/// its tail, and the clip after it at its head. The sequence's duration is unchanged, which
/// is what separates a slide from a plain move.
///
/// # Errors
///
/// [`EditError`] when a neighbour is missing, when a neighbour would run past its source or
/// be emptied, or when the amount is not a whole number of source frames for them.
pub fn slide(
    project: &Project,
    sequence: SequenceId,
    clip: ClipId,
    by: i64,
) -> Result<Vec<Command>, EditError> {
    let (track, target) = locate(project, sequence, clip)?;
    if by == 0 {
        return Ok(Vec::new());
    }
    let before =
        neighbour(track, target, Edge::Head).ok_or(EditError::NoNeighbour { edge: Edge::Head })?;
    let after =
        neighbour(track, target, Edge::Tail).ok_or(EditError::NoNeighbour { edge: Edge::Tail })?;

    // The clip before gains at its tail exactly what the clip after loses at its head.
    let before_moved = move_edge(before, Edge::Tail, in_source_frames(before, by)?)?;
    let after_moved = move_edge(after, Edge::Head, in_source_frames(after, by)?)?;
    let moved = target
        .timeline_start()
        .checked_add(by)
        .ok_or(EditError::OutOfRange)?;
    if moved < 0 {
        return Err(EditError::OutOfRange);
    }

    Ok(vec![
        Command::TrimClip {
            sequence,
            clip: before.id(),
            source_range: before_moved.source_range,
            timeline_start: before_moved.timeline_start,
        },
        Command::MoveClip {
            sequence,
            clip,
            timeline_start: moved,
        },
        Command::TrimClip {
            sequence,
            clip: after.id(),
            source_range: after_moved.source_range,
            timeline_start: after_moved.timeline_start,
        },
    ])
}

/// Pulls everything after a gap back to close it.
///
/// `at` is any frame inside the gap. The gap is the empty span between two clips on the
/// track, or between the start of the timeline and the first clip.
///
/// # Errors
///
/// [`EditError::NoGap`] when that frame is covered by a clip, or when there is nothing after
/// it to pull back.
pub fn close_gap(
    project: &Project,
    sequence: SequenceId,
    track: TrackId,
    at: i64,
) -> Result<Vec<Command>, EditError> {
    let lane = lane_of(project, sequence, track)?;
    let ordered = lane.clips_in_timeline_order();
    if ordered.iter().any(|clip| clip.covers(at)) {
        return Err(EditError::NoGap { at });
    }
    // The gap runs from the end of the last clip that finishes at or before `at` — or from
    // zero — to the start of the first clip that begins after it.
    let gap_start = ordered
        .iter()
        .filter_map(|clip| clip.timeline_range())
        .filter(|range| range.end() <= at)
        .map(|range| range.end())
        .max()
        .unwrap_or(0);
    let Some(next) = ordered
        .iter()
        .filter(|clip| clip.timeline_start() > at)
        .min_by_key(|clip| clip.timeline_start())
    else {
        return Err(EditError::NoGap { at });
    };
    let length = next.timeline_start() - gap_start;
    if length <= 0 {
        return Err(EditError::NoGap { at });
    }
    Ok(shift_from(
        sequence,
        &ordered,
        next.timeline_start(),
        -length,
    ))
}

/// Pushes everything from a frame onwards later, leaving empty space behind.
///
/// # Errors
///
/// [`EditError::OutOfRange`] for a negative length or an overflow.
pub fn insert_blank(
    project: &Project,
    sequence: SequenceId,
    track: TrackId,
    at: i64,
    length: i64,
) -> Result<Vec<Command>, EditError> {
    if length < 0 {
        return Err(EditError::OutOfRange);
    }
    if length == 0 {
        return Ok(Vec::new());
    }
    let lane = lane_of(project, sequence, track)?;
    let ordered = lane.clips_in_timeline_order();
    Ok(shift_from(sequence, &ordered, at, length))
}

/// A clip's two mutable positions, after an edge has moved.
struct Moved {
    source_range: FrameRange,
    timeline_start: i64,
}

/// Moves one edge of a clip through its source, keeping the other where it is.
///
/// A head that moves also moves the clip's timeline start by the same amount, because the
/// frames are being added to or taken from the front: the clip does not slide over its media.
fn move_edge(clip: &Clip, edge: Edge, source_by: i64) -> Result<Moved, EditError> {
    let range = clip.source_range();
    match edge {
        Edge::Head => {
            let start = range
                .start()
                .checked_add(source_by)
                .ok_or(EditError::OutOfRange)?;
            if start < 0 {
                return Err(EditError::PastSource {
                    edge: Edge::Head,
                    available: range.start(),
                });
            }
            if start >= range.end() {
                return Err(EditError::WouldEmpty);
            }
            let timeline_start = clip
                .timeline_start()
                .checked_add(timeline_delta(clip, source_by))
                .ok_or(EditError::OutOfRange)?;
            if timeline_start < 0 {
                return Err(EditError::OutOfRange);
            }
            Ok(Moved {
                source_range: FrameRange::new(start, range.end())
                    .map_err(|_| EditError::OutOfRange)?,
                timeline_start,
            })
        }
        Edge::Tail => {
            let end = range
                .end()
                .checked_add(source_by)
                .ok_or(EditError::OutOfRange)?;
            if end <= range.start() {
                return Err(EditError::WouldEmpty);
            }
            Ok(Moved {
                source_range: FrameRange::new(range.start(), end)
                    .map_err(|_| EditError::OutOfRange)?,
                timeline_start: clip.timeline_start(),
            })
        }
    }
}

/// How many timeline frames a number of source frames is worth for this clip.
///
/// The inverse of [`in_source_frames`], and it is only ever called with an amount that
/// function already produced, so it divides exactly.
fn timeline_delta(clip: &Clip, source_by: i64) -> i64 {
    let speed = clip.speed();
    if speed.is_zero() {
        return source_by;
    }
    // source = timeline × speed, so timeline = source × den / num.
    let scaled = i128::from(source_by) * i128::from(speed.den());
    i64::try_from(scaled / i128::from(speed.num())).unwrap_or(i64::MAX)
}

/// How many source frames a number of timeline frames is worth for this clip.
///
/// Refuses rather than rounds when the product is not whole. See the crate documentation.
fn in_source_frames(clip: &Clip, by: i64) -> Result<i64, EditError> {
    let speed = clip.speed();
    let scaled = i128::from(by) * i128::from(speed.num());
    let den = i128::from(speed.den());
    if den == 0 || scaled % den != 0 {
        return Err(EditError::NotOnAFrame { speed, by });
    }
    i64::try_from(scaled / den).map_err(|_| EditError::OutOfRange)
}

/// How many source frames the clip's media offers in total.
fn source_available(project: &Project, clip: &Clip) -> Option<i64> {
    project
        .asset(clip.asset_id())
        .map(maj_core::MediaAsset::duration_frames)
}

/// The clip that touches this one on the given side, by timeline position.
fn neighbour<'a>(track: &'a Track, clip: &Clip, edge: Edge) -> Option<&'a Clip> {
    let ordered = track.clips_in_timeline_order();
    match edge {
        Edge::Head => ordered
            .into_iter()
            .filter(|other| {
                other.id() != clip.id() && other.timeline_start() < clip.timeline_start()
            })
            .max_by_key(|other| other.timeline_start()),
        Edge::Tail => ordered
            .into_iter()
            .filter(|other| {
                other.id() != clip.id() && other.timeline_start() > clip.timeline_start()
            })
            .min_by_key(|other| other.timeline_start()),
    }
}

/// Moves every clip that starts at or after `from` by `delta`.
fn shift_from(sequence: SequenceId, ordered: &[&Clip], from: i64, delta: i64) -> Vec<Command> {
    ordered
        .iter()
        .filter(|clip| clip.timeline_start() >= from)
        .filter_map(|clip| {
            let moved = clip.timeline_start().checked_add(delta)?;
            (moved >= 0).then_some(Command::MoveClip {
                sequence,
                clip: clip.id(),
                timeline_start: moved,
            })
        })
        .collect()
}

/// The track a clip is on, and the clip.
fn locate(
    project: &Project,
    sequence: SequenceId,
    clip: ClipId,
) -> Result<(&Track, &Clip), EditError> {
    let entity = project
        .sequence(sequence)
        .ok_or(EditError::NoSequence(sequence))?;
    entity
        .tracks()
        .iter()
        .find_map(|track| track.clip(clip).map(|found| (track, found)))
        .ok_or(EditError::NoClip(clip))
}

/// One track of one sequence.
fn lane_of(project: &Project, sequence: SequenceId, track: TrackId) -> Result<&Track, EditError> {
    project
        .sequence(sequence)
        .ok_or(EditError::NoSequence(sequence))?
        .tracks()
        .iter()
        .find(|lane| lane.id() == track)
        .ok_or(EditError::NoClip(ClipId::new(track.raw())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_its_role_and_layer() {
        assert!(!ROLE.is_empty(), "every crate must declare its role");
        assert_eq!(LAYER, 5);
    }

    #[test]
    fn an_edge_is_named_the_way_an_editor_names_it() {
        for edge in [Edge::Head, Edge::Tail] {
            assert_eq!(Edge::from_key(edge.key()), Some(edge));
        }
        assert_eq!(Edge::from_key(" IN "), Some(Edge::Head));
        assert_eq!(Edge::from_key("out"), Some(Edge::Tail));
        assert_eq!(Edge::from_key("middle"), None);
    }
}
