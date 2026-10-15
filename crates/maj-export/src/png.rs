//! A PNG writer that is a pure function of its pixels.
//!
//! PNG is the right container for a reference still: it is lossless, it carries sixteen
//! bits per channel, and every browser and NLE reads it. It is also a container in which it
//! is easy to be accidentally non-deterministic — an encoder that stamps a creation time,
//! picks a compression level from a heuristic or reorders its chunks produces two different
//! files from the same frame, which is exactly what "three renders produce identical bytes"
//! cannot tolerate.
//!
//! So this writer makes every choice explicitly:
//!
//! * **Stored deflate.** The `IDAT` stream is a zlib stream whose deflate blocks are all of
//!   type 00 — no compression, no match search, no Huffman table. It is larger than a real
//!   encoder's output and it is bit-identical every time. A tighter encoder can be added
//!   later behind the same declaration, and the determinism gate will say whether it earned
//!   its place.
//! * **One filter, always.** Every scanline is written with filter type 0 (None), so the
//!   bytes are the pixels and nothing else, and a person can read the file in a hex dump.
//! * **No metadata.** No `tIME`, no `tEXt`, no software string. The manifest records what
//!   was rendered, with which policy; the file records the pixels.
//! * **Eight or sixteen bits per channel**, RGBA, so alpha survives the trip.
//!
//! The transfer function is declared in the file when PNG has a way to declare it — an
//! `sRGB` chunk — and left to the manifest otherwise, because PNG has no chunk that can
//! express a PQ or HLG curve and guessing would be worse than saying nothing.

use maj_color::TransferFunction;
use maj_render::{BitDepth, Framebuffer};

/// The eight bytes every PNG begins with.
pub const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Colour type 6: red, green, blue and alpha.
const COLOR_TYPE_RGBA: u8 = 6;

/// The largest number of bytes a single stored deflate block can carry.
const MAX_STORED_BLOCK: usize = 65_535;

/// Encodes a frame as a PNG file.
///
/// The frame is quantised once, at this boundary, through `transfer` and at `depth`: the
/// same single rounding the certified path declares. Nothing above this call sees an
/// integer and nothing below it sees light.
///
/// # Panics
///
/// Panics if the frame's dimensions do not fit in a 32-bit PNG header, which cannot happen
/// for a frame the renderer will allocate.
#[must_use]
pub fn encode(frame: &Framebuffer, depth: BitDepth, transfer: TransferFunction) -> Vec<u8> {
    let width = frame.width();
    let height = frame.height();
    let bytes_per_sample = match depth {
        BitDepth::Eight => 1,
        BitDepth::Sixteen => 2,
    };
    let raw = scanlines(frame, depth, transfer, bytes_per_sample);

    let mut out = Vec::with_capacity(raw.len() + 128);
    out.extend_from_slice(&SIGNATURE);
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.push(if bytes_per_sample == 1 { 8 } else { 16 });
    header.push(COLOR_TYPE_RGBA);
    header.push(0); // deflate, the only method PNG defines
    header.push(0); // filter method zero, the only one PNG defines
    header.push(0); // no interlacing: rows are rows
    write_chunk(b"IHDR", &header, &mut out);

    if transfer == TransferFunction::Srgb {
        // Rendering intent zero, perceptual: the one every viewer expects, and the one the
        // sRGB specification recommends as a default.
        write_chunk(b"sRGB", &[0], &mut out);
        // The sRGB gamma as PNG's fixed-point 1/100000 units: 1/2.2 is 45455.
        write_chunk(b"gAMA", &45_455_u32.to_be_bytes(), &mut out);
    }

    write_chunk(b"IDAT", &zlib_stored(&raw), &mut out);
    write_chunk(b"IEND", &[], &mut out);
    out
}

/// The picture's scanlines, each prefixed with its filter byte.
///
/// Filter zero is not a compression heuristic, it is a statement: the bytes after the
/// filter byte are the pixel's code values, most significant byte first, in RGBA order.
#[must_use]
pub fn scanlines(
    frame: &Framebuffer,
    depth: BitDepth,
    transfer: TransferFunction,
    bytes_per_sample: usize,
) -> Vec<u8> {
    let width = frame.width() as usize;
    let height = frame.height() as usize;
    let codes = frame.to_code_values(transfer, depth);
    let mut raw = Vec::with_capacity(height * (1 + width * 4 * bytes_per_sample));
    for row in 0..height {
        raw.push(0);
        for column in 0..width {
            let base = (row * width + column) * 4;
            for channel in 0..4 {
                let value = codes[base + channel];
                if bytes_per_sample == 2 {
                    raw.extend_from_slice(&value.to_be_bytes());
                } else {
                    // The quantiser already bounded an eight-bit value by 255, so the low
                    // byte is the whole value and no truncation can occur.
                    raw.push(value as u8);
                }
            }
        }
    }
    raw
}

/// A zlib stream holding the data in uncompressed deflate blocks.
///
/// The two header bytes are `0x78 0x01`: a 32 KiB window, the default compression level
/// recorded for completeness, and the check value the format requires — `0x7801` is
/// divisible by 31. Each block carries its own length and its one's complement, and the
/// stream ends with the Adler-32 of the *uncompressed* data, which is what a decoder checks.
#[must_use]
pub fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() + raw.len() / MAX_STORED_BLOCK * 5 + 16);
    out.push(0x78);
    out.push(0x01);
    if raw.is_empty() {
        // An empty stream still needs one final block, or a decoder keeps waiting.
        out.push(0x01);
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.extend_from_slice(&(!0_u16).to_le_bytes());
    }
    let blocks = raw.len().div_ceil(MAX_STORED_BLOCK);
    for (index, chunk) in raw.chunks(MAX_STORED_BLOCK).enumerate() {
        let last = index + 1 == blocks;
        // Bit zero is BFINAL and two zero bits follow: BTYPE 00, stored.
        out.push(u8::from(last));
        let length = chunk.len() as u16;
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(&(!length).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

/// Writes one chunk: length, type, data, and the CRC of the type and data.
fn write_chunk(kind: &[u8; 4], data: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut checked = Vec::with_capacity(4 + data.len());
    checked.extend_from_slice(kind);
    checked.extend_from_slice(data);
    out.extend_from_slice(&crc32(&checked).to_be_bytes());
}

/// The CRC-32 PNG uses: the reflected polynomial `0xEDB88320`, initial value all ones and a
/// final complement.
#[must_use]
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        crc = (crc >> 8) ^ CRC_TABLE[(crc & 0xFF) as usize];
    }
    !crc
}

/// The Adler-32 checksum zlib appends to a stream.
#[must_use]
pub fn adler32(bytes: &[u8]) -> u32 {
    const MODULUS: u32 = 65_521;
    let mut low = 1_u32;
    let mut high = 0_u32;
    for byte in bytes {
        low = (low + u32::from(*byte)) % MODULUS;
        high = (high + low) % MODULUS;
    }
    (high << 16) | low
}

/// The CRC lookup table, built at compile time so the writer has no initialisation step and
/// no chance of two processes disagreeing about a table that was built lazily.
const CRC_TABLE: [u32; 256] = build_crc_table();

const fn build_crc_table() -> [u32; 256] {
    let mut table = [0_u32; 256];
    let mut index = 0;
    while index < 256 {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 == 1 {
                0xEDB8_8320 ^ (value >> 1)
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;
    use maj_render::Framebuffer;
    use maj_render::Pixel;

    /// A frame with a known, asymmetric pattern: size, position and channel order all
    /// matter, so a writer that transposed or reordered anything fails here.
    fn pattern(width: u32, height: u32) -> Framebuffer {
        let mut frame = Framebuffer::black(width, height);
        for y in 0..height {
            for x in 0..width {
                let pixel: Pixel = [
                    x as f32 / (width.max(2) - 1) as f32,
                    y as f32 / (height.max(2) - 1) as f32,
                    if x == y { 1.0 } else { 0.25 },
                    if x + y < width { 1.0 } else { 0.5 },
                ];
                frame.set_pixel(x, y, pixel);
            }
        }
        frame
    }

    /// A parsed chunk, as a test reads one.
    struct Chunk {
        kind: [u8; 4],
        data: Vec<u8>,
        crc_ok: bool,
    }

    /// Walks a PNG and returns its chunks, checking every length and CRC on the way.
    fn parse(bytes: &[u8]) -> Vec<Chunk> {
        assert_eq!(bytes[..8], SIGNATURE, "every PNG begins with the signature");
        let mut chunks = Vec::new();
        let mut offset = 8;
        while offset + 12 <= bytes.len() {
            let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            let mut kind = [0_u8; 4];
            kind.copy_from_slice(&bytes[offset + 4..offset + 8]);
            let data = bytes[offset + 8..offset + 8 + length].to_vec();
            let stored = u32::from_be_bytes(
                bytes[offset + 8 + length..offset + 12 + length]
                    .try_into()
                    .unwrap(),
            );
            let mut checked = kind.to_vec();
            checked.extend_from_slice(&data);
            chunks.push(Chunk {
                kind,
                data,
                crc_ok: stored == crc32(&checked),
            });
            offset += 12 + length;
            if &kind == b"IEND" {
                break;
            }
        }
        assert_eq!(offset, bytes.len(), "a PNG is nothing but its chunks");
        chunks
    }

    /// The one chunk of a given kind, so a test does not depend on the index a chunk
    /// happens to land at: an sRGB file has two more chunks than a linear one.
    fn chunk_of<'a>(chunks: &'a [Chunk], kind: &[u8; 4]) -> &'a Chunk {
        chunks
            .iter()
            .find(|chunk| &chunk.kind == kind)
            .unwrap_or_else(|| {
                panic!(
                    "the file must hold a {} chunk",
                    String::from_utf8_lossy(kind)
                )
            })
    }

    /// Inflates a stream this writer produced: stored blocks only, so the reader is short
    /// and the test proves the stream rather than trusting it.
    fn inflate_stored(stream: &[u8]) -> Vec<u8> {
        assert_eq!(&stream[..2], &[0x78, 0x01], "the zlib header");
        let mut out = Vec::new();
        let mut offset = 2;
        loop {
            let header = stream[offset];
            assert_eq!(header & 0b0000_0110, 0, "every block must be stored");
            let length = u16::from_le_bytes(stream[offset + 1..offset + 3].try_into().unwrap());
            let complement = u16::from_le_bytes(stream[offset + 3..offset + 5].try_into().unwrap());
            assert_eq!(
                complement, !length,
                "the length and its complement must agree"
            );
            let start = offset + 5;
            out.extend_from_slice(&stream[start..start + length as usize]);
            offset = start + length as usize;
            if header & 1 == 1 {
                break;
            }
        }
        let stored = u32::from_be_bytes(stream[offset..offset + 4].try_into().unwrap());
        assert_eq!(stored, adler32(&out), "the Adler-32 of the inflated data");
        assert_eq!(
            offset + 4,
            stream.len(),
            "the stream ends with its checksum"
        );
        out
    }

    #[test]
    fn the_checksums_match_the_published_test_vectors() {
        // The CRC of the IEND chunk with no data is a value every PNG implementation knows.
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
        // Adler-32 of "Wikipedia" and of nothing at all.
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        assert_eq!(adler32(b""), 1);
    }

    #[test]
    fn the_file_has_the_chunks_a_png_must_have_and_every_crc_holds() {
        let frame = pattern(5, 3);
        let bytes = encode(&frame, BitDepth::Eight, TransferFunction::Srgb);
        let chunks = parse(&bytes);
        let kinds: Vec<&[u8]> = chunks.iter().map(|chunk| &chunk.kind[..]).collect();
        let expected: Vec<&[u8]> = vec![b"IHDR", b"sRGB", b"gAMA", b"IDAT", b"IEND"];
        assert_eq!(kinds, expected);
        assert!(
            chunks.iter().all(|chunk| chunk.crc_ok),
            "every CRC must check out"
        );

        let header = &chunks[0].data;
        assert_eq!(u32::from_be_bytes(header[0..4].try_into().unwrap()), 5);
        assert_eq!(u32::from_be_bytes(header[4..8].try_into().unwrap()), 3);
        assert_eq!(header[8], 8, "eight bits per sample");
        assert_eq!(header[9], COLOR_TYPE_RGBA);
        assert_eq!(
            &header[10..13],
            &[0, 0, 0],
            "deflate, filter zero, no interlace"
        );
        assert_eq!(
            chunks[1].data,
            vec![0],
            "a perceptual sRGB rendering intent"
        );
        assert_eq!(chunks[2].data, 45_455_u32.to_be_bytes());
        assert!(chunks[4].data.is_empty(), "IEND carries nothing");
    }

    #[test]
    fn the_stored_stream_inflates_to_the_pixels_that_went_in() {
        let frame = pattern(7, 4);
        let bytes = encode(&frame, BitDepth::Eight, TransferFunction::Linear);
        let chunks = parse(&bytes);
        // Without a curve, the code values are the linear values rounded once: a scanline is
        // its filter byte followed by RGBA in row-major order, most significant byte first.
        let raw = inflate_stored(&chunk_of(&chunks, b"IDAT").data);
        let expected = scanlines(&frame, BitDepth::Eight, TransferFunction::Linear, 1);
        assert_eq!(raw, expected);
        assert_eq!(raw.len(), 4 * (1 + 7 * 4));
        for row in 0..4 {
            assert_eq!(raw[row * (1 + 7 * 4)], 0, "every scanline is unfiltered");
        }

        // The first pixel is the frame's own first pixel: the pattern's blue where x is y,
        // with a full alpha.
        assert_eq!(&raw[1..5], &[0, 0, 255, 255]);
        // The second pixel's red channel is 1/6 of the way up the ramp, rounded once.
        assert_eq!(raw[5], ((255.0_f32) / 6.0 + 0.5).floor() as u8);
    }

    #[test]
    fn sixteen_bit_samples_are_written_most_significant_byte_first() {
        let mut frame = Framebuffer::black(2, 1);
        frame.set_pixel(0, 0, [1.0, 0.0, 0.5, 1.0]);
        frame.set_pixel(1, 0, [0.0, 0.0, 0.0, 0.0]);
        let bytes = encode(&frame, BitDepth::Sixteen, TransferFunction::Linear);
        let chunks = parse(&bytes);
        assert_eq!(
            chunk_of(&chunks, b"IHDR").data[8],
            16,
            "sixteen bits per sample"
        );
        let raw = inflate_stored(&chunk_of(&chunks, b"IDAT").data);
        assert_eq!(raw[0], 0);
        assert_eq!(&raw[1..3], &[0xFF, 0xFF], "red, at the full scale");
        assert_eq!(&raw[3..5], &[0x00, 0x00], "green, at zero");
        assert_eq!(&raw[5..7], &[0x80, 0x00], "blue, at the half, exactly");
        assert_eq!(&raw[7..9], &[0xFF, 0xFF], "and a full alpha");
        assert_eq!(
            raw.len(),
            1 + 2 * 4 * 2,
            "one filter byte and two sixteen-bit pixels"
        );
    }

    #[test]
    fn the_same_frame_encodes_to_the_same_bytes_and_a_different_one_does_not() {
        let frame = pattern(9, 9);
        let first = encode(&frame, BitDepth::Eight, TransferFunction::Srgb);
        let second = encode(&frame, BitDepth::Eight, TransferFunction::Srgb);
        assert_eq!(first, second, "the writer has no clock and no state");

        let mut changed = pattern(9, 9);
        changed.set_pixel(8, 8, [0.0, 0.0, 0.0, 1.0]);
        assert_ne!(
            encode(&changed, BitDepth::Eight, TransferFunction::Srgb),
            first
        );

        // The declared boundary is part of the output, not a detail of the caller.
        assert_ne!(
            encode(&frame, BitDepth::Sixteen, TransferFunction::Srgb),
            first
        );
        assert_ne!(
            encode(&frame, BitDepth::Eight, TransferFunction::Linear),
            first
        );
    }

    #[test]
    fn a_curve_png_cannot_declare_is_left_to_the_manifest() {
        let frame = pattern(3, 2);
        for transfer in [
            TransferFunction::Linear,
            TransferFunction::Pq,
            TransferFunction::Hlg,
        ] {
            let chunks = parse(&encode(&frame, BitDepth::Eight, transfer));
            let kinds: Vec<&[u8]> = chunks.iter().map(|chunk| &chunk.kind[..]).collect();
            let expected: Vec<&[u8]> = vec![b"IHDR", b"IDAT", b"IEND"];
            assert_eq!(kinds, expected, "{transfer:?}");
        }
    }

    #[test]
    fn a_frame_wide_enough_to_need_two_deflate_blocks_round_trips() {
        // A row of 8192 pixels is over the 65535-byte block boundary, so this crosses it
        // deliberately: the length, complement and final-block bookkeeping is what is
        // being tested, and a mistake there would truncate every large render.
        let width = 8_192;
        let mut wide = Framebuffer::black(width, 3);
        for x in 0..width {
            wide.set_pixel(x, 0, [(x % 256) as f32 / 255.0, 0.0, 0.0, 1.0]);
        }
        let bytes = encode(&wide, BitDepth::Eight, TransferFunction::Linear);
        let chunks = parse(&bytes);
        let idat = chunk_of(&chunks, b"IDAT");
        let raw = inflate_stored(&idat.data);
        assert_eq!(raw.len(), 3 * (1 + width as usize * 4));
        assert!(
            idat.data.len() > MAX_STORED_BLOCK,
            "more than one stored block"
        );
        assert_eq!(
            raw,
            scanlines(&wide, BitDepth::Eight, TransferFunction::Linear, 1)
        );
    }

    #[test]
    fn an_empty_stream_is_still_a_valid_zlib_stream() {
        let stream = zlib_stored(&[]);
        assert_eq!(
            stream,
            vec![
                0x78, 0x01, 0x01, 0x00, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x01
            ]
        );
        assert!(inflate_stored(&stream).is_empty());
    }
}
