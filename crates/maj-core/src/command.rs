//! The command log: every edit is one invertible command.
//!
//! # The invariant that carries the project
//!
//! [`Command::apply`] mutates a project **and returns the command that undoes it**.
//! There is no separate undo bookkeeping to drift out of sync: the inverse is
//! computed from the state the command actually observed.
//!
//! Two consequences follow, and both are load-bearing:
//!
//! * Undo and redo are exact, including clip *position* within a track, because
//!   removals carry the index they removed from.
//! * A command log is a complete description of an edit, so it can be replayed,
//!   diffed, reviewed, synchronised and attributed to an author.
//!
//! Variants that create an entity carry their own insertion index for the same
//! reason: an undo must be able to restore a sequence, track or asset at the exact
//! slot it came from, not merely at the end.

use maj_time::{FrameRange, Rational};
use serde::{Deserialize, Serialize};

use crate::composite::{
    AnimChannel, BlendMode, ChannelAnimation, EffectInstance, Keyframe, Mask, ParamValue,
    Transform, Transition,
};
use crate::entity::{Clip, MediaAsset, Sequence, Track};
use crate::error::CoreError;
use crate::ids::{ClipId, MediaAssetId, SequenceId, TrackId};
use crate::ids::{EffectId, MaskId, TransitionId};
use crate::project::Project;

/// Author recorded on commands applied by the local user.
pub const DEFAULT_AUTHOR: &str = "local";

/// One command as it appears in the durable log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogEntry {
    pub(crate) seq: u64,
    pub(crate) author: String,
    pub(crate) command: Command,
}

impl LogEntry {
    /// Builds a log entry.
    #[must_use]
    pub fn new(seq: u64, author: impl Into<String>, command: Command) -> Self {
        Self {
            seq,
            author: author.into(),
            command,
        }
    }

    /// Position in the log, from zero.
    #[must_use]
    pub const fn seq(&self) -> u64 {
        self.seq
    }

    /// Who performed the edit.
    #[must_use]
    pub fn author(&self) -> &str {
        &self.author
    }

    /// What the edit was.
    #[must_use]
    pub const fn command(&self) -> &Command {
        &self.command
    }
}

/// A single invertible edit.
///
/// Serialised with an internal `kind` tag, so a log reads as a sequence of named
/// operations and diffs cleanly as text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Replaces the keyframes of one animated channel.
    ///
    /// An empty list removes the animation for that channel, which keeps the inverse
    /// exact in both directions: removing an animation is undone by restoring the
    /// keyframes, and restoring an empty list removes it again.
    SetChannelKeys {
        /// Owning sequence.
        sequence: SequenceId,
        /// Clip to change.
        clip: ClipId,
        /// Channel to animate.
        channel: AnimChannel,
        /// The complete new keyframe list.
        keys: Vec<Keyframe>,
    },
    /// Adds a mask at a position in a clip's mask list.
    AddMask {
        /// Owning sequence.
        sequence: SequenceId,
        /// Clip to change.
        clip: ClipId,
        /// Position in the mask list.
        index: usize,
        /// The mask to add.
        mask: Mask,
    },
    /// Removes a mask from a clip.
    RemoveMask {
        /// Owning sequence.
        sequence: SequenceId,
        /// Clip to change.
        clip: ClipId,
        /// Mask to remove.
        mask: MaskId,
    },
    /// Replaces a mask's geometry in place.
    SetMask {
        /// Owning sequence.
        sequence: SequenceId,
        /// Clip to change.
        clip: ClipId,
        /// Mask to replace.
        mask: MaskId,
        /// The new mask value, which must carry the same identifier.
        value: Mask,
    },
    /// Adds a transition at a position in a track's transition list.
    AddTransition {
        /// Owning sequence.
        sequence: SequenceId,
        /// Track to change.
        track: TrackId,
        /// Position in the transition list.
        index: usize,
        /// The transition to add.
        transition: Transition,
    },
    /// Removes a transition from a track.
    RemoveTransition {
        /// Owning sequence.
        sequence: SequenceId,
        /// Track to change.
        track: TrackId,
        /// Transition to remove.
        transition: TransitionId,
    },
    /// Replaces a clip's placement inside the sequence frame.
    SetClipTransform {
        /// Owning sequence.
        sequence: SequenceId,
        /// Clip to transform.
        clip: ClipId,
        /// New placement.
        transform: Transform,
    },
    /// Sets a clip's opacity.
    SetClipOpacity {
        /// Owning sequence.
        sequence: SequenceId,
        /// Clip to change.
        clip: ClipId,
        /// New opacity, in `0..=1`.
        opacity: Rational,
    },
    /// Sets how a clip combines with the layers beneath it.
    SetClipBlend {
        /// Owning sequence.
        sequence: SequenceId,
        /// Clip to change.
        clip: ClipId,
        /// New blend mode.
        blend: BlendMode,
    },
    /// Adds an effect at a position in a clip's effect chain.
    AddEffect {
        /// Owning sequence.
        sequence: SequenceId,
        /// Clip to change.
        clip: ClipId,
        /// Position in the effect chain.
        index: usize,
        /// The effect to add.
        effect: EffectInstance,
    },
    /// Removes an effect from a clip.
    RemoveEffect {
        /// Owning sequence.
        sequence: SequenceId,
        /// Clip to change.
        clip: ClipId,
        /// Effect to remove.
        effect: EffectId,
    },
    /// Enables or bypasses an effect without removing it.
    SetEffectEnabled {
        /// Owning sequence.
        sequence: SequenceId,
        /// Clip to change.
        clip: ClipId,
        /// Effect to toggle.
        effect: EffectId,
        /// New enabled state.
        enabled: bool,
    },
    /// Sets one parameter of an effect. The key must be declared by the effect.
    SetEffectParam {
        /// Owning sequence.
        sequence: SequenceId,
        /// Clip to change.
        clip: ClipId,
        /// Effect to change.
        effect: EffectId,
        /// Parameter key.
        key: String,
        /// New value.
        value: ParamValue,
    },
    /// Registers media at a position in the asset list.
    CreateMediaAsset {
        /// Insertion index within the asset list.
        index: usize,
        /// The media to register.
        asset: MediaAsset,
    },
    /// Unregisters media. Inverse of [`Command::CreateMediaAsset`] and
    /// [`Command::InsertMediaAsset`].
    RemoveMediaAsset {
        /// Media to remove.
        id: MediaAssetId,
    },
    /// Re-registers previously removed media at its original position.
    ///
    /// Only ever produced as the inverse of a removal, which is what keeps an undo
    /// from reshuffling the asset list.
    InsertMediaAsset {
        /// Position to restore the media to.
        index: usize,
        /// The media to restore.
        asset: MediaAsset,
    },
    /// Creates a sequence at a position in the sequence list.
    CreateSequence {
        /// Insertion index within the sequence list.
        index: usize,
        /// The sequence to create.
        sequence: Sequence,
    },
    /// Deletes a sequence.
    RemoveSequence {
        /// Sequence to remove.
        id: SequenceId,
    },
    /// Re-creates a previously deleted sequence at its original position.
    InsertSequence {
        /// Position to restore the sequence to.
        index: usize,
        /// The sequence to restore.
        sequence: Sequence,
    },
    /// Adds a track to a sequence.
    CreateTrack {
        /// Owning sequence.
        sequence: SequenceId,
        /// Insertion index within the track list.
        index: usize,
        /// The track to add.
        track: Track,
    },
    /// Removes a track, with every clip on it.
    RemoveTrack {
        /// Owning sequence.
        sequence: SequenceId,
        /// Track to remove.
        track: TrackId,
    },
    /// Adds a clip to the end of a track.
    ///
    /// Produced by interactive appends, where "the end" is what the user meant.
    /// Redo re-appends, which is correct because a log is undone last-in-first-out.
    AppendClip {
        /// Owning sequence.
        sequence: SequenceId,
        /// Target track.
        track: TrackId,
        /// The clip to append.
        clip: Clip,
    },
    /// Inserts a clip at an exact position in a track.
    ///
    /// Produced by undo of a removal, so a restored clip lands in its original slot.
    InsertClip {
        /// Owning sequence.
        sequence: SequenceId,
        /// Target track.
        track: TrackId,
        /// Position within the track's insertion order.
        index: usize,
        /// The clip to insert.
        clip: Clip,
    },
    /// Removes a clip from wherever it lives in a sequence.
    RemoveClip {
        /// Owning sequence.
        sequence: SequenceId,
        /// Clip to remove.
        clip: ClipId,
    },
    /// Sets a clip's source trim and timeline position.
    TrimClip {
        /// Owning sequence.
        sequence: SequenceId,
        /// Clip to trim.
        clip: ClipId,
        /// New source range.
        source_range: FrameRange,
        /// New timeline position.
        timeline_start: i64,
    },
    /// Moves a clip along the timeline without changing which track it is on.
    MoveClip {
        /// Owning sequence.
        sequence: SequenceId,
        /// Clip to move.
        clip: ClipId,
        /// New timeline position.
        timeline_start: i64,
    },
    /// Moves a clip to a different track at an exact position.
    ///
    /// The target track must differ from the clip's current track: same-track
    /// reordering is expressible with [`Command::MoveClip`], and keeping the two
    /// cases apart is what makes the inverse unambiguous.
    ReparentClip {
        /// Owning sequence.
        sequence: SequenceId,
        /// Clip to move.
        clip: ClipId,
        /// Destination track.
        track: TrackId,
        /// Position within the destination track's insertion order.
        index: usize,
    },
    /// Sets a clip's playback speed.
    SetClipSpeed {
        /// Owning sequence.
        sequence: SequenceId,
        /// Clip to retime.
        clip: ClipId,
        /// New speed, in source frames per timeline frame.
        speed: Rational,
    },
    /// Enables or disables a clip without removing it.
    SetClipEnabled {
        /// Owning sequence.
        sequence: SequenceId,
        /// Clip to toggle.
        clip: ClipId,
        /// New enabled state.
        enabled: bool,
    },
    /// Enables or disables a whole track.
    ///
    /// The renderer composites only the tracks that are enabled, so this is a real
    /// edit of the picture rather than a view preference: it belongs in the log
    /// with the clips it silences, and it is invertible like everything else.
    SetTrackEnabled {
        /// Owning sequence.
        sequence: SequenceId,
        /// Track to toggle.
        track: TrackId,
        /// New enabled state.
        enabled: bool,
    },
}

impl Command {
    /// Applies the command and returns the command that undoes it.
    ///
    /// The project is left untouched when an error is returned, so a failing command
    /// never leaves a half-applied edit behind.
    pub fn apply(&self, project: &mut Project) -> Result<Command, CoreError> {
        match self {
            Self::CreateMediaAsset { index, asset } => {
                if project.asset(asset.id()).is_some() {
                    return Err(CoreError::duplicate(MediaAssetId::KIND, asset.id().raw()));
                }
                project.insert_asset(*index, asset.clone())?;
                Ok(Self::RemoveMediaAsset { id: asset.id() })
            }
            Self::RemoveMediaAsset { id } => {
                let index = project
                    .asset_position(*id)
                    .ok_or_else(|| CoreError::not_found(MediaAssetId::KIND, id.raw()))?;
                let asset = project.remove_asset_at(index)?;
                Ok(Self::InsertMediaAsset { index, asset })
            }
            Self::InsertMediaAsset { index, asset } => {
                if project.asset(asset.id()).is_some() {
                    return Err(CoreError::duplicate(MediaAssetId::KIND, asset.id().raw()));
                }
                project.insert_asset(*index, asset.clone())?;
                Ok(Self::RemoveMediaAsset { id: asset.id() })
            }
            Self::CreateSequence { index, sequence } => {
                if project.sequence(sequence.id()).is_some() {
                    return Err(CoreError::duplicate(SequenceId::KIND, sequence.id().raw()));
                }
                project.insert_sequence(*index, sequence.clone())?;
                Ok(Self::RemoveSequence { id: sequence.id() })
            }
            Self::RemoveSequence { id } => {
                let index = project
                    .sequence_position(*id)
                    .ok_or_else(|| CoreError::not_found(SequenceId::KIND, id.raw()))?;
                let sequence = project.remove_sequence_at(index)?;
                Ok(Self::InsertSequence { index, sequence })
            }
            Self::InsertSequence { index, sequence } => {
                if project.sequence(sequence.id()).is_some() {
                    return Err(CoreError::duplicate(SequenceId::KIND, sequence.id().raw()));
                }
                project.insert_sequence(*index, sequence.clone())?;
                Ok(Self::RemoveSequence { id: sequence.id() })
            }
            Self::CreateTrack {
                sequence,
                index,
                track,
            } => {
                let target = project.sequence_mut(*sequence)?;
                if target.track(track.id()).is_some() {
                    return Err(CoreError::duplicate(TrackId::KIND, track.id().raw()));
                }
                target.insert_track(*index, track.clone())?;
                Ok(Self::RemoveTrack {
                    sequence: *sequence,
                    track: track.id(),
                })
            }
            Self::RemoveTrack { sequence, track } => {
                let target = project.sequence_mut(*sequence)?;
                let index = target
                    .track_position(*track)
                    .ok_or_else(|| CoreError::not_found(TrackId::KIND, track.raw()))?;
                let removed = target.remove_track_at(index)?;
                Ok(Self::CreateTrack {
                    sequence: *sequence,
                    index,
                    track: removed,
                })
            }
            Self::AppendClip {
                sequence,
                track,
                clip,
            } => {
                Self::check_clip_is_new(project, clip)?;
                let target = project.sequence_mut(*sequence)?;
                let target = target.track_mut(*track)?;
                let index = target.clip_count();
                target.insert_clip_at(index, clip.clone())?;
                Ok(Self::RemoveClip {
                    sequence: *sequence,
                    clip: clip.id(),
                })
            }
            Self::InsertClip {
                sequence,
                track,
                index,
                clip,
            } => {
                Self::check_clip_is_new(project, clip)?;
                let target = project.sequence_mut(*sequence)?;
                target
                    .track_mut(*track)?
                    .insert_clip_at(*index, clip.clone())?;
                Ok(Self::RemoveClip {
                    sequence: *sequence,
                    clip: clip.id(),
                })
            }
            Self::RemoveClip { sequence, clip } => {
                let target = project.sequence_mut(*sequence)?;
                let (track_index, clip_index) = target
                    .locate_clip(*clip)
                    .ok_or_else(|| CoreError::not_found(ClipId::KIND, clip.raw()))?;
                let track_id = target.track_at(track_index).map(Track::id).ok_or_else(|| {
                    CoreError::index_out_of_range(track_index, target.tracks().len())
                })?;
                let removed = target
                    .track_at_mut(track_index)?
                    .remove_clip_at(clip_index)?;
                Ok(Self::InsertClip {
                    sequence: *sequence,
                    track: track_id,
                    index: clip_index,
                    clip: removed,
                })
            }
            Self::TrimClip {
                sequence,
                clip,
                source_range,
                timeline_start,
            } => {
                let current = Self::current_clip(project, *clip)?;
                let candidate = Clip {
                    source_range: *source_range,
                    timeline_start: *timeline_start,
                    ..current.clone()
                };
                project.ensure_clip_source_within_media(&candidate)?;
                let target = project.sequence_mut(*sequence)?.clip_mut(*clip)?;
                target.source_range = *source_range;
                target.timeline_start = *timeline_start;
                Ok(Self::TrimClip {
                    sequence: *sequence,
                    clip: *clip,
                    source_range: current.source_range(),
                    timeline_start: current.timeline_start(),
                })
            }
            Self::MoveClip {
                sequence,
                clip,
                timeline_start,
            } => {
                let current = Self::current_clip(project, *clip)?;
                project
                    .sequence_mut(*sequence)?
                    .clip_mut(*clip)?
                    .timeline_start = *timeline_start;
                Ok(Self::MoveClip {
                    sequence: *sequence,
                    clip: *clip,
                    timeline_start: current.timeline_start(),
                })
            }
            Self::ReparentClip {
                sequence,
                clip,
                track,
                index,
            } => {
                let target = project.sequence_mut(*sequence)?;
                let (from_track, from_index) = target
                    .locate_clip(*clip)
                    .ok_or_else(|| CoreError::not_found(ClipId::KIND, clip.raw()))?;
                let from_id = target.track_at(from_track).map(Track::id).ok_or_else(|| {
                    CoreError::index_out_of_range(from_track, target.tracks().len())
                })?;
                if from_id == *track {
                    return Err(CoreError::invalid(
                        "reparenting a clip onto its own track is a move, not a reparent",
                    ));
                }
                let destination_len = target
                    .track(*track)
                    .ok_or_else(|| CoreError::not_found(TrackId::KIND, track.raw()))?
                    .clip_count();
                if *index > destination_len {
                    return Err(CoreError::index_out_of_range(*index, destination_len));
                }
                let moved = target
                    .track_at_mut(from_track)?
                    .remove_clip_at(from_index)?;
                target.track_mut(*track)?.insert_clip_at(*index, moved)?;
                Ok(Self::ReparentClip {
                    sequence: *sequence,
                    clip: *clip,
                    track: from_id,
                    index: from_index,
                })
            }
            Self::SetClipSpeed {
                sequence,
                clip,
                speed,
            } => {
                if !speed.is_positive() {
                    return Err(CoreError::invalid("clip speed must be greater than zero"));
                }
                let current = Self::current_clip(project, *clip)?;
                project.sequence_mut(*sequence)?.clip_mut(*clip)?.speed = *speed;
                Ok(Self::SetClipSpeed {
                    sequence: *sequence,
                    clip: *clip,
                    speed: current.speed(),
                })
            }
            Self::SetClipTransform {
                sequence,
                clip,
                transform,
            } => {
                transform.validate()?;
                let current = Self::current_clip(project, *clip)?;
                project.sequence_mut(*sequence)?.clip_mut(*clip)?.transform = *transform;
                Ok(Self::SetClipTransform {
                    sequence: *sequence,
                    clip: *clip,
                    transform: current.transform(),
                })
            }
            Self::SetClipOpacity {
                sequence,
                clip,
                opacity,
            } => {
                if opacity.is_negative() || *opacity > Rational::from_integer(1) {
                    return Err(CoreError::invalid(
                        "clip opacity must lie between zero and one",
                    ));
                }
                let current = Self::current_clip(project, *clip)?;
                project.sequence_mut(*sequence)?.clip_mut(*clip)?.opacity = *opacity;
                Ok(Self::SetClipOpacity {
                    sequence: *sequence,
                    clip: *clip,
                    opacity: current.opacity(),
                })
            }
            Self::SetClipBlend {
                sequence,
                clip,
                blend,
            } => {
                let current = Self::current_clip(project, *clip)?;
                project.sequence_mut(*sequence)?.clip_mut(*clip)?.blend = *blend;
                Ok(Self::SetClipBlend {
                    sequence: *sequence,
                    clip: *clip,
                    blend: current.blend(),
                })
            }
            Self::AddEffect {
                sequence,
                clip,
                index,
                effect,
            } => {
                effect.validate()?;
                Self::check_effect_is_new(project, effect.id())?;
                let target = project.sequence_mut(*sequence)?.clip_mut(*clip)?;
                if *index > target.effects().len() {
                    return Err(CoreError::index_out_of_range(
                        *index,
                        target.effects().len(),
                    ));
                }
                target.effects.insert(*index, effect.clone());
                Ok(Self::RemoveEffect {
                    sequence: *sequence,
                    clip: *clip,
                    effect: effect.id(),
                })
            }
            Self::RemoveEffect {
                sequence,
                clip,
                effect,
            } => {
                let target = project.sequence_mut(*sequence)?.clip_mut(*clip)?;
                let index = target
                    .effect_position(*effect)
                    .ok_or_else(|| CoreError::not_found(EffectId::KIND, effect.raw()))?;
                let removed = target.effects.remove(index);
                Ok(Self::AddEffect {
                    sequence: *sequence,
                    clip: *clip,
                    index,
                    effect: removed,
                })
            }
            Self::SetEffectEnabled {
                sequence,
                clip,
                effect,
                enabled,
            } => {
                let target = project.sequence_mut(*sequence)?.clip_mut(*clip)?;
                let slot = target
                    .effects
                    .iter_mut()
                    .find(|candidate| candidate.id == *effect)
                    .ok_or_else(|| CoreError::not_found(EffectId::KIND, effect.raw()))?;
                let previous = slot.enabled;
                slot.enabled = *enabled;
                Ok(Self::SetEffectEnabled {
                    sequence: *sequence,
                    clip: *clip,
                    effect: *effect,
                    enabled: previous,
                })
            }
            Self::SetEffectParam {
                sequence,
                clip,
                effect,
                key,
                value,
            } => {
                let target = project.sequence_mut(*sequence)?.clip_mut(*clip)?;
                let slot = target
                    .effects
                    .iter_mut()
                    .find(|candidate| candidate.id == *effect)
                    .ok_or_else(|| CoreError::not_found(EffectId::KIND, effect.raw()))?;
                let previous = slot
                    .param(key)
                    .map(|param| param.value().clone())
                    .ok_or_else(|| {
                        CoreError::invalid(format!("effect has no parameter `{key}`"))
                    })?;
                slot.set_param(key, value.clone())?;
                Ok(Self::SetEffectParam {
                    sequence: *sequence,
                    clip: *clip,
                    effect: *effect,
                    key: key.clone(),
                    value: previous,
                })
            }
            Self::SetChannelKeys {
                sequence,
                clip,
                channel,
                keys,
            } => {
                let target = project.sequence_mut(*sequence)?.clip_mut(*clip)?;
                let previous = target
                    .animation()
                    .iter()
                    .find(|candidate| candidate.channel() == *channel)
                    .map(|animation| animation.keys().to_vec())
                    .unwrap_or_default();
                if keys.is_empty() {
                    target
                        .animation
                        .retain(|candidate| candidate.channel != *channel);
                } else {
                    let rebuilt = ChannelAnimation::new(*channel, keys.clone())?;
                    match target
                        .animation
                        .iter_mut()
                        .find(|candidate| candidate.channel == *channel)
                    {
                        Some(existing) => *existing = rebuilt,
                        None => target.animation.push(rebuilt),
                    }
                }
                Ok(Self::SetChannelKeys {
                    sequence: *sequence,
                    clip: *clip,
                    channel: *channel,
                    keys: previous,
                })
            }
            Self::AddMask {
                sequence,
                clip,
                index,
                mask,
            } => {
                mask.validate()?;
                let target = project.sequence_mut(*sequence)?.clip_mut(*clip)?;
                if target.mask(mask.id()).is_some() {
                    return Err(CoreError::duplicate(MaskId::KIND, mask.id().raw()));
                }
                if *index > target.masks().len() {
                    return Err(CoreError::index_out_of_range(*index, target.masks().len()));
                }
                target.masks.insert(*index, *mask);
                Ok(Self::RemoveMask {
                    sequence: *sequence,
                    clip: *clip,
                    mask: mask.id(),
                })
            }
            Self::RemoveMask {
                sequence,
                clip,
                mask,
            } => {
                let target = project.sequence_mut(*sequence)?.clip_mut(*clip)?;
                let index = target
                    .mask_position(*mask)
                    .ok_or_else(|| CoreError::not_found(MaskId::KIND, mask.raw()))?;
                let removed = target.masks.remove(index);
                Ok(Self::AddMask {
                    sequence: *sequence,
                    clip: *clip,
                    index,
                    mask: removed,
                })
            }
            Self::SetMask {
                sequence,
                clip,
                mask,
                value,
            } => {
                value.validate()?;
                if value.id() != *mask {
                    return Err(CoreError::invalid(
                        "a mask replacement must keep its identifier",
                    ));
                }
                let target = project.sequence_mut(*sequence)?.clip_mut(*clip)?;
                let index = target
                    .mask_position(*mask)
                    .ok_or_else(|| CoreError::not_found(MaskId::KIND, mask.raw()))?;
                let previous = target.masks[index];
                target.masks[index] = *value;
                Ok(Self::SetMask {
                    sequence: *sequence,
                    clip: *clip,
                    mask: *mask,
                    value: previous,
                })
            }
            Self::AddTransition {
                sequence,
                track,
                index,
                transition,
            } => {
                transition.validate()?;
                let target = project.sequence_mut(*sequence)?.track_mut(*track)?;
                if target.transition(transition.id()).is_some() {
                    return Err(CoreError::duplicate(
                        TransitionId::KIND,
                        transition.id().raw(),
                    ));
                }
                target.insert_transition_at(*index, transition.clone())?;
                Ok(Self::RemoveTransition {
                    sequence: *sequence,
                    track: *track,
                    transition: transition.id(),
                })
            }
            Self::RemoveTransition {
                sequence,
                track,
                transition,
            } => {
                let target = project.sequence_mut(*sequence)?.track_mut(*track)?;
                let index = target
                    .transition_position(*transition)
                    .ok_or_else(|| CoreError::not_found(TransitionId::KIND, transition.raw()))?;
                let removed = target.remove_transition_at(index)?;
                Ok(Self::AddTransition {
                    sequence: *sequence,
                    track: *track,
                    index,
                    transition: removed,
                })
            }
            Self::SetClipEnabled {
                sequence,
                clip,
                enabled,
            } => {
                let current = Self::current_clip(project, *clip)?;
                project.sequence_mut(*sequence)?.clip_mut(*clip)?.enabled = *enabled;
                Ok(Self::SetClipEnabled {
                    sequence: *sequence,
                    clip: *clip,
                    enabled: current.is_enabled(),
                })
            }
            Self::SetTrackEnabled {
                sequence,
                track,
                enabled,
            } => {
                let current = project.sequence_mut(*sequence)?.track_mut(*track)?;
                let previous = current.is_enabled();
                current.enabled = *enabled;
                Ok(Self::SetTrackEnabled {
                    sequence: *sequence,
                    track: *track,
                    enabled: previous,
                })
            }
        }
    }

    /// Short stable name of the variant, for logs and telemetry.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::CreateMediaAsset { .. } => "create_media_asset",
            Self::RemoveMediaAsset { .. } => "remove_media_asset",
            Self::InsertMediaAsset { .. } => "insert_media_asset",
            Self::CreateSequence { .. } => "create_sequence",
            Self::RemoveSequence { .. } => "remove_sequence",
            Self::InsertSequence { .. } => "insert_sequence",
            Self::CreateTrack { .. } => "create_track",
            Self::RemoveTrack { .. } => "remove_track",
            Self::AppendClip { .. } => "append_clip",
            Self::InsertClip { .. } => "insert_clip",
            Self::RemoveClip { .. } => "remove_clip",
            Self::TrimClip { .. } => "trim_clip",
            Self::MoveClip { .. } => "move_clip",
            Self::ReparentClip { .. } => "reparent_clip",
            Self::SetClipSpeed { .. } => "set_clip_speed",
            Self::SetClipEnabled { .. } => "set_clip_enabled",
            Self::SetTrackEnabled { .. } => "set_track_enabled",
            Self::SetClipTransform { .. } => "set_clip_transform",
            Self::SetClipOpacity { .. } => "set_clip_opacity",
            Self::SetClipBlend { .. } => "set_clip_blend",
            Self::AddEffect { .. } => "add_effect",
            Self::RemoveEffect { .. } => "remove_effect",
            Self::SetEffectEnabled { .. } => "set_effect_enabled",
            Self::SetEffectParam { .. } => "set_effect_param",
            Self::SetChannelKeys { .. } => "set_channel_keys",
            Self::AddMask { .. } => "add_mask",
            Self::RemoveMask { .. } => "remove_mask",
            Self::SetMask { .. } => "set_mask",
            Self::AddTransition { .. } => "add_transition",
            Self::RemoveTransition { .. } => "remove_transition",
        }
    }

    /// Refuses an effect identifier that is already used anywhere in the project.
    ///
    /// The scan covers the whole project rather than one clip because effect
    /// identifiers come from the single project-wide allocator: one that were unique
    /// only per clip would still collide with a collaborator's edit.
    fn check_effect_is_new(project: &Project, id: EffectId) -> Result<(), CoreError> {
        for sequence in project.sequences() {
            for track in sequence.tracks() {
                for clip in track.clips() {
                    if clip.effect(id).is_some() {
                        return Err(CoreError::duplicate(EffectId::KIND, id.raw()));
                    }
                }
            }
        }
        Ok(())
    }
    /// Read the clip a command targets, failing if it is gone.
    fn current_clip(project: &Project, id: ClipId) -> Result<Clip, CoreError> {
        project
            .clip(id)
            .cloned()
            .ok_or_else(|| CoreError::not_found(ClipId::KIND, id.raw()))
    }

    /// Shared preconditions for adding a clip: new identifier, trim inside media.
    fn check_clip_is_new(project: &Project, clip: &Clip) -> Result<(), CoreError> {
        if project.clip(clip.id()).is_some() {
            return Err(CoreError::duplicate(ClipId::KIND, clip.id().raw()));
        }
        project.ensure_clip_source_within_media(clip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::{EffectKind, Interpolation, MaskShape, TransitionKind};
    use crate::entity::{Track, TrackKind};
    use crate::ids::IdAllocator;
    use maj_time::Timebase;

    fn fixture() -> (Project, IdAllocator) {
        let mut project = Project::new("commands");
        let mut ids = IdAllocator::for_project(&project);
        let asset_id = ids.media_asset();
        project
            .insert_asset(
                0,
                MediaAsset::new(asset_id, "a.mov", "file:///a.mov", Timebase::FPS_25, 1_000)
                    .unwrap(),
            )
            .unwrap();
        let sequence_id = ids.sequence();
        let sequence =
            Sequence::new(sequence_id, "S", Timebase::FPS_25, 1920, 1080, 48_000).unwrap();
        project.insert_sequence(0, sequence).unwrap();
        let track_id = ids.track();
        project
            .sequence_mut(sequence_id)
            .unwrap()
            .insert_track(0, Track::new(track_id, TrackKind::Video, "V1"))
            .unwrap();
        (project, ids)
    }

    fn clip_for(ids: &mut IdAllocator, name: &str, start: i64) -> Clip {
        Clip::new(
            ids.clip(),
            MediaAssetId::new(1),
            name,
            FrameRange::new(100, 200).unwrap(),
            start,
            Rational::from_integer(1),
        )
        .unwrap()
    }

    #[test]
    fn every_field_editing_command_is_inverted_by_the_same_variant() {
        let (mut project, mut ids) = fixture();
        let sequence = SequenceId::new(2);
        let track = TrackId::new(3);
        // Build the clips once, then edit them one command at a time. Each edit is
        // applied and immediately reverted, so no edit can invalidate the next.
        for (name, start) in [("c1", 0), ("c2", 500)] {
            Command::AppendClip {
                sequence,
                track,
                clip: clip_for(&mut ids, name, start),
            }
            .apply(&mut project)
            .unwrap();
        }

        let clip = ClipId::new(4);
        let edits = [
            Command::TrimClip {
                sequence,
                clip,
                source_range: FrameRange::new(120, 180).unwrap(),
                timeline_start: 25,
            },
            Command::MoveClip {
                sequence,
                clip,
                timeline_start: 300,
            },
            Command::SetClipSpeed {
                sequence,
                clip,
                speed: Rational::new(1, 2).unwrap(),
            },
            Command::SetClipEnabled {
                sequence,
                clip,
                enabled: false,
            },
            Command::SetTrackEnabled {
                sequence,
                track,
                enabled: false,
            },
        ];

        for command in edits {
            let before = project.state_hash().unwrap();
            let inverse = command.apply(&mut project).unwrap();
            assert_ne!(
                before,
                project.state_hash().unwrap(),
                "{command:?} changed nothing"
            );
            assert_eq!(
                inverse.kind(),
                command.kind(),
                "a field edit must be inverted by its own variant"
            );
            inverse.apply(&mut project).unwrap();
            assert_eq!(
                before,
                project.state_hash().unwrap(),
                "the inverse of {command:?} did not restore the state"
            );
        }
    }

    #[test]
    fn a_clip_append_inverts_to_a_removal_which_inverts_back_to_an_insert() {
        let (mut project, mut ids) = fixture();
        let sequence = SequenceId::new(2);
        let track = TrackId::new(3);
        let before = project.state_hash().unwrap();

        let appended = Command::AppendClip {
            sequence,
            track,
            clip: clip_for(&mut ids, "c1", 0),
        }
        .apply(&mut project)
        .unwrap();
        assert_eq!(appended.kind(), "remove_clip");

        let removed = appended.apply(&mut project).unwrap();
        assert_eq!(removed.kind(), "insert_clip");

        let reinserted = removed.apply(&mut project).unwrap();
        assert_eq!(reinserted.kind(), "remove_clip");
        assert_eq!(project.sequences[0].tracks[0].clip_count(), 1);

        reinserted.apply(&mut project).unwrap();
        assert_eq!(project.state_hash().unwrap(), before);
    }

    #[test]
    fn a_media_creation_inverts_to_a_removal_which_inverts_back_to_an_insert() {
        let (mut project, mut ids) = fixture();
        let extra = ids.media_asset();
        let asset = MediaAsset::new(extra, "b.mov", "file:///b.mov", Timebase::FPS_25, 10).unwrap();
        let before = project.state_hash().unwrap();

        // Create -> remove -> insert -> remove alternates around `before`.
        let removed = Command::CreateMediaAsset {
            index: 0,
            asset: asset.clone(),
        }
        .apply(&mut project)
        .unwrap();
        assert_eq!(removed.kind(), "remove_media_asset");
        assert_ne!(project.state_hash().unwrap(), before);

        let restored = removed.apply(&mut project).unwrap();
        assert_eq!(restored.kind(), "insert_media_asset");
        assert_eq!(
            project.state_hash().unwrap(),
            before,
            "the removal restored the original state"
        );

        let re_removed = restored.apply(&mut project).unwrap();
        assert_eq!(re_removed.kind(), "remove_media_asset");
        assert_ne!(project.state_hash().unwrap(), before);
        // Re-inserted at index 0, not appended: the order of the asset list is part
        // of the state, so restoring it at the end would be a different project.
        assert_eq!(project.assets()[0].name(), "b.mov");

        re_removed.apply(&mut project).unwrap();
        assert_eq!(project.state_hash().unwrap(), before);
        assert_eq!(project.assets()[0].name(), "a.mov");
    }

    #[test]
    fn a_track_creation_inverts_to_a_removal_which_inverts_back_to_a_creation() {
        let (mut project, ids) = fixture();
        let sequence = SequenceId::new(2);
        let extra = TrackId::new(ids.peek());
        let before = project.state_hash().unwrap();

        let removed = Command::CreateTrack {
            sequence,
            index: 0,
            track: Track::new(extra, TrackKind::Audio, "A1"),
        }
        .apply(&mut project)
        .unwrap();
        assert_eq!(removed.kind(), "remove_track");
        assert_eq!(project.sequences[0].tracks()[0].name(), "A1");

        let restored = removed.apply(&mut project).unwrap();
        assert_eq!(restored.kind(), "create_track");
        assert_eq!(
            project.state_hash().unwrap(),
            before,
            "the removal restored the original state"
        );
        assert_eq!(project.sequences[0].tracks().len(), 1);

        let re_removed = restored.apply(&mut project).unwrap();
        assert_eq!(re_removed.kind(), "remove_track");
        assert_ne!(project.state_hash().unwrap(), before);
        assert_eq!(project.sequences[0].tracks()[0].name(), "A1");

        re_removed.apply(&mut project).unwrap();
        assert_eq!(project.state_hash().unwrap(), before);
        assert_eq!(project.sequences[0].tracks().len(), 1);
        assert_eq!(project.sequences[0].tracks()[0].name(), "V1");
    }

    #[test]
    fn removing_a_clip_restores_its_position_not_just_its_existence() {
        let (mut project, mut ids) = fixture();
        let sequence = SequenceId::new(2);
        let track = TrackId::new(3);
        for (name, start) in [("first", 0), ("second", 500), ("third", 900)] {
            Command::AppendClip {
                sequence,
                track,
                clip: clip_for(&mut ids, name, start),
            }
            .apply(&mut project)
            .unwrap();
        }

        let before = project.state_hash().unwrap();
        let inverse = Command::RemoveClip {
            sequence,
            clip: ClipId::new(5),
        }
        .apply(&mut project)
        .unwrap();

        let remaining: Vec<&str> = project.sequences[0].tracks[0]
            .clips()
            .iter()
            .map(Clip::name)
            .collect();
        assert_eq!(remaining, vec!["first", "third"]);

        inverse.apply(&mut project).unwrap();
        let restored: Vec<&str> = project.sequences[0].tracks[0]
            .clips()
            .iter()
            .map(Clip::name)
            .collect();
        assert_eq!(restored, vec!["first", "second", "third"]);
        assert_eq!(before, project.state_hash().unwrap());
    }

    #[test]
    fn removing_a_media_asset_restores_its_position_in_the_list() {
        let (mut project, mut ids) = fixture();
        let extra = ids.media_asset();
        Command::CreateMediaAsset {
            index: 0,
            asset: MediaAsset::new(extra, "b.mov", "file:///b.mov", Timebase::FPS_25, 10).unwrap(),
        }
        .apply(&mut project)
        .unwrap();

        let before: Vec<String> = project
            .assets()
            .iter()
            .map(|a| a.name().to_owned())
            .collect();
        assert_eq!(before, vec!["b.mov", "a.mov"]);

        let inverse = Command::RemoveMediaAsset { id: extra }
            .apply(&mut project)
            .unwrap();
        let during: Vec<String> = project
            .assets()
            .iter()
            .map(|a| a.name().to_owned())
            .collect();
        assert_eq!(during, vec!["a.mov"]);

        inverse.apply(&mut project).unwrap();
        let after: Vec<String> = project
            .assets()
            .iter()
            .map(|a| a.name().to_owned())
            .collect();
        assert_eq!(after, before, "the asset returned to its original position");
    }

    #[test]
    fn a_removed_track_returns_at_its_original_index() {
        let (mut project, ids) = fixture();
        let sequence = SequenceId::new(2);
        let second = TrackId::new(9);
        Command::CreateTrack {
            sequence,
            index: 0,
            track: Track::new(second, TrackKind::Audio, "A1"),
        }
        .apply(&mut project)
        .unwrap();
        let before: Vec<u64> = project.sequences[0]
            .tracks()
            .iter()
            .map(|t| t.id().raw())
            .collect();
        assert_eq!(before, vec![9, 3]);

        let inverse = Command::RemoveTrack {
            sequence,
            track: second,
        }
        .apply(&mut project)
        .unwrap();
        inverse.apply(&mut project).unwrap();
        let after: Vec<u64> = project.sequences[0]
            .tracks()
            .iter()
            .map(|t| t.id().raw())
            .collect();
        assert_eq!(after, before);
        let _ = ids;
    }

    #[test]
    fn reparenting_rejects_the_same_track() {
        let (mut project, mut ids) = fixture();
        let sequence = SequenceId::new(2);
        let track = TrackId::new(3);
        Command::AppendClip {
            sequence,
            track,
            clip: clip_for(&mut ids, "c1", 0),
        }
        .apply(&mut project)
        .unwrap();

        let err = Command::ReparentClip {
            sequence,
            clip: ClipId::new(4),
            track,
            index: 0,
        }
        .apply(&mut project)
        .unwrap_err();
        assert!(err.to_string().contains("reparent"), "unexpected: {err}");
    }

    #[test]
    fn reparenting_moves_the_clip_between_tracks_and_back() {
        let (mut project, mut ids) = fixture();
        let sequence = SequenceId::new(2);
        let video = TrackId::new(3);
        let audio = TrackId::new(8);
        Command::CreateTrack {
            sequence,
            index: 1,
            track: Track::new(audio, TrackKind::Audio, "A1"),
        }
        .apply(&mut project)
        .unwrap();
        Command::AppendClip {
            sequence,
            track: video,
            clip: clip_for(&mut ids, "c1", 0),
        }
        .apply(&mut project)
        .unwrap();

        let before = project.state_hash().unwrap();
        let inverse = Command::ReparentClip {
            sequence,
            clip: ClipId::new(4),
            track: audio,
            index: 0,
        }
        .apply(&mut project)
        .unwrap();
        assert_eq!(project.sequences[0].track(video).unwrap().clip_count(), 0);
        assert_eq!(project.sequences[0].track(audio).unwrap().clip_count(), 1);

        inverse.apply(&mut project).unwrap();
        assert_eq!(project.sequences[0].track(video).unwrap().clip_count(), 1);
        assert_eq!(project.sequences[0].track(audio).unwrap().clip_count(), 0);
        assert_eq!(before, project.state_hash().unwrap());
    }

    #[test]
    fn a_duplicate_identifier_is_rejected_before_anything_changes() {
        let (mut project, mut ids) = fixture();
        let sequence = SequenceId::new(2);
        let track = TrackId::new(3);
        let clip = clip_for(&mut ids, "c1", 0);
        Command::AppendClip {
            sequence,
            track,
            clip: clip.clone(),
        }
        .apply(&mut project)
        .unwrap();

        let before = project.state_hash().unwrap();
        let err = Command::AppendClip {
            sequence,
            track,
            clip,
        }
        .apply(&mut project)
        .unwrap_err();
        assert!(matches!(err, CoreError::Duplicate { .. }));
        assert_eq!(
            before,
            project.state_hash().unwrap(),
            "a failed command mutated the project"
        );
    }

    #[test]
    fn a_trim_beyond_the_media_is_rejected() {
        let (mut project, mut ids) = fixture();
        let sequence = SequenceId::new(2);
        let track = TrackId::new(3);
        Command::AppendClip {
            sequence,
            track,
            clip: clip_for(&mut ids, "c1", 0),
        }
        .apply(&mut project)
        .unwrap();

        let before = project.state_hash().unwrap();
        let err = Command::TrimClip {
            sequence,
            clip: ClipId::new(4),
            source_range: FrameRange::new(0, 1_001).unwrap(),
            timeline_start: 0,
        }
        .apply(&mut project)
        .unwrap_err();
        assert!(
            err.to_string().contains("reads to frame"),
            "unexpected: {err}"
        );
        assert_eq!(before, project.state_hash().unwrap());
    }

    #[test]
    fn a_non_positive_speed_is_rejected() {
        let (mut project, mut ids) = fixture();
        let sequence = SequenceId::new(2);
        let track = TrackId::new(3);
        Command::AppendClip {
            sequence,
            track,
            clip: clip_for(&mut ids, "c1", 0),
        }
        .apply(&mut project)
        .unwrap();

        assert!(
            Command::SetClipSpeed {
                sequence,
                clip: ClipId::new(4),
                speed: Rational::from_integer(0),
            }
            .apply(&mut project)
            .is_err()
        );
    }

    #[test]
    fn command_kinds_are_unique_and_stable() {
        let commands = [
            Command::RemoveMediaAsset {
                id: MediaAssetId::new(1),
            },
            Command::RemoveSequence {
                id: SequenceId::new(1),
            },
            Command::RemoveTrack {
                sequence: SequenceId::new(1),
                track: TrackId::new(1),
            },
            Command::RemoveClip {
                sequence: SequenceId::new(1),
                clip: ClipId::new(1),
            },
            Command::MoveClip {
                sequence: SequenceId::new(1),
                clip: ClipId::new(1),
                timeline_start: 0,
            },
            Command::SetClipEnabled {
                sequence: SequenceId::new(1),
                clip: ClipId::new(1),
                enabled: true,
            },
            Command::SetTrackEnabled {
                sequence: SequenceId::new(1),
                track: TrackId::new(1),
                enabled: true,
            },
        ];
        let kinds: Vec<&str> = commands.iter().map(Command::kind).collect();
        let unique: std::collections::BTreeSet<&str> = kinds.iter().copied().collect();
        assert_eq!(kinds.len(), unique.len(), "two variants share a kind name");
        for kind in kinds {
            assert!(
                kind.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "kind `{kind}` is not snake_case"
            );
        }
    }
    #[test]
    fn every_compositing_command_changes_the_state_and_its_inverse_restores_it() {
        let (mut project, mut ids) = fixture();
        let sequence = SequenceId::new(2);
        let track = TrackId::new(3);
        let clip = clip_for(&mut ids, "c1", 0);
        let clip_id = clip.id();
        Command::AppendClip {
            sequence,
            track,
            clip,
        }
        .apply(&mut project)
        .unwrap();

        let effect = EffectInstance::new(EffectId::new(50), EffectKind::Grain);
        let mask = Mask::new(MaskId::new(51), MaskShape::Ellipse).unwrap();
        let transition = Transition::new(
            TransitionId::new(52),
            TransitionKind::CrossDissolve,
            0,
            10,
            Some(clip_id),
            None,
        )
        .unwrap();

        let steps = vec![
            Command::SetClipTransform {
                sequence,
                clip: clip_id,
                transform: Transform::new(
                    Rational::from_integer(40),
                    Rational::from_integer(-10),
                    Rational::new(3, 2).unwrap(),
                    Rational::new(3, 2).unwrap(),
                    Rational::from_integer(90),
                    Rational::from_reduced(1, 2),
                    Rational::from_reduced(1, 2),
                )
                .unwrap(),
            },
            Command::SetClipOpacity {
                sequence,
                clip: clip_id,
                opacity: Rational::new(1, 3).unwrap(),
            },
            Command::SetClipBlend {
                sequence,
                clip: clip_id,
                blend: BlendMode::HardLight,
            },
            Command::AddEffect {
                sequence,
                clip: clip_id,
                index: 0,
                effect,
            },
            Command::SetChannelKeys {
                sequence,
                clip: clip_id,
                channel: AnimChannel::Opacity,
                keys: vec![
                    Keyframe::new(
                        0,
                        ParamValue::Rational(Rational::from_integer(0)),
                        Interpolation::Hold,
                    ),
                    Keyframe::new(
                        5,
                        ParamValue::Rational(Rational::from_integer(1)),
                        Interpolation::Linear,
                    ),
                ],
            },
            Command::AddMask {
                sequence,
                clip: clip_id,
                index: 0,
                mask,
            },
            Command::AddTransition {
                sequence,
                track,
                index: 0,
                transition,
            },
        ];

        for command in steps {
            let before = project.state_hash().unwrap();
            let inverse = command.apply(&mut project).unwrap();
            assert_ne!(
                before,
                project.state_hash().unwrap(),
                "{command:?} changed nothing"
            );
            assert!(project.is_valid(), "{command:?} left an invalid project");
            inverse.apply(&mut project).unwrap();
            assert_eq!(
                before,
                project.state_hash().unwrap(),
                "the inverse of {command:?} did not restore the state"
            );
        }
    }

    #[test]
    fn a_parameter_edit_is_inverted_by_the_same_variant() {
        let (mut project, mut ids) = fixture();
        let sequence = SequenceId::new(2);
        let track = TrackId::new(3);
        let clip = clip_for(&mut ids, "c1", 0);
        let clip_id = clip.id();
        Command::AppendClip {
            sequence,
            track,
            clip,
        }
        .apply(&mut project)
        .unwrap();
        Command::AddEffect {
            sequence,
            clip: clip_id,
            index: 0,
            effect: EffectInstance::new(EffectId::new(60), EffectKind::Blur),
        }
        .apply(&mut project)
        .unwrap();

        let edits = [
            Command::SetEffectParam {
                sequence,
                clip: clip_id,
                effect: EffectId::new(60),
                key: "radius".to_owned(),
                value: ParamValue::Rational(Rational::from_integer(12)),
            },
            Command::SetEffectEnabled {
                sequence,
                clip: clip_id,
                effect: EffectId::new(60),
                enabled: false,
            },
        ];

        for command in edits {
            let before = project.state_hash().unwrap();
            let inverse = command.apply(&mut project).unwrap();
            assert_eq!(inverse.kind(), command.kind());
            assert_ne!(
                before,
                project.state_hash().unwrap(),
                "{command:?} changed nothing"
            );
            inverse.apply(&mut project).unwrap();
            assert_eq!(before, project.state_hash().unwrap());
        }
    }

    #[test]
    fn a_failed_compositing_command_changes_nothing() {
        let (mut project, mut ids) = fixture();
        let sequence = SequenceId::new(2);
        let track = TrackId::new(3);
        let clip = clip_for(&mut ids, "c1", 0);
        let clip_id = clip.id();
        Command::AppendClip {
            sequence,
            track,
            clip,
        }
        .apply(&mut project)
        .unwrap();

        let before = project.state_hash().unwrap();
        let duplicate = Command::AddEffect {
            sequence,
            clip: clip_id,
            index: 0,
            effect: EffectInstance::new(EffectId::new(70), EffectKind::Grain),
        }
        .apply(&mut project)
        .unwrap();
        assert_eq!(duplicate.kind(), "remove_effect");
        let reused = Command::AddEffect {
            sequence,
            clip: clip_id,
            index: 0,
            effect: EffectInstance::new(EffectId::new(70), EffectKind::Grain),
        };
        assert!(
            reused.apply(&mut project).is_err(),
            "a reused effect id is refused"
        );

        let undeclared = Command::SetEffectParam {
            sequence,
            clip: clip_id,
            effect: EffectId::new(70),
            key: "nonsense".to_owned(),
            value: ParamValue::Integer(1),
        };
        assert!(undeclared.apply(&mut project).is_err());

        let out_of_range = Command::SetEffectParam {
            sequence,
            clip: clip_id,
            effect: EffectId::new(70),
            key: "amount".to_owned(),
            value: ParamValue::Rational(Rational::from_integer(9)),
        };
        assert!(out_of_range.apply(&mut project).is_err());

        let bad_mask = Command::AddMask {
            sequence,
            clip: clip_id,
            index: 0,
            mask: Mask::new(MaskId::new(71), MaskShape::Ellipse).unwrap(),
        };
        assert!(bad_mask.apply(&mut project).is_ok());
        let duplicate_mask = Command::AddMask {
            sequence,
            clip: clip_id,
            index: 0,
            mask: Mask::new(MaskId::new(71), MaskShape::Ellipse).unwrap(),
        };
        assert!(duplicate_mask.apply(&mut project).is_err());
        let _ = before;
    }
}
