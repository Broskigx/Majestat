//! Matroska and WebM: `.mkv`, `.webm`.
//!
//! EBML is a tree of variable-length identifiers and lengths, which makes it the format where
//! a parser is most tempted to trust a number it was handed. Every length here is checked
//! against the window it has to fit in, the recursion is depth-limited, and an element that
//! does not advance ends the walk.
//!
//! Two honest limits, both consequences of probing headers only:
//!
//! * **A rate comes from `DefaultDuration`**, in nanoseconds. Nanoseconds cannot express
//!   24000/1001 exactly, so a declared duration that is a standard rate's rounding to the
//!   nanosecond is read back as that exact rate, by integer comparison, and anything else is
//!   kept as the exact ratio of a second to the duration stated. Nothing is approximated.
//! * **A track with no `DefaultDuration` is variable-rate with no timestamps read.** Matroska
//!   states per-frame times in its clusters, which is the body and not the headers; M2b's
//!   demuxer fills them in. [`FrameRateKind::frames`] answers `None` for that shape rather
//!   than claiming a count of zero.

use maj_time::{Rational, Timebase};

use crate::container::ContainerKind;
use crate::error::MediaError;
use crate::picture::{
    BitDepth, ChromaSiting, ColourMatrix, ColourPrimaries, PlaneLayout, SampleRange,
    TransferCharacteristics,
};
use crate::stream::{
    AudioStream, CodecKind, FieldOrder, FrameRateKind, Rotation, StreamInfo, StreamRole,
    VideoStream,
};

use super::bytes::Source;
use super::codec;

/// How much of the front of a file is read to find the track declarations.
///
/// Every muxer this build has met writes the segment information and the tracks before the
/// first cluster, because a player that had to seek to the end to start would be unusable.
const HEADER_WINDOW: u64 = 4 * 1024 * 1024;

/// The deepest this parser will descend.
const MAX_DEPTH: u32 = 12;

/// The most elements this parser will accept at one level.
const MAX_ELEMENTS: usize = 8_192;

/// Matroska's default timestamp scale, in nanoseconds per tick.
const DEFAULT_TIMESTAMP_SCALE: u64 = 1_000_000;

/// Whether these bytes open with the EBML signature.
pub(crate) fn looks_like(head: &[u8]) -> bool {
    head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3])
}

/// Reads the headers of a Matroska file.
///
/// # Errors
///
/// [`MediaError::Malformed`] when the element structure does not close or there is no track
/// declaration in the window this build reads.
pub(crate) fn probe(source: &mut Source) -> Result<(ContainerKind, Vec<StreamInfo>), MediaError> {
    let window = source.len().min(HEADER_WINDOW) as usize;
    let bytes = source.bytes(0, window)?;
    let top = elements(&bytes, 0)?;
    let segment = top
        .into_iter()
        .find(|(id, _)| *id == 0x1853_8067)
        .map(|(_, body)| body)
        .ok_or_else(|| {
            MediaError::Malformed("the file has no segment: there is nothing to read".to_owned())
        })?;
    let inside = elements(segment, 1)?;

    let mut scale = DEFAULT_TIMESTAMP_SCALE;
    for (id, body) in &inside {
        if *id == 0x1549_A966 {
            for (id, body) in elements(body, 2)? {
                if id == 0x002A_D7B1 {
                    if let Some(stated) = unsigned(body).filter(|stated| *stated > 0) {
                        scale = stated;
                    }
                }
            }
        }
    }

    let tracks = inside
        .into_iter()
        .find(|(id, _)| *id == 0x1654_AE6B)
        .map(|(_, body)| body)
        .ok_or_else(|| {
            MediaError::Malformed(
                "no track declaration in the first four mebibytes: this build reads headers, not clusters"
                    .to_owned(),
            )
        })?;

    let mut streams = Vec::new();
    for (id, body) in elements(tracks, 2)? {
        if id == 0xAE {
            streams.push(track(body, scale, streams.len())?);
        }
    }
    Ok((ContainerKind::Matroska, streams))
}

/// The elements directly inside a buffer, as identifiers and bodies borrowed from it.
///
/// Borrowed from the slice they were parsed out of, deliberately: an element's offsets are
/// relative to its parent, so a type holding positions and a caller holding the whole file
/// are one subtraction away from reading the wrong bytes. A borrow cannot be used against the
/// wrong buffer.
fn elements(bytes: &[u8], depth: u32) -> Result<Vec<(u64, &[u8])>, MediaError> {
    if depth > MAX_DEPTH {
        return Err(MediaError::Malformed(format!(
            "elements nested deeper than {MAX_DEPTH}"
        )));
    }
    let mut found = Vec::new();
    let mut at = 0_usize;
    while at < bytes.len() {
        let Some((id, after_id)) = vint(bytes, at, true) else {
            break;
        };
        let Some((length, after_length)) = vint(bytes, after_id, false) else {
            break;
        };
        // An unknown-length element runs to the end of what holds it, which is legal for a
        // segment written while it was still being recorded.
        let end = if length == UNKNOWN_LENGTH {
            bytes.len()
        } else {
            match usize::try_from(length).ok().and_then(|length| {
                after_length
                    .checked_add(length)
                    .filter(|end| *end <= bytes.len())
            }) {
                Some(end) => end,
                // Truncated by the header window or by damage: the walk stops either way,
                // and what was found before it is still a true reading of the file.
                None => break,
            }
        };
        if end < after_length {
            break;
        }
        found.push((id, bytes.get(after_length..end).unwrap_or_default()));
        if found.len() > MAX_ELEMENTS {
            return Err(MediaError::Malformed(format!(
                "more than {MAX_ELEMENTS} elements at one level"
            )));
        }
        if end == at {
            break;
        }
        at = end;
    }
    Ok(found)
}

/// What an all-ones length means: the element runs to the end of its parent.
const UNKNOWN_LENGTH: u64 = u64::MAX;

/// A variable-length integer, as an identifier (marker bits kept) or a length (stripped).
fn vint(bytes: &[u8], at: usize, keep_marker: bool) -> Option<(u64, usize)> {
    let first = *bytes.get(at)?;
    if first == 0 {
        return None;
    }
    let width = usize::from(first.leading_zeros() as u8) + 1;
    if width > 8 {
        return None;
    }
    let end = at.checked_add(width)?;
    let slice = bytes.get(at..end)?;
    // At the widest, every bit of the first byte is marker, so the value mask is empty.
    // Written as a checked shift because `0xFF_u8 >> 8` is not zero, it is a panic.
    let mask = 0xFF_u8.checked_shr(width as u32).unwrap_or(0);
    let mut value = if keep_marker {
        u64::from(first)
    } else {
        u64::from(first & mask)
    };
    let mut all_ones = !keep_marker && first & mask == mask;
    for byte in slice.iter().skip(1) {
        value = value.checked_mul(256)?.checked_add(u64::from(*byte))?;
        all_ones = all_ones && *byte == 0xFF;
    }
    if all_ones {
        return Some((UNKNOWN_LENGTH, end));
    }
    Some((value, end))
}

/// A big-endian unsigned integer of one to eight bytes.
fn unsigned(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() || bytes.len() > 8 {
        return None;
    }
    Some(
        bytes
            .iter()
            .fold(0_u64, |value, byte| (value << 8) | u64::from(*byte)),
    )
}

/// An IEEE 754 value, which is how Matroska states a sample rate and a projection angle.
///
/// Read because the container states it that way; it leaves this module as an integer in
/// every case — a whole number of hertz, or a quarter-turn count — and no float reaches the
/// model.
fn float(bytes: &[u8]) -> Option<f64> {
    match bytes.len() {
        4 => Some(f64::from(f32::from_be_bytes(bytes.try_into().ok()?))),
        8 => Some(f64::from_be_bytes(bytes.try_into().ok()?)),
        _ => None,
    }
}

/// One track entry, as whichever kind of stream it declares.
fn track(entry: &[u8], scale: u64, index: usize) -> Result<StreamInfo, MediaError> {
    let mut kind = 0_u64;
    let mut codec_id = String::new();
    let mut default_duration = None;
    let mut video = None;
    let mut audio = None;
    for (id, body) in elements(entry, 3)? {
        match id {
            0x83 => kind = unsigned(body).unwrap_or(0),
            0x86 => {
                codec_id = String::from_utf8_lossy(body)
                    .trim_end_matches('\0')
                    .to_owned();
            }
            0x0023_E383 => default_duration = unsigned(body).filter(|ns| *ns > 0),
            0xE0 => video = Some(body),
            0xE1 => audio = Some(body),
            _ => {}
        }
    }
    let stream_timebase = Rational::new(i64::try_from(scale).unwrap_or(i64::MAX), 1_000_000_000)
        .map_err(|error| MediaError::Malformed(format!("track {index}: {error}")))?;

    match kind {
        1 => {
            let settings = video.ok_or_else(|| {
                MediaError::Malformed(format!("picture track {index} declares no video settings"))
            })?;
            video_track(
                settings,
                &codec_id,
                default_duration,
                stream_timebase,
                index,
            )
        }
        2 => Ok(audio_track(audio, &codec_id, stream_timebase, index)),
        17 => Ok(StreamInfo::Other {
            index,
            role: StreamRole::Subtitle,
        }),
        _ => Ok(StreamInfo::Other {
            index,
            role: StreamRole::Unknown,
        }),
    }
}

/// A Matroska picture track.
fn video_track(
    settings: &[u8],
    codec_id: &str,
    default_duration: Option<u64>,
    stream_timebase: Rational,
    index: usize,
) -> Result<StreamInfo, MediaError> {
    let mut size = (0_u32, 0_u32);
    let mut interlaced = None;
    let mut declared_order = None;
    let mut rotation = Rotation::None;
    let mut picture = codec::undeclared_format();
    let mut chroma = (None, None);
    for (id, body) in elements(settings, 4)? {
        match id {
            0xB0 => {
                size.0 = unsigned(body)
                    .and_then(|value| u32::try_from(value).ok())
                    .unwrap_or(0);
            }
            0xBA => {
                size.1 = unsigned(body)
                    .and_then(|value| u32::try_from(value).ok())
                    .unwrap_or(0);
            }
            0x9A => interlaced = unsigned(body),
            0x9D => declared_order = unsigned(body),
            0x55B0 => {
                for (id, body) in elements(body, 5)? {
                    match id {
                        0x55B1 => {
                            if let Some(code) = small(body) {
                                picture.matrix = ColourMatrix::from_code_point(code);
                            }
                        }
                        0x55B2 => {
                            if let Some(depth) = small(body).and_then(BitDepth::from_bits) {
                                picture.depth = depth;
                            }
                        }
                        0x55B3 => chroma.0 = small(body),
                        0x55B4 => chroma.1 = small(body),
                        0x55B7 | 0x55B8 => {
                            if let Some(siting) = small(body) {
                                picture.chroma_siting = match siting {
                                    1 => ChromaSiting::TopLeft,
                                    2 => ChromaSiting::Centre,
                                    _ => ChromaSiting::Unspecified,
                                };
                            }
                        }
                        0x55B9 => {
                            picture.range = match small(body) {
                                Some(1) => SampleRange::Limited,
                                Some(2) => SampleRange::Full,
                                _ => SampleRange::Unspecified,
                            };
                        }
                        0x55BA => {
                            if let Some(code) = small(body) {
                                picture.transfer = TransferCharacteristics::from_code_point(code);
                            }
                        }
                        0x55BB => {
                            if let Some(code) = small(body) {
                                picture.primaries = ColourPrimaries::from_code_point(code);
                            }
                        }
                        _ => {}
                    }
                }
            }
            0x7670 => {
                for (id, body) in elements(body, 5)? {
                    if id == 0x7674 {
                        if let Some(roll) = float(body) {
                            rotation = rotation_of_roll(roll);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if let (Some(horizontal), Some(vertical)) = chroma {
        picture.layout = match (horizontal, vertical) {
            (1, 1) => PlaneLayout::Yuv420,
            (1, 0) => PlaneLayout::Yuv422,
            (0, 0) => PlaneLayout::Yuv444,
            _ => picture.layout,
        };
    }
    let field_order = field_order_of(interlaced, declared_order);
    let (rate, nominal) = match default_duration {
        Some(nanoseconds) => {
            let timebase = timebase_of_duration(nanoseconds).ok_or_else(|| {
                MediaError::Malformed(format!(
                    "picture track {index} declares a frame lasting {nanoseconds} nanoseconds, which is not a rate"
                ))
            })?;
            (FrameRateKind::Constant(timebase), Some(timebase))
        }
        None => (
            FrameRateKind::Variable {
                timestamps: Vec::new(),
            },
            None,
        ),
    };
    Ok(StreamInfo::Video(VideoStream {
        index,
        codec: codec_of_id(codec_id),
        coded_size: size,
        sample_aspect: Rational::from_integer(1),
        timebase: nominal,
        stream_timebase,
        frames: None,
        rate,
        rotation,
        field_order,
        picture,
    }))
}

/// A Matroska sound track.
fn audio_track(
    settings: Option<&[u8]>,
    codec_id: &str,
    stream_timebase: Rational,
    index: usize,
) -> StreamInfo {
    let mut channels = 1_u16;
    let mut sample_rate = 0_u32;
    let mut sample_bits = None;
    if let Some(settings) = settings {
        for (id, body) in elements(settings, 4).unwrap_or_default() {
            match id {
                0xB5 => {
                    // Stated as a float by the format; kept only when it is a whole number of
                    // hertz, which every real file states.
                    if let Some(hertz) = float(body) {
                        if hertz > 0.0 && hertz < 1e9 && hertz.fract() == 0.0 {
                            sample_rate = hertz as u32;
                        }
                    }
                }
                0x9F => {
                    channels = unsigned(body)
                        .and_then(|value| u16::try_from(value).ok())
                        .unwrap_or(1);
                }
                0x6264 => sample_bits = small(body),
                _ => {}
            }
        }
    }
    StreamInfo::Audio(AudioStream {
        index,
        codec: codec_of_id(codec_id),
        channels,
        sample_rate,
        sample_bits: sample_bits.filter(|bits| *bits > 0),
        stream_timebase,
        frames: None,
    })
}

/// A value small enough to be a code point or a bit count.
fn small(bytes: &[u8]) -> Option<u8> {
    unsigned(bytes).and_then(|value| u8::try_from(value).ok())
}

/// Matroska's `FieldOrder`, against its `FlagInterlaced`.
fn field_order_of(interlaced: Option<u64>, order: Option<u64>) -> FieldOrder {
    match order {
        Some(0) => return FieldOrder::Progressive,
        Some(1 | 14) => return FieldOrder::TopFirst,
        Some(6 | 9) => return FieldOrder::BottomFirst,
        _ => {}
    }
    // Interlaced with the order undetermined, which is a thing the format lets a muxer say.
    // Top-first keeps the interlaced fact, which is the one that must not be lost.
    match interlaced {
        Some(1) => FieldOrder::TopFirst,
        _ => FieldOrder::Progressive,
    }
}

/// A projection roll, snapped to a quarter turn or reported upright.
///
/// The angle arrives as a float because the format stores one. It is compared against the
/// four right angles and becomes a quarter-turn count or nothing: no arithmetic is done with
/// it, and a roll that is not a right angle is reported upright rather than rounded, for the
/// same reason a sheared display matrix is.
fn rotation_of_roll(roll: f64) -> Rotation {
    if !roll.is_finite() {
        return Rotation::None;
    }
    for turns in 0..4_i32 {
        let expected = f64::from(turns * 90);
        if (roll - expected).abs() < 0.5 || (roll + expected).abs() < 0.5 && turns != 0 {
            // A positive roll is clockwise, which is the direction `Rotation` counts in.
            return Rotation::from_quarter_turns(if roll < 0.0 { -turns } else { turns });
        }
    }
    Rotation::None
}

/// The rate a frame duration in nanoseconds describes.
///
/// A nanosecond cannot express 24000/1001, so a duration that is a standard rate's rounding
/// — within one nanosecond, by integer comparison — is read back as that exact rate. Anything
/// else is the exact ratio of one second to the duration stated.
fn timebase_of_duration(nanoseconds: u64) -> Option<Timebase> {
    const STANDARD: [Timebase; 8] = [
        Timebase::FPS_23_976,
        Timebase::FPS_24,
        Timebase::FPS_25,
        Timebase::FPS_29_97,
        Timebase::FPS_30,
        Timebase::FPS_50,
        Timebase::FPS_59_94,
        Timebase::FPS_60,
    ];
    let stated = i128::from(nanoseconds);
    for rate in STANDARD {
        let numerator = i128::from(rate.numerator());
        let denominator = i128::from(rate.denominator());
        // The nanoseconds that rate would state, rounded half away from zero.
        let ideal = (1_000_000_000_i128 * denominator * 2 + numerator) / (numerator * 2);
        if (stated - ideal).abs() <= 1 {
            return Some(rate);
        }
    }
    Timebase::new(Rational::new(1_000_000_000, i64::try_from(nanoseconds).ok()?).ok()?).ok()
}

/// The codec a Matroska codec identifier names.
fn codec_of_id(id: &str) -> CodecKind {
    match id {
        "V_MPEG4/ISO/AVC" => CodecKind::H264,
        "V_MPEGH/ISO/HEVC" => CodecKind::Hevc,
        "V_AV1" => CodecKind::Av1,
        "V_VP9" => CodecKind::Vp9,
        "V_PRORES" => CodecKind::ProRes,
        "V_UNCOMPRESSED" => CodecKind::Uncompressed,
        "V_MJPEG" => CodecKind::MotionJpeg,
        "A_AAC" => CodecKind::Aac,
        "A_FLAC" => CodecKind::Flac,
        "A_OPUS" => CodecKind::Opus,
        "A_PCM/INT/LIT" | "A_PCM/INT/BIG" => CodecKind::PcmInteger,
        "A_PCM/FLOAT/IEEE" => CodecKind::PcmFloat,
        _ => CodecKind::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_variable_length_integer_reads_its_own_width() {
        // 0x82 is a one-byte length of two.
        assert_eq!(vint(&[0x82], 0, false), Some((2, 1)));
        // 0x4002 is a two-byte length of two.
        assert_eq!(vint(&[0x40, 0x02], 0, false), Some((2, 2)));
        // As an identifier, the marker bits stay.
        assert_eq!(vint(&[0x86], 0, true), Some((0x86, 1)));
        // All-ones is the unknown length.
        assert_eq!(vint(&[0xFF], 0, false), Some((UNKNOWN_LENGTH, 1)));
        // A zero first byte is not a valid integer of any width.
        assert_eq!(vint(&[0x00], 0, false), None);
        assert_eq!(vint(&[], 0, false), None);
        // A width the buffer cannot hold.
        assert_eq!(vint(&[0x40], 0, false), None);
    }

    #[test]
    fn the_widest_length_does_not_overflow_the_mask_that_reads_it() {
        // A regression case, found by the corpus's byte-flip sweep: a first byte of 0x01 is a
        // length eight bytes wide, and `0xFF_u8 >> 8` is a panic rather than a zero. The file
        // is damaged either way; the prober's job is to say so and return.
        assert_eq!(
            vint(&[0x01, 0, 0, 0, 0, 0, 0, 0x2A], 0, false),
            Some((42, 8))
        );
        assert_eq!(
            vint(&[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF], 0, false),
            Some((UNKNOWN_LENGTH, 8)),
            "at the widest, the first byte is all marker, so the value field is the other \
             fifty-six bits — and all of them set is the unknown length"
        );
        assert_eq!(
            vint(&[0x01, 0, 0], 0, false),
            None,
            "eight bytes, three given"
        );
    }

    #[test]
    fn standard_rates_survive_the_nanosecond_they_are_stated_in() {
        assert_eq!(timebase_of_duration(41_708_333), Some(Timebase::FPS_23_976));
        assert_eq!(timebase_of_duration(41_666_667), Some(Timebase::FPS_24));
        assert_eq!(timebase_of_duration(40_000_000), Some(Timebase::FPS_25));
        assert_eq!(timebase_of_duration(33_366_667), Some(Timebase::FPS_29_97));
        assert_eq!(timebase_of_duration(16_666_667), Some(Timebase::FPS_60));
    }

    #[test]
    fn an_unusual_duration_stays_the_exact_ratio_it_is() {
        let rate = timebase_of_duration(100_000_000).expect("ten frames a second");
        assert_eq!(rate.rate(), Rational::new(10, 1).expect("ten"));
        assert_eq!(timebase_of_duration(0), None);
    }

    #[test]
    fn field_order_keeps_the_interlaced_fact_even_when_the_order_is_undetermined() {
        assert_eq!(field_order_of(Some(0), Some(0)), FieldOrder::Progressive);
        assert_eq!(field_order_of(Some(1), Some(1)), FieldOrder::TopFirst);
        assert_eq!(field_order_of(Some(1), Some(6)), FieldOrder::BottomFirst);
        assert_eq!(field_order_of(Some(1), Some(2)), FieldOrder::TopFirst);
        assert!(field_order_of(Some(1), Some(2)).is_interlaced());
        assert_eq!(field_order_of(None, None), FieldOrder::Progressive);
    }

    #[test]
    fn a_projection_roll_becomes_a_quarter_turn_or_nothing() {
        assert_eq!(rotation_of_roll(0.0), Rotation::None);
        assert_eq!(rotation_of_roll(90.0), Rotation::Quarter);
        assert_eq!(rotation_of_roll(180.0), Rotation::Half);
        assert_eq!(rotation_of_roll(-90.0), Rotation::ThreeQuarters);
        assert_eq!(rotation_of_roll(37.0), Rotation::None);
        assert_eq!(rotation_of_roll(f64::NAN), Rotation::None);
    }

    #[test]
    fn integers_are_read_at_the_width_they_are_written() {
        assert_eq!(unsigned(&[0x01]), Some(1));
        assert_eq!(unsigned(&[0x01, 0x00]), Some(256));
        assert_eq!(unsigned(&[]), None);
        assert_eq!(unsigned(&[0; 9]), None);
        assert_eq!(small(&[0x09]), Some(9));
        assert_eq!(small(&[0x01, 0x00]), None);
    }

    #[test]
    fn codec_identifiers_name_what_they_name() {
        assert_eq!(codec_of_id("V_MPEG4/ISO/AVC"), CodecKind::H264);
        assert_eq!(codec_of_id("A_OPUS"), CodecKind::Opus);
        assert_eq!(codec_of_id("V_THEORA"), CodecKind::Unknown);
    }

    #[test]
    fn signatures() {
        assert!(looks_like(&[0x1A, 0x45, 0xDF, 0xA3, 0x01]));
        assert!(!looks_like(b"RIFF"));
        assert!(!looks_like(&[0x1A]));
    }
}
