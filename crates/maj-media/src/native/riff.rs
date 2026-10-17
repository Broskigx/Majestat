//! RIFF: `.avi` and `.wav`.
//!
//! Two formats in one parser because they are one format with two payloads. AVI states its
//! rate as an exact ratio of two integers — `dwRate` over `dwScale` — which is the one thing
//! it does better than most containers, and it is read as that ratio and never as a decimal.
//!
//! What AVI does not state is field order: the `vprp` extension that could is optional and
//! almost never written, so an AVI's picture stream is reported progressive. That is a
//! declaration this container does not make, and reporting it as an unknown interlacing
//! would be inventing a fact in the other direction.

use maj_time::{Rational, Timebase};

use crate::container::ContainerKind;
use crate::error::MediaError;
use crate::stream::{
    AudioStream, CodecKind, FieldOrder, FrameRateKind, Rotation, StreamInfo, VideoStream,
};

use super::bytes::{Cursor, Source, fourcc_name};
use super::codec;

/// The most chunks this parser will accept at one level.
const MAX_CHUNKS: usize = 4_096;

/// How much of the front of a file the header structures are read from.
///
/// An AVI's `hdrl` and a WAV's `fmt ` are at the start by definition of the formats; the rest
/// of the file is sample data this parser has no business in.
const HEADER_WINDOW: u64 = 1024 * 1024;

/// Whether these bytes open like a RIFF file.
pub(crate) fn looks_like(head: &[u8]) -> bool {
    head.starts_with(b"RIFF")
}

/// Reads the headers of a RIFF file.
///
/// # Errors
///
/// [`MediaError::Malformed`] for a chunk structure that does not close or a form this build
/// recognises and cannot make sense of, and [`MediaError::Unsupported`] for a RIFF form that
/// is neither AVI nor WAVE.
pub(crate) fn probe(source: &mut Source) -> Result<(ContainerKind, Vec<StreamInfo>), MediaError> {
    let window = source.len().min(HEADER_WINDOW) as usize;
    let bytes = source.bytes(0, window)?;
    let mut cursor = Cursor::new(&bytes);
    cursor.fourcc().ok_or_else(short)?;
    let declared = cursor.le32().ok_or_else(short)?;
    let form = cursor.fourcc().ok_or_else(short)?;
    // The declared size counts everything after it, so eight bytes less than the file.
    if u64::from(declared).saturating_add(8) < 12 {
        return Err(MediaError::Malformed(format!(
            "a RIFF file declaring {declared} bytes cannot hold its own form"
        )));
    }
    match &form {
        b"AVI " => avi(&bytes[12..]).map(|streams| (ContainerKind::Avi, streams)),
        b"WAVE" => wave(&bytes[12..]).map(|streams| (ContainerKind::Wav, streams)),
        other => Err(MediaError::Unsupported(format!(
            "a RIFF file of form `{}`",
            fourcc_name(*other)
        ))),
    }
}

/// One chunk: its four-character identifier and the body that follows it.
type Chunk<'bytes> = ([u8; 4], &'bytes [u8]);

/// The chunks directly inside a buffer, in file order.
fn chunks(bytes: &[u8]) -> Result<Vec<Chunk<'_>>, MediaError> {
    let mut found = Vec::new();
    let mut at = 0_usize;
    while at + 8 <= bytes.len() {
        let mut cursor = Cursor::new(&bytes[at..]);
        let id = cursor.fourcc().ok_or_else(short)?;
        let size = cursor.le32().ok_or_else(short)? as usize;
        let start = at + 8;
        let end = start.checked_add(size).ok_or_else(|| {
            MediaError::Malformed(format!("the {} chunk's size overflows", fourcc_name(id)))
        })?;
        // A chunk may be cut off by the header window rather than by damage, so the body is
        // taken as far as it goes and the walk stops there.
        let Some(body) = bytes.get(start..end) else {
            break;
        };
        found.push((id, body));
        if found.len() > MAX_CHUNKS {
            return Err(MediaError::Malformed(format!(
                "more than {MAX_CHUNKS} chunks at one level"
            )));
        }
        // Chunks are padded to an even length, and the padding byte is not in the size.
        at = end + (end & 1);
    }
    Ok(found)
}

/// The streams of an AVI's header list.
fn avi(bytes: &[u8]) -> Result<Vec<StreamInfo>, MediaError> {
    let top = chunks(bytes)?;
    let header_list = top
        .iter()
        .find(|(id, body)| id == b"LIST" && body.starts_with(b"hdrl"))
        .map(|(_, body)| &body[4..])
        .ok_or_else(|| {
            MediaError::Malformed(
                "the file has no header list: there is nothing to read".to_owned(),
            )
        })?;
    let mut streams = Vec::new();
    for (id, body) in chunks(header_list)? {
        if id == *b"LIST" && body.starts_with(b"strl") {
            let stream = stream_list(&body[4..], streams.len())?;
            streams.push(stream);
        }
    }
    Ok(streams)
}

/// One `strl`: the stream header and the format that follows it.
fn stream_list(bytes: &[u8], index: usize) -> Result<StreamInfo, MediaError> {
    let parts = chunks(bytes)?;
    let header = parts
        .iter()
        .find(|(id, _)| id == b"strh")
        .map(|(_, body)| *body)
        .ok_or_else(|| MediaError::Malformed(format!("stream {index} has no header")))?;
    let format = parts
        .iter()
        .find(|(id, _)| id == b"strf")
        .map(|(_, body)| *body);

    let mut cursor = Cursor::new(header);
    let kind = cursor.fourcc().ok_or_else(short)?;
    let handler = cursor.fourcc().ok_or_else(short)?;
    cursor.skip(4).ok_or_else(short)?; // flags
    cursor.skip(4).ok_or_else(short)?; // priority and language
    cursor.le32().ok_or_else(short)?; // initial frames
    let scale = cursor.le32().ok_or_else(short)?;
    let rate = cursor.le32().ok_or_else(short)?;
    cursor.le32().ok_or_else(short)?; // start
    let length = cursor.le32().ok_or_else(short)?;

    match &kind {
        b"vids" => {
            let ratio = Rational::new(i64::from(rate), i64::from(scale)).map_err(|error| {
                MediaError::Malformed(format!(
                    "stream {index} declares a rate of {rate}/{scale}: {error}"
                ))
            })?;
            let timebase = Timebase::new(ratio).map_err(|error| {
                MediaError::Malformed(format!(
                    "stream {index} declares a rate of {rate}/{scale}, which is not one: {error}"
                ))
            })?;
            let mut size = (0_u32, 0_u32);
            let mut codec = codec::codec_of(handler);
            let mut picture = codec::undeclared_format();
            if let Some(format) = format {
                let mut cursor = Cursor::new(format);
                cursor.le32().ok_or_else(short)?; // biSize
                let width = cursor.le32().ok_or_else(short)? as i32;
                let height = cursor.le32().ok_or_else(short)? as i32;
                cursor.le16().ok_or_else(short)?; // biPlanes
                cursor.le16().ok_or_else(short)?; // biBitCount
                if let Some(compression) = cursor.fourcc() {
                    let named = codec::codec_of(compression);
                    if named != CodecKind::Unknown {
                        codec = named;
                    }
                    if let Some((layout, depth)) = codec::format_of_code(compression) {
                        picture.layout = layout;
                        picture.depth = depth;
                    }
                }
                // A negative height means the rows are stored top-down. It is an orientation
                // of the storage, not of the picture, so it changes no reported size.
                size = (width.unsigned_abs(), height.unsigned_abs());
            }
            Ok(StreamInfo::Video(VideoStream {
                index,
                codec,
                coded_size: size,
                sample_aspect: Rational::from_integer(1),
                timebase: Some(timebase),
                // One tick is one frame, which is what AVI's own rate and scale describe.
                stream_timebase: Rational::new(i64::from(scale), i64::from(rate))
                    .map_err(|error| MediaError::Malformed(format!("stream {index}: {error}")))?,
                frames: Some(u64::from(length)),
                rate: FrameRateKind::Constant(timebase),
                rotation: Rotation::None,
                field_order: FieldOrder::Progressive,
                picture,
            }))
        }
        b"auds" => {
            let mut channels = 0;
            let mut sample_rate = 0;
            let mut bits = None;
            let mut codec = CodecKind::Unknown;
            if let Some(format) = format {
                let mut cursor = Cursor::new(format);
                let tag = cursor.le16().ok_or_else(short)?;
                codec = match tag {
                    1 => CodecKind::PcmInteger,
                    3 => CodecKind::PcmFloat,
                    0x00FF => CodecKind::Aac,
                    _ => CodecKind::Unknown,
                };
                channels = cursor.le16().ok_or_else(short)?;
                sample_rate = cursor.le32().ok_or_else(short)?;
                cursor.le32().ok_or_else(short)?; // average bytes per second
                cursor.le16().ok_or_else(short)?; // block alignment
                bits = cursor.le16().and_then(|bits| u8::try_from(bits).ok());
            }
            Ok(StreamInfo::Audio(AudioStream {
                index,
                codec,
                channels,
                sample_rate,
                sample_bits: bits.filter(|bits| *bits > 0),
                stream_timebase: Rational::new(1, i64::from(sample_rate.max(1)))
                    .map_err(|error| MediaError::Malformed(format!("stream {index}: {error}")))?,
                frames: Some(u64::from(length)),
            }))
        }
        other => Ok(StreamInfo::Other {
            index,
            role: match other {
                b"txts" => crate::stream::StreamRole::Subtitle,
                _ => crate::stream::StreamRole::Data,
            },
        }),
    }
}

/// The one stream of a WAVE file.
fn wave(bytes: &[u8]) -> Result<Vec<StreamInfo>, MediaError> {
    let parts = chunks(bytes)?;
    let format = parts
        .iter()
        .find(|(id, _)| id == b"fmt ")
        .map(|(_, body)| *body)
        .ok_or_else(|| {
            MediaError::Malformed("the file has no format chunk: it declares no sound".to_owned())
        })?;
    let mut cursor = Cursor::new(format);
    let tag = cursor.le16().ok_or_else(short)?;
    let channels = cursor.le16().ok_or_else(short)?;
    let sample_rate = cursor.le32().ok_or_else(short)?;
    cursor.le32().ok_or_else(short)?; // average bytes per second
    let block = cursor.le16().ok_or_else(short)?;
    let bits = cursor.le16().ok_or_else(short)?;
    if sample_rate == 0 || channels == 0 {
        return Err(MediaError::Malformed(format!(
            "a WAVE declaring {channels} channels at {sample_rate} hertz"
        )));
    }
    let codec = match tag {
        1 => CodecKind::PcmInteger,
        3 => CodecKind::PcmFloat,
        // The extensible tag carries the real one in a sub-format this build does not read;
        // integer PCM is what it is in every file this build has met, and saying `Unknown`
        // would be truer. It says `Unknown`.
        _ => CodecKind::Unknown,
    };
    let data = parts
        .iter()
        .find(|(id, _)| id == b"data")
        .map(|(_, body)| body.len() as u64);
    let frames = match (data, block) {
        (Some(bytes), block) if block > 0 => Some(bytes / u64::from(block)),
        _ => None,
    };
    Ok(vec![StreamInfo::Audio(AudioStream {
        index: 0,
        codec,
        channels,
        sample_rate,
        sample_bits: u8::try_from(bits).ok().filter(|bits| *bits > 0),
        stream_timebase: Rational::new(1, i64::from(sample_rate))
            .map_err(|error| MediaError::Malformed(error.to_string()))?,
        frames,
    })])
}

/// The one message a structure that stops early produces.
fn short() -> MediaError {
    MediaError::Malformed("a RIFF chunk ends inside a field it declares".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(body);
        if body.len() % 2 == 1 {
            out.push(0);
        }
        out
    }

    #[test]
    fn a_chunk_walk_respects_the_pad_byte() {
        let mut bytes = chunk(b"odd ", &[1, 2, 3]);
        bytes.extend_from_slice(&chunk(b"next", &[4, 5]));
        let found = chunks(&bytes).expect("a walk");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].1, &[1, 2, 3]);
        assert_eq!(found[1].0, *b"next");
    }

    #[test]
    fn a_chunk_longer_than_the_bytes_stops_the_walk_rather_than_panicking() {
        let mut bytes = b"trun".to_vec();
        bytes.extend_from_slice(&1_000_u32.to_le_bytes());
        bytes.extend_from_slice(&[1, 2, 3]);
        assert!(chunks(&bytes).expect("a walk").is_empty());
    }

    #[test]
    fn a_wave_reports_its_sound() {
        let mut format = Vec::new();
        format.extend_from_slice(&1_u16.to_le_bytes());
        format.extend_from_slice(&2_u16.to_le_bytes());
        format.extend_from_slice(&48_000_u32.to_le_bytes());
        format.extend_from_slice(&192_000_u32.to_le_bytes());
        format.extend_from_slice(&4_u16.to_le_bytes());
        format.extend_from_slice(&16_u16.to_le_bytes());
        let mut body = chunk(b"fmt ", &format);
        body.extend_from_slice(&chunk(b"data", &[0; 400]));

        let streams = wave(&body).expect("readable");
        let audio = streams[0].as_audio().expect("sound");
        assert_eq!(audio.channels, 2);
        assert_eq!(audio.sample_rate, 48_000);
        assert_eq!(audio.sample_bits, Some(16));
        assert_eq!(audio.frames, Some(100));
        assert_eq!(audio.codec, CodecKind::PcmInteger);
    }

    #[test]
    fn a_wave_with_no_format_chunk_is_damaged() {
        let body = chunk(b"data", &[0; 4]);
        assert_eq!(wave(&body).expect_err("no fmt").label(), "damaged");
    }

    #[test]
    fn a_wave_declaring_no_channels_is_damaged() {
        let mut format = Vec::new();
        format.extend_from_slice(&1_u16.to_le_bytes());
        format.extend_from_slice(&0_u16.to_le_bytes());
        format.extend_from_slice(&0_u32.to_le_bytes());
        format.extend_from_slice(&0_u32.to_le_bytes());
        format.extend_from_slice(&0_u16.to_le_bytes());
        format.extend_from_slice(&0_u16.to_le_bytes());
        let body = chunk(b"fmt ", &format);
        assert_eq!(wave(&body).expect_err("no sound").label(), "damaged");
    }

    #[test]
    fn signatures() {
        assert!(looks_like(b"RIFF\0\0\0\0AVI "));
        assert!(!looks_like(b"RIFX"));
        assert!(!looks_like(b""));
    }
}
