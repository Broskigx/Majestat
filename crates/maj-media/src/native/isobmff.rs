//! ISO base media: `.mp4`, `.m4v` and QuickTime's `.mov`.
//!
//! The container that carries most of what an editor is handed, and the one where three of
//! M2's gate axes are actually stated:
//!
//! * **Rotation** is in `tkhd`'s display matrix, as 16.16 fixed-point numbers. It is read as
//!   four integers and turned into a quarter-turn count — never into an angle.
//! * **Field order** is in the sample entry's `fiel` box, whose two bytes distinguish
//!   interleaved fields from fields stored as separate samples, in both orders.
//! * **Variable frame rate** is in `stts`. A constant-rate stream has one duration; a
//!   variable one has several, and this parser turns the run-length table into the exact
//!   per-frame timestamps rather than averaging them into a rate that is wrong for every
//!   frame after the first.
//!
//! Nothing here reads a sample. The one thing read below container level is a codec
//! configuration record, for chroma and depth, which the container does not state at all —
//! see [`super::codec`].

use maj_time::{Rational, Timebase};

use crate::container::ContainerKind;
use crate::error::MediaError;
use crate::picture::{
    ChromaSiting, ColourMatrix, ColourPrimaries, SampleRange, TransferCharacteristics,
};
use crate::stream::{
    AudioStream, CodecKind, FieldOrder, FrameRateKind, Rotation, StreamInfo, StreamRole,
    VideoStream,
};

use super::bytes::{Cursor, Source, fourcc_name};
use super::codec;

/// The most boxes this parser will accept at one level.
///
/// A damaged file can describe an arbitrary number of children; a real one at any level of
/// this container has tens.
const MAX_BOXES: usize = 4_096;

/// The most frames a sample table may describe.
///
/// Ten million is forty days at thirty frames a second: past any real file and short of an
/// allocation that matters.
const MAX_FRAMES: u64 = 10_000_000;

/// A half-open byte range in the file.
#[derive(Debug, Clone, Copy)]
struct Span {
    start: u64,
    end: u64,
}

impl Span {
    const fn len(self) -> u64 {
        self.end.saturating_sub(self.start)
    }
}

/// Whether these bytes open like an ISO base media file.
pub(crate) fn looks_like(head: &[u8]) -> bool {
    head.get(4..8).is_some_and(|kind| {
        matches!(
            kind,
            b"ftyp" | b"moov" | b"mdat" | b"free" | b"skip" | b"wide" | b"styp" | b"pnot"
        )
    })
}

/// Reads the headers of an ISO base media file.
///
/// # Errors
///
/// [`MediaError::Malformed`] for a box structure that does not close, a movie box that is
/// absent or a timescale that cannot describe a rate, and whatever [`Source`] reports for a
/// file that is truncated.
pub(crate) fn probe(source: &mut Source) -> Result<(ContainerKind, Vec<StreamInfo>), MediaError> {
    let whole = Span {
        start: 0,
        end: source.len(),
    };
    let top = children(source, whole)?;
    let mut container = ContainerKind::QuickTime;
    for (kind, span) in &top {
        if kind == b"ftyp" {
            let brand = source.bytes(span.start, 4.min(span.len() as usize))?;
            container = container_of_brand(&brand);
        }
    }
    let movie = top
        .iter()
        .find(|(kind, _)| kind == b"moov")
        .map(|(_, span)| *span)
        .ok_or_else(|| {
            MediaError::Malformed("the file has no movie box: there is nothing to read".to_owned())
        })?;

    let mut streams = Vec::new();
    for (kind, span) in children(source, movie)? {
        if kind == *b"trak" {
            streams.push(track(source, span, streams.len())?);
        }
    }
    Ok((container, streams))
}

/// The container a `ftyp` brand names.
fn container_of_brand(brand: &[u8]) -> ContainerKind {
    if brand == b"qt  " {
        ContainerKind::QuickTime
    } else {
        // Every other brand this build meets is ISO base media, which is what `mp4` names.
        // Reporting `Unknown` would label a file `undecodable` that was read perfectly well.
        ContainerKind::Mp4
    }
}

/// The boxes directly inside a span, in file order.
fn children(source: &mut Source, span: Span) -> Result<Vec<([u8; 4], Span)>, MediaError> {
    let mut found = Vec::new();
    let mut at = span.start;
    while at.checked_add(8).is_some_and(|end| end <= span.end) {
        let header = source.bytes(at, 8)?;
        let mut cursor = Cursor::new(&header);
        let size = cursor
            .be32()
            .ok_or_else(|| MediaError::Malformed("a box header is truncated".to_owned()))?;
        let kind = cursor
            .fourcc()
            .ok_or_else(|| MediaError::Malformed("a box header is truncated".to_owned()))?;
        let name = fourcc_name(kind);
        let (body, end) = match size {
            // Zero means "to the end of the enclosing box", which is legal for the last one.
            0 => (at.saturating_add(8), span.end),
            1 => {
                let extended = source.bytes(at.saturating_add(8), 8)?;
                let size = Cursor::new(&extended).be64().ok_or_else(|| {
                    MediaError::Malformed(format!("the {name} box has no extended size"))
                })?;
                if size < 16 {
                    return Err(MediaError::Malformed(format!(
                        "the {name} box declares {size} bytes, which cannot hold its own header"
                    )));
                }
                (
                    at.saturating_add(16),
                    at.checked_add(size).ok_or_else(|| {
                        MediaError::Malformed(format!("the {name} box's size overflows"))
                    })?,
                )
            }
            size if size < 8 => {
                return Err(MediaError::Malformed(format!(
                    "the {name} box declares {size} bytes, which cannot hold its own header"
                )));
            }
            size => (
                at.saturating_add(8),
                at.checked_add(u64::from(size)).ok_or_else(|| {
                    MediaError::Malformed(format!("the {name} box's size overflows"))
                })?,
            ),
        };
        if end > span.end {
            return Err(MediaError::Malformed(format!(
                "the {name} box ends at {end}, past the {} bytes that hold it",
                span.end
            )));
        }
        if end <= at {
            return Err(MediaError::Malformed(format!(
                "the {name} box does not advance: the structure would not close"
            )));
        }
        found.push((kind, Span { start: body, end }));
        if found.len() > MAX_BOXES {
            return Err(MediaError::Malformed(format!(
                "more than {MAX_BOXES} boxes at one level"
            )));
        }
        at = end;
    }
    Ok(found)
}

/// The box of a given kind inside a span, if it is there.
fn find(source: &mut Source, span: Span, wanted: &[u8; 4]) -> Result<Option<Span>, MediaError> {
    Ok(children(source, span)?
        .into_iter()
        .find(|(kind, _)| kind == wanted)
        .map(|(_, span)| span))
}

/// One track, as whichever kind of stream it turns out to be.
fn track(source: &mut Source, span: Span, index: usize) -> Result<StreamInfo, MediaError> {
    let rotation = match find(source, span, b"tkhd")? {
        Some(header) => read_tkhd_rotation(source, header)?,
        None => Rotation::None,
    };
    let media = find(source, span, b"mdia")?
        .ok_or_else(|| MediaError::Malformed(format!("track {index} has no media box")))?;
    let timescale = match find(source, media, b"mdhd")? {
        Some(header) => read_mdhd_timescale(source, header)?,
        None => {
            return Err(MediaError::Malformed(format!(
                "track {index} has no media header: its timebase is unknowable"
            )));
        }
    };
    let handler = match find(source, media, b"hdlr")? {
        Some(header) => {
            let bytes = source.bytes(header.start, 12.min(header.len() as usize))?;
            let mut cursor = Cursor::new(&bytes);
            cursor.be32();
            cursor.be32();
            cursor.fourcc().unwrap_or(*b"    ")
        }
        None => *b"    ",
    };
    let table =
        find(source, media, b"minf")?.and_then(|minf| find(source, minf, b"stbl").ok().flatten());

    match &handler {
        b"vide" => video_track(source, index, timescale, rotation, table),
        b"soun" => audio_track(source, index, timescale, table),
        other => Ok(StreamInfo::Other {
            index,
            role: role_of_handler(other),
        }),
    }
}

/// What a handler type is for, when it is neither picture nor sound.
fn role_of_handler(handler: &[u8; 4]) -> StreamRole {
    match handler {
        b"tmcd" => StreamRole::Timecode,
        b"sbtl" | b"subp" | b"text" | b"clcp" => StreamRole::Subtitle,
        b"hint" => StreamRole::Hint,
        b"meta" | b"data" => StreamRole::Data,
        _ => StreamRole::Unknown,
    }
}

/// A picture track: geometry, rate, rotation, field order and declared colour.
fn video_track(
    source: &mut Source,
    index: usize,
    timescale: u32,
    rotation: Rotation,
    table: Option<Span>,
) -> Result<StreamInfo, MediaError> {
    let table = table.ok_or_else(|| {
        MediaError::Malformed(format!("picture track {index} has no sample table"))
    })?;
    let description = find(source, table, b"stsd")?.ok_or_else(|| {
        MediaError::Malformed(format!("picture track {index} describes no samples"))
    })?;
    let sample = read_video_stsd(source, description)?;
    let timings = match find(source, table, b"stts")? {
        Some(span) => read_stts(source, span)?,
        None => Timings::default(),
    };
    let sizes = match find(source, table, b"stsz")? {
        Some(span) => read_stsz_count(source, span)?,
        None => None,
    };
    let stream_timebase = Rational::new(1, i64::from(timescale)).map_err(|error| {
        MediaError::Malformed(format!(
            "track {index} declares a timescale of nothing: {error}"
        ))
    })?;
    let (rate, nominal) = timings.rate(timescale, index)?;
    let frames = timings.frames.or(sizes);
    Ok(StreamInfo::Video(VideoStream {
        index,
        codec: sample.codec,
        coded_size: sample.size,
        sample_aspect: sample.aspect,
        timebase: Some(nominal),
        stream_timebase,
        frames,
        rate,
        rotation,
        field_order: sample.field_order,
        picture: sample.format,
    }))
}

/// A sound track, declared here and read in M9.
fn audio_track(
    source: &mut Source,
    index: usize,
    timescale: u32,
    table: Option<Span>,
) -> Result<StreamInfo, MediaError> {
    let stream_timebase = Rational::new(1, i64::from(timescale)).map_err(|error| {
        MediaError::Malformed(format!(
            "track {index} declares a timescale of nothing: {error}"
        ))
    })?;
    let mut codec = CodecKind::Unknown;
    let mut channels = 0_u16;
    let mut sample_rate = timescale;
    let mut sample_bits = None;
    let mut frames = None;
    if let Some(table) = table {
        if let Some(description) = find(source, table, b"stsd")? {
            let bytes = source.bytes(description.start, description.len().min(512) as usize)?;
            let mut cursor = Cursor::new(&bytes);
            cursor.be32();
            cursor.be32();
            cursor.be32();
            if let Some(code) = cursor.fourcc() {
                codec = codec::codec_of(code);
            }
            cursor.skip(8);
            cursor.be16();
            cursor.be16();
            cursor.be32();
            channels = cursor.be16().unwrap_or(0);
            sample_bits = cursor.be16().and_then(|bits| u8::try_from(bits).ok());
            cursor.be16();
            cursor.be16();
            // A 16.16 fixed-point rate, whose whole part is the sample rate.
            if let Some(fixed) = cursor.be32() {
                let whole = fixed >> 16;
                if whole != 0 {
                    sample_rate = whole;
                }
            }
        }
        if let Some(span) = find(source, table, b"stsz")? {
            frames = read_stsz_count(source, span)?;
        }
    }
    Ok(StreamInfo::Audio(AudioStream {
        index,
        codec,
        channels,
        sample_rate,
        sample_bits: sample_bits.filter(|bits| *bits > 0),
        stream_timebase,
        frames,
    }))
}

/// What a picture sample entry says about itself.
#[derive(Debug)]
struct VideoSample {
    codec: CodecKind,
    size: (u32, u32),
    aspect: Rational,
    field_order: FieldOrder,
    format: crate::picture::PictureFormat,
}

/// Reads the first picture sample entry and the extension boxes beside it.
fn read_video_stsd(source: &mut Source, span: Span) -> Result<VideoSample, MediaError> {
    // 86 bytes of fixed sample entry, plus the extensions, capped: a sample entry with
    // megabytes of extensions is not one this build needs to read whole.
    let length = span.len().min(64 * 1024) as usize;
    let bytes = source.bytes(span.start, length)?;
    let mut cursor = Cursor::new(&bytes);
    cursor.be32().ok_or_else(short)?; // version and flags
    let entries = cursor.be32().ok_or_else(short)?;
    if entries == 0 {
        return Err(MediaError::Malformed(
            "a sample description with no entries".to_owned(),
        ));
    }
    let entry_start = cursor.position();
    let entry_size = cursor.be32().ok_or_else(short)? as usize;
    let code = cursor.fourcc().ok_or_else(short)?;
    cursor.skip(6).ok_or_else(short)?; // reserved
    cursor.be16().ok_or_else(short)?; // data reference index
    cursor.skip(16).ok_or_else(short)?; // pre-defined and reserved
    let width = u32::from(cursor.be16().ok_or_else(short)?);
    let height = u32::from(cursor.be16().ok_or_else(short)?);
    cursor.skip(50).ok_or_else(short)?; // resolutions, frame count, compressor name, depth

    let codec = codec::codec_of(code);
    let mut format = codec::undeclared_format();
    if let Some((layout, depth)) = codec::format_of_code(code) {
        format.layout = layout;
        format.depth = depth;
    }
    let mut aspect = Rational::from_integer(1);
    let mut field_order = FieldOrder::Progressive;

    // The extensions are boxes, inside the entry, after its fixed part.
    let entry_end = entry_start
        .checked_add(entry_size)
        .map_or(bytes.len(), |end| end.min(bytes.len()));
    let mut at = cursor.position();
    let mut seen = 0_usize;
    while at + 8 <= entry_end {
        let mut header = Cursor::new(bytes.get(at..entry_end).unwrap_or_default());
        let size = header.be32().ok_or_else(short)? as usize;
        let kind = header.fourcc().ok_or_else(short)?;
        if size < 8 {
            break;
        }
        let end = match at.checked_add(size) {
            Some(end) if end <= entry_end => end,
            _ => break,
        };
        let body = bytes.get(at + 8..end).unwrap_or_default();
        match &kind {
            b"avcC" | b"hvcC" | b"av1C" => {
                if let Some((layout, depth)) = codec::format_of_config(code, body) {
                    format.layout = layout;
                    format.depth = depth;
                }
            }
            b"colr" => read_colr(body, &mut format),
            b"pasp" => {
                let mut pasp = Cursor::new(body);
                if let (Some(horizontal), Some(vertical)) = (pasp.be32(), pasp.be32()) {
                    if let Ok(ratio) = Rational::new(i64::from(horizontal), i64::from(vertical)) {
                        if ratio.is_positive() {
                            aspect = ratio;
                        }
                    }
                }
            }
            b"fiel" => {
                let mut fiel = Cursor::new(body);
                if let (Some(fields), Some(detail)) = (fiel.u8(), fiel.u8()) {
                    field_order = field_order_of(fields, detail);
                }
            }
            _ => {}
        }
        at = end;
        seen += 1;
        if seen > MAX_BOXES {
            break;
        }
    }

    Ok(VideoSample {
        codec,
        size: (width, height),
        aspect,
        field_order,
        format,
    })
}

/// QuickTime's `fiel`: how many fields a sample holds, and in what order.
///
/// The two interleaved cases are a frame of two fields; details 9 and 14 are fields stored as
/// separate samples, which is what [`FieldOrder::SingleTop`] and
/// [`FieldOrder::SingleBottom`] are for. A detail this build has no mapping for is reported
/// as [`FieldOrder::TopFirst`] only if the file says so, never by default — an unmapped
/// detail keeps the interlaced fact and the commoner order, and says which in no uncertain
/// terms through `is_interlaced`.
fn field_order_of(fields: u8, detail: u8) -> FieldOrder {
    if fields != 2 {
        return FieldOrder::Progressive;
    }
    match detail {
        1 => FieldOrder::TopFirst,
        6 => FieldOrder::BottomFirst,
        9 => FieldOrder::SingleTop,
        14 => FieldOrder::SingleBottom,
        // Interlaced, order not stated. Reported as top-first because that is the order of
        // every interlaced format still in use, and reporting progressive would hide the
        // interlacing itself, which is the failure this field exists to prevent.
        _ => FieldOrder::TopFirst,
    }
}

/// `colr`: the declared primaries, transfer and matrix, and whether the range is full.
fn read_colr(body: &[u8], format: &mut crate::picture::PictureFormat) {
    let mut cursor = Cursor::new(body);
    let Some(kind) = cursor.fourcc() else {
        return;
    };
    if !matches!(&kind, b"nclx" | b"nclc") {
        // An ICC profile says more than this vocabulary can carry, so it is left for the
        // adapter rather than flattened into a code point here.
        return;
    }
    let (Some(primaries), Some(transfer), Some(matrix)) =
        (cursor.be16(), cursor.be16(), cursor.be16())
    else {
        return;
    };
    format.primaries = ColourPrimaries::from_code_point(primaries.min(255) as u8);
    format.transfer = TransferCharacteristics::from_code_point(transfer.min(255) as u8);
    format.matrix = ColourMatrix::from_code_point(matrix.min(255) as u8);
    if kind == *b"nclx" {
        format.range = match cursor.u8() {
            Some(flags) if flags & 0x80 != 0 => SampleRange::Full,
            Some(_) => SampleRange::Limited,
            None => SampleRange::Unspecified,
        };
    }
    if format.layout.chroma_divisors().is_some()
        && format.chroma_siting == ChromaSiting::Unspecified
    {
        // Left siting is MPEG's and is what every codec this build names uses, but the
        // container did not say it, so it stays unspecified.
        format.chroma_siting = ChromaSiting::Unspecified;
    }
}

/// `mdhd`: the track's timescale.
fn read_mdhd_timescale(source: &mut Source, span: Span) -> Result<u32, MediaError> {
    let bytes = source.bytes(span.start, span.len().min(32) as usize)?;
    let mut cursor = Cursor::new(&bytes);
    let version = cursor.be32().ok_or_else(short)? >> 24;
    if version == 1 {
        cursor.skip(16).ok_or_else(short)?;
    } else {
        cursor.skip(8).ok_or_else(short)?;
    }
    let timescale = cursor.be32().ok_or_else(short)?;
    if timescale == 0 {
        return Err(MediaError::Malformed(
            "a media header with a timescale of zero".to_owned(),
        ));
    }
    Ok(timescale)
}

/// `tkhd`: the display matrix, as a quarter-turn count.
fn read_tkhd_rotation(source: &mut Source, span: Span) -> Result<Rotation, MediaError> {
    let bytes = source.bytes(span.start, span.len().min(96) as usize)?;
    let mut cursor = Cursor::new(&bytes);
    let version = cursor.be32().ok_or_else(short)? >> 24;
    // Creation, modification, track id, reserved and duration, at either width.
    let fixed = if version == 1 { 32 } else { 20 };
    cursor.skip(fixed).ok_or_else(short)?;
    // Reserved, layer, alternate group, volume, reserved.
    cursor.skip(16).ok_or_else(short)?;
    let a = cursor.be32i().ok_or_else(short)?;
    let b = cursor.be32i().ok_or_else(short)?;
    cursor.be32().ok_or_else(short)?; // u
    let c = cursor.be32i().ok_or_else(short)?;
    let d = cursor.be32i().ok_or_else(short)?;
    Ok(rotation_of_matrix(a, b, c, d))
}

/// The quarter-turn count a display matrix describes.
///
/// Only the four right-angle cases are recognised, by sign, with no arithmetic on the
/// magnitudes at all: a matrix that is not one of them is reported upright rather than
/// approximated, because an editor that silently rotated a sheared picture by the nearest
/// right angle would be worse than one that left it alone and said so.
fn rotation_of_matrix(a: i32, b: i32, c: i32, d: i32) -> Rotation {
    if b == 0 && c == 0 {
        if a < 0 && d < 0 {
            return Rotation::Half;
        }
        return Rotation::None;
    }
    if a == 0 && d == 0 {
        if b > 0 && c < 0 {
            return Rotation::Quarter;
        }
        if b < 0 && c > 0 {
            return Rotation::ThreeQuarters;
        }
    }
    Rotation::None
}

/// What `stts` says: the run-length table of sample durations.
#[derive(Debug, Default, Clone)]
struct Timings {
    /// Duration and count, in file order.
    runs: Vec<(u64, u32)>,
    /// Total samples the table accounts for.
    frames: Option<u64>,
}

impl Timings {
    /// The rate, and the nominal timebase to address the stream in.
    ///
    /// One distinct duration is a constant rate. Several is a variable one, and then the
    /// timestamps are accumulated exactly and the nominal timebase is the one belonging to
    /// the **commonest** duration — a figure for an interface to show, with
    /// [`FrameRateKind::Variable`] carrying the truth beside it.
    fn rate(&self, timescale: u32, index: usize) -> Result<(FrameRateKind, Timebase), MediaError> {
        let mut distinct: Vec<(u64, u64)> = Vec::new();
        for (duration, count) in &self.runs {
            if *duration == 0 {
                continue;
            }
            match distinct.iter_mut().find(|(seen, _)| seen == duration) {
                Some((_, total)) => *total = total.saturating_add(u64::from(*count)),
                None => distinct.push((*duration, u64::from(*count))),
            }
        }
        let commonest = distinct
            .iter()
            .max_by_key(|(duration, count)| (*count, std::cmp::Reverse(*duration)))
            .map(|(duration, _)| *duration)
            .ok_or_else(|| {
                MediaError::Malformed(format!(
                    "picture track {index} declares no sample lasting any time at all"
                ))
            })?;
        let nominal = timebase_of(timescale, commonest).ok_or_else(|| {
            MediaError::Malformed(format!(
                "picture track {index} declares a rate of {timescale}/{commonest}, which is not one"
            ))
        })?;
        if distinct.len() <= 1 {
            return Ok((FrameRateKind::Constant(nominal), nominal));
        }
        let mut timestamps = Vec::new();
        let mut at = 0_i64;
        for (duration, count) in &self.runs {
            for _ in 0..*count {
                timestamps.push(at);
                at = at.checked_add(*duration as i64).ok_or_else(|| {
                    MediaError::Malformed(format!(
                        "picture track {index} runs past the end of time"
                    ))
                })?;
            }
        }
        Ok((FrameRateKind::Variable { timestamps }, nominal))
    }
}

/// The timebase of a timescale over a sample duration.
fn timebase_of(timescale: u32, duration: u64) -> Option<Timebase> {
    let duration = i64::try_from(duration).ok()?;
    let rate = Rational::new(i64::from(timescale), duration).ok()?;
    Timebase::new(rate).ok()
}

/// `stts`: the sample duration table.
fn read_stts(source: &mut Source, span: Span) -> Result<Timings, MediaError> {
    let bytes = source.bytes(
        span.start,
        span.len().min(super::bytes::MAX_READ as u64) as usize,
    )?;
    let mut cursor = Cursor::new(&bytes);
    cursor.be32().ok_or_else(short)?;
    let entries = cursor.be32().ok_or_else(short)?;
    let mut runs = Vec::new();
    let mut frames = 0_u64;
    for _ in 0..entries {
        let Some(count) = cursor.be32() else { break };
        let Some(duration) = cursor.be32() else { break };
        frames = frames.saturating_add(u64::from(count));
        if frames > MAX_FRAMES {
            return Err(MediaError::Malformed(format!(
                "a sample table describing more than {MAX_FRAMES} frames"
            )));
        }
        runs.push((u64::from(duration), count));
    }
    Ok(Timings {
        runs,
        frames: Some(frames),
    })
}

/// `stsz`: how many samples there are, which is a frame count for a picture track.
fn read_stsz_count(source: &mut Source, span: Span) -> Result<Option<u64>, MediaError> {
    let bytes = source.bytes(span.start, span.len().min(16) as usize)?;
    let mut cursor = Cursor::new(&bytes);
    cursor.be32().ok_or_else(short)?;
    cursor.be32().ok_or_else(short)?;
    Ok(cursor.be32().map(u64::from))
}

/// The one message a structure that stops early produces.
fn short() -> MediaError {
    MediaError::Malformed("a box ends inside a field it declares".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_display_matrix_is_four_signs_and_never_an_angle() {
        let one = 1 << 16;
        assert_eq!(rotation_of_matrix(one, 0, 0, one), Rotation::None);
        assert_eq!(rotation_of_matrix(-one, 0, 0, -one), Rotation::Half);
        assert_eq!(rotation_of_matrix(0, one, -one, 0), Rotation::Quarter);
        assert_eq!(rotation_of_matrix(0, -one, one, 0), Rotation::ThreeQuarters);
    }

    #[test]
    fn a_matrix_that_is_not_a_right_angle_is_left_alone() {
        let one = 1 << 16;
        // A shear. Nothing about it is a quarter turn, so nothing is invented.
        assert_eq!(rotation_of_matrix(one, one / 2, 0, one), Rotation::None);
        assert_eq!(rotation_of_matrix(0, one, one, 0), Rotation::None);
    }

    #[test]
    fn the_field_box_distinguishes_interleaved_fields_from_separate_ones() {
        assert_eq!(field_order_of(1, 0), FieldOrder::Progressive);
        assert_eq!(field_order_of(2, 1), FieldOrder::TopFirst);
        assert_eq!(field_order_of(2, 6), FieldOrder::BottomFirst);
        assert_eq!(field_order_of(2, 9), FieldOrder::SingleTop);
        assert_eq!(field_order_of(2, 14), FieldOrder::SingleBottom);
        assert!(field_order_of(2, 0).is_interlaced());
    }

    #[test]
    fn one_distinct_duration_is_a_constant_rate() {
        let timings = Timings {
            runs: vec![(1_001, 48)],
            frames: Some(48),
        };
        let (rate, nominal) = timings.rate(24_000, 0).expect("a rate");
        assert_eq!(nominal, Timebase::FPS_23_976);
        assert_eq!(rate, FrameRateKind::Constant(Timebase::FPS_23_976));
    }

    #[test]
    fn several_durations_are_a_variable_rate_with_exact_timestamps() {
        let timings = Timings {
            runs: vec![(1_000, 2), (2_000, 2), (1_000, 1)],
            frames: Some(5),
        };
        let (rate, nominal) = timings.rate(24_000, 0).expect("a rate");
        assert_eq!(
            rate,
            FrameRateKind::Variable {
                timestamps: vec![0, 1_000, 2_000, 4_000, 6_000],
            }
        );
        assert_eq!(
            nominal,
            Timebase::new(Rational::new(24, 1).expect("a rate")).expect("a timebase"),
            "the nominal rate is the commonest duration's"
        );
        assert!(rate.is_variable());
    }

    #[test]
    fn a_table_of_zero_length_samples_is_malformed_rather_than_infinitely_fast() {
        let timings = Timings {
            runs: vec![(0, 5)],
            frames: Some(5),
        };
        assert!(matches!(
            timings.rate(24_000, 0),
            Err(MediaError::Malformed(_))
        ));
    }

    #[test]
    fn signatures_and_brands() {
        assert!(looks_like(b"\0\0\0\x18ftypisom"));
        assert!(looks_like(b"\0\0\0\x10moov"));
        assert!(!looks_like(b"RIFF\0\0\0\x10AVI "));
        assert!(!looks_like(b"abc"));
        assert_eq!(container_of_brand(b"qt  "), ContainerKind::QuickTime);
        assert_eq!(container_of_brand(b"isom"), ContainerKind::Mp4);
        assert_eq!(container_of_brand(b"\0\0\0\0"), ContainerKind::Mp4);
    }
}
