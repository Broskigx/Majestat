//! How samples are laid out and what they mean.
//!
//! Every type here **declares** and converts nothing. `maj-media` has no opinion about
//! scene-linear light: it says what the samples are and lets layer 3 and layer 4 decide what
//! to do about it. That is the same decision as disabling `swscale` in the pinned FFmpeg
//! build (`docs/FFMPEG.md`), and the two have to stay in step — YUV-to-RGB and the transfer
//! function are arithmetic on the certified path under ADR 0006, so they belong to
//! `maj-color`, which is layer 3 and therefore above this crate.
//!
//! # Why the transfer function is not `maj_color::TransferFunction`
//!
//! `docs/MEDIA.md` §4 types this field as `maj_color`'s enum, with a comment calling layer 3
//! allowed from layer 2. That comment is wrong and the architecture is right: dependencies
//! point downward only, so `maj-media` cannot name a layer-3 type. The deviation is also the
//! better model. A container does not declare a rendering curve; it declares an ITU-T H.273
//! **code point**, including ones no renderer implements and the code point `2`, which means
//! "unspecified" and is extremely common. [`TransferCharacteristics`] is that vocabulary, and
//! mapping it onto `maj_color::TransferFunction` — including deciding what an unspecified
//! transfer is assumed to be — is a colour decision made once, in the layer-4 adapter, where
//! the rest of the colour decisions already live.

/// How many bits a sample carries.
///
/// A closed set, because a ten-bit frame must never be rounded to eight on the way in:
/// integer rounding happens once, at the declared output boundary (ADR 0006), and a
/// conversion in the decoder would be a second one nobody declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BitDepth {
    /// Eight bits per sample.
    Eight,
    /// Ten bits per sample.
    Ten,
    /// Twelve bits per sample.
    Twelve,
    /// Sixteen bits per sample.
    Sixteen,
}

impl BitDepth {
    /// The depth in bits.
    #[must_use]
    pub const fn bits(self) -> u8 {
        match self {
            Self::Eight => 8,
            Self::Ten => 10,
            Self::Twelve => 12,
            Self::Sixteen => 16,
        }
    }

    /// The depth a stream declares, or `None` for one this build has no model for.
    ///
    /// Nine-bit and fourteen-bit streams exist and are deliberately not invented a variant
    /// for: a probe that cannot model a depth says so rather than rounding it to a depth it
    /// can.
    #[must_use]
    pub const fn from_bits(bits: u8) -> Option<Self> {
        match bits {
            8 => Some(Self::Eight),
            10 => Some(Self::Ten),
            12 => Some(Self::Twelve),
            16 => Some(Self::Sixteen),
            _ => None,
        }
    }

    /// The largest code value at this depth.
    #[must_use]
    pub const fn max_code(self) -> u32 {
        (1_u32 << self.bits()) - 1
    }
}

/// How the planes are arranged and how chroma is subsampled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaneLayout {
    /// Luma only.
    Monochrome,
    /// Chroma at half the width and half the height.
    Yuv420,
    /// Chroma at half the width, full height.
    Yuv422,
    /// Chroma at full resolution.
    Yuv444,
    /// Three interleaved colour components.
    Rgb,
    /// Three interleaved colour components and an alpha.
    Rgba,
}

impl PlaneLayout {
    /// The short stable name, for a manifest or a log line.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Monochrome => "y",
            Self::Yuv420 => "yuv420",
            Self::Yuv422 => "yuv422",
            Self::Yuv444 => "yuv444",
            Self::Rgb => "rgb",
            Self::Rgba => "rgba",
        }
    }

    /// Chroma subsampling as an exact pair of integer divisors, horizontal then vertical.
    ///
    /// `None` for a layout with no chroma planes to subsample.
    #[must_use]
    pub const fn chroma_divisors(self) -> Option<(u32, u32)> {
        match self {
            Self::Yuv420 => Some((2, 2)),
            Self::Yuv422 => Some((2, 1)),
            Self::Yuv444 => Some((1, 1)),
            Self::Monochrome | Self::Rgb | Self::Rgba => None,
        }
    }

    /// What H.264 and HEVC call `chroma_format_idc`, mapped to a layout.
    ///
    /// The one place a parameter-set number becomes a Majestat type, kept here so the
    /// container parsers do not each invent the mapping.
    #[must_use]
    pub const fn from_chroma_format_idc(idc: u8) -> Option<Self> {
        match idc {
            0 => Some(Self::Monochrome),
            1 => Some(Self::Yuv420),
            2 => Some(Self::Yuv422),
            3 => Some(Self::Yuv444),
            _ => None,
        }
    }
}

/// Whether code values use the full range or the video-legal one.
///
/// A stream can be tagged wrongly, which is one of the corpus's axes. A probe reports the
/// tag; it does not second-guess it, because guessing would make two files with identical
/// samples decode differently depending on a heuristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SampleRange {
    /// The container says nothing, which is the commonest case of all.
    #[default]
    Unspecified,
    /// Video range: 16..235 at eight bits, scaled at higher depths.
    Limited,
    /// The whole code range.
    Full,
}

impl SampleRange {
    /// The short stable name.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Unspecified => "unspecified",
            Self::Limited => "limited",
            Self::Full => "full",
        }
    }
}

/// The declared matrix coefficients, as ITU-T H.273 code points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ColourMatrix {
    /// Code point 2: the container declares nothing.
    #[default]
    Unspecified,
    /// Code point 0: identity, as used by RGB and YCgCo.
    Identity,
    /// Code point 1: BT.709.
    Bt709,
    /// Code point 6: BT.601 at 525 lines, which code point 5 also selects in practice.
    Bt601,
    /// Code point 9: BT.2020 non-constant luminance.
    Bt2020Ncl,
    /// Code point 10: BT.2020 constant luminance.
    Bt2020Cl,
    /// A code point this build has no model for, kept verbatim rather than flattened.
    Reserved(u8),
}

impl ColourMatrix {
    /// The declared code point.
    #[must_use]
    pub const fn code_point(self) -> u8 {
        match self {
            Self::Identity => 0,
            Self::Bt709 => 1,
            Self::Unspecified => 2,
            Self::Bt601 => 6,
            Self::Bt2020Ncl => 9,
            Self::Bt2020Cl => 10,
            Self::Reserved(code) => code,
        }
    }

    /// The matrix a code point names.
    #[must_use]
    pub const fn from_code_point(code: u8) -> Self {
        match code {
            0 => Self::Identity,
            1 => Self::Bt709,
            2 => Self::Unspecified,
            5 | 6 => Self::Bt601,
            9 => Self::Bt2020Ncl,
            10 => Self::Bt2020Cl,
            other => Self::Reserved(other),
        }
    }
}

/// The declared colour primaries, as ITU-T H.273 code points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ColourPrimaries {
    /// Code point 2: the container declares nothing.
    #[default]
    Unspecified,
    /// Code point 1: BT.709, which sRGB shares.
    Bt709,
    /// Code point 5: BT.470 system B/G, which PAL shares.
    Bt470Bg,
    /// Code point 6: SMPTE 170M, which NTSC shares.
    Smpte170m,
    /// Code point 9: BT.2020.
    Bt2020,
    /// Code point 12: DCI-P3 with a D65 white point.
    DisplayP3,
    /// A code point this build has no model for.
    Reserved(u8),
}

impl ColourPrimaries {
    /// The declared code point.
    #[must_use]
    pub const fn code_point(self) -> u8 {
        match self {
            Self::Bt709 => 1,
            Self::Unspecified => 2,
            Self::Bt470Bg => 5,
            Self::Smpte170m => 6,
            Self::Bt2020 => 9,
            Self::DisplayP3 => 12,
            Self::Reserved(code) => code,
        }
    }

    /// The primaries a code point names.
    #[must_use]
    pub const fn from_code_point(code: u8) -> Self {
        match code {
            1 => Self::Bt709,
            2 => Self::Unspecified,
            5 => Self::Bt470Bg,
            6 => Self::Smpte170m,
            9 => Self::Bt2020,
            12 => Self::DisplayP3,
            other => Self::Reserved(other),
        }
    }
}

/// The declared transfer characteristics, as ITU-T H.273 code points.
///
/// A declaration, not a curve: see this module's documentation for why this is `maj-media`'s
/// own vocabulary rather than `maj_color::TransferFunction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TransferCharacteristics {
    /// Code point 2: the container declares nothing.
    #[default]
    Unspecified,
    /// Code point 1: BT.709.
    Bt709,
    /// Code point 4: assumed display gamma 2.2.
    Gamma22,
    /// Code point 5: assumed display gamma 2.8.
    Gamma28,
    /// Code point 6: SMPTE 170M, numerically BT.709's curve.
    Smpte170m,
    /// Code point 8: linear light.
    Linear,
    /// Code point 13: IEC 61966-2-1, which is sRGB.
    Srgb,
    /// Code point 16: SMPTE ST 2084, the perceptual quantiser.
    Pq,
    /// Code point 18: ARIB STD-B67, hybrid log-gamma.
    Hlg,
    /// A code point this build has no model for.
    Reserved(u8),
}

impl TransferCharacteristics {
    /// The declared code point.
    #[must_use]
    pub const fn code_point(self) -> u8 {
        match self {
            Self::Bt709 => 1,
            Self::Unspecified => 2,
            Self::Gamma22 => 4,
            Self::Gamma28 => 5,
            Self::Smpte170m => 6,
            Self::Linear => 8,
            Self::Srgb => 13,
            Self::Pq => 16,
            Self::Hlg => 18,
            Self::Reserved(code) => code,
        }
    }

    /// The characteristics a code point names.
    #[must_use]
    pub const fn from_code_point(code: u8) -> Self {
        match code {
            1 => Self::Bt709,
            2 => Self::Unspecified,
            4 => Self::Gamma22,
            5 => Self::Gamma28,
            6 => Self::Smpte170m,
            8 => Self::Linear,
            13 => Self::Srgb,
            16 => Self::Pq,
            18 => Self::Hlg,
            other => Self::Reserved(other),
        }
    }
}

/// Where a chroma sample sits relative to the luma samples it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ChromaSiting {
    /// The container declares nothing.
    #[default]
    Unspecified,
    /// Co-sited horizontally with the left luma sample, interstitial vertically. MPEG-2.
    Left,
    /// Interstitial in both directions. JPEG and MPEG-1.
    Centre,
    /// Co-sited in both directions.
    TopLeft,
}

impl ChromaSiting {
    /// The short stable name.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Unspecified => "unspecified",
            Self::Left => "left",
            Self::Centre => "centre",
            Self::TopLeft => "top-left",
        }
    }
}

/// How samples are laid out and what they mean. No conversion is implied by naming it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PictureFormat {
    /// Plane arrangement and chroma subsampling.
    pub layout: PlaneLayout,
    /// Bits per sample.
    pub depth: BitDepth,
    /// Full or video-legal code range, as tagged.
    pub range: SampleRange,
    /// Matrix coefficients, as tagged.
    pub matrix: ColourMatrix,
    /// Colour primaries, as tagged.
    pub primaries: ColourPrimaries,
    /// Transfer characteristics, as tagged.
    pub transfer: TransferCharacteristics,
    /// Chroma siting, as tagged.
    pub chroma_siting: ChromaSiting,
}

impl PictureFormat {
    /// A layout and a depth, with nothing about colour declared.
    ///
    /// The commonest thing a container actually says, and the honest starting point for a
    /// parser that will overwrite the fields it finds declarations for.
    #[must_use]
    pub const fn undeclared(layout: PlaneLayout, depth: BitDepth) -> Self {
        Self {
            layout,
            depth,
            range: SampleRange::Unspecified,
            matrix: ColourMatrix::Unspecified,
            primaries: ColourPrimaries::Unspecified,
            transfer: TransferCharacteristics::Unspecified,
            chroma_siting: ChromaSiting::Unspecified,
        }
    }

    /// Whether every colour field was declared by the container.
    ///
    /// A delivery wants to know this: a master built from untagged media has had an
    /// assumption applied to it somewhere, and the honest place to notice is here.
    #[must_use]
    pub fn colour_is_fully_declared(&self) -> bool {
        self.range != SampleRange::Unspecified
            && self.matrix != ColourMatrix::Unspecified
            && self.primaries != ColourPrimaries::Unspecified
            && self.transfer != TransferCharacteristics::Unspecified
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depths_round_trip_and_unmodelled_ones_are_refused() {
        for depth in [
            BitDepth::Eight,
            BitDepth::Ten,
            BitDepth::Twelve,
            BitDepth::Sixteen,
        ] {
            assert_eq!(BitDepth::from_bits(depth.bits()), Some(depth));
        }
        assert_eq!(
            BitDepth::from_bits(14),
            None,
            "a depth we cannot model is not rounded to one we can"
        );
        assert_eq!(BitDepth::Ten.max_code(), 1_023);
    }

    #[test]
    fn code_points_round_trip() {
        for code in 0_u8..=255 {
            assert_eq!(ColourPrimaries::from_code_point(code).code_point(), code);
            assert_eq!(
                TransferCharacteristics::from_code_point(code).code_point(),
                code,
                "transfer code point {code} did not survive the round trip"
            );
        }
    }

    #[test]
    fn bt601_collapses_two_code_points_and_keeps_the_canonical_one() {
        // 5 and 6 differ only in line count and select the same matrix, so the round trip is
        // not the identity here. It is the one deliberate exception, and the rest round trip.
        assert_eq!(ColourMatrix::from_code_point(5), ColourMatrix::Bt601);
        assert_eq!(ColourMatrix::Bt601.code_point(), 6);
        for code in (0_u8..=255).filter(|code| *code != 5) {
            assert_eq!(ColourMatrix::from_code_point(code).code_point(), code);
        }
    }

    #[test]
    fn chroma_subsampling_is_exact_integer_divisors() {
        assert_eq!(PlaneLayout::Yuv420.chroma_divisors(), Some((2, 2)));
        assert_eq!(PlaneLayout::Yuv422.chroma_divisors(), Some((2, 1)));
        assert_eq!(PlaneLayout::Yuv444.chroma_divisors(), Some((1, 1)));
        assert_eq!(PlaneLayout::Rgb.chroma_divisors(), None);
    }

    #[test]
    fn chroma_format_idc_maps_only_where_it_is_defined() {
        assert_eq!(
            PlaneLayout::from_chroma_format_idc(1),
            Some(PlaneLayout::Yuv420)
        );
        assert_eq!(PlaneLayout::from_chroma_format_idc(4), None);
    }

    #[test]
    fn an_undeclared_format_says_so() {
        let format = PictureFormat::undeclared(PlaneLayout::Yuv420, BitDepth::Eight);
        assert!(!format.colour_is_fully_declared());
        let tagged = PictureFormat {
            range: SampleRange::Limited,
            matrix: ColourMatrix::Bt709,
            primaries: ColourPrimaries::Bt709,
            transfer: TransferCharacteristics::Bt709,
            ..format
        };
        assert!(tagged.colour_is_fully_declared());
    }
}
