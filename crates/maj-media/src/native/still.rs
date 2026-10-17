//! The two stills the renderer already reads, probed rather than decoded.
//!
//! These two formats matter out of proportion to their size, because they are the ones
//! `maj-render` can actually open today. The debt this crate repays — a present-but-damaged
//! still named for its container in a manifest while the frame beside it was a `damaged`
//! placeholder — is a debt about a PNG. So the probe's verdict on a still has to agree with
//! the reader's, and that fixes how much validation belongs here:
//!
//! * **PNG**: the signature, every chunk's length and CRC, a well-formed `IHDR`, a colour type
//!   and depth this build reads, a non-empty `IDAT` and a terminating `IEND`. That is every
//!   structural check `maj_render::Png` makes, and not one byte of inflation — the compressed
//!   data is covered by the CRCs it sits under.
//! * **Netpbm**: the magic, the header numbers, and that the body is long enough for the size
//!   it declares. Samples are not parsed.
//!
//! What is left is one honest residual: a PNG whose chunk CRCs are all correct and whose
//! deflate stream is internally inconsistent would probe readable and fail to read. Catching
//! that means inflating, which is a decode, which is the one thing `kind` may never cost.

use maj_time::Rational;

use crate::error::MediaError;
use crate::picture::{
    BitDepth, ChromaSiting, ColourMatrix, ColourPrimaries, PictureFormat, PlaneLayout, SampleRange,
    TransferCharacteristics,
};
use crate::stream::{CodecKind, FieldOrder, FrameRateKind, Rotation, StreamInfo, VideoStream};

/// The eight bytes every PNG starts with.
pub(crate) const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Whether these bytes open with the PNG signature.
pub(crate) fn looks_like_png(head: &[u8]) -> bool {
    head.len() >= PNG_SIGNATURE.len() && head[..8] == PNG_SIGNATURE
}

/// Whether these bytes open with a Netpbm magic number.
pub(crate) fn looks_like_netpbm(head: &[u8]) -> bool {
    matches!(head, [b'P', second, ..] if (b'1'..=b'6').contains(second))
}

/// The one stream of a PNG still.
///
/// # Errors
///
/// [`MediaError::Malformed`] for a file whose structure or checksums are wrong, and
/// [`MediaError::Unsupported`] for a PNG whose colour type, depth or interlacing this build
/// does not read — which are exactly `maj_render::Png`'s own two refusals.
pub(crate) fn probe_png(bytes: &[u8]) -> Result<Vec<StreamInfo>, MediaError> {
    if !looks_like_png(bytes) {
        return Err(MediaError::Malformed(
            "the bytes are not a PNG: the signature is missing".to_owned(),
        ));
    }
    let mut offset = PNG_SIGNATURE.len();
    let mut header: Option<[u8; 13]> = None;
    let mut data_bytes: u64 = 0;
    let mut saw_end = false;
    let mut palette_entries = 0_usize;
    while offset + 12 <= bytes.len() {
        let length = bytes
            .get(offset..offset + 4)
            .and_then(|slice| slice.try_into().ok())
            .map(u32::from_be_bytes)
            .ok_or_else(|| MediaError::Malformed("a chunk header is truncated".to_owned()))?
            as usize;
        let kind: [u8; 4] = bytes
            .get(offset + 4..offset + 8)
            .and_then(|slice| slice.try_into().ok())
            .ok_or_else(|| MediaError::Malformed("a chunk kind is truncated".to_owned()))?;
        let name = super::bytes::fourcc_name(kind);
        let end = offset
            .checked_add(8)
            .and_then(|start| start.checked_add(length))
            .ok_or_else(|| MediaError::Malformed(format!("the {name} chunk's length overflows")))?;
        let body = bytes
            .get(offset + 8..end)
            .ok_or_else(|| MediaError::Malformed(format!("the {name} chunk is truncated")))?;
        let declared = bytes
            .get(end..end + 4)
            .and_then(|slice| slice.try_into().ok())
            .map(u32::from_be_bytes)
            .ok_or_else(|| MediaError::Malformed(format!("the {name} chunk has no checksum")))?;
        let covered = bytes
            .get(offset + 4..end)
            .ok_or_else(|| MediaError::Malformed(format!("the {name} chunk is truncated")))?;
        if declared != crc32(covered) {
            return Err(MediaError::Malformed(format!(
                "the {name} chunk fails its checksum"
            )));
        }
        match &kind {
            b"IHDR" => {
                if length != 13 {
                    return Err(MediaError::Malformed(format!(
                        "the header is {length} bytes, not 13"
                    )));
                }
                header = body.try_into().ok();
            }
            b"PLTE" => {
                if length % 3 != 0 {
                    return Err(MediaError::Malformed(
                        "the palette is not a whole number of entries".to_owned(),
                    ));
                }
                palette_entries = length / 3;
            }
            b"IDAT" => data_bytes = data_bytes.saturating_add(length as u64),
            b"IEND" => {
                saw_end = true;
                break;
            }
            _ => {}
        }
        offset = end + 4;
    }
    let header =
        header.ok_or_else(|| MediaError::Malformed("the file has no header chunk".to_owned()))?;
    if data_bytes == 0 {
        return Err(MediaError::Malformed(
            "the file holds no image data".to_owned(),
        ));
    }
    if !saw_end {
        return Err(MediaError::Malformed(
            "the file has no end chunk: it is truncated".to_owned(),
        ));
    }
    let width = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
    let height = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
    let depth = header[8];
    let colour = header[9];
    if width == 0 || height == 0 {
        return Err(MediaError::Malformed(
            "a PNG needs a non-zero size".to_owned(),
        ));
    }
    if header[12] != 0 {
        return Err(MediaError::Unsupported(
            "an interlaced PNG, which this build does not read".to_owned(),
        ));
    }
    let layout = match colour {
        0 => PlaneLayout::Monochrome,
        2 => PlaneLayout::Rgb,
        // An indexed PNG's samples are colours once the palette is applied.
        3 => PlaneLayout::Rgb,
        4 => PlaneLayout::Monochrome,
        6 => PlaneLayout::Rgba,
        other => {
            return Err(MediaError::Unsupported(format!("PNG colour type {other}")));
        }
    };
    let allowed = match colour {
        0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
        3 => matches!(depth, 1 | 2 | 4 | 8),
        _ => matches!(depth, 8 | 16),
    };
    if !allowed {
        return Err(MediaError::Unsupported(format!(
            "{depth} bits per sample for PNG colour type {colour}"
        )));
    }
    if colour == 3 && palette_entries == 0 {
        return Err(MediaError::Malformed(
            "an indexed PNG with no palette".to_owned(),
        ));
    }
    // A depth below eight is a packed representation: every reader in this workspace expands
    // it losslessly to eight bits, so eight is the depth the samples arrive at and the number
    // a caller sizing a buffer needs. Nothing is lost and nothing is rounded.
    let reported = BitDepth::from_bits(depth).unwrap_or(BitDepth::Eight);
    Ok(vec![StreamInfo::Video(still_stream(
        CodecKind::Png,
        (width, height),
        PictureFormat {
            layout,
            depth: reported,
            range: SampleRange::Full,
            matrix: ColourMatrix::Identity,
            primaries: ColourPrimaries::Unspecified,
            // PNG's own default, which is what a file without an `iCCP` or `sRGB` chunk means.
            transfer: TransferCharacteristics::Srgb,
            chroma_siting: ChromaSiting::Unspecified,
        },
    ))])
}

/// The one stream of a Netpbm still.
///
/// # Errors
///
/// [`MediaError::Malformed`] for a header that does not parse or a body too short for the
/// size it declares, and [`MediaError::Unsupported`] for a magic number `maj_render::Ppm`
/// does not read.
pub(crate) fn probe_netpbm(bytes: &[u8]) -> Result<Vec<StreamInfo>, MediaError> {
    let mut reader = Tokens::new(bytes);
    let magic = reader
        .token()
        .ok_or_else(|| MediaError::Malformed("the file has no magic number".to_owned()))?;
    let width = reader
        .number()
        .ok_or_else(|| MediaError::Malformed("the header has no width".to_owned()))?;
    let height = reader
        .number()
        .ok_or_else(|| MediaError::Malformed("the header has no height".to_owned()))?;
    let maximum = reader
        .number()
        .ok_or_else(|| MediaError::Malformed("the header has no maximum sample".to_owned()))?;
    if width == 0 || height == 0 {
        return Err(MediaError::Malformed(
            "a PPM needs a non-zero size".to_owned(),
        ));
    }
    if maximum == 0 || maximum > 65_535 {
        return Err(MediaError::Malformed(format!(
            "PPM maximum sample value {maximum} is out of range"
        )));
    }
    let magic = String::from_utf8_lossy(magic).to_string();
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| MediaError::Malformed("the declared size overflows".to_owned()))?;
    let samples = pixels
        .checked_mul(3)
        .ok_or_else(|| MediaError::Malformed("the declared size overflows".to_owned()))?;
    match magic.as_str() {
        "P6" => {
            let wide = maximum > 255;
            let needed = samples
                .checked_mul(if wide { 2 } else { 1 })
                .ok_or_else(|| MediaError::Malformed("the declared size overflows".to_owned()))?;
            // One byte of separator after the maximum, exactly as the reader consumes.
            let body = reader.rest_after_separator().ok_or_else(|| {
                MediaError::Malformed("the header has no separator after it".to_owned())
            })?;
            if (body.len() as u64) < needed {
                return Err(MediaError::Malformed(format!(
                    "P6 body holds {} bytes for {pixels} pixels, {needed} are needed",
                    body.len()
                )));
            }
        }
        "P3" => {
            // Counting tokens, not parsing them: enough to know the body is complete, and it
            // keeps this agreeing with the reader on a truncated file.
            let found = reader.count_tokens();
            if (found as u64) < samples {
                return Err(MediaError::Malformed(format!(
                    "P3 body holds {found} samples for {pixels} pixels, {samples} are needed"
                )));
            }
        }
        other => {
            return Err(MediaError::Unsupported(format!("PPM magic `{other}`")));
        }
    }
    let depth = if maximum > 255 {
        BitDepth::Sixteen
    } else {
        BitDepth::Eight
    };
    Ok(vec![StreamInfo::Video(still_stream(
        CodecKind::Netpbm,
        (width, height),
        PictureFormat {
            layout: PlaneLayout::Rgb,
            depth,
            range: SampleRange::Full,
            matrix: ColourMatrix::Identity,
            primaries: ColourPrimaries::Unspecified,
            transfer: TransferCharacteristics::Unspecified,
            chroma_siting: ChromaSiting::Unspecified,
        },
    ))])
}

/// The stream a single still is.
///
/// One frame, at time zero, in a stream that imposes no rate — which is why `timebase` is
/// `None` and the rate is one enumerated timestamp rather than a frames-per-second figure
/// nobody stated. A still addressed by a sequence is addressed at the sequence's rate.
fn still_stream(codec: CodecKind, size: (u32, u32), picture: PictureFormat) -> VideoStream {
    VideoStream {
        index: 0,
        codec,
        coded_size: size,
        sample_aspect: Rational::from_integer(1),
        timebase: None,
        stream_timebase: Rational::from_integer(1),
        frames: Some(1),
        rate: FrameRateKind::Variable {
            timestamps: vec![0],
        },
        rotation: Rotation::None,
        field_order: FieldOrder::Progressive,
        picture,
    }
}

/// Whitespace-separated tokens with `#` comments, which is how a Netpbm header is written.
struct Tokens<'bytes> {
    bytes: &'bytes [u8],
    position: usize,
}

impl<'bytes> Tokens<'bytes> {
    const fn new(bytes: &'bytes [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    /// Skips whitespace and comments, leaving the position on a token or at the end.
    fn skip_blanks(&mut self) {
        while let Some(byte) = self.bytes.get(self.position) {
            if byte.is_ascii_whitespace() {
                self.position += 1;
            } else if *byte == b'#' {
                while let Some(byte) = self.bytes.get(self.position) {
                    self.position += 1;
                    if *byte == b'\n' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    fn token(&mut self) -> Option<&'bytes [u8]> {
        self.skip_blanks();
        let start = self.position;
        while self
            .bytes
            .get(self.position)
            .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b'#')
        {
            self.position += 1;
        }
        if self.position == start {
            return None;
        }
        self.bytes.get(start..self.position)
    }

    fn number(&mut self) -> Option<u32> {
        let token = self.token()?;
        let text = std::str::from_utf8(token).ok()?;
        text.parse().ok()
    }

    /// Everything after exactly one separator byte, which is how a binary body begins.
    fn rest_after_separator(&mut self) -> Option<&'bytes [u8]> {
        let separator = self.bytes.get(self.position)?;
        if !separator.is_ascii_whitespace() {
            return None;
        }
        self.position += 1;
        self.bytes.get(self.position..)
    }

    fn count_tokens(&mut self) -> usize {
        let mut found = 0;
        while self.token().is_some() {
            found += 1;
        }
        found
    }
}

/// CRC-32 as PNG defines it, which is the one every chunk carries.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        let mut covered = kind.to_vec();
        covered.extend_from_slice(body);
        out.extend_from_slice(&crc32(&covered).to_be_bytes());
        out
    }

    fn png(width: u32, height: u32, depth: u8, colour: u8) -> Vec<u8> {
        let mut header = Vec::new();
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        header.extend_from_slice(&[depth, colour, 0, 0, 0]);
        let mut out = PNG_SIGNATURE.to_vec();
        out.extend_from_slice(&chunk(b"IHDR", &header));
        if colour == 3 {
            out.extend_from_slice(&chunk(b"PLTE", &[0, 0, 0]));
        }
        out.extend_from_slice(&chunk(b"IDAT", &[1, 2, 3, 4]));
        out.extend_from_slice(&chunk(b"IEND", &[]));
        out
    }

    #[test]
    fn a_png_reports_its_geometry_and_depth() {
        let streams = probe_png(&png(16, 9, 16, 6)).expect("a readable PNG");
        let stream = streams[0].as_video().expect("a still is a picture");
        assert_eq!(stream.coded_size, (16, 9));
        assert_eq!(stream.picture.depth, BitDepth::Sixteen);
        assert_eq!(stream.picture.layout, PlaneLayout::Rgba);
        assert_eq!(stream.codec, CodecKind::Png);
        assert_eq!(stream.frames, Some(1));
        assert_eq!(stream.timebase, None, "a still imposes no rate");
    }

    #[test]
    fn a_packed_png_reports_the_depth_its_samples_arrive_at() {
        let streams = probe_png(&png(8, 8, 4, 3)).expect("an indexed PNG is readable");
        let stream = streams[0].as_video().expect("a picture");
        assert_eq!(stream.picture.depth, BitDepth::Eight);
        assert_eq!(stream.picture.layout, PlaneLayout::Rgb);
    }

    #[test]
    fn a_damaged_png_is_damaged_and_an_unreadable_one_is_undecodable() {
        let mut bad_crc = png(4, 4, 8, 2);
        let last = bad_crc.len() - 20;
        bad_crc[last] ^= 0xFF;
        assert_eq!(
            probe_png(&bad_crc).expect_err("a flipped byte").label(),
            "damaged"
        );

        let truncated = &png(4, 4, 8, 2)[..30];
        assert_eq!(
            probe_png(truncated).expect_err("cut short").label(),
            "damaged"
        );

        let mut interlaced = Vec::new();
        let mut header = Vec::new();
        header.extend_from_slice(&4_u32.to_be_bytes());
        header.extend_from_slice(&4_u32.to_be_bytes());
        header.extend_from_slice(&[8, 2, 0, 0, 1]);
        interlaced.extend_from_slice(&PNG_SIGNATURE);
        interlaced.extend_from_slice(&chunk(b"IHDR", &header));
        interlaced.extend_from_slice(&chunk(b"IDAT", &[1, 2]));
        interlaced.extend_from_slice(&chunk(b"IEND", &[]));
        assert_eq!(
            probe_png(&interlaced).expect_err("interlaced").label(),
            "undecodable"
        );

        assert_eq!(
            probe_png(&png(4, 4, 2, 2))
                .expect_err("two-bit truecolour")
                .label(),
            "undecodable"
        );
        assert_eq!(
            probe_png(b"not a png at all")
                .expect_err("no signature")
                .label(),
            "damaged"
        );
    }

    #[test]
    fn a_png_with_no_end_chunk_is_damaged_even_when_every_checksum_passes() {
        let mut header = Vec::new();
        header.extend_from_slice(&4_u32.to_be_bytes());
        header.extend_from_slice(&4_u32.to_be_bytes());
        header.extend_from_slice(&[8, 2, 0, 0, 0]);
        let mut out = PNG_SIGNATURE.to_vec();
        out.extend_from_slice(&chunk(b"IHDR", &header));
        out.extend_from_slice(&chunk(b"IDAT", &[1, 2, 3]));
        assert_eq!(probe_png(&out).expect_err("no IEND").label(), "damaged");
    }

    #[test]
    fn a_binary_netpbm_reports_its_geometry() {
        let mut file = b"P6\n# a comment\n4 2\n255\n".to_vec();
        file.extend(std::iter::repeat_n(0_u8, 4 * 2 * 3));
        let streams = probe_netpbm(&file).expect("readable");
        let stream = streams[0].as_video().expect("a picture");
        assert_eq!(stream.coded_size, (4, 2));
        assert_eq!(stream.picture.depth, BitDepth::Eight);
        assert_eq!(stream.codec, CodecKind::Netpbm);
    }

    #[test]
    fn a_sixteen_bit_netpbm_says_sixteen() {
        let mut file = b"P6\n2 2\n65535\n".to_vec();
        file.extend(std::iter::repeat_n(0_u8, 2 * 2 * 3 * 2));
        let streams = probe_netpbm(&file).expect("readable");
        assert_eq!(
            streams[0].as_video().expect("a picture").picture.depth,
            BitDepth::Sixteen
        );
    }

    #[test]
    fn a_short_netpbm_body_is_damaged_in_both_spellings() {
        let mut binary = b"P6\n4 4\n255\n".to_vec();
        binary.extend(std::iter::repeat_n(0_u8, 10));
        assert_eq!(
            probe_netpbm(&binary).expect_err("short body").label(),
            "damaged"
        );

        let text = b"P3\n2 2\n255\n0 0 0 1 1 1\n".to_vec();
        assert_eq!(
            probe_netpbm(&text)
                .expect_err("eight samples short")
                .label(),
            "damaged"
        );
    }

    #[test]
    fn a_text_netpbm_with_every_sample_is_readable() {
        let text = b"P3\n2 1\n255\n0 0 0 255 255 255\n".to_vec();
        let streams = probe_netpbm(&text).expect("readable");
        assert_eq!(streams[0].as_video().expect("a picture").coded_size, (2, 1));
    }

    #[test]
    fn a_greyscale_netpbm_is_undecodable_because_the_reader_does_not_read_it() {
        let mut grey = b"P5\n2 2\n255\n".to_vec();
        grey.extend(std::iter::repeat_n(0_u8, 4));
        assert_eq!(probe_netpbm(&grey).expect_err("P5").label(), "undecodable");
    }

    #[test]
    fn a_header_that_does_not_parse_is_damaged() {
        assert_eq!(
            probe_netpbm(b"P6\n").expect_err("no size").label(),
            "damaged"
        );
        assert_eq!(
            probe_netpbm(b"P6\n0 0\n255\n")
                .expect_err("zero size")
                .label(),
            "damaged"
        );
        assert_eq!(
            probe_netpbm(b"P6\n2 2\n0\n")
                .expect_err("zero maximum")
                .label(),
            "damaged"
        );
        assert_eq!(probe_netpbm(b"").expect_err("empty").label(), "damaged");
    }

    #[test]
    fn signatures_are_recognised_without_reading_further() {
        assert!(looks_like_png(&PNG_SIGNATURE));
        assert!(!looks_like_png(b"P6\n"));
        assert!(looks_like_netpbm(b"P6\n2 2\n"));
        assert!(looks_like_netpbm(b"P1\n"));
        assert!(!looks_like_netpbm(b"P7\n"));
        assert!(!looks_like_netpbm(b"RIFF"));
        assert!(!looks_like_netpbm(b""));
    }
}
