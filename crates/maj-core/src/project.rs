use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::command::LogEntry;
use crate::determinism;
use crate::entity::{Clip, MediaAsset, Sequence};
use crate::error::CoreError;
use crate::ids::{ClipId, MediaAssetId, SequenceId, TrackId};
use crate::ids::{EffectId, MaskId, TransitionId};
use maj_time::Rational;

/// Marker that identifies a file as a Majestat project.
pub const MAJ_MAGIC: &str = "MAJESTAT-PROJECT";

/// Schema version this build writes.
pub const CURRENT_SCHEMA_VERSION: u32 = 3;

/// The authoritative document.
///
/// A project is the model plus the command log that produced it. Nothing else is
/// durable: preview, export, caches, the AI agent and remote collaborators are all
/// consumers of this one structure.
///
/// ## What is deliberately absent
///
/// There is no floating-point value anywhere in this type. Rates, ranges and speeds
/// are integers and [`maj_time::Rational`]s, so the canonical JSON of two equal
/// projects is byte-identical and only one text representation exists to compare.
/// There is also no identifier allocator: see [`crate::IdAllocator`] for why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub(crate) magic: String,
    pub(crate) schema_version: u32,
    pub(crate) name: String,
    pub(crate) assets: Vec<MediaAsset>,
    pub(crate) sequences: Vec<Sequence>,
    pub(crate) log: Vec<LogEntry>,
}

impl Project {
    /// Creates an empty project with the current schema version.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            magic: MAJ_MAGIC.to_owned(),
            schema_version: CURRENT_SCHEMA_VERSION,
            name: name.into(),
            assets: Vec::new(),
            sequences: Vec::new(),
            log: Vec::new(),
        }
    }

    /// The format marker stored in the document.
    #[must_use]
    pub fn magic(&self) -> &str {
        &self.magic
    }

    /// The schema version stored in the document.
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// User-facing project name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Renames the project. Not a logged command: the name is presentation, and it is
    /// excluded from the state hash only in the sense that two different names are
    /// genuinely different states.
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    /// Number of applied commands. Also the revision a reviewer would approve.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.log.len() as u64
    }

    /// Registered media.
    #[must_use]
    pub fn assets(&self) -> &[MediaAsset] {
        &self.assets
    }

    /// Timelines.
    #[must_use]
    pub fn sequences(&self) -> &[Sequence] {
        &self.sequences
    }

    /// The command log, oldest first.
    #[must_use]
    pub fn log(&self) -> &[LogEntry] {
        &self.log
    }

    /// Looks up media by identifier.
    #[must_use]
    pub fn asset(&self, id: MediaAssetId) -> Option<&MediaAsset> {
        self.assets.iter().find(|asset| asset.id == id)
    }

    /// Position of media in registration order.
    #[must_use]
    pub fn asset_position(&self, id: MediaAssetId) -> Option<usize> {
        self.assets.iter().position(|asset| asset.id == id)
    }

    /// Looks up a sequence by identifier.
    #[must_use]
    pub fn sequence(&self, id: SequenceId) -> Option<&Sequence> {
        self.sequences.iter().find(|sequence| sequence.id == id)
    }

    /// Position of a sequence in creation order.
    #[must_use]
    pub fn sequence_position(&self, id: SequenceId) -> Option<usize> {
        self.sequences.iter().position(|sequence| sequence.id == id)
    }

    /// Looks up a clip anywhere in the project.
    ///
    /// Clip identifiers are unique across the whole project, so this is unambiguous.
    #[must_use]
    pub fn clip(&self, id: ClipId) -> Option<&Clip> {
        self.sequences.iter().find_map(|sequence| sequence.clip(id))
    }

    /// Finds a clip as `(sequence, track, clip)` positions.
    #[must_use]
    pub fn locate_clip(&self, id: ClipId) -> Option<(SequenceId, usize, usize)> {
        for sequence in &self.sequences {
            if let Some((track, clip)) = sequence.locate_clip(id) {
                return Some((sequence.id, track, clip));
            }
        }
        None
    }

    /// The first identifier this project has never used.
    ///
    /// Derived, never stored, so replaying a log always yields the same answer. See
    /// [`crate::IdAllocator`].
    #[must_use]
    pub fn next_unused_id(&self) -> u64 {
        let mut highest = 0;
        for asset in &self.assets {
            highest = highest.max(asset.id.raw());
        }
        for sequence in &self.sequences {
            highest = highest.max(sequence.id.raw());
            for track in &sequence.tracks {
                highest = highest.max(track.id.raw());
                for transition in &track.transitions {
                    highest = highest.max(transition.id.raw());
                }
                for clip in &track.clips {
                    highest = highest.max(clip.id.raw());
                    for effect in &clip.effects {
                        highest = highest.max(effect.id.raw());
                    }
                    for mask in &clip.masks {
                        highest = highest.max(mask.id.raw());
                    }
                }
            }
        }
        highest.saturating_add(1)
    }

    /// Checks every model invariant.
    ///
    /// Run on load, so a hand-edited or truncated document cannot reach the engine.
    /// The checks, in order: format marker, schema version, unique identifiers per
    /// kind, non-degenerate sequence settings, positive clip speeds, every clip's
    /// source range inside its media, and a contiguous command log.
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.magic != MAJ_MAGIC {
            return Err(CoreError::MagicMismatch {
                found: self.magic.clone(),
            });
        }
        if self.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(CoreError::SchemaUnsupported {
                found: self.schema_version,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }

        let mut assets = BTreeSet::new();
        for asset in &self.assets {
            if !assets.insert(asset.id) {
                return Err(CoreError::duplicate(MediaAssetId::KIND, asset.id.raw()));
            }
            if asset.duration_frames < 0 {
                return Err(CoreError::invalid("media duration cannot be negative"));
            }
        }

        let mut sequences = BTreeSet::new();
        let mut tracks = BTreeSet::new();
        let mut clips = BTreeSet::new();
        for sequence in &self.sequences {
            if !sequences.insert(sequence.id) {
                return Err(CoreError::duplicate(SequenceId::KIND, sequence.id.raw()));
            }
            if sequence.width == 0 || sequence.height == 0 {
                return Err(CoreError::invalid("sequence frame size must be non-zero"));
            }
            if sequence.sample_rate == 0 {
                return Err(CoreError::invalid("sequence sample rate must be non-zero"));
            }
            for track in &sequence.tracks {
                if !tracks.insert(track.id) {
                    return Err(CoreError::duplicate(TrackId::KIND, track.id.raw()));
                }
                let mut transitions = BTreeSet::new();
                for transition in &track.transitions {
                    if !transitions.insert(transition.id) {
                        return Err(CoreError::duplicate(
                            TransitionId::KIND,
                            transition.id.raw(),
                        ));
                    }
                    transition.validate()?;
                    for partner in [transition.from(), transition.to()].into_iter().flatten() {
                        if track.clip(partner).is_none() {
                            return Err(CoreError::not_found(ClipId::KIND, partner.raw()));
                        }
                    }
                }
                for clip in &track.clips {
                    if !clips.insert(clip.id) {
                        return Err(CoreError::duplicate(ClipId::KIND, clip.id.raw()));
                    }
                    clip.transform.validate()?;
                    if clip.opacity.is_negative() || clip.opacity > Rational::from_integer(1) {
                        return Err(CoreError::invalid(
                            "clip opacity must lie between zero and one",
                        ));
                    }
                    let mut effects = BTreeSet::new();
                    for effect in &clip.effects {
                        if !effects.insert(effect.id) {
                            return Err(CoreError::duplicate(EffectId::KIND, effect.id.raw()));
                        }
                        effect.validate()?;
                    }
                    let mut channels = BTreeSet::new();
                    for animation in &clip.animation {
                        if !channels.insert(animation.channel) {
                            return Err(CoreError::invalid(
                                "a clip may animate a channel only once",
                            ));
                        }
                        animation.validate()?;
                    }
                    let mut masks = BTreeSet::new();
                    for mask in &clip.masks {
                        if !masks.insert(mask.id) {
                            return Err(CoreError::duplicate(MaskId::KIND, mask.id.raw()));
                        }
                        mask.validate()?;
                    }
                    if !clip.speed.is_positive() {
                        return Err(CoreError::invalid("clip speed must be greater than zero"));
                    }
                    if clip.source_range.start() < 0 {
                        return Err(CoreError::invalid(
                            "clip source range cannot start before frame zero",
                        ));
                    }
                    let asset = self.asset(clip.asset_id).ok_or_else(|| {
                        CoreError::not_found(MediaAssetId::KIND, clip.asset_id.raw())
                    })?;
                    if clip.source_range.end() > asset.duration_frames {
                        return Err(CoreError::invalid(format!(
                            "clip {} reads to frame {} but its media is {} frames long",
                            clip.id,
                            clip.source_range.end(),
                            asset.duration_frames
                        )));
                    }
                }
            }
        }

        for (index, entry) in self.log.iter().enumerate() {
            if entry.seq != index as u64 {
                return Err(CoreError::invalid(
                    "log entries must be numbered contiguously from zero",
                ));
            }
        }
        Ok(())
    }

    /// Convenience wrapper over [`Project::validate`].
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.validate().is_ok()
    }

    /// Canonical JSON of the *state*, excluding the command log.
    pub fn canonical_state_json(&self) -> Result<String, CoreError> {
        determinism::canonical_state_json(self)
    }

    /// Hash of the state. Two projects with the same content hash identically.
    pub fn state_hash(&self) -> Result<String, CoreError> {
        determinism::state_hash(self)
    }

    /// Hash of the whole document, command log included.
    pub fn document_hash(&self) -> Result<String, CoreError> {
        determinism::document_hash(self)
    }

    /// Hash of the semantic command trace: authors and commands, in order.
    pub fn command_trace_hash(&self) -> Result<String, CoreError> {
        determinism::command_trace_hash(&self.log)
    }

    // ---- crate-internal mutation, only reachable through commands ----

    pub(crate) fn insert_asset(
        &mut self,
        index: usize,
        asset: MediaAsset,
    ) -> Result<(), CoreError> {
        if index > self.assets.len() {
            return Err(CoreError::index_out_of_range(index, self.assets.len()));
        }
        self.assets.insert(index, asset);
        Ok(())
    }

    pub(crate) fn remove_asset_at(&mut self, index: usize) -> Result<MediaAsset, CoreError> {
        if index >= self.assets.len() {
            return Err(CoreError::index_out_of_range(index, self.assets.len()));
        }
        Ok(self.assets.remove(index))
    }

    pub(crate) fn insert_sequence(
        &mut self,
        index: usize,
        sequence: Sequence,
    ) -> Result<(), CoreError> {
        if index > self.sequences.len() {
            return Err(CoreError::index_out_of_range(index, self.sequences.len()));
        }
        self.sequences.insert(index, sequence);
        Ok(())
    }

    pub(crate) fn remove_sequence_at(&mut self, index: usize) -> Result<Sequence, CoreError> {
        if index >= self.sequences.len() {
            return Err(CoreError::index_out_of_range(index, self.sequences.len()));
        }
        Ok(self.sequences.remove(index))
    }

    pub(crate) fn sequence_mut(&mut self, id: SequenceId) -> Result<&mut Sequence, CoreError> {
        self.sequences
            .iter_mut()
            .find(|sequence| sequence.id == id)
            .ok_or_else(|| CoreError::not_found(SequenceId::KIND, id.raw()))
    }

    pub(crate) fn asset_mut(&mut self, id: MediaAssetId) -> Result<&mut MediaAsset, CoreError> {
        self.assets
            .iter_mut()
            .find(|asset| asset.id == id)
            .ok_or_else(|| CoreError::not_found(MediaAssetId::KIND, id.raw()))
    }

    /// Rejects a clip whose trim falls outside its media.
    pub(crate) fn ensure_clip_source_within_media(&self, clip: &Clip) -> Result<(), CoreError> {
        let asset = self
            .asset(clip.asset_id)
            .ok_or_else(|| CoreError::not_found(MediaAssetId::KIND, clip.asset_id.raw()))?;
        if clip.source_range.start() < 0 {
            return Err(CoreError::invalid(
                "clip source range cannot start before frame zero",
            ));
        }
        if clip.source_range.end() > asset.duration_frames {
            return Err(CoreError::invalid(format!(
                "clip {} reads to frame {} but its media is {} frames long",
                clip.id,
                clip.source_range.end(),
                asset.duration_frames
            )));
        }
        if !clip.speed.is_positive() {
            return Err(CoreError::invalid("clip speed must be greater than zero"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::IdAllocator;
    use maj_time::{FrameRange, Rational, Timebase};

    fn sample_project() -> Project {
        let mut project = Project::new("sample");
        let mut ids = IdAllocator::for_project(&project);

        let asset_id = ids.media_asset();
        project
            .insert_asset(
                0,
                MediaAsset::new(asset_id, "a.mov", "file:///a.mov", Timebase::FPS_25, 100).unwrap(),
            )
            .unwrap();

        let sequence_id = ids.sequence();
        let mut sequence =
            Sequence::new(sequence_id, "S", Timebase::FPS_25, 1920, 1080, 48_000).unwrap();
        let track_id = ids.track();
        let mut track = crate::entity::Track::new(track_id, crate::entity::TrackKind::Video, "V1");
        track
            .insert_clip_at(
                0,
                Clip::new(
                    ids.clip(),
                    asset_id,
                    "c",
                    FrameRange::new(0, 100).unwrap(),
                    0,
                    Rational::from_integer(1),
                )
                .unwrap(),
            )
            .unwrap();
        sequence.insert_track(0, track).unwrap();
        project.insert_sequence(0, sequence).unwrap();
        project
    }

    #[test]
    fn a_fresh_project_is_valid_and_empty() {
        let project = Project::new("p");
        assert_eq!(project.magic(), MAJ_MAGIC);
        assert_eq!(project.schema_version(), CURRENT_SCHEMA_VERSION);
        assert_eq!(project.revision(), 0);
        assert_eq!(project.assets().len(), 0);
        assert!(project.is_valid());
    }

    #[test]
    fn the_next_identifier_follows_the_highest_in_use() {
        let project = sample_project();
        assert_eq!(project.next_unused_id(), 5, "track 3 and clip 4 are in use");
    }

    #[test]
    fn renaming_changes_the_state() {
        let mut project = Project::new("first");
        let before = project.state_hash().unwrap();
        project.set_name("second");
        assert_ne!(before, project.state_hash().unwrap());
    }

    #[test]
    fn validation_rejects_a_foreign_magic_string() {
        let mut project = Project::new("p");
        project.magic = "SOMETHING-ELSE".into();
        assert!(matches!(
            project.validate(),
            Err(CoreError::MagicMismatch { .. })
        ));
    }

    #[test]
    fn validation_rejects_a_newer_schema() {
        let mut project = Project::new("p");
        project.schema_version = CURRENT_SCHEMA_VERSION + 1;
        assert!(matches!(
            project.validate(),
            Err(CoreError::SchemaUnsupported { .. })
        ));
    }

    #[test]
    fn validation_rejects_duplicate_identifiers() {
        let mut project = sample_project();
        let duplicate = project.assets[0].clone();
        project.assets.push(duplicate);
        assert!(matches!(
            project.validate(),
            Err(CoreError::Duplicate { .. })
        ));
    }

    #[test]
    fn validation_rejects_a_clip_that_reads_past_its_media() {
        let mut project = sample_project();
        project.sequences[0].tracks[0].clips[0].source_range = FrameRange::new(0, 101).unwrap();
        let err = project.validate().unwrap_err();
        assert!(
            err.to_string().contains("reads to frame 101"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validation_rejects_a_non_positive_clip_speed() {
        let mut project = sample_project();
        project.sequences[0].tracks[0].clips[0].speed = Rational::from_integer(0);
        assert!(project.validate().is_err());
    }

    #[test]
    fn validation_rejects_a_non_contiguous_log() {
        let mut project = Project::new("p");
        project.log.push(crate::command::LogEntry::new(
            7,
            "local",
            crate::command::Command::RemoveMediaAsset {
                id: MediaAssetId::new(1),
            },
        ));
        let err = project.validate().unwrap_err();
        assert!(
            err.to_string().contains("contiguously"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn validation_rejects_a_dangling_clip_asset_reference() {
        let mut project = sample_project();
        project.sequences[0].tracks[0].clips[0].asset_id = MediaAssetId::new(999);
        assert!(matches!(
            project.validate(),
            Err(CoreError::NotFound { .. })
        ));
    }

    #[test]
    fn clip_lookup_crosses_sequence_and_track_boundaries() {
        let project = sample_project();
        let id = project.sequences[0].tracks[0].clips[0].id;
        assert!(project.clip(id).is_some());
        assert_eq!(
            project.locate_clip(id).map(|(s, t, c)| (s.raw(), t, c)),
            Some((2, 0, 0))
        );
        assert!(project.clip(ClipId::new(999)).is_none());
    }
}
