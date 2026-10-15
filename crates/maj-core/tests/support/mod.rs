//! Shared helpers for the `maj-core` integration gates.
//!
//! `dead_code` is allowed here because each integration test binary compiles this
//! module separately, so helpers used by one binary look unused to the other.
#![allow(dead_code)]

use maj_core::{
    Clip, ClipId, Command, CoreError, History, MediaAsset, MediaAssetId, Project, Sequence,
    SequenceId, Track, TrackId, TrackKind,
};
use maj_time::testing::DeterministicRng;
use maj_time::{FrameRange, Rational, Timebase};

/// Frames in every generated media asset. Small enough to keep the fuzz cheap, large
/// enough that random trims are interesting.
pub(crate) const MEDIA_FRAMES: i64 = 2_000;

/// How many commands a fuzz run applies when built for tests.
///
/// The M1 gate asks for a ten-thousand-command run. Debug builds are an order of
/// magnitude slower and `validate` is called per step, so the debug budget is smaller
/// and the full budget is exercised under `--release`. Both numbers are real runs, not
/// estimates.
pub(crate) fn command_budget() -> usize {
    if cfg!(debug_assertions) {
        2_000
    } else {
        10_000
    }
}

/// How many commands a state-trace run applies. These hash on every step, which is
/// quadratic in the project size by nature.
pub(crate) fn trace_budget() -> usize {
    if cfg!(debug_assertions) { 400 } else { 2_000 }
}

/// A deterministic project builder driven by a seeded PRNG.
///
/// Nothing here consults the clock, the filesystem, the environment or a hash map, so
/// the same seed produces the same project on every machine — which is the property
/// the tests below then check.
pub(crate) struct Harness {
    /// The project under construction.
    pub(crate) project: Project,
    /// Undo and redo stacks for the session.
    pub(crate) history: History,
    /// Seeded generator driving the command choices.
    pub(crate) rng: DeterministicRng,
}

impl Harness {
    /// Creates an empty harness.
    #[must_use]
    pub(crate) fn new(seed: u64) -> Self {
        Self {
            project: Project::new("fuzz"),
            history: History::new(),
            rng: DeterministicRng::new(seed),
        }
    }

    /// The next identifier this project has never used.
    #[must_use]
    pub(crate) fn next_id(&self) -> u64 {
        self.project.next_unused_id()
    }

    /// Creates the minimum a fuzz run needs: media, a sequence, one video and one
    /// audio track. Applied through commands, so the log stays the whole truth.
    pub(crate) fn bootstrap(&mut self) -> Result<(), CoreError> {
        let asset = MediaAsset::new(
            MediaAssetId::new(self.next_id()),
            "media.mov",
            "file:///media.mov",
            Timebase::FPS_25,
            MEDIA_FRAMES,
        )?;
        self.history.apply_local(
            &mut self.project,
            Command::CreateMediaAsset { index: 0, asset },
        )?;

        let sequence = Sequence::new(
            SequenceId::new(self.next_id()),
            "S",
            Timebase::FPS_25,
            1920,
            1080,
            48_000,
        )?;
        self.history.apply_local(
            &mut self.project,
            Command::CreateSequence { index: 0, sequence },
        )?;

        let owner = self.project.sequences()[0].id();
        for (index, (kind, name)) in [(TrackKind::Video, "V1"), (TrackKind::Audio, "A1")]
            .into_iter()
            .enumerate()
        {
            let track = Track::new(TrackId::new(self.next_id()), kind, name);
            self.history.apply_local(
                &mut self.project,
                Command::CreateTrack {
                    sequence: owner,
                    index,
                    track,
                },
            )?;
        }
        Ok(())
    }

    /// Applies `count` random commands, skipping any whose preconditions fail.
    ///
    /// Returns how many commands were actually applied.
    pub(crate) fn run(&mut self, count: usize) -> Result<usize, CoreError> {
        let mut applied = 0;
        for _ in 0..count {
            if let Some(command) = self.random_command() {
                self.history.apply_local(&mut self.project, command)?;
                applied += 1;
            }
        }
        Ok(applied)
    }

    /// Builds a complete random project.
    pub(crate) fn random_project(seed: u64, count: usize) -> Result<Self, CoreError> {
        let mut harness = Self::new(seed);
        harness.bootstrap()?;
        harness.run(count)?;
        Ok(harness)
    }

    fn sequences(&self) -> Vec<SequenceId> {
        self.project.sequences().iter().map(Sequence::id).collect()
    }

    fn tracks(&self) -> Vec<(SequenceId, TrackId)> {
        let mut all = Vec::new();
        for sequence in self.project.sequences() {
            for track in sequence.tracks() {
                all.push((sequence.id(), track.id()));
            }
        }
        all
    }

    fn clips(&self) -> Vec<(SequenceId, ClipId)> {
        let mut all = Vec::new();
        for sequence in self.project.sequences() {
            for track in sequence.tracks() {
                for clip in track.clips() {
                    all.push((sequence.id(), clip.id()));
                }
            }
        }
        all
    }

    fn assets(&self) -> Vec<MediaAssetId> {
        self.project.assets().iter().map(MediaAsset::id).collect()
    }

    fn pick_sequence(&mut self) -> Option<SequenceId> {
        let all = self.sequences();
        if all.is_empty() {
            None
        } else {
            Some(*self.rng.pick(&all))
        }
    }

    fn pick_track(&mut self) -> Option<(SequenceId, TrackId)> {
        let all = self.tracks();
        if all.is_empty() {
            None
        } else {
            Some(*self.rng.pick(&all))
        }
    }

    fn pick_clip(&mut self) -> Option<(SequenceId, ClipId)> {
        let all = self.clips();
        if all.is_empty() {
            None
        } else {
            Some(*self.rng.pick(&all))
        }
    }

    fn pick_asset(&mut self) -> Option<MediaAssetId> {
        let all = self.assets();
        if all.is_empty() {
            None
        } else {
            Some(*self.rng.pick(&all))
        }
    }

    /// Picks a source range that is guaranteed to sit inside the clip's media.
    fn random_range_inside(&mut self, asset: MediaAssetId) -> Option<FrameRange> {
        let duration = self.project.asset(asset)?.duration_frames();
        if duration <= 0 {
            return None;
        }
        let start = self.rng.next_i64_in(0, duration);
        let remaining = duration - start;
        let length = if remaining > 0 {
            self.rng.next_i64_in(0, remaining + 1)
        } else {
            0
        };
        FrameRange::from_start_len(start, length).ok()
    }

    /// Produces one random command, or `None` when the state cannot support any.
    pub(crate) fn random_command(&mut self) -> Option<Command> {
        match self.rng.next_u32_in(0, 100) {
            0..=39 => self.append_clip(),
            40..=54 => self.trim_clip(),
            55..=64 => self.move_clip(),
            65..=74 => self.set_speed(),
            75..=79 => self.toggle_clip(),
            80..=87 => self.create_track(),
            88..=95 => self.remove_clip(),
            _ => self.create_asset(),
        }
    }

    fn append_clip(&mut self) -> Option<Command> {
        let (sequence, track) = self.pick_track()?;
        let asset = self.pick_asset()?;
        let source_range = self.random_range_inside(asset)?;
        let timeline_start = self.rng.next_i64_in(-2_000, 20_000);
        let speed = self.random_speed();
        let clip = Clip::new(
            ClipId::new(self.next_id()),
            asset,
            format!("clip{}", self.next_id()),
            source_range,
            timeline_start,
            speed,
        )
        .ok()?;
        Some(Command::AppendClip {
            sequence,
            track,
            clip,
        })
    }

    fn trim_clip(&mut self) -> Option<Command> {
        let (sequence, clip) = self.pick_clip()?;
        let asset = self.project.clip(clip)?.asset_id();
        let source_range = self.random_range_inside(asset)?;
        let timeline_start = self.rng.next_i64_in(-2_000, 20_000);
        Some(Command::TrimClip {
            sequence,
            clip,
            source_range,
            timeline_start,
        })
    }

    fn move_clip(&mut self) -> Option<Command> {
        let (sequence, clip) = self.pick_clip()?;
        let timeline_start = self.rng.next_i64_in(-5_000, 50_000);
        Some(Command::MoveClip {
            sequence,
            clip,
            timeline_start,
        })
    }

    fn set_speed(&mut self) -> Option<Command> {
        let (sequence, clip) = self.pick_clip()?;
        let speed = self.random_speed();
        Some(Command::SetClipSpeed {
            sequence,
            clip,
            speed,
        })
    }

    fn toggle_clip(&mut self) -> Option<Command> {
        let (sequence, clip) = self.pick_clip()?;
        let enabled = self.rng.next_bool();
        Some(Command::SetClipEnabled {
            sequence,
            clip,
            enabled,
        })
    }

    fn create_track(&mut self) -> Option<Command> {
        let sequence = self.pick_sequence()?;
        let len = self.project.sequence(sequence)?.tracks().len();
        let index = usize::try_from(self.rng.next_u32_in(0, u32::try_from(len + 1).ok()?)).ok()?;
        let kind = if self.rng.next_bool() {
            TrackKind::Video
        } else {
            TrackKind::Audio
        };
        let track = Track::new(TrackId::new(self.next_id()), kind, format!("T{index}"));
        Some(Command::CreateTrack {
            sequence,
            index,
            track,
        })
    }

    fn remove_clip(&mut self) -> Option<Command> {
        let (sequence, clip) = self.pick_clip()?;
        Some(Command::RemoveClip { sequence, clip })
    }

    fn create_asset(&mut self) -> Option<Command> {
        let duration = self.rng.next_i64_in(1, 5_000);
        let index = self
            .rng
            .next_u32_in(0, u32::try_from(self.assets().len() + 1).ok()?);
        let asset = MediaAsset::new(
            MediaAssetId::new(self.next_id()),
            format!("media{duration}.mov"),
            format!("file:///media{duration}.mov"),
            Timebase::FPS_25,
            duration,
        )
        .ok()?;
        Some(Command::CreateMediaAsset {
            index: usize::try_from(index).ok()?,
            asset,
        })
    }

    /// A speed from a small set of awkward ratios, including NTSC-derived ones.
    fn random_speed(&mut self) -> Rational {
        const NUMERATORS: [i64; 6] = [1, 1, 2, 3, 24_000, 60_001];
        const DENOMINATORS: [i64; 6] = [1, 2, 1, 1, 30_000, 60_000];
        let index = self.rng.next_u32_in(0, 6) as usize;
        Rational::new(NUMERATORS[index], DENOMINATORS[index]).unwrap_or(Rational::from_integer(1))
    }
}
