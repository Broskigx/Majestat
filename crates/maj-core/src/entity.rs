//! The editable model: media, sequences, tracks and clips.
//!
//! A clip instance is deliberately distinct from the media it reads. The same camera
//! file can appear in a dozen timeline positions with different trims, speeds and
//! effects, and each appearance is its own [`Clip`] with its own identifier.

use maj_time::{FrameRange, Rational, RetimeMap, Timebase};
use serde::{Deserialize, Serialize};

use crate::composite::{
    AnimChannel, BlendMode, ChannelAnimation, EffectInstance, Mask, Transform, Transition,
};
use crate::error::CoreError;
use crate::ids::{ClipId, MediaAssetId, SequenceId, TrackId};
use crate::ids::{EffectId, MaskId, TransitionId};

/// A piece of source media on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaAsset {
    pub(crate) id: MediaAssetId,
    pub(crate) name: String,
    pub(crate) uri: String,
    pub(crate) timebase: Timebase,
    pub(crate) duration_frames: i64,
    pub(crate) has_video: bool,
    pub(crate) has_audio: bool,
}

impl MediaAsset {
    /// Registers media with its own timebase and duration.
    ///
    /// Returns [`CoreError::InvalidArgument`] for a negative duration.
    pub fn new(
        id: MediaAssetId,
        name: impl Into<String>,
        uri: impl Into<String>,
        timebase: Timebase,
        duration_frames: i64,
    ) -> Result<Self, CoreError> {
        if duration_frames < 0 {
            return Err(CoreError::invalid("media duration cannot be negative"));
        }
        Ok(Self {
            id,
            name: name.into(),
            uri: uri.into(),
            timebase,
            duration_frames,
            has_video: true,
            has_audio: true,
        })
    }

    /// Declares which streams the media carries.
    #[must_use]
    pub fn with_streams(mut self, has_video: bool, has_audio: bool) -> Self {
        self.has_video = has_video;
        self.has_audio = has_audio;
        self
    }

    /// Identifier of this asset.
    #[must_use]
    pub const fn id(&self) -> MediaAssetId {
        self.id
    }

    /// Display name, usually the file name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Location of the media.
    #[must_use]
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// The rate the media was captured at.
    #[must_use]
    pub const fn timebase(&self) -> Timebase {
        self.timebase
    }

    /// Length of the media in its own frames.
    #[must_use]
    pub const fn duration_frames(&self) -> i64 {
        self.duration_frames
    }

    /// Reports whether the media carries a video stream.
    #[must_use]
    pub const fn has_video(&self) -> bool {
        self.has_video
    }

    /// Reports whether the media carries an audio stream.
    #[must_use]
    pub const fn has_audio(&self) -> bool {
        self.has_audio
    }

    /// The full addressable range of the media.
    #[must_use]
    pub fn full_range(&self) -> FrameRange {
        FrameRange::from_start_len(0, self.duration_frames)
            .unwrap_or_else(|_| FrameRange::empty_at(0))
    }
}

/// One appearance of media on a timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clip {
    pub(crate) id: ClipId,
    pub(crate) asset_id: MediaAssetId,
    pub(crate) name: String,
    pub(crate) source_range: FrameRange,
    pub(crate) timeline_start: i64,
    pub(crate) speed: Rational,
    pub(crate) enabled: bool,
    pub(crate) transform: Transform,
    pub(crate) opacity: Rational,
    pub(crate) blend: BlendMode,
    pub(crate) effects: Vec<EffectInstance>,
    pub(crate) animation: Vec<ChannelAnimation>,
    pub(crate) masks: Vec<Mask>,
}

impl Clip {
    /// Places a range of an asset at a timeline position.
    ///
    /// Returns [`CoreError::InvalidArgument`] for a speed of zero or below. Speed is
    /// stored exactly, so a `24000/30000` retime is not rounded.
    pub fn new(
        id: ClipId,
        asset_id: MediaAssetId,
        name: impl Into<String>,
        source_range: FrameRange,
        timeline_start: i64,
        speed: Rational,
    ) -> Result<Self, CoreError> {
        if !speed.is_positive() {
            return Err(CoreError::invalid("clip speed must be greater than zero"));
        }
        Ok(Self {
            id,
            asset_id,
            name: name.into(),
            source_range,
            timeline_start,
            speed,
            enabled: true,
            transform: Transform::identity(),
            opacity: Rational::from_integer(1),
            blend: BlendMode::Normal,
            effects: Vec::new(),
            animation: Vec::new(),
            masks: Vec::new(),
        })
    }

    /// Identifier of this clip instance.
    #[must_use]
    pub const fn id(&self) -> ClipId {
        self.id
    }

    /// The asset this clip reads from.
    #[must_use]
    pub const fn asset_id(&self) -> MediaAssetId {
        self.asset_id
    }

    /// Display name of the clip.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The trimmed portion of the source that this clip uses.
    #[must_use]
    pub const fn source_range(&self) -> FrameRange {
        self.source_range
    }

    /// First timeline frame the clip occupies.
    #[must_use]
    pub const fn timeline_start(&self) -> i64 {
        self.timeline_start
    }

    /// Exact playback speed, in source frames per timeline frame.
    #[must_use]
    pub const fn speed(&self) -> Rational {
        self.speed
    }

    /// Reports whether the clip contributes to the output.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Placement of the layer inside the sequence frame.
    #[must_use]
    pub const fn transform(&self) -> Transform {
        self.transform
    }

    /// Layer opacity, in `0..=1`.
    #[must_use]
    pub const fn opacity(&self) -> Rational {
        self.opacity
    }

    /// How the layer combines with the tracks beneath it.
    #[must_use]
    pub const fn blend(&self) -> BlendMode {
        self.blend
    }

    /// Effects applied to the clip, in application order.
    #[must_use]
    pub fn effects(&self) -> &[EffectInstance] {
        &self.effects
    }

    /// Looks up an effect by identifier.
    #[must_use]
    pub fn effect(&self, id: EffectId) -> Option<&EffectInstance> {
        self.effects.iter().find(|effect| effect.id == id)
    }

    /// Looks up an effect's position in application order.
    #[must_use]
    pub fn effect_position(&self, id: EffectId) -> Option<usize> {
        self.effects.iter().position(|effect| effect.id == id)
    }

    /// Animated channels of the clip, in insertion order.
    #[must_use]
    pub fn animation(&self) -> &[ChannelAnimation] {
        &self.animation
    }

    /// Mutable access to a channel's animation, for command application.
    pub fn animation_mut(
        &mut self,
        channel: AnimChannel,
    ) -> Result<&mut ChannelAnimation, CoreError> {
        self.animation
            .iter_mut()
            .find(|animation| animation.channel == channel)
            .ok_or_else(|| CoreError::invalid("the clip does not animate that channel"))
    }

    /// Masks applied to the clip, in intersection order.
    #[must_use]
    pub fn masks(&self) -> &[Mask] {
        &self.masks
    }

    /// Looks up a mask by identifier.
    #[must_use]
    pub fn mask(&self, id: MaskId) -> Option<&Mask> {
        self.masks.iter().find(|mask| mask.id == id)
    }

    /// Looks up a mask's position.
    #[must_use]
    pub fn mask_position(&self, id: MaskId) -> Option<usize> {
        self.masks.iter().position(|mask| mask.id == id)
    }
    /// The mapping from timeline frames to source frames.
    ///
    /// Falls back to an identity mapping only if the clip somehow carries a
    /// non-positive speed, which [`crate::Project::validate`] rejects on load. The
    /// fallback keeps this infallible without ever panicking.
    #[must_use]
    pub fn retime(&self) -> RetimeMap {
        let speed = if self.speed.is_positive() {
            self.speed
        } else {
            Rational::from_integer(1)
        };
        RetimeMap::new(self.source_range.start(), self.timeline_start, speed)
            .unwrap_or_else(|_| RetimeMap::identity())
    }

    /// Source frame shown at `timeline_frame`, if the clip covers it.
    #[must_use]
    pub fn source_frame_at(&self, timeline_frame: i64) -> Option<i64> {
        let retime = self.retime();
        let source = retime.source_at(timeline_frame);
        self.source_range.contains(source).then_some(source)
    }

    /// Number of timeline frames this clip needs.
    ///
    /// For a fractional speed this rounds up, so the final timeline frame still has a
    /// source frame to show: 240 frames at `1/2` speed occupies 480 timeline frames,
    /// and 5 frames at `1/2` occupies 10.
    #[must_use]
    pub fn timeline_length(&self) -> i64 {
        let source_len = self.source_range.len();
        if source_len == 0 {
            return 0;
        }
        let numerator = i128::from(self.speed.num());
        let denominator = i128::from(self.speed.den());
        let scaled = i128::from(source_len) * denominator;
        let rounded_up =
            scaled.div_euclid(numerator) + i128::from(scaled.rem_euclid(numerator) != 0);
        i64::try_from(rounded_up).unwrap_or(i64::MAX)
    }

    /// The timeline range the clip occupies, if it fits in the addressable space.
    #[must_use]
    pub fn timeline_range(&self) -> Option<FrameRange> {
        FrameRange::from_start_len(self.timeline_start, self.timeline_length()).ok()
    }

    /// Reports whether the clip covers a timeline frame.
    #[must_use]
    pub fn covers(&self, timeline_frame: i64) -> bool {
        self.timeline_range()
            .is_some_and(|range| range.contains(timeline_frame))
    }
}

/// Whether a track carries picture or sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    /// A video track.
    Video,
    /// An audio track.
    Audio,
}

/// An ordered row of clips, plus its enable, mute and lock state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub(crate) id: TrackId,
    pub(crate) kind: TrackKind,
    pub(crate) name: String,
    pub(crate) enabled: bool,
    pub(crate) locked: bool,
    pub(crate) muted: bool,
    pub(crate) clips: Vec<Clip>,
    pub(crate) transitions: Vec<Transition>,
}

impl Track {
    /// Creates an empty, enabled, unlocked track.
    #[must_use]
    pub fn new(id: TrackId, kind: TrackKind, name: impl Into<String>) -> Self {
        Self {
            id,
            kind,
            name: name.into(),
            enabled: true,
            locked: false,
            muted: false,
            clips: Vec::new(),
            transitions: Vec::new(),
        }
    }

    /// Identifier of this track.
    #[must_use]
    pub const fn id(&self) -> TrackId {
        self.id
    }

    /// Whether this is a video or an audio track.
    #[must_use]
    pub const fn kind(&self) -> TrackKind {
        self.kind
    }

    /// Display name of the track.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Reports whether the track contributes to the output.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Reports whether edits are blocked on this track.
    #[must_use]
    pub const fn is_locked(&self) -> bool {
        self.locked
    }

    /// Reports whether the track is muted.
    #[must_use]
    pub const fn is_muted(&self) -> bool {
        self.muted
    }

    /// Transitions on the track, in insertion order.
    #[must_use]
    pub fn transitions(&self) -> &[Transition] {
        &self.transitions
    }

    /// Looks up a transition by identifier.
    #[must_use]
    pub fn transition(&self, id: TransitionId) -> Option<&Transition> {
        self.transitions
            .iter()
            .find(|transition| transition.id == id)
    }

    /// Looks up a transition's position.
    #[must_use]
    pub fn transition_position(&self, id: TransitionId) -> Option<usize> {
        self.transitions
            .iter()
            .position(|transition| transition.id == id)
    }

    /// Inserts a transition at a position.
    pub fn insert_transition_at(
        &mut self,
        index: usize,
        transition: Transition,
    ) -> Result<(), CoreError> {
        if index > self.transitions.len() {
            return Err(CoreError::index_out_of_range(index, self.transitions.len()));
        }
        self.transitions.insert(index, transition);
        Ok(())
    }

    /// Removes the transition at a position.
    pub fn remove_transition_at(&mut self, index: usize) -> Result<Transition, CoreError> {
        if index >= self.transitions.len() {
            return Err(CoreError::index_out_of_range(index, self.transitions.len()));
        }
        Ok(self.transitions.remove(index))
    }
    /// Clips in insertion order.
    ///
    /// Insertion order is what makes command inverses exact: undoing a removal
    /// restores the clip at the position it occupied. Rendering and queries that need
    /// timeline order should use [`Track::clips_in_timeline_order`].
    #[must_use]
    pub fn clips(&self) -> &[Clip] {
        &self.clips
    }

    /// Number of clips on the track.
    #[must_use]
    pub fn clip_count(&self) -> usize {
        self.clips.len()
    }

    /// Looks up a clip by identifier.
    #[must_use]
    pub fn clip(&self, id: ClipId) -> Option<&Clip> {
        self.clips.iter().find(|clip| clip.id == id)
    }

    /// Looks up a clip's position in insertion order.
    #[must_use]
    pub fn clip_position(&self, id: ClipId) -> Option<usize> {
        self.clips.iter().position(|clip| clip.id == id)
    }

    /// Mutable access to a clip, for command application.
    pub fn clip_mut(&mut self, id: ClipId) -> Result<&mut Clip, CoreError> {
        self.clips
            .iter_mut()
            .find(|clip| clip.id == id)
            .ok_or_else(|| CoreError::not_found(ClipId::KIND, id.raw()))
    }

    /// Inserts a clip at a position in insertion order.
    pub fn insert_clip_at(&mut self, index: usize, clip: Clip) -> Result<(), CoreError> {
        if index > self.clips.len() {
            return Err(CoreError::index_out_of_range(index, self.clips.len()));
        }
        self.clips.insert(index, clip);
        Ok(())
    }

    /// Removes the clip at a position in insertion order.
    pub fn remove_clip_at(&mut self, index: usize) -> Result<Clip, CoreError> {
        if index >= self.clips.len() {
            return Err(CoreError::index_out_of_range(index, self.clips.len()));
        }
        Ok(self.clips.remove(index))
    }

    /// Clips sorted by their timeline start, for rendering and queries.
    #[must_use]
    pub fn clips_in_timeline_order(&self) -> Vec<&Clip> {
        let mut ordered: Vec<&Clip> = self.clips.iter().collect();
        ordered.sort_by_key(|clip| clip.timeline_start);
        ordered
    }

    /// First timeline frame after the last clip on the track.
    ///
    /// Returns `None` for an empty track. Overlapping clips are legal in this model
    /// (overwrite semantics arrive with the editorial operations in M6), so this is
    /// the end of the longest clip, not a tiling boundary.
    #[must_use]
    pub fn end_frame(&self) -> Option<i64> {
        self.clips
            .iter()
            .filter_map(Clip::timeline_range)
            .map(|range| range.end())
            .max()
    }
}

/// A timeline: a timebase, a frame size, and its tracks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sequence {
    pub(crate) id: SequenceId,
    pub(crate) name: String,
    pub(crate) timebase: Timebase,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) sample_rate: u32,
    pub(crate) tracks: Vec<Track>,
}

impl Sequence {
    /// Creates an empty sequence.
    ///
    /// Returns [`CoreError::InvalidArgument`] for a zero frame size or sample rate.
    pub fn new(
        id: SequenceId,
        name: impl Into<String>,
        timebase: Timebase,
        width: u32,
        height: u32,
        sample_rate: u32,
    ) -> Result<Self, CoreError> {
        if width == 0 || height == 0 {
            return Err(CoreError::invalid("sequence frame size must be non-zero"));
        }
        if sample_rate == 0 {
            return Err(CoreError::invalid("sequence sample rate must be non-zero"));
        }
        Ok(Self {
            id,
            name: name.into(),
            timebase,
            width,
            height,
            sample_rate,
            tracks: Vec::new(),
        })
    }

    /// Converts a clock delta while preserving the original value.
    #[must_use]
    pub fn with_timebase(mut self, timebase: Timebase) -> Self {
        self.timebase = timebase;
        self
    }

    /// Identifier of this sequence.
    #[must_use]
    pub const fn id(&self) -> SequenceId {
        self.id
    }

    /// Display name of the sequence.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The sequence timebase: every frame number in this sequence means this rate.
    #[must_use]
    pub const fn timebase(&self) -> Timebase {
        self.timebase
    }

    /// Raster width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Raster height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Audio sample rate.
    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Tracks in order.
    #[must_use]
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// Looks up a track by identifier.
    #[must_use]
    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.iter().find(|track| track.id == id)
    }

    /// Looks up a track's position.
    #[must_use]
    pub fn track_position(&self, id: TrackId) -> Option<usize> {
        self.tracks.iter().position(|track| track.id == id)
    }

    /// A track by position.
    #[must_use]
    pub fn track_at(&self, index: usize) -> Option<&Track> {
        self.tracks.get(index)
    }

    /// A mutable track by position, for command application.
    pub fn track_at_mut(&mut self, index: usize) -> Result<&mut Track, CoreError> {
        let len = self.tracks.len();
        self.tracks
            .get_mut(index)
            .ok_or_else(|| CoreError::index_out_of_range(index, len))
    }

    /// A mutable track by identifier, for command application.
    pub fn track_mut(&mut self, id: TrackId) -> Result<&mut Track, CoreError> {
        self.tracks
            .iter_mut()
            .find(|track| track.id == id)
            .ok_or_else(|| CoreError::not_found(TrackId::KIND, id.raw()))
    }

    /// Inserts a track at a position.
    pub fn insert_track(&mut self, index: usize, track: Track) -> Result<(), CoreError> {
        if index > self.tracks.len() {
            return Err(CoreError::index_out_of_range(index, self.tracks.len()));
        }
        self.tracks.insert(index, track);
        Ok(())
    }

    /// Removes the track at a position.
    pub fn remove_track_at(&mut self, index: usize) -> Result<Track, CoreError> {
        if index >= self.tracks.len() {
            return Err(CoreError::index_out_of_range(index, self.tracks.len()));
        }
        Ok(self.tracks.remove(index))
    }

    /// Looks up a clip anywhere in the sequence.
    #[must_use]
    pub fn clip(&self, id: ClipId) -> Option<&Clip> {
        self.tracks.iter().find_map(|track| track.clip(id))
    }

    /// Mutable access to a clip anywhere in the sequence.
    pub fn clip_mut(&mut self, id: ClipId) -> Result<&mut Clip, CoreError> {
        for track in &mut self.tracks {
            if track.clip_position(id).is_some() {
                return track.clip_mut(id);
            }
        }
        Err(CoreError::not_found(ClipId::KIND, id.raw()))
    }

    /// Finds a clip as `(track index, clip index)` in insertion order.
    #[must_use]
    pub fn locate_clip(&self, id: ClipId) -> Option<(usize, usize)> {
        for (track_index, track) in self.tracks.iter().enumerate() {
            if let Some(clip_index) = track.clip_position(id) {
                return Some((track_index, clip_index));
            }
        }
        None
    }

    /// Total number of clips in the sequence.
    #[must_use]
    pub fn clip_count(&self) -> usize {
        self.tracks.iter().map(Track::clip_count).sum()
    }

    /// Last frame covered by any track, if the sequence is not empty.
    #[must_use]
    pub fn duration_frames(&self) -> Option<i64> {
        self.tracks.iter().filter_map(Track::end_frame).max()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(id: u64, duration: i64) -> MediaAsset {
        MediaAsset::new(
            MediaAssetId::new(id),
            format!("asset{id}"),
            format!("file:///asset{id}.mov"),
            Timebase::FPS_25,
            duration,
        )
        .unwrap()
    }

    fn clip(id: u64, asset_id: u64, source: (i64, i64), start: i64, speed: Rational) -> Clip {
        Clip::new(
            ClipId::new(id),
            MediaAssetId::new(asset_id),
            format!("clip{id}"),
            FrameRange::new(source.0, source.1).unwrap(),
            start,
            speed,
        )
        .unwrap()
    }

    #[test]
    fn a_clip_at_normal_speed_occupies_its_source_length() {
        let c = clip(1, 1, (0, 100), 50, Rational::from_integer(1));
        assert_eq!(c.timeline_length(), 100);
        assert_eq!(c.timeline_range(), FrameRange::new(50, 150).ok());
        assert!(c.covers(50));
        assert!(c.covers(149));
        assert!(!c.covers(150));
    }

    #[test]
    fn slow_motion_occupies_more_timeline_frames() {
        let c = clip(1, 1, (0, 100), 0, Rational::new(1, 2).unwrap());
        assert_eq!(c.timeline_length(), 200, "half speed doubles the length");
    }

    #[test]
    fn fast_motion_occupies_fewer_timeline_frames() {
        let c = clip(1, 1, (0, 100), 0, Rational::from_integer(4));
        assert_eq!(c.timeline_length(), 25);
    }

    #[test]
    fn odd_source_lengths_round_up_at_fractional_speeds() {
        let c = clip(1, 1, (0, 5), 0, Rational::new(1, 2).unwrap());
        assert_eq!(c.timeline_length(), 10);
        let threes = clip(2, 1, (0, 5), 0, Rational::new(1, 3).unwrap());
        assert_eq!(threes.timeline_length(), 15);
        let awkward = clip(3, 1, (0, 7), 0, Rational::new(2, 3).unwrap());
        // 7 source frames at 2/3 speed needs ceil(7 * 3 / 2) = 11 timeline frames.
        assert_eq!(awkward.timeline_length(), 11);
    }

    #[test]
    fn an_empty_source_range_occupies_nothing() {
        let empty = clip(1, 1, (10, 10), 0, Rational::from_integer(1));
        assert_eq!(empty.timeline_length(), 0);
        assert!(empty.timeline_range().is_some_and(|r| r.is_empty()));
    }

    #[test]
    fn source_frame_at_respects_the_trim() {
        let c = clip(1, 1, (100, 200), 0, Rational::from_integer(2));
        assert_eq!(c.source_frame_at(0), Some(100));
        assert_eq!(c.source_frame_at(10), Some(120));
        assert_eq!(c.source_frame_at(49), Some(198));
        assert_eq!(c.source_frame_at(50), None, "past the trim");
    }

    #[test]
    fn clips_of_the_same_asset_are_independent_instances() {
        let a = clip(1, 7, (0, 10), 0, Rational::from_integer(1));
        let b = clip(2, 7, (5, 20), 100, Rational::from_integer(1));
        assert_eq!(a.asset_id(), b.asset_id());
        assert_ne!(a.source_range(), b.source_range());
        assert_ne!(a.timeline_start(), b.timeline_start());
    }

    #[test]
    fn a_clip_rejects_a_non_positive_speed() {
        let err = Clip::new(
            ClipId::new(1),
            MediaAssetId::new(1),
            "bad",
            FrameRange::new(0, 10).unwrap(),
            0,
            Rational::from_integer(0),
        );
        assert!(err.is_err());
    }

    #[test]
    fn tracks_keep_insertion_order_and_offer_timeline_order() {
        let mut track = Track::new(TrackId::new(1), TrackKind::Video, "V1");
        track
            .insert_clip_at(0, clip(1, 1, (0, 10), 300, Rational::from_integer(1)))
            .unwrap();
        track
            .insert_clip_at(1, clip(2, 1, (0, 10), 100, Rational::from_integer(1)))
            .unwrap();

        assert_eq!(track.clips()[0].id(), ClipId::new(1), "insertion order");
        assert_eq!(
            track.clips_in_timeline_order()[0].id(),
            ClipId::new(2),
            "timeline order sorts the earlier clip first"
        );
        assert_eq!(track.end_frame(), Some(310));
    }

    #[test]
    fn insertion_rejects_an_out_of_range_index() {
        let mut track = Track::new(TrackId::new(1), TrackKind::Video, "V1");
        assert_eq!(
            track.insert_clip_at(5, clip(1, 1, (0, 10), 0, Rational::from_integer(1))),
            Err(CoreError::index_out_of_range(5, 0))
        );
    }

    #[test]
    fn removal_returns_the_clip_so_an_undo_can_restore_it() {
        let mut track = Track::new(TrackId::new(1), TrackKind::Video, "V1");
        let original = clip(1, 1, (0, 10), 0, Rational::from_integer(1));
        track.insert_clip_at(0, original.clone()).unwrap();
        let removed = track.remove_clip_at(0).unwrap();
        assert_eq!(removed, original);
        assert_eq!(track.clip_count(), 0);
    }

    #[test]
    fn a_sequence_reports_the_last_frame_of_any_track() {
        let mut sequence = Sequence::new(
            SequenceId::new(1),
            "S",
            Timebase::FPS_25,
            1920,
            1080,
            48_000,
        )
        .unwrap();
        let mut video = Track::new(TrackId::new(1), TrackKind::Video, "V1");
        video
            .insert_clip_at(0, clip(1, 1, (0, 10), 0, Rational::from_integer(1)))
            .unwrap();
        let mut audio = Track::new(TrackId::new(2), TrackKind::Audio, "A1");
        audio
            .insert_clip_at(0, clip(2, 1, (0, 10), 0, Rational::from_integer(1)))
            .unwrap();
        sequence.insert_track(0, video).unwrap();
        sequence.insert_track(1, audio).unwrap();

        assert_eq!(sequence.duration_frames(), Some(10));
        assert_eq!(sequence.clip_count(), 2);
        assert_eq!(
            sequence.locate_clip(ClipId::new(2)),
            Some((1, 0)),
            "locate_clip reports track and clip position"
        );
    }

    #[test]
    fn a_sequence_rejects_degenerate_settings() {
        assert!(Sequence::new(SequenceId::new(1), "S", Timebase::FPS_25, 0, 1080, 48_000).is_err());
        assert!(Sequence::new(SequenceId::new(1), "S", Timebase::FPS_25, 1920, 0, 48_000).is_err());
        assert!(Sequence::new(SequenceId::new(1), "S", Timebase::FPS_25, 1920, 1080, 0).is_err());
    }

    #[test]
    fn media_rejects_a_negative_duration_and_reports_its_range() {
        assert!(
            MediaAsset::new(
                MediaAssetId::new(1),
                "bad",
                "file:///bad",
                Timebase::FPS_25,
                -1
            )
            .is_err()
        );
        let ok = asset(1, 500);
        assert_eq!(ok.full_range(), FrameRange::new(0, 500).unwrap());
        assert_eq!(ok.duration_frames(), 500);
        let silent = ok.clone().with_streams(true, false);
        assert!(silent.has_video());
        assert!(!silent.has_audio());
    }
}
