//! What a sample entry's four-character code means, and the two facts only a parameter set
//! carries.
//!
//! Chroma subsampling and bit depth are axes of M2's corpus, and a container does not state
//! either: they live in the codec's own configuration record. Reading one is **not** decoding
//! — no sample is touched, no frame is reconstructed — but it is the deepest this backend
//! goes, and it goes there for exactly two numbers:
//!
//! * **HEVC** and **AV1** put both in their configuration box, so a few bytes answer it.
//! * **H.264** puts them in the sequence parameter set, which needs an exponential-Golomb
//!   reader. The reader stops as soon as the two numbers are out; everything after them
//!   (scaling lists, the VUI, the frame-mbs flag) is left unread, because the container's own
//!   `colr` and `fiel` boxes are where a muxer is supposed to state colour and field order
//!   and where this backend reads them from.
//! * **ProRes** and **DNxHR** encode the answer in the four-character code itself.
//!
//! Where nothing says, the format is reported as eight-bit 4:2:0 with the picture's colour
//! undeclared — the commonest truth, and visibly a default rather than a measurement because
//! `PictureFormat::colour_is_fully_declared` says so.

use crate::picture::{BitDepth, PictureFormat, PlaneLayout};
use crate::stream::CodecKind;

/// The codec a sample entry's four-character code names.
pub(crate) fn codec_of(code: [u8; 4]) -> CodecKind {
    match &code {
        b"avc1" | b"avc3" | b"h264" | b"H264" | b"x264" => CodecKind::H264,
        b"hvc1" | b"hev1" | b"hevc" | b"HEVC" => CodecKind::Hevc,
        b"av01" => CodecKind::Av1,
        b"vp09" | b"VP90" => CodecKind::Vp9,
        b"apch" | b"apcn" | b"apcs" | b"apco" | b"ap4h" | b"ap4x" => CodecKind::ProRes,
        b"AVdn" | b"AVdh" => CodecKind::DnxHr,
        b"jpeg" | b"mjpa" | b"mjpb" | b"MJPG" => CodecKind::MotionJpeg,
        b"raw " | b"2vuy" | b"v210" | b"yuv2" | b"DIB " => CodecKind::Uncompressed,
        b"mp4a" => CodecKind::Aac,
        b"fLaC" => CodecKind::Flac,
        b"Opus" => CodecKind::Opus,
        b"sowt" | b"twos" | b"in24" | b"in32" | b"lpcm" | b"NONE" => CodecKind::PcmInteger,
        b"fl32" | b"fl64" => CodecKind::PcmFloat,
        _ => CodecKind::Unknown,
    }
}

/// What a four-character code alone says about layout and depth.
///
/// ProRes and uncompressed codes carry it; everything else answers `None` and has to be asked
/// through its configuration record.
pub(crate) fn format_of_code(code: [u8; 4]) -> Option<(PlaneLayout, BitDepth)> {
    match &code {
        // ProRes 422 in all four bitrates: 4:2:2, ten bits.
        b"apch" | b"apcn" | b"apcs" | b"apco" => Some((PlaneLayout::Yuv422, BitDepth::Ten)),
        // ProRes 4444 and 4444 XQ: 4:4:4, twelve bits.
        b"ap4h" | b"ap4x" => Some((PlaneLayout::Yuv444, BitDepth::Twelve)),
        b"2vuy" | b"yuv2" => Some((PlaneLayout::Yuv422, BitDepth::Eight)),
        b"v210" => Some((PlaneLayout::Yuv422, BitDepth::Ten)),
        _ => None,
    }
}

/// Reads layout and depth out of a codec configuration box, where this build can.
///
/// `code` is the sample entry's four-character code and `config` the body of the `avcC`,
/// `hvcC` or `av1C` box beside it. Answers `None` when the box is absent, too short or for a
/// codec whose configuration this build does not read — never a guess.
pub(crate) fn format_of_config(code: [u8; 4], config: &[u8]) -> Option<(PlaneLayout, BitDepth)> {
    match codec_of(code) {
        CodecKind::H264 => avc_format(config),
        CodecKind::Hevc => hevc_format(config),
        CodecKind::Av1 => av1_format(config),
        _ => None,
    }
}

/// `avcC`: version, profile, compatibility, level, lengths, then the sequence parameter sets.
fn avc_format(config: &[u8]) -> Option<(PlaneLayout, BitDepth)> {
    let count = usize::from(config.get(5)? & 0x1F);
    if count == 0 {
        return None;
    }
    let length = usize::from(u16::from_be_bytes([*config.get(6)?, *config.get(7)?]));
    let parameter_set = config.get(8..8usize.checked_add(length)?)?;
    sps_format(parameter_set)
}

/// The two numbers, out of a sequence parameter set, and not a byte further.
fn sps_format(parameter_set: &[u8]) -> Option<(PlaneLayout, BitDepth)> {
    // The NAL header is one byte, and emulation-prevention bytes have to go before the
    // bit reader sees the payload.
    let payload = unescape(parameter_set.get(1..)?);
    let mut bits = Bits::new(&payload);
    let profile = bits.u(8)?;
    bits.skip(8)?; // constraint flags and reserved bits
    bits.skip(8)?; // level
    bits.ue()?; // seq_parameter_set_id
    // Only the High profiles and above state chroma and depth; the others are 4:2:0 at eight
    // bits by definition of the profile, which is a fact and not a default.
    let high = matches!(
        profile,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    );
    if !high {
        return Some((PlaneLayout::Yuv420, BitDepth::Eight));
    }
    let chroma = u8::try_from(bits.ue()?).ok()?;
    if chroma == 3 {
        bits.skip(1)?; // separate_colour_plane_flag
    }
    let luma_depth = u8::try_from(bits.ue()?.checked_add(8)?).ok()?;
    let layout = PlaneLayout::from_chroma_format_idc(chroma)?;
    let depth = BitDepth::from_bits(luma_depth)?;
    Some((layout, depth))
}

/// `hvcC` states both numbers outright, at fixed offsets.
fn hevc_format(config: &[u8]) -> Option<(PlaneLayout, BitDepth)> {
    let layout = PlaneLayout::from_chroma_format_idc(config.get(16)? & 0x03)?;
    let depth = BitDepth::from_bits((config.get(17)? & 0x07).checked_add(8)?)?;
    Some((layout, depth))
}

/// `av1C` states both in one byte of flags.
fn av1_format(config: &[u8]) -> Option<(PlaneLayout, BitDepth)> {
    let flags = *config.get(2)?;
    let high = flags & 0x40 != 0;
    let twelve = flags & 0x20 != 0;
    let monochrome = flags & 0x10 != 0;
    let subsampling_x = flags & 0x08 != 0;
    let subsampling_y = flags & 0x04 != 0;
    let depth = match (high, twelve) {
        (false, _) => BitDepth::Eight,
        (true, false) => BitDepth::Ten,
        (true, true) => BitDepth::Twelve,
    };
    let layout = if monochrome {
        PlaneLayout::Monochrome
    } else {
        match (subsampling_x, subsampling_y) {
            (true, true) => PlaneLayout::Yuv420,
            (true, false) => PlaneLayout::Yuv422,
            (false, false) => PlaneLayout::Yuv444,
            // x=0 with y=1 is not a defined subsampling.
            (false, true) => return None,
        }
    };
    Some((layout, depth))
}

/// The format to report when nothing in the file says.
pub(crate) fn undeclared_format() -> PictureFormat {
    PictureFormat::undeclared(PlaneLayout::Yuv420, BitDepth::Eight)
}

/// Removes the `00 00 03` emulation-prevention bytes a NAL unit is escaped with.
fn unescape(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len());
    let mut zeros = 0_usize;
    for byte in payload {
        if zeros >= 2 && *byte == 3 {
            zeros = 0;
            continue;
        }
        if *byte == 0 {
            zeros += 1;
        } else {
            zeros = 0;
        }
        out.push(*byte);
    }
    out
}

/// A big-endian bit reader with exponential-Golomb codes, and no way to read past the end.
struct Bits<'bytes> {
    bytes: &'bytes [u8],
    position: usize,
}

impl<'bytes> Bits<'bytes> {
    const fn new(bytes: &'bytes [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn bit(&mut self) -> Option<u32> {
        let byte = *self.bytes.get(self.position / 8)?;
        let shift = 7 - (self.position % 8);
        self.position = self.position.checked_add(1)?;
        Some(u32::from((byte >> shift) & 1))
    }

    fn u(&mut self, count: u32) -> Option<u32> {
        let mut value = 0_u32;
        for _ in 0..count {
            value = value.checked_mul(2)?.checked_add(self.bit()?)?;
        }
        Some(value)
    }

    fn skip(&mut self, count: u32) -> Option<()> {
        for _ in 0..count {
            self.bit()?;
        }
        Some(())
    }

    /// An unsigned exponential-Golomb code.
    ///
    /// The leading-zero count is capped at thirty-one: a longer run is a damaged bitstream,
    /// and a reader that kept counting would turn one into a shift that overflows.
    fn ue(&mut self) -> Option<u32> {
        let mut leading = 0_u32;
        while self.bit()? == 0 {
            leading += 1;
            if leading > 31 {
                return None;
            }
        }
        if leading == 0 {
            return Some(0);
        }
        let tail = self.u(leading)?;
        (1_u32 << leading).checked_sub(1)?.checked_add(tail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_character_codes_name_the_codec_they_are() {
        assert_eq!(codec_of(*b"avc1"), CodecKind::H264);
        assert_eq!(codec_of(*b"hvc1"), CodecKind::Hevc);
        assert_eq!(codec_of(*b"apcn"), CodecKind::ProRes);
        assert_eq!(codec_of(*b"mp4a"), CodecKind::Aac);
        assert_eq!(codec_of(*b"zzzz"), CodecKind::Unknown);
    }

    #[test]
    fn prores_states_its_format_in_its_own_code() {
        assert_eq!(
            format_of_code(*b"apcn"),
            Some((PlaneLayout::Yuv422, BitDepth::Ten))
        );
        assert_eq!(
            format_of_code(*b"ap4h"),
            Some((PlaneLayout::Yuv444, BitDepth::Twelve))
        );
        assert_eq!(format_of_code(*b"avc1"), None);
    }

    /// An `avcC` around one sequence parameter set.
    fn avcc(parameter_set: &[u8]) -> Vec<u8> {
        let mut out = vec![1, parameter_set[1], 0, 0, 0xFF, 0xE1];
        out.extend_from_slice(&(parameter_set.len() as u16).to_be_bytes());
        out.extend_from_slice(parameter_set);
        out
    }

    /// A sequence parameter set stating a profile, a chroma format and a luma depth.
    ///
    /// Written with the same bit writer the corpus generator uses, so the test exercises the
    /// reader against an independently assembled bitstream rather than against itself.
    fn sps(profile: u8, chroma: u32, depth_minus_eight: u32) -> Vec<u8> {
        let mut writer = BitWriter::new();
        writer.u(8, u32::from(profile));
        writer.u(8, 0);
        writer.u(8, 40);
        writer.ue(0);
        if matches!(profile, 100 | 110 | 122 | 244) {
            writer.ue(chroma);
            if chroma == 3 {
                writer.u(1, 0);
            }
            writer.ue(depth_minus_eight);
            writer.ue(depth_minus_eight);
            writer.u(1, 0);
            writer.u(1, 0);
        }
        writer.ue(4);
        let mut out = vec![0x67];
        out.extend_from_slice(&writer.finish());
        out
    }

    struct BitWriter {
        bytes: Vec<u8>,
        used: u32,
    }

    impl BitWriter {
        fn new() -> Self {
            Self {
                bytes: Vec::new(),
                used: 0,
            }
        }

        fn u(&mut self, count: u32, value: u32) {
            for index in (0..count).rev() {
                let bit = (value >> index) & 1;
                if self.used % 8 == 0 {
                    self.bytes.push(0);
                }
                let last = self.bytes.len() - 1;
                let shift = 7 - (self.used % 8);
                self.bytes[last] |= (bit as u8) << shift;
                self.used += 1;
            }
        }

        fn ue(&mut self, value: u32) {
            let shifted = value + 1;
            let bits = 32 - shifted.leading_zeros();
            self.u(bits - 1, 0);
            self.u(bits, shifted);
        }

        fn finish(mut self) -> Vec<u8> {
            self.u(1, 1);
            while self.used % 8 != 0 {
                self.u(1, 0);
            }
            self.bytes
        }
    }

    #[test]
    fn an_h264_parameter_set_gives_up_its_chroma_and_depth() {
        assert_eq!(
            format_of_config(*b"avc1", &avcc(&sps(100, 1, 0))),
            Some((PlaneLayout::Yuv420, BitDepth::Eight))
        );
        assert_eq!(
            format_of_config(*b"avc1", &avcc(&sps(110, 2, 2))),
            Some((PlaneLayout::Yuv422, BitDepth::Ten))
        );
        assert_eq!(
            format_of_config(*b"avc1", &avcc(&sps(244, 3, 4))),
            Some((PlaneLayout::Yuv444, BitDepth::Twelve))
        );
    }

    #[test]
    fn a_baseline_profile_is_four_two_zero_at_eight_bits_by_definition() {
        assert_eq!(
            format_of_config(*b"avc1", &avcc(&sps(66, 0, 0))),
            Some((PlaneLayout::Yuv420, BitDepth::Eight))
        );
    }

    #[test]
    fn a_truncated_configuration_answers_nothing_rather_than_guessing() {
        assert_eq!(format_of_config(*b"avc1", &[]), None);
        assert_eq!(
            format_of_config(*b"avc1", &[1, 100, 0, 0, 0xFF, 0xE1]),
            None
        );
        assert_eq!(format_of_config(*b"hvc1", &[1, 2, 3]), None);
        assert_eq!(format_of_config(*b"av01", &[]), None);
        assert_eq!(format_of_config(*b"apcn", &[0; 32]), None);
    }

    #[test]
    fn hevc_and_av1_state_both_numbers_in_their_configuration() {
        let mut hvcc = vec![0_u8; 23];
        hvcc[16] = 0x02; // 4:2:2
        hvcc[17] = 0x02; // ten bits
        assert_eq!(
            format_of_config(*b"hvc1", &hvcc),
            Some((PlaneLayout::Yuv422, BitDepth::Ten))
        );

        // high_bitdepth, not twelve_bit, not monochrome, no subsampling: 4:4:4 at ten bits.
        let av1c = [0x81_u8, 0x00, 0x40];
        assert_eq!(
            format_of_config(*b"av01", &av1c),
            Some((PlaneLayout::Yuv444, BitDepth::Ten))
        );
        // Eight-bit 4:2:0.
        let av1c = [0x81_u8, 0x00, 0x0C];
        assert_eq!(
            format_of_config(*b"av01", &av1c),
            Some((PlaneLayout::Yuv420, BitDepth::Eight))
        );
        // An undefined subsampling is refused.
        let av1c = [0x81_u8, 0x00, 0x04];
        assert_eq!(format_of_config(*b"av01", &av1c), None);
    }

    #[test]
    fn emulation_prevention_bytes_are_removed_and_nothing_else_is() {
        assert_eq!(unescape(&[0, 0, 3, 1]), vec![0, 0, 1]);
        assert_eq!(unescape(&[0, 3, 1]), vec![0, 3, 1]);
        assert_eq!(unescape(&[1, 2, 3]), vec![1, 2, 3]);
    }

    #[test]
    fn a_bit_reader_stops_at_the_end_of_its_buffer() {
        let mut bits = Bits::new(&[0x00]);
        assert_eq!(bits.ue(), None, "nine zero bits in a row cannot be a code");
        let mut empty = Bits::new(&[]);
        assert_eq!(empty.bit(), None);
        assert_eq!(empty.u(4), None);
        assert_eq!(empty.ue(), None);
        assert_eq!(empty.skip(1), None);
    }

    #[test]
    fn golomb_codes_round_trip_against_an_independent_writer() {
        for value in [0_u32, 1, 2, 3, 7, 8, 255, 65_535] {
            let mut writer = BitWriter::new();
            writer.ue(value);
            let bytes = writer.finish();
            let mut bits = Bits::new(&bytes);
            assert_eq!(bits.ue(), Some(value), "ue({value}) did not round trip");
        }
    }

    #[test]
    fn the_default_format_is_visibly_a_default() {
        let format = undeclared_format();
        assert_eq!(format.layout, PlaneLayout::Yuv420);
        assert_eq!(format.depth, BitDepth::Eight);
        assert!(!format.colour_is_fully_declared());
    }
}
