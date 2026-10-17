//! What a container says it holds: the streams, and the four facts about a picture stream
//! that cannot be got right unless they are in the model.
//!
//! Three of M2's gate axes live in these types rather than in a parser, on purpose. The gate
//! is "variable frame rate, interlacing and rotation handled correctly", and a thing that is
//! not in the model cannot be handled correctly:
//!
//! * [`FrameRateKind::Variable`] is a **different kind of stream**, not a flag on one, and it
//!   carries its per-frame presentation times as exact integers — so the index-to-timestamp
//!   map is a lookup rather than a multiplication that is wrong for every frame after the
//!   first.
//! * [`FieldOrder`] is declared, so a renderer can refuse or de-interlace deliberately.
//!   Silently treating a field as a frame is the failure mode this prevents.
//! * [`Rotation`] is a quarter-turn count, not degrees. Four cases, exact, no angle
//!   arithmetic, and no chance of a 90 that is really 89.999.

use maj_time::{Rational, Timebase};

use crate::picture::{BitDepth, PictureFormat};

/// What codec a stream's samples are in, from a closed vocabulary.
///
/// Closed for the same reason [`crate::ContainerKind`] is: the name reaches a manifest, and a
/// string a library handed us is not a vocabulary. A codec with no variant here is
/// [`Unknown`](CodecKind::Unknown), which is honest and costs nothing — the stream is still
/// counted, its geometry still read, and the frame still labelled `undecodable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodecKind {
    /// H.264 / AVC.
    H264,
    /// H.265 / HEVC.
    Hevc,
    /// AV1.
    Av1,
    /// VP9.
    Vp9,
    /// Apple ProRes, any variant.
    ProRes,
    /// Avid DNxHD / DNxHR.
    DnxHr,
    /// Motion JPEG.
    MotionJpeg,
    /// Uncompressed picture samples.
    Uncompressed,
    /// Linear PCM, integer samples.
    PcmInteger,
    /// Linear PCM, floating-point samples.
    PcmFloat,
    /// AAC.
    Aac,
    /// FLAC.
    Flac,
    /// Opus.
    Opus,
    /// A single PNG still.
    Png,
    /// A single Netpbm still.
    Netpbm,
    /// A codec this build has no name for.
    Unknown,
}

impl CodecKind {
    /// The short stable name, for a manifest or a log line.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::Hevc => "hevc",
            Self::Av1 => "av1",
            Self::Vp9 => "vp9",
            Self::ProRes => "prores",
            Self::DnxHr => "dnxhr",
            Self::MotionJpeg => "mjpeg",
            Self::Uncompressed => "uncompressed",
            Self::PcmInteger => "pcm",
            Self::PcmFloat => "pcm-float",
            Self::Aac => "aac",
            Self::Flac => "flac",
            Self::Opus => "opus",
            Self::Png => "png",
            Self::Netpbm => "netpbm",
            Self::Unknown => "unknown",
        }
    }
}

/// Rotation as a quarter-turn count.
///
/// A rotation is not an angle, so it is not a number. Four cases, applied exactly, with no
/// trigonometry anywhere near a display matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Rotation {
    /// Upright.
    #[default]
    None,
    /// A quarter turn clockwise: 90 degrees.
    Quarter,
    /// A half turn: 180 degrees.
    Half,
    /// Three quarter turns clockwise: 270 degrees.
    ThreeQuarters,
}

impl Rotation {
    /// Quarter turns clockwise, `0..=3`.
    #[must_use]
    pub const fn quarter_turns(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Quarter => 1,
            Self::Half => 2,
            Self::ThreeQuarters => 3,
        }
    }

    /// The rotation in degrees clockwise, which is what a person reads in a report.
    #[must_use]
    pub const fn degrees(self) -> u16 {
        self.quarter_turns() as u16 * 90
    }

    /// The rotation for a count of quarter turns, in any direction and any magnitude.
    ///
    /// Takes an `i32` and reduces it modulo four, so `-1` is three quarter turns and `5` is
    /// one. A container can state either sign and either magnitude, and the reduction is the
    /// reason this is integer arithmetic rather than an angle.
    #[must_use]
    pub const fn from_quarter_turns(turns: i32) -> Self {
        match turns.rem_euclid(4) {
            1 => Self::Quarter,
            2 => Self::Half,
            3 => Self::ThreeQuarters,
            _ => Self::None,
        }
    }

    /// Whether applying this rotation swaps width and height.
    #[must_use]
    pub const fn swaps_dimensions(self) -> bool {
        matches!(self, Self::Quarter | Self::ThreeQuarters)
    }

    /// The displayed size of a coded size under this rotation.
    #[must_use]
    pub const fn apply_to(self, size: (u32, u32)) -> (u32, u32) {
        if self.swaps_dimensions() {
            (size.1, size.0)
        } else {
            size
        }
    }
}

/// How the fields of a picture stream are ordered, as declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FieldOrder {
    /// One frame per sample, no fields.
    #[default]
    Progressive,
    /// Two interleaved fields per sample, the top one first in time.
    TopFirst,
    /// Two interleaved fields per sample, the bottom one first in time.
    BottomFirst,
    /// One top field per sample.
    SingleTop,
    /// One bottom field per sample.
    SingleBottom,
}

impl FieldOrder {
    /// The short stable name.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Progressive => "progressive",
            Self::TopFirst => "top-first",
            Self::BottomFirst => "bottom-first",
            Self::SingleTop => "single-top",
            Self::SingleBottom => "single-bottom",
        }
    }

    /// Whether a sample carries fields rather than a whole frame.
    #[must_use]
    pub const fn is_interlaced(self) -> bool {
        !matches!(self, Self::Progressive)
    }
}

/// Constant or variable, where variable is a different kind of stream rather than a flag.
///
/// A constant-rate stream states one rate and every frame's timestamp follows from it. A
/// variable-rate stream has no rate to state, so it carries every frame's presentation time
/// instead, as exact integers in the stream's own timebase. Pretending the second is the
/// first with a nominal rate attached is the bug this type exists to make impossible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameRateKind {
    /// One rate, exactly.
    Constant(Timebase),
    /// Each frame's presentation time, in the stream's own timebase. Exact integers.
    Variable {
        /// Presentation timestamps, ascending, one per frame.
        timestamps: Vec<i64>,
    },
}

impl FrameRateKind {
    /// Whether the rate varies.
    #[must_use]
    pub const fn is_variable(&self) -> bool {
        matches!(self, Self::Variable { .. })
    }

    /// The short stable name.
    #[must_use]
    pub const fn key(&self) -> &'static str {
        match self {
            Self::Constant(_) => "constant",
            Self::Variable { .. } => "variable",
        }
    }

    /// How many frames the rate accounts for, when it accounts for a known number.
    ///
    /// A constant rate says nothing about length, so it answers `None` and the stream's own
    /// frame count is the thing to ask. A variable rate with **no** timestamps answers `None`
    /// too: that is the honest shape for a container which declares that its rate varies and
    /// states the per-frame times only in the body it has not been asked to read.
    #[must_use]
    pub fn frames(&self) -> Option<u64> {
        match self {
            Self::Constant(_) => None,
            Self::Variable { timestamps } => {
                (!timestamps.is_empty()).then_some(timestamps.len() as u64)
            }
        }
    }

    /// The presentation timestamp of a frame, in `stream_timebase` ticks.
    ///
    /// This is the index-to-timestamp map, and it is the whole reason variable rate is a
    /// separate variant: under [`Variable`](FrameRateKind::Variable) it is a lookup, and
    /// under [`Constant`](FrameRateKind::Constant) it is one exact multiplication.
    ///
    /// `stream_timebase` is seconds per tick, so `1/24000` for a stream with a timescale of
    /// 24000.
    ///
    /// Answers `None` rather than approximating when the index is out of range, when the
    /// frame duration is not a whole number of ticks, or when the arithmetic would overflow.
    /// A timestamp that is nearly right is worse than one that is absent: it seeks to the
    /// wrong frame and nothing says so.
    #[must_use]
    pub fn timestamp(&self, index: i64, stream_timebase: Rational) -> Option<i64> {
        if index < 0 {
            return None;
        }
        match self {
            Self::Variable { timestamps } => usize::try_from(index)
                .ok()
                .and_then(|index| timestamps.get(index).copied()),
            Self::Constant(rate) => {
                let ticks_per_frame = rate
                    .rate()
                    .checked_recip()
                    .ok()?
                    .checked_div(stream_timebase)
                    .ok()?;
                if !ticks_per_frame.is_integer() {
                    return None;
                }
                ticks_per_frame.num().checked_mul(index)
            }
        }
    }
}

/// A picture stream, as the container declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoStream {
    /// Index of the stream within the container, counted from zero over all stream types.
    pub index: usize,
    /// What the samples are coded with.
    pub codec: CodecKind,
    /// Coded size, before rotation and before the sample aspect is applied.
    pub coded_size: (u32, u32),
    /// Pixel aspect as an exact ratio. Never an `f64`.
    pub sample_aspect: Rational,
    /// The timebase the project will address this stream in, when the stream imposes one.
    ///
    /// `None` for a stream that imposes no rate — a single still is the case that forces the
    /// option. `docs/MEDIA.md` types this field as a bare [`Timebase`], which would mean
    /// inventing a frames-per-second figure for a PNG; a number in a manifest that no file
    /// ever stated is worse than an absence that says so.
    pub timebase: Option<Timebase>,
    /// The stream's own timebase, in seconds per tick, for the index-to-timestamp map.
    pub stream_timebase: Rational,
    /// Frame count when the container states it or it can be derived exactly.
    pub frames: Option<u64>,
    /// Constant or variable rate.
    pub rate: FrameRateKind,
    /// Rotation to apply for display.
    pub rotation: Rotation,
    /// Field order, as declared.
    pub field_order: FieldOrder,
    /// How the samples are laid out and what they mean.
    pub picture: PictureFormat,
}

impl VideoStream {
    /// The size as it should be displayed: coded size, rotated, with the sample aspect
    /// applied to the width.
    ///
    /// The aspect is applied by exact rational arithmetic and rounded once, at the end,
    /// half away from zero. Rounding is unavoidable — a pixel count is an integer and
    /// `720 × 16/15` is not — so it happens exactly once and in a documented direction.
    #[must_use]
    pub fn display_size(&self) -> (u32, u32) {
        let (width, height) = self.rotation.apply_to(self.coded_size);
        let (num, den) = (self.sample_aspect.num(), self.sample_aspect.den());
        if num <= 0 || den <= 0 || num == den {
            return (width, height);
        }
        // Rotation moves the axis the aspect applies to along with the picture.
        let (scaled, untouched) = if self.rotation.swaps_dimensions() {
            (height, width)
        } else {
            (width, height)
        };
        let Some(product) = i64::from(scaled).checked_mul(num) else {
            return (width, height);
        };
        let rounded = (product * 2 + den) / (den * 2);
        let Ok(rounded) = u32::try_from(rounded) else {
            return (width, height);
        };
        if self.rotation.swaps_dimensions() {
            (untouched, rounded)
        } else {
            (rounded, untouched)
        }
    }
}

/// A sound stream, as the container declares it.
///
/// Declared in M2 and read in M9, except for what waveforms need. The depth is a plain bit
/// count rather than a [`BitDepth`]: audio arrives at twenty-four bits often enough that a
/// four-case enum would have to lie about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioStream {
    /// Index of the stream within the container, counted from zero over all stream types.
    pub index: usize,
    /// What the samples are coded with.
    pub codec: CodecKind,
    /// Channel count, as declared.
    pub channels: u16,
    /// Samples per second, as declared.
    pub sample_rate: u32,
    /// Bits per sample, where the container states it.
    pub sample_bits: Option<u8>,
    /// The stream's own timebase, in seconds per tick.
    pub stream_timebase: Rational,
    /// Sample frames when the container states a count or it can be derived exactly.
    pub frames: Option<u64>,
}

impl AudioStream {
    /// The declared depth as a [`BitDepth`], when it is one this build models.
    #[must_use]
    pub fn modelled_depth(&self) -> Option<BitDepth> {
        self.sample_bits.and_then(BitDepth::from_bits)
    }
}

/// What a stream that is neither picture nor sound is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StreamRole {
    /// A timecode track.
    Timecode,
    /// Subtitles or captions.
    Subtitle,
    /// Opaque data.
    Data,
    /// A streaming hint track.
    Hint,
    /// Present, and this build has no name for what it is for.
    Unknown,
}

impl StreamRole {
    /// The short stable name.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Timecode => "timecode",
            Self::Subtitle => "subtitle",
            Self::Data => "data",
            Self::Hint => "hint",
            Self::Unknown => "unknown",
        }
    }
}

/// One stream in a container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamInfo {
    /// A picture stream.
    Video(VideoStream),
    /// A sound stream.
    Audio(AudioStream),
    /// Present, counted, not read: timecode, data and subtitle tracks.
    Other {
        /// Index of the stream within the container.
        index: usize,
        /// What it is for.
        role: StreamRole,
    },
}

impl StreamInfo {
    /// Index of the stream within the container.
    #[must_use]
    pub const fn index(&self) -> usize {
        match self {
            Self::Video(stream) => stream.index,
            Self::Audio(stream) => stream.index,
            Self::Other { index, .. } => *index,
        }
    }

    /// The picture stream, if this is one.
    #[must_use]
    pub const fn as_video(&self) -> Option<&VideoStream> {
        match self {
            Self::Video(stream) => Some(stream),
            Self::Audio(_) | Self::Other { .. } => None,
        }
    }

    /// The sound stream, if this is one.
    #[must_use]
    pub const fn as_audio(&self) -> Option<&AudioStream> {
        match self {
            Self::Audio(stream) => Some(stream),
            Self::Video(_) | Self::Other { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picture::PlaneLayout;

    fn stream(rate: FrameRateKind, rotation: Rotation, aspect: Rational) -> VideoStream {
        VideoStream {
            index: 0,
            codec: CodecKind::H264,
            coded_size: (720, 576),
            sample_aspect: aspect,
            timebase: Some(Timebase::FPS_25),
            stream_timebase: Rational::from_reduced(1, 25),
            frames: Some(10),
            rate,
            rotation,
            field_order: FieldOrder::Progressive,
            picture: PictureFormat::undeclared(PlaneLayout::Yuv420, BitDepth::Eight),
        }
    }

    #[test]
    fn rotation_is_modular_integer_arithmetic_in_both_directions() {
        assert_eq!(Rotation::from_quarter_turns(-1), Rotation::ThreeQuarters);
        assert_eq!(Rotation::from_quarter_turns(5), Rotation::Quarter);
        assert_eq!(Rotation::from_quarter_turns(4), Rotation::None);
        assert_eq!(Rotation::ThreeQuarters.degrees(), 270);
        assert!(Rotation::Quarter.swaps_dimensions());
        assert!(!Rotation::Half.swaps_dimensions());
        assert_eq!(Rotation::Quarter.apply_to((1920, 1080)), (1080, 1920));
    }

    #[test]
    fn a_constant_rate_maps_an_index_by_one_exact_multiplication() {
        let rate = FrameRateKind::Constant(Timebase::FPS_23_976);
        let timebase = Rational::from_reduced(1, 24_000);
        assert_eq!(rate.timestamp(0, timebase), Some(0));
        assert_eq!(rate.timestamp(1, timebase), Some(1_001));
        assert_eq!(rate.timestamp(48, timebase), Some(48_048));
        assert_eq!(rate.timestamp(-1, timebase), None);
    }

    #[test]
    fn a_rate_that_is_not_a_whole_number_of_ticks_answers_nothing() {
        // 23.976 in a timescale of 1000: 41.7083... ticks per frame. A timestamp that is
        // nearly right seeks to the wrong frame and nothing says so, so there is none.
        let rate = FrameRateKind::Constant(Timebase::FPS_23_976);
        assert_eq!(rate.timestamp(1, Rational::from_reduced(1, 1_000)), None);
    }

    #[test]
    fn a_variable_rate_maps_an_index_by_lookup() {
        let rate = FrameRateKind::Variable {
            timestamps: vec![0, 1_001, 3_003, 4_004],
        };
        let timebase = Rational::from_reduced(1, 24_000);
        assert_eq!(rate.timestamp(2, timebase), Some(3_003));
        assert_eq!(
            rate.timestamp(4, timebase),
            None,
            "out of range is not invented"
        );
        assert_eq!(rate.frames(), Some(4));
        assert!(rate.is_variable());
    }

    #[test]
    fn the_sample_aspect_is_applied_exactly_once_and_rotation_moves_the_axis() {
        let anamorphic = Rational::from_reduced(16, 15);
        let upright = stream(
            FrameRateKind::Constant(Timebase::FPS_25),
            Rotation::None,
            anamorphic,
        );
        // 720 * 16/15 = 768 exactly.
        assert_eq!(upright.display_size(), (768, 576));

        let turned = stream(
            FrameRateKind::Constant(Timebase::FPS_25),
            Rotation::Quarter,
            anamorphic,
        );
        // Rotated to 576x720, and the aspect still stretches the picture's own width.
        assert_eq!(turned.display_size(), (576, 768));
    }

    #[test]
    fn a_square_sample_aspect_leaves_the_size_alone() {
        let square = stream(
            FrameRateKind::Constant(Timebase::FPS_25),
            Rotation::None,
            Rational::from_integer(1),
        );
        assert_eq!(square.display_size(), (720, 576));
    }

    #[test]
    fn a_stream_knows_which_kind_it_is() {
        let video = StreamInfo::Video(stream(
            FrameRateKind::Constant(Timebase::FPS_25),
            Rotation::None,
            Rational::from_integer(1),
        ));
        assert_eq!(video.index(), 0);
        assert!(video.as_video().is_some());
        assert!(video.as_audio().is_none());

        let other = StreamInfo::Other {
            index: 3,
            role: StreamRole::Timecode,
        };
        assert_eq!(other.index(), 3);
        assert_eq!(StreamRole::Timecode.key(), "timecode");
    }

    #[test]
    fn an_audio_depth_the_model_cannot_hold_is_reported_as_absent_not_rounded() {
        let audio = AudioStream {
            index: 1,
            codec: CodecKind::PcmInteger,
            channels: 2,
            sample_rate: 48_000,
            sample_bits: Some(24),
            stream_timebase: Rational::from_reduced(1, 48_000),
            frames: Some(48_000),
        };
        assert_eq!(audio.sample_bits, Some(24));
        assert_eq!(audio.modelled_depth(), None);
    }

    #[test]
    fn field_order_distinguishes_progressive_from_everything_else() {
        assert!(!FieldOrder::Progressive.is_interlaced());
        for order in [
            FieldOrder::TopFirst,
            FieldOrder::BottomFirst,
            FieldOrder::SingleTop,
            FieldOrder::SingleBottom,
        ] {
            assert!(order.is_interlaced(), "{} is not progressive", order.key());
        }
    }
}
