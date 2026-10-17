//! Writing containers, byte by byte, so the corpus can be synthesised rather than downloaded.
//!
//! This module is a muxer in the narrowest possible sense: it writes **headers**, with sample
//! data that is deliberately a known filler pattern rather than a coded picture. That is
//! exactly enough for the probe path, which reads headers and no frames, and it is why the
//! corpus can be generated on a machine with no encoder on it.
//!
//! What it buys, beyond existing at all: the expected answer for every file is *stated* by
//! the thing that wrote it, so the gate compares a probe against a declaration instead of
//! against a second opinion. What it does not buy is independence — see `corpus`'s own
//! documentation for that limit, said plainly, and for what retires it.

/// A big-endian byte sink, which is the order every box in this module is written in.
#[derive(Debug, Default)]
pub(super) struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    /// An empty sink.
    #[must_use]
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// The bytes written so far.
    #[must_use]
    pub(super) fn finish(self) -> Vec<u8> {
        self.bytes
    }

    /// Appends raw bytes.
    pub(super) fn raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.bytes.extend_from_slice(bytes);
        self
    }

    /// Appends one byte.
    pub(super) fn u8(&mut self, value: u8) -> &mut Self {
        self.bytes.push(value);
        self
    }

    /// Appends a big-endian `u16`.
    pub(super) fn be16(&mut self, value: u16) -> &mut Self {
        self.raw(&value.to_be_bytes())
    }

    /// Appends a big-endian `u32`.
    pub(super) fn be32(&mut self, value: u32) -> &mut Self {
        self.raw(&value.to_be_bytes())
    }

    /// Appends a big-endian `i32`.
    pub(super) fn be32i(&mut self, value: i32) -> &mut Self {
        self.raw(&value.to_be_bytes())
    }

    /// Appends a little-endian `u16`.
    pub(super) fn le16(&mut self, value: u16) -> &mut Self {
        self.raw(&value.to_le_bytes())
    }

    /// Appends a little-endian `u32`.
    pub(super) fn le32(&mut self, value: u32) -> &mut Self {
        self.raw(&value.to_le_bytes())
    }

    /// Appends `count` copies of a byte.
    pub(super) fn zeros(&mut self, count: usize) -> &mut Self {
        self.bytes.extend(std::iter::repeat_n(0_u8, count));
        self
    }
}

/// An ISO base media box: a length, a four-character code and a body.
#[must_use]
pub(super) fn box_of(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.be32((body.len() + 8) as u32).raw(kind).raw(body);
    writer.finish()
}

/// A full box: a box whose body opens with a version and three flag bytes.
#[must_use]
pub(super) fn full_box(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut writer = Writer::new();
    writer
        .be32((version as u32) << 24 | (flags & 0x00FF_FFFF))
        .raw(body);
    box_of(kind, &writer.finish())
}

/// A RIFF chunk: a four-character code, a little-endian length, a body and a pad byte.
#[must_use]
pub(super) fn riff_chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.raw(id).le32(body.len() as u32).raw(body);
    if body.len() % 2 == 1 {
        writer.u8(0);
    }
    writer.finish()
}

/// A RIFF list: a chunk whose body opens with the list's own type.
#[must_use]
pub(super) fn riff_list(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.raw(kind).raw(body);
    riff_chunk(b"LIST", &writer.finish())
}

/// An EBML element: an identifier as written, a length as a variable-length integer, a body.
#[must_use]
pub(super) fn ebml(id: &[u8], body: &[u8]) -> Vec<u8> {
    let mut writer = Writer::new();
    writer
        .raw(id)
        .raw(&ebml_length(body.len() as u64))
        .raw(body);
    writer.finish()
}

/// A length as an EBML variable-length integer, in the narrowest width that holds it.
#[must_use]
pub(super) fn ebml_length(value: u64) -> Vec<u8> {
    for width in 1_u32..=8 {
        let bits = width * 7;
        let limit = (1_u64 << bits) - 1;
        if value < limit {
            let mut bytes = value.to_be_bytes().to_vec();
            bytes.drain(..8 - width as usize);
            bytes[0] |= 0x80_u8 >> (width - 1);
            return bytes;
        }
    }
    vec![0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFE]
}

/// An EBML unsigned integer element, written at the narrowest width that holds the value.
#[must_use]
pub(super) fn ebml_uint(id: &[u8], value: u64) -> Vec<u8> {
    let mut body = value.to_be_bytes().to_vec();
    while body.len() > 1 && body[0] == 0 {
        body.remove(0);
    }
    ebml(id, &body)
}

/// An EBML string element.
#[must_use]
pub(super) fn ebml_string(id: &[u8], value: &str) -> Vec<u8> {
    ebml(id, value.as_bytes())
}

/// An EBML 64-bit float element, which is how Matroska states a sample rate.
#[must_use]
pub(super) fn ebml_float(id: &[u8], value: f64) -> Vec<u8> {
    ebml(id, &value.to_be_bytes())
}

/// A big-endian bit writer, for the one bitstream the corpus has to state: a parameter set.
#[derive(Debug, Default)]
pub(super) struct BitWriter {
    bytes: Vec<u8>,
    used: u32,
}

impl BitWriter {
    /// An empty writer.
    #[must_use]
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Writes `count` bits of `value`, most significant first.
    pub(super) fn u(&mut self, count: u32, value: u32) -> &mut Self {
        for index in (0..count).rev() {
            let bit = (value >> index) & 1;
            if self.used % 8 == 0 {
                self.bytes.push(0);
            }
            if let Some(last) = self.bytes.last_mut() {
                *last |= (bit as u8) << (7 - (self.used % 8));
            }
            self.used += 1;
        }
        self
    }

    /// Writes an unsigned exponential-Golomb code.
    pub(super) fn ue(&mut self, value: u32) -> &mut Self {
        let shifted = value.saturating_add(1);
        let bits = 32 - shifted.leading_zeros();
        self.u(bits - 1, 0).u(bits, shifted)
    }

    /// Closes the bitstream with a stop bit and alignment zeros.
    #[must_use]
    pub(super) fn finish(mut self) -> Vec<u8> {
        self.u(1, 1);
        while self.used % 8 != 0 {
            self.u(1, 0);
        }
        self.bytes
    }
}

/// Deterministic filler, so a file's sample data is reproducible and is not a picture.
///
/// A counted sequence rather than zeros: a run of zeros is also what a sparse file and a
/// failed write look like, and filler that is distinguishable from both is worth two lines.
#[must_use]
pub(super) fn filler(len: usize) -> Vec<u8> {
    (0..len).map(|index| (index % 251) as u8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_states_its_own_length() {
        let written = box_of(b"free", &[1, 2, 3, 4]);
        assert_eq!(&written[0..4], &12_u32.to_be_bytes());
        assert_eq!(&written[4..8], b"free");
        assert_eq!(written.len(), 12);
    }

    #[test]
    fn a_full_box_opens_with_its_version_and_flags() {
        let written = full_box(b"tkhd", 1, 0x0003, &[]);
        assert_eq!(&written[8..12], &[1, 0, 0, 3]);
    }

    #[test]
    fn a_riff_chunk_pads_to_an_even_length_without_counting_the_pad() {
        let written = riff_chunk(b"odd ", &[1, 2, 3]);
        assert_eq!(&written[4..8], &3_u32.to_le_bytes());
        assert_eq!(
            written.len(),
            12,
            "eight of header, three of body, one of pad"
        );
    }

    #[test]
    fn an_ebml_length_uses_the_narrowest_width_that_holds_it() {
        assert_eq!(ebml_length(2), vec![0x82]);
        assert_eq!(ebml_length(127), vec![0x40, 0x7F]);
        assert_eq!(ebml_length(0x4000), vec![0x20, 0x40, 0x00]);
    }

    #[test]
    fn an_ebml_integer_drops_its_leading_zeros() {
        assert_eq!(ebml_uint(&[0x83], 1), vec![0x83, 0x81, 0x01]);
        assert_eq!(
            ebml_uint(&[0x83], 300),
            vec![0x83, 0x82, 0x01, 0x2C],
            "two bytes of value, and a length that says two"
        );
    }

    #[test]
    fn a_bit_writer_and_a_golomb_code_agree_with_the_reader_that_reads_them() {
        let mut writer = BitWriter::new();
        writer.u(8, 100).ue(0).ue(1);
        let bytes = writer.finish();
        assert_eq!(bytes[0], 100);
        // A ue(0) of one bit, a ue(1) of three, then the stop bit `finish` appends.
        assert_eq!(bytes[1], 0b1010_1000);
    }

    #[test]
    fn filler_is_deterministic_and_is_not_a_run_of_zeros() {
        assert_eq!(filler(4), vec![0, 1, 2, 3]);
        assert_eq!(filler(300).len(), 300);
        assert_eq!(filler(300)[251], 0, "the pattern wraps at a prime");
        assert_eq!(filler(8), filler(8));
    }
}
