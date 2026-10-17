//! The synthetic validation corpus: the files, and what each one is expected to probe as.
//!
//! `testdata/media-corpus/README.md` describes a corpus and commits none of it, because real
//! media runs to hundreds of gigabytes and carries its own distribution terms. It also states
//! the preference that settles how this module exists at all:
//!
//! > Prefer synthetic files generated from a script over downloads: they are reproducible,
//! > they carry no third-party terms, and their expected decode output is known exactly.
//!
//! So the corpus is **generated**, by this module, which is committed; the files it writes are
//! not. Every file is a few kilobytes, every byte of it is determined by the code here, and
//! every one carries its generation command and its expected reading in
//! [`CorpusFile::command`] and [`CorpusFile::expect`].
//!
//! # The limit of a synthetic corpus, said plainly
//!
//! A corpus written by this workspace and read by this workspace cannot catch a
//! misunderstanding both halves share. If this module writes a `fiel` box the way this build
//! reads one, the pair agree and a real muxer may still disagree with both. That is a real
//! limit and no amount of care inside it removes it.
//!
//! Three things make it worth having anyway, and one retires it:
//!
//! * The **pathologies are not symmetric**. A truncated file, a zero-length file, a file whose
//!   extension lies and a checksum that fails are hostile inputs whatever wrote them, and
//!   "zero crashes over the corpus" is a claim about this build's robustness that holds
//!   regardless of who was right about `fiel`.
//! * The layouts are **written from the published specifications**, not from this build's
//!   parsers: the box orders, the fixed-point matrices, the run-length sample tables and the
//!   EBML widths are what the formats document, and several of the readers were corrected
//!   against the writers rather than the other way round.
//! * The **expectation is declarative**. [`CorpusFile::check`] compares a probe against a
//!   stated reading, so a regression shows up as a named difference rather than as an
//!   absence of complaints.
//! * It is retired by **real files**. The hundred-file gate M2 actually has to pass is real
//!   media from real cameras and real muxers; this corpus is what makes the first twenty-six
//!   of those files arrive on a build that already handles twenty-six, instead of on one that
//!   has never seen a container.

mod mux;

use std::io;
use std::path::{Path, PathBuf};

use maj_time::{Rational, Timebase};

use crate::container::ContainerKind;
use crate::error::MediaError;
use crate::picture::{BitDepth, PlaneLayout, SampleRange};
use crate::probe::MediaProbe;
use crate::stream::{FieldOrder, FrameRateKind, Rotation};

mod files;

/// An axis of the corpus, as `testdata/media-corpus/README.md` lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Axis {
    /// A frame rate, constant.
    Rate,
    /// A variable frame rate.
    VariableRate,
    /// Bits per sample.
    BitDepth,
    /// Chroma subsampling.
    Chroma,
    /// A declared field order.
    Interlacing,
    /// A display matrix that turns the picture.
    Rotation,
    /// Full or limited code range.
    Range,
    /// Declared primaries, transfer and matrix.
    Colour,
    /// A non-square sample aspect.
    SampleAspect,
    /// Sound: channels, rates and depths.
    Audio,
    /// A container this build has a parser for.
    Container,
    /// A stream that is present, counted and not read.
    OtherStream,
    /// A file that is wrong in a way a prober has to survive.
    Pathology,
    /// A duration far longer than a test file's.
    LongDuration,
}

impl Axis {
    /// Every axis, in a stable order.
    pub const ALL: [Self; 14] = [
        Self::Rate,
        Self::VariableRate,
        Self::BitDepth,
        Self::Chroma,
        Self::Interlacing,
        Self::Rotation,
        Self::Range,
        Self::Colour,
        Self::SampleAspect,
        Self::Audio,
        Self::Container,
        Self::OtherStream,
        Self::Pathology,
        Self::LongDuration,
    ];

    /// The short stable name.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Rate => "rate",
            Self::VariableRate => "variable-rate",
            Self::BitDepth => "bit-depth",
            Self::Chroma => "chroma",
            Self::Interlacing => "interlacing",
            Self::Rotation => "rotation",
            Self::Range => "range",
            Self::Colour => "colour",
            Self::SampleAspect => "sample-aspect",
            Self::Audio => "audio",
            Self::Container => "container",
            Self::OtherStream => "other-stream",
            Self::Pathology => "pathology",
            Self::LongDuration => "long-duration",
        }
    }
}

/// What the corpus deliberately does **not** reach, and what would.
///
/// Stated in the code rather than in prose somewhere, so a report of the gate can print it and
/// nobody has to take the generator's coverage on trust. Each entry is an axis of
/// `testdata/media-corpus/README.md` and the reason synthesis cannot stand in for it.
pub const NOT_COVERED: [(&str, &str); 8] = [
    (
        "coded pictures",
        "every file's sample data is filler, not a coded picture, so no codec's bitstream is \
         exercised beyond the parameter set that states chroma and depth",
    ),
    (
        "codecs",
        "H.264, HEVC and AV1 appear only as configuration records; ProRes, DNxHR and an \
         intra-frame camera format appear only as four-character codes, and AV1 and VP9 have \
         no real stream here at all",
    ),
    (
        "camera RAW",
        "BRAW, R3D and stills RAW are proprietary layouts with no published synthesis path; \
         they have to be real files with their provenance recorded",
    ),
    (
        "HDR and log",
        "HLG and PQ are present as declared code points only. Real HLG, PQ and two \
         manufacturers' log curves need real footage, because what is being tested is the \
         transfer applied to real samples",
    ),
    (
        "MXF",
        "recognised by signature and refused; there is no MXF parser in this build, so a \
         synthetic MXF would only test the refusal",
    ),
    (
        "genuinely damaged media",
        "the pathologies here are damage this module caused on purpose. A file a camera \
         corrupted is damaged in ways nobody designs",
    ),
    (
        "image sequences",
        "`ContainerKind::ImageSequence` has no probe path yet: a numbered run of stills is a \
         stream assembled from many files, which is a different question from what one file is",
    ),
    (
        "surround and non-integer rate pairs",
        "5.1 and 44.1 kHz are declared in headers here; mixing them, and resampling between \
         rates that do not divide, is M9's and is where the arithmetic matters",
    ),
];

/// A rate, as the generator states it and the probe must report it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateExpectation {
    /// One exact rate.
    Constant(Timebase),
    /// Per-frame presentation times, exactly.
    Variable(Vec<i64>),
    /// Variable, with the timestamps in a body this build does not read.
    VariableUnread,
}

impl RateExpectation {
    fn matches(&self, found: &FrameRateKind) -> bool {
        match (self, found) {
            (Self::Constant(expected), FrameRateKind::Constant(found)) => expected == found,
            (Self::Variable(expected), FrameRateKind::Variable { timestamps }) => {
                expected == timestamps
            }
            (Self::VariableUnread, FrameRateKind::Variable { timestamps }) => timestamps.is_empty(),
            _ => false,
        }
    }
}

/// What a file's picture stream is expected to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoExpectation {
    /// Coded size, before rotation and aspect.
    pub coded_size: (u32, u32),
    /// Size as it should be displayed.
    pub display_size: (u32, u32),
    /// Quarter turns.
    pub rotation: Rotation,
    /// Declared field order.
    pub field_order: FieldOrder,
    /// Constant or variable, and the exact times when variable.
    pub rate: RateExpectation,
    /// Plane arrangement and subsampling.
    pub layout: PlaneLayout,
    /// Bits per sample.
    pub depth: BitDepth,
    /// Declared code range.
    pub range: SampleRange,
    /// Pixel aspect, exactly.
    pub sample_aspect: Rational,
    /// Frames, where the container states a count.
    pub frames: Option<u64>,
}

/// What a file's sound stream is expected to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioExpectation {
    /// Channel count.
    pub channels: u16,
    /// Samples per second.
    pub sample_rate: u32,
    /// Bits per sample, where the container states it.
    pub sample_bits: Option<u8>,
}

/// What a probe of a corpus file is expected to answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expectation {
    /// The file is readable, and these are its facts.
    Readable {
        /// The container the bytes turn out to be, whatever the extension claims.
        container: ContainerKind,
        /// How many streams are declared, read or merely counted.
        streams: usize,
        /// The first picture stream, when there is one.
        video: Option<VideoExpectation>,
        /// The first sound stream, when there is one.
        audio: Option<AudioExpectation>,
    },
    /// The probe refuses, with this label: `offline`, `damaged` or `undecodable`.
    Refused(&'static str),
}

/// One file of the corpus: its name, its bytes, how it was made and what it should probe as.
#[derive(Debug, Clone)]
pub struct CorpusFile {
    /// The file's name, which encodes what it is for.
    pub name: &'static str,
    /// One line about what this file exercises.
    pub purpose: &'static str,
    /// The axes of the corpus this file covers.
    pub axes: &'static [Axis],
    /// The command that regenerates this exact file, recorded beside it as the README asks.
    pub command: String,
    /// The bytes.
    pub bytes: Vec<u8>,
    /// What a probe of it must answer.
    pub expect: Expectation,
}

impl CorpusFile {
    /// Compares a probe of this file against what the generator declared.
    ///
    /// # Errors
    ///
    /// A sentence naming the first difference, which is what a failing gate should print. The
    /// comparison is deliberately total: every stated fact is checked, so a probe that stops
    /// reading one of them is a failure rather than a silence.
    pub fn check(&self, found: &Result<MediaProbe, MediaError>) -> Result<(), String> {
        match (&self.expect, found) {
            (Expectation::Refused(label), Err(error)) => {
                if error.label() == *label {
                    Ok(())
                } else {
                    Err(format!(
                        "{}: expected {label}, got {} ({})",
                        self.name,
                        error.label(),
                        error.reason()
                    ))
                }
            }
            (Expectation::Refused(label), Ok(probe)) => Err(format!(
                "{}: expected {label}, but it probed as {} with {} streams",
                self.name,
                probe.container.key(),
                probe.streams.len()
            )),
            (Expectation::Readable { .. }, Err(error)) => Err(format!(
                "{}: expected a readable file, got {} ({})",
                self.name,
                error.label(),
                error.reason()
            )),
            (
                Expectation::Readable {
                    container,
                    streams,
                    video,
                    audio,
                },
                Ok(probe),
            ) => {
                if probe.container != *container {
                    return Err(format!(
                        "{}: expected container {}, got {}",
                        self.name,
                        container.key(),
                        probe.container.key()
                    ));
                }
                if probe.streams.len() != *streams {
                    return Err(format!(
                        "{}: expected {streams} streams, got {}",
                        self.name,
                        probe.streams.len()
                    ));
                }
                match (video, probe.primary_video()) {
                    (None, Some(found)) => {
                        return Err(format!(
                            "{}: expected no picture stream, found one of {:?}",
                            self.name, found.coded_size
                        ));
                    }
                    (Some(_), None) => {
                        return Err(format!(
                            "{}: expected a picture stream, found none",
                            self.name
                        ));
                    }
                    (None, None) => {}
                    (Some(expected), Some(found)) => {
                        let mut wrong = Vec::new();
                        if found.coded_size != expected.coded_size {
                            wrong.push(format!(
                                "coded size {:?} not {:?}",
                                found.coded_size, expected.coded_size
                            ));
                        }
                        if found.display_size() != expected.display_size {
                            wrong.push(format!(
                                "display size {:?} not {:?}",
                                found.display_size(),
                                expected.display_size
                            ));
                        }
                        if found.rotation != expected.rotation {
                            wrong.push(format!(
                                "rotation {} not {}",
                                found.rotation.degrees(),
                                expected.rotation.degrees()
                            ));
                        }
                        if found.field_order != expected.field_order {
                            wrong.push(format!(
                                "field order {} not {}",
                                found.field_order.key(),
                                expected.field_order.key()
                            ));
                        }
                        if !expected.rate.matches(&found.rate) {
                            wrong.push(format!("rate {:?} not {:?}", found.rate, expected.rate));
                        }
                        if found.picture.layout != expected.layout {
                            wrong.push(format!(
                                "layout {} not {}",
                                found.picture.layout.key(),
                                expected.layout.key()
                            ));
                        }
                        if found.picture.depth != expected.depth {
                            wrong.push(format!(
                                "depth {} not {}",
                                found.picture.depth.bits(),
                                expected.depth.bits()
                            ));
                        }
                        if found.picture.range != expected.range {
                            wrong.push(format!(
                                "range {} not {}",
                                found.picture.range.key(),
                                expected.range.key()
                            ));
                        }
                        if found.sample_aspect != expected.sample_aspect {
                            wrong.push(format!(
                                "sample aspect {}:{} not {}:{}",
                                found.sample_aspect.num(),
                                found.sample_aspect.den(),
                                expected.sample_aspect.num(),
                                expected.sample_aspect.den()
                            ));
                        }
                        if found.frames != expected.frames {
                            wrong.push(format!(
                                "frames {:?} not {:?}",
                                found.frames, expected.frames
                            ));
                        }
                        if !wrong.is_empty() {
                            return Err(format!("{}: {}", self.name, wrong.join("; ")));
                        }
                    }
                }
                match (audio, probe.audio_streams().next()) {
                    (None, Some(found)) => Err(format!(
                        "{}: expected no sound stream, found one of {} channels",
                        self.name, found.channels
                    )),
                    (Some(_), None) => Err(format!(
                        "{}: expected a sound stream, found none",
                        self.name
                    )),
                    (None, None) => Ok(()),
                    (Some(expected), Some(found)) => {
                        if found.channels != expected.channels
                            || found.sample_rate != expected.sample_rate
                            || found.sample_bits != expected.sample_bits
                        {
                            return Err(format!(
                                "{}: sound is {} channels at {} hertz, {:?} bits, not {} at {}, {:?}",
                                self.name,
                                found.channels,
                                found.sample_rate,
                                found.sample_bits,
                                expected.channels,
                                expected.sample_rate,
                                expected.sample_bits
                            ));
                        }
                        Ok(())
                    }
                }
            }
        }
    }
}

/// Every file of the corpus, built in memory.
#[must_use]
pub fn files() -> Vec<CorpusFile> {
    files::all()
}

/// The axes the corpus covers, deduplicated and in a stable order.
#[must_use]
pub fn covered_axes() -> Vec<Axis> {
    let mut covered: Vec<Axis> = files()
        .iter()
        .flat_map(|file| file.axes.iter().copied())
        .collect();
    covered.sort_unstable();
    covered.dedup();
    covered
}

/// Where the corpus lives, relative to the workspace root.
pub const DIRECTORY: &str = "testdata/media-corpus/media";

/// Writes every file into a directory, with a manifest beside them.
///
/// The manifest records each file's generation command next to it, which is what
/// `testdata/media-corpus/README.md` asks for, and the directory it writes into is the one
/// that README nominates and `.gitignore` ignores.
///
/// # Errors
///
/// Whatever the filesystem reports.
pub fn write_all(directory: &Path) -> io::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(directory)?;
    let corpus = files();
    let mut written = Vec::with_capacity(corpus.len());
    let mut manifest = String::from(
        "# Synthetic media corpus\n\n\
         Generated, not committed. Regenerate the whole directory with:\n\n\
         ```sh\ncargo run -p maj-media --example make-corpus\n```\n\n\
         Every file below is written byte by byte by `maj_media::corpus`, which is committed, \
         and every one carries the command that regenerates it alone and the reading a probe \
         of it must produce. See that module's documentation for what a synthetic corpus \
         cannot stand in for.\n\n\
         | File | Bytes | Axes | Expected | Generated by |\n|---|---|---|---|---|\n",
    );
    for file in &corpus {
        let path = directory.join(file.name);
        std::fs::write(&path, &file.bytes)?;
        let axes = file
            .axes
            .iter()
            .map(|axis| axis.key())
            .collect::<Vec<_>>()
            .join(", ");
        let expected = match &file.expect {
            Expectation::Refused(label) => format!("refused: {label}"),
            Expectation::Readable { container, .. } => format!("readable: {}", container.key()),
        };
        manifest.push_str(&format!(
            "| `{}` | {} | {axes} | {expected} | `{}` |\n",
            file.name,
            file.bytes.len(),
            file.command
        ));
        written.push(path);
    }
    manifest.push_str("\n## Axes this corpus does not reach\n\n");
    for (axis, why) in NOT_COVERED {
        manifest.push_str(&format!("* **{axis}** — {why}\n"));
    }
    std::fs::write(directory.join("MANIFEST.md"), manifest)?;
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn the_corpus_is_at_least_the_twenty_files_the_phase_asked_for() {
        assert!(
            files().len() >= 20,
            "the corpus has {} files",
            files().len()
        );
    }

    #[test]
    fn every_file_has_a_distinct_name_and_a_generation_command() {
        let names: BTreeSet<&str> = files().iter().map(|file| file.name).collect();
        assert_eq!(names.len(), files().len(), "two files share a name");
        for file in files() {
            assert!(!file.bytes.is_empty() || file.name.contains("zero-length"));
            assert!(
                file.command.contains("make-corpus"),
                "{} has no generation command",
                file.name
            );
            assert!(!file.purpose.is_empty(), "{} says nothing", file.name);
            assert!(!file.axes.is_empty(), "{} covers no axis", file.name);
        }
    }

    #[test]
    fn every_axis_is_covered_by_at_least_one_file() {
        let covered = covered_axes();
        for axis in Axis::ALL {
            assert!(
                covered.contains(&axis),
                "no file covers the {} axis",
                axis.key()
            );
        }
    }

    #[test]
    fn the_corpus_is_byte_for_byte_reproducible() {
        for (first, second) in files().iter().zip(files().iter()) {
            assert_eq!(first.name, second.name);
            assert_eq!(
                first.bytes, second.bytes,
                "{} is not reproducible",
                first.name
            );
        }
    }

    #[test]
    fn a_stated_expectation_can_fail_as_well_as_pass() {
        let file = files()
            .into_iter()
            .find(|file| matches!(file.expect, Expectation::Readable { .. }))
            .expect("a readable file");
        let wrong = file.check(&Err(MediaError::Io("gone".to_owned())));
        assert!(wrong.is_err(), "a readable file must not accept an error");

        let refused = files()
            .into_iter()
            .find(|file| matches!(file.expect, Expectation::Refused(_)))
            .expect("a refused file");
        assert!(
            refused
                .check(&Err(MediaError::Unsupported("wrong label".to_owned())))
                .is_err()
                || matches!(refused.expect, Expectation::Refused("undecodable")),
            "a refusal must check its label"
        );
    }

    #[test]
    fn what_the_corpus_does_not_reach_is_stated_rather_than_implied() {
        for (axis, why) in NOT_COVERED {
            assert!(!axis.is_empty());
            assert!(why.len() > 40, "{axis} is dismissed rather than explained");
        }
    }
}
