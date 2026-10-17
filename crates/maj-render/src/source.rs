//! Where a clip's picture comes from, behind one trait.
//!
//! The renderer never opens a file, never calls a decoder and never knows whether the
//! pixels in front of it came from a camera, a generator or a test chart. It asks a
//! [`PictureSource`] for a sample, and that is the whole contract. This is the abstraction
//! ADR 0005 puts FFmpeg behind: the decoder is an implementation of a trait the renderer
//! already had, so the image path can be finished, tested and verified before a single
//! line of codec code exists.
//!
//! The sources implemented here cover the ladder from "no media at all" to "a file a person
//! exported from a viewer", and one rung below the bottom of it: media that is named and not
//! available.
//!
//! * [`Generated`] — a deterministic test chart derived from an integer seed. It needs no
//!   file, no decoder and no media on disk, so every test in this crate and the whole
//!   verification path run anywhere.
//! * [`Solid`] — a flat field, which is the simplest picture there is.
//! * [`Ppm`] — a real file read off disk, in the one container that parses without a
//!   library. It is the reference for "the renderer reads media".
//! * [`Png`] — a real still image, in the container every screenshot, title card and logo
//!   actually arrives in, inflated by a pure-Rust implementation rather than by a C library
//!   (see `docs/LICENSING.md` for what that dependency costs and why it is affordable).
//! * [`Title`] — real type, set in one of four built-in faces and drawn as antialiased
//!   strokes over whatever is beneath it, from an alphabet that lives in this module because
//!   a machine with no fonts on it still has to render a title.
//! * [`Placeholder`] — what is drawn in place of media that could not be read, labelled with
//!   *which* of the three reasons applies, because the alternative is a frame that lies.
//!
//! Both file formats store code values, not light, so a source carries the transfer function
//! that describes it and decodes once, at load, with the deterministic kernels in
//! [`maj_color`]. A sample that comes back from this module is always scene-linear.
//!
//! # Media this build cannot read
//!
//! What is deliberately absent is a video decoder. This module reads **stills**: one frame,
//! held, with the clip's retime map deciding how long it is on the timeline. Frames that move
//! are M2's.
//!
//! Until they exist, a project that names a movie is in the same position as a project whose
//! still has been moved or truncated: the picture it asked for is not available. That is
//! **one condition**, and [`MissingMedia`] is the one policy over it — stand in for it with a
//! labelled [`Placeholder`], which is what an editor wants, or refuse the frame, which is
//! what a delivery wants.
//!
//! It did not used to be one condition. A missing `.png` failed the render and a missing
//! `.mov` quietly became a test chart, so the same fact produced opposite behaviour
//! depending on the characters after the last dot — and the chart it became was the same
//! chart a project gets when it deliberately asks for `generated:`, so a manifest could not
//! tell a substitution from a picture somebody wanted. Both halves of that are fixed here:
//! the policy is uniform, and a placeholder says `offline`, `damaged` or `undecodable`
//! rather than `generated`.

use std::fmt;
use std::path::Path;

use maj_color::{TransferFunction, det};

use crate::framebuffer::Pixel;

/// The default transfer function assumed for a file that does not declare one.
///
/// sRGB is the overwhelming default for still images and for anything a person exported
/// from a viewer, and being explicit about the assumption is what makes it revisable.
pub const DEFAULT_TRANSFER: TransferFunction = TransferFunction::Srgb;

/// Why a source could not be produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    /// The container or its variant is not one this build reads.
    Unsupported(String),
    /// The bytes are not what the container claims.
    Malformed(String),
    /// The file could not be read.
    Io(String),
}

impl fmt::Display for SourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(what) => write!(formatter, "unsupported source: {what}"),
            Self::Malformed(what) => write!(formatter, "malformed source: {what}"),
            Self::Io(what) => write!(formatter, "source could not be read: {what}"),
        }
    }
}

impl std::error::Error for SourceError {}

/// A picture the renderer can read samples from.
///
/// Samples are taken at pixel centres in source coordinates: `(0.5, 0.5)` is the centre of
/// the top-left pixel, exactly as in [`crate::Framebuffer`]. Implementations clamp beyond
/// their own edges rather than returning transparency, because a source that fades out
/// where the renderer looks for it hides a framing bug.
pub trait PictureSource: fmt::Debug {
    /// Pixels across.
    fn width(&self) -> u32;
    /// Pixels down.
    fn height(&self) -> u32;
    /// One sample in scene-linear light.
    fn sample(&self, x: f32, y: f32) -> Pixel;
    /// Short name of the source, for the manifest.
    fn kind(&self) -> &'static str;
}

/// A generated test chart, derived entirely from an integer seed.
///
/// The chart is deliberately built from integers and comparisons, so it is identical on
/// every machine and in every run. It contains the three things a renderer has to get
/// right: flat areas of saturated colour (fusion and identity), a smooth ramp (banding and
/// quantisation) and hard horizontal and vertical edges at known positions (sampling and
/// transforms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Generated {
    width: u32,
    height: u32,
    seed: u64,
    frame: i64,
}

impl Generated {
    /// Builds a chart of the given size for a seed, at source frame zero.
    ///
    /// # Panics
    ///
    /// Panics if either dimension is zero.
    #[must_use]
    pub fn new(width: u32, height: u32, seed: u64) -> Self {
        assert!(width > 0 && height > 0, "a generated chart needs a size");
        Self {
            width,
            height,
            seed,
            frame: 0,
        }
    }

    /// A chart whose size and seed come from an asset's name, so a project with several
    /// generated assets gets a different picture for each and the same one every time.
    #[must_use]
    pub fn from_label(label: &str, width: u32, height: u32) -> Self {
        Self::new(width, height, hash_label(label))
    }

    /// The seed this chart was built from.
    #[must_use]
    pub const fn seed(&self) -> u64 {
        self.seed
    }

    /// The same chart at a source frame, sliding it across a pixel per frame.
    ///
    /// A procedural chart is not a still, and a renderer that cannot tell one timeline
    /// frame from another cannot demonstrate that a cut, a retime or a transition did
    /// anything. The chart moves by exactly one pixel per source frame and wraps, so two
    /// frames are always distinguishable and the motion is inspectable by eye. Frame zero is
    /// the chart the seed alone describes, so a chart's frame-zero picture never changes.
    #[must_use]
    pub const fn at_frame(self, frame: i64) -> Self {
        Self {
            width: self.width,
            height: self.height,
            seed: self.seed,
            frame,
        }
    }

    /// The source frame this chart is showing.
    #[must_use]
    pub const fn frame(&self) -> i64 {
        self.frame
    }

    /// The eight bars of the chart, as linear-light colours in the Rec.709 primaries.
    ///
    /// The values are the conventional 75 per cent bars expressed in linear light, and
    /// they are written as exact rationals of the integer code values they came from
    /// (`191 / 255`), so nothing here is a rounded decimal.
    const BARS: [Pixel; 8] = [
        [0.56, 0.56, 0.56, 1.0], // grey
        [0.56, 0.56, 0.0, 1.0],  // yellow
        [0.0, 0.56, 0.56, 1.0],  // cyan
        [0.0, 0.56, 0.0, 1.0],   // green
        [0.56, 0.0, 0.56, 1.0],  // magenta
        [0.56, 0.0, 0.0, 1.0],   // red
        [0.0, 0.0, 0.56, 1.0],   // blue
        [0.0, 0.0, 0.0, 1.0],    // black
    ];
}

impl PictureSource for Generated {
    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn sample(&self, x: f32, y: f32) -> Pixel {
        // The chart slides by one pixel per source frame, wrapping at its own width, so the
        // picture is a function of (seed, frame, size) and of nothing else.
        let shift = u64::try_from(self.frame.rem_euclid(i64::from(self.width))).unwrap_or(0);
        let xi = ((u64::from(clamp_axis(x, self.width)) + shift) % u64::from(self.width)) as u32;
        let yi = clamp_axis(y, self.height);
        let column = (u64::from(xi) * 8 / u64::from(self.width)) as usize;
        let bar = Self::BARS[column.min(7)];

        // The top quarter is bars, the middle is a linear ramp, the rest is a checker whose
        // phase comes from the seed.
        let quarter = self.height / 4;
        if yi < quarter {
            return bar;
        }
        if yi < quarter * 2 {
            let ramp = f64::from(xi) / f64::from(self.width - 1).max(1.0);
            let value = ramp * ramp; // a quadratic ramp spends more codes in the shadows
            return [value as f32, value as f32, value as f32, 1.0];
        }
        // A two-level checker whose two levels come from the seed: exactly two values per
        // channel, both a function of the seed, and adjacent cells always differ.
        let check = ((u64::from(xi) / 4) + (u64::from(yi) / 4)) % 2;
        let (low, high) = if self.seed & 1 == 0 {
            (0.05, 0.2)
        } else {
            (0.06, 0.18)
        };
        let bright = if check == 0 { low } else { high };
        [bright, bright * 0.5, bright * 0.25, 1.0]
    }

    fn kind(&self) -> &'static str {
        "generated"
    }
}

/// A binary or ASCII PPM, decoded to linear light once at load.
///
/// Both the plain `P6` form and the ASCII `P3` form are read, at either eight or sixteen
/// bits per channel, because those are the variants tools actually write. The `P7` and
/// `PFM` family is not read: it is a different format wearing the same magic, and guessing
/// at it would be worse than refusing it.
#[derive(Debug, Clone, PartialEq)]
pub struct Ppm {
    width: u32,
    height: u32,
    pixels: Vec<Pixel>,
}

impl Ppm {
    /// Parses a PPM from memory.
    pub fn from_bytes(bytes: &[u8], transfer: TransferFunction) -> Result<Self, SourceError> {
        let mut cursor = Cursor::new(bytes);
        let magic = cursor.token()?;
        let width = cursor.number()?;
        let height = cursor.number()?;
        let maximum = cursor.number()?;
        if width == 0 || height == 0 {
            return Err(SourceError::Malformed(
                "a PPM needs a non-zero size".to_owned(),
            ));
        }
        if maximum == 0 || maximum > 65_535 {
            return Err(SourceError::Malformed(format!(
                "PPM maximum sample value {maximum} is out of range"
            )));
        }
        let count = (width as usize) * (height as usize);
        let mut pixels = Vec::with_capacity(count);

        match magic.as_str() {
            "P6" => {
                cursor.expect_separator()?;
                let wide = maximum > 255;
                let needed = count * 3 * if wide { 2 } else { 1 };
                let body = cursor.rest();
                if body.len() < needed {
                    return Err(SourceError::Malformed(format!(
                        "P6 body holds {} bytes for {count} pixels, {needed} are needed",
                        body.len()
                    )));
                }
                for index in 0..count {
                    let mut channels = [0_u16; 3];
                    for (channel, value) in channels.iter_mut().enumerate() {
                        let offset = (index * 3 + channel) * if wide { 2 } else { 1 };
                        *value = if wide {
                            u16::from(body[offset]) << 8 | u16::from(body[offset + 1])
                        } else {
                            u16::from(body[offset])
                        };
                    }
                    pixels.push(decode_channels(channels, maximum, transfer));
                }
            }
            "P3" => {
                for _ in 0..count {
                    let mut channels = [0_u16; 3];
                    for value in &mut channels {
                        let sample = cursor.sample()?;
                        if sample > maximum as u16 {
                            return Err(SourceError::Malformed(format!(
                                "the sample {sample} exceeds the declared maximum {maximum}"
                            )));
                        }
                        *value = sample;
                    }
                    pixels.push(decode_channels(channels, maximum, transfer));
                }
            }
            other => {
                return Err(SourceError::Unsupported(format!("PPM magic `{other}`")));
            }
        }

        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    /// Reads a PPM from disk.
    pub fn from_path(
        path: impl AsRef<Path>,
        transfer: TransferFunction,
    ) -> Result<Self, SourceError> {
        let path = path.as_ref();
        let bytes =
            std::fs::read(path).map_err(|error| SourceError::Io(format!("{path:?}: {error}")))?;
        Self::from_bytes(&bytes, transfer)
    }
}

impl PictureSource for Ppm {
    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn sample(&self, x: f32, y: f32) -> Pixel {
        let xi = clamp_axis(x, self.width) as usize;
        let yi = clamp_axis(y, self.height) as usize;
        self.pixels[yi * self.width as usize + xi]
    }

    fn kind(&self) -> &'static str {
        "ppm"
    }
}

/// The eight bytes every PNG starts with, including the two that catch a text-mode transfer.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// A PNG, decoded to linear light once at load.
///
/// This is the container a still image actually arrives in: a screenshot, an export from a
/// design tool, a title card, the project's own logo. The variants read are the ones real
/// files use — greyscale, truecolour, indexed, and the two with an alpha channel, at eight or
/// sixteen bits per sample, with `tRNS` transparency honoured for the indexed and colour-keyed
/// cases. Interlaced files and the packed greyscale depths below eight bits are refused by
/// name rather than guessed at, in the same spirit as the PPM reader refusing `P7`.
///
/// The file's own `gAMA` or `sRGB` chunk is **not** used to override the transfer function the
/// caller declares. It would be a second opinion about the same pixels, and the renderer's
/// rule is that the transfer function travels with the request: the command line, the interface
/// and the manifest all say which one was used, and a file cannot quietly disagree.
#[derive(Debug, Clone, PartialEq)]
pub struct Png {
    width: u32,
    height: u32,
    pixels: Vec<Pixel>,
}

impl Png {
    /// Reads a PNG from disk.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError::Io`] when the file cannot be read, and the same refusals
    /// [`Self::from_bytes`] produces.
    pub fn from_path(
        path: impl AsRef<Path>,
        transfer: TransferFunction,
    ) -> Result<Self, SourceError> {
        let path = path.as_ref();
        let bytes =
            std::fs::read(path).map_err(|error| SourceError::Io(format!("{path:?}: {error}")))?;
        Self::from_bytes(&bytes, transfer)
    }

    /// Decodes a PNG from memory.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError::Malformed`] when the bytes are not a PNG, when a chunk is
    /// truncated or fails its checksum, or when the image data does not inflate; and
    /// [`SourceError::Unsupported`] for a file whose interlacing or bit depth this build does
    /// not read.
    pub fn from_bytes(bytes: &[u8], transfer: TransferFunction) -> Result<Self, SourceError> {
        let image = Image::read(bytes)?;
        let count = image.width as usize * image.height as usize;
        let mut pixels = Vec::with_capacity(count);
        for index in 0..count {
            pixels.push(image.pixel(index, transfer));
        }
        Ok(Self {
            width: image.width,
            height: image.height,
            pixels,
        })
    }

    /// Pixels across.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Pixels down.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }
}

impl PictureSource for Png {
    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn sample(&self, x: f32, y: f32) -> Pixel {
        let xi = clamp_axis(x, self.width) as usize;
        let yi = clamp_axis(y, self.height) as usize;
        self.pixels[yi * self.width as usize + xi]
    }

    fn kind(&self) -> &'static str {
        "png"
    }
}

/// A decoded PNG, before the transfer function has been applied.
///
/// Samples are held at the file's own scale, which is what makes one decode serve every
/// sample the renderer asks for: the conversion to scene-linear light happens per pixel at
/// load, once, rather than per read.
#[derive(Debug, Clone, PartialEq)]
struct Image {
    width: u32,
    height: u32,
    /// The colour type from the header: 0 grey, 2 truecolour, 3 indexed, 4 grey and alpha,
    /// 6 truecolour and alpha.
    colour: u8,
    /// Samples per pixel, in the order the file stores them.
    channels: usize,
    /// The largest sample value in `samples`, which is what turns a code into a fraction.
    maximum: u16,
    /// Every sample in the image, row-major, interleaved per pixel.
    samples: Vec<u16>,
    /// The palette, for colour type 3, as eight-bit code values.
    palette: Vec<[u16; 3]>,
    /// The per-entry alpha of the palette, or empty when every entry is opaque.
    palette_alpha: Vec<u16>,
    /// A colour the file declares fully transparent, as samples at the header's scale.
    key: Option<[u16; 3]>,
}

impl Image {
    /// Reads the chunks, inflates the image data and unfilters it.
    fn read(bytes: &[u8]) -> Result<Self, SourceError> {
        if bytes.len() < PNG_SIGNATURE.len() || bytes[..8] != PNG_SIGNATURE {
            return Err(SourceError::Malformed(
                "the bytes are not a PNG: the signature is missing".to_owned(),
            ));
        }
        let mut header: Option<[u8; 13]> = None;
        let mut palette: Vec<[u16; 3]> = Vec::new();
        let mut palette_alpha: Vec<u16> = Vec::new();
        let mut key: Option<[u16; 3]> = None;
        let mut transparency: Vec<u8> = Vec::new();
        let mut data: Vec<u8> = Vec::new();
        let mut offset = PNG_SIGNATURE.len();
        while offset + 12 <= bytes.len() {
            let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            let kind = &bytes[offset + 4..offset + 8];
            let end = offset + 8 + length;
            if end + 4 > bytes.len() {
                return Err(SourceError::Malformed(format!(
                    "a {} chunk is truncated",
                    String::from_utf8_lossy(kind)
                )));
            }
            let body = &bytes[offset + 8..end];
            let declared = u32::from_be_bytes(bytes[end..end + 4].try_into().unwrap());
            let computed = crc32(&bytes[offset + 4..end]);
            if declared != computed {
                return Err(SourceError::Malformed(format!(
                    "a {} chunk fails its checksum",
                    String::from_utf8_lossy(kind)
                )));
            }
            match kind {
                b"IHDR" => {
                    if length != 13 {
                        return Err(SourceError::Malformed(format!(
                            "the header is {length} bytes, not 13"
                        )));
                    }
                    header = Some(body.try_into().unwrap());
                }
                b"PLTE" => {
                    if length % 3 != 0 {
                        return Err(SourceError::Malformed(
                            "the palette is not a whole number of entries".to_owned(),
                        ));
                    }
                    for entry in body.chunks_exact(3) {
                        palette.push([
                            u16::from(entry[0]),
                            u16::from(entry[1]),
                            u16::from(entry[2]),
                        ]);
                    }
                }
                b"tRNS" => transparency = body.to_vec(),
                b"IDAT" => data.extend_from_slice(body),
                b"IEND" => break,
                _ => {}
            }
            offset = end + 4;
        }
        let Some(header) = header else {
            return Err(SourceError::Malformed(
                "the file has no header chunk".to_owned(),
            ));
        };
        if data.is_empty() {
            return Err(SourceError::Malformed(
                "the file holds no image data".to_owned(),
            ));
        }
        let width = u32::from_be_bytes(header[0..4].try_into().unwrap());
        let height = u32::from_be_bytes(header[4..8].try_into().unwrap());
        let depth = header[8];
        let colour = header[9];
        if width == 0 || height == 0 {
            return Err(SourceError::Malformed(
                "a PNG needs a non-zero size".to_owned(),
            ));
        }
        if header[12] != 0 {
            return Err(SourceError::Unsupported(
                "an interlaced PNG, which this build does not read".to_owned(),
            ));
        }
        let channels = match colour {
            0 => 1,
            2 => 3,
            3 => 1,
            4 => 2,
            6 => 4,
            other => {
                return Err(SourceError::Unsupported(format!("PNG colour type {other}")));
            }
        };
        let allowed = match colour {
            0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
            3 => matches!(depth, 1 | 2 | 4 | 8),
            _ => matches!(depth, 8 | 16),
        };
        if !allowed {
            return Err(SourceError::Unsupported(format!(
                "{depth} bits per sample for PNG colour type {colour}"
            )));
        }
        if colour == 3 && palette.is_empty() {
            return Err(SourceError::Malformed(
                "an indexed PNG with no palette".to_owned(),
            ));
        }

        let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&data).map_err(|error| {
            SourceError::Malformed(format!("the PNG image data did not inflate: {error:?}"))
        })?;
        let rows = unfilter(&raw, width, height, channels, depth)?;
        // A packed greyscale sample is scaled to the full range; a packed palette index is an
        // index, and scaling it would name a different colour.
        let samples = samples_of(&rows, width, height, channels, depth, colour == 0);

        // Transparency: at most three of the file's own samples.
        match colour {
            3 => {
                for alpha in transparency.iter().take(palette.len()) {
                    palette_alpha.push(u16::from(*alpha));
                }
            }
            0 if transparency.len() >= 2 => {
                let grey = u32::from(u16::from_be_bytes([transparency[0], transparency[1]]));
                let scaled = (grey * packed_scale(depth)) as u16;
                key = Some([scaled, scaled, scaled]);
            }
            2 if transparency.len() >= 6 => {
                let channel =
                    |at: usize| u16::from_be_bytes([transparency[at], transparency[at + 1]]);
                key = Some([channel(0), channel(2), channel(4)]);
            }
            _ => {}
        }

        Ok(Self {
            width,
            height,
            colour,
            channels,
            maximum: sample_scale(depth),
            samples,
            palette,
            palette_alpha,
            key,
        })
    }

    /// One pixel, in scene-linear light.
    fn pixel(&self, index: usize, transfer: TransferFunction) -> Pixel {
        let base = index * self.channels;
        let (codes, maximum, alpha, alpha_maximum) = match self.colour {
            0 => {
                let grey = self.samples[base];
                let codes = [grey, grey, grey];
                (codes, self.maximum, self.key_alpha(codes), self.maximum)
            }
            2 => {
                let codes = [
                    self.samples[base],
                    self.samples[base + 1],
                    self.samples[base + 2],
                ];
                (codes, self.maximum, self.key_alpha(codes), self.maximum)
            }
            3 => {
                // A palette entry is eight bits whether the index is one bit or eight.
                let entry = self.samples[base] as usize;
                let codes = self.palette.get(entry).copied().unwrap_or([0, 0, 0]);
                let alpha = self.palette_alpha.get(entry).copied().unwrap_or(255);
                (codes, 255, alpha, 255)
            }
            4 => {
                let grey = self.samples[base];
                (
                    [grey, grey, grey],
                    self.maximum,
                    self.samples[base + 1],
                    self.maximum,
                )
            }
            _ => {
                let codes = [
                    self.samples[base],
                    self.samples[base + 1],
                    self.samples[base + 2],
                ];
                (codes, self.maximum, self.samples[base + 3], self.maximum)
            }
        };
        let mut decoded = decode_channels(codes, u32::from(maximum), transfer);
        decoded[3] =
            det::clamp(f64::from(alpha) / f64::from(alpha_maximum.max(1)), 0.0, 1.0) as f32;
        decoded
    }

    /// The alpha of a colour, against the file's own transparent colour.
    fn key_alpha(&self, codes: [u16; 3]) -> u16 {
        match self.key {
            Some(key) if key == codes => 0,
            _ => self.maximum,
        }
    }
}

/// The largest sample a file stores, for the colour and bit depth it declares.
///
/// PNG stores a greyscale image at one, two or four bits per sample when that is all the
/// values it needs; those samples are scaled up to `u16::MAX` as they are read, so every
/// path after this can assume a sample is a fraction of one of two denominators.
fn sample_scale(depth: u8) -> u16 {
    if depth == 8 { 255 } else { u16::MAX }
}

/// What a packed sample is multiplied by to reach the full sixteen-bit range.
fn packed_scale(depth: u8) -> u32 {
    match depth {
        1 => 65_535,
        2 => 21_845,
        4 => 4_369,
        _ => 1,
    }
}

/// Reverses the five PNG filters, row by row.
///
/// The rows are the file's own bytes, before any unpacking: a filter runs over the bytes of a
/// row, which for a sub-eight-bit image is fewer bytes than it has samples.
fn unfilter(
    raw: &[u8],
    width: u32,
    height: u32,
    channels: usize,
    depth: u8,
) -> Result<Vec<u8>, SourceError> {
    let bits = channels * usize::from(depth);
    let stride = (width as usize * bits).div_ceil(8);
    let needed = (stride + 1) * height as usize;
    if raw.len() < needed {
        return Err(SourceError::Malformed(format!(
            "the image data holds {} bytes for a {width}x{height} image, {needed} are needed",
            raw.len()
        )));
    }
    let bpp = bits.div_ceil(8).max(1);
    let mut out = vec![0_u8; stride * height as usize];
    for row in 0..height as usize {
        let start = row * (stride + 1);
        let filter = raw[start];
        let source = &raw[start + 1..start + 1 + stride];
        let target = row * stride;
        let above = target.wrapping_sub(stride);
        for index in 0..stride {
            let left = if index >= bpp {
                out[target + index - bpp]
            } else {
                0
            };
            let up = if row > 0 { out[above + index] } else { 0 };
            let up_left = if row > 0 && index >= bpp {
                out[above + index - bpp]
            } else {
                0
            };
            out[target + index] = match filter {
                0 => source[index],
                1 => source[index].wrapping_add(left),
                2 => source[index].wrapping_add(up),
                3 => source[index].wrapping_add(((u16::from(left) + u16::from(up)) / 2) as u8),
                4 => source[index].wrapping_add(paeth(left, up, up_left)),
                other => {
                    return Err(SourceError::Malformed(format!(
                        "the image uses filter {other}, which PNG does not define"
                    )));
                }
            };
        }
    }
    Ok(out)
}

/// Every sample of every pixel, at the file's own scale.
fn samples_of(
    rows: &[u8],
    width: u32,
    height: u32,
    channels: usize,
    depth: u8,
    scale_packed: bool,
) -> Vec<u16> {
    let count = width as usize * height as usize;
    let stride = (width as usize * channels * usize::from(depth)).div_ceil(8);
    let mut samples = Vec::with_capacity(count * channels);
    if depth == 16 {
        for row in 0..height as usize {
            let line = &rows[row * stride..(row * stride) + width as usize * channels * 2];
            for sample in line.chunks_exact(2) {
                samples.push(u16::from_be_bytes([sample[0], sample[1]]));
            }
        }
    } else if depth == 8 {
        for row in 0..height as usize {
            let line = &rows[row * stride..(row * stride) + width as usize * channels];
            samples.extend(line.iter().map(|byte| u16::from(*byte)));
        }
    } else {
        let factor = if scale_packed { packed_scale(depth) } else { 1 };
        for row in 0..height as usize {
            let line = &rows[row * stride..(row + 1) * stride];
            for index in 0..width as usize * channels {
                let bit = index * usize::from(depth);
                let byte = line[bit / 8];
                let shift = 8 - usize::from(depth) - bit % 8;
                let mask = (1_u16 << depth) - 1;
                let value = u32::from((u16::from(byte) >> shift) & mask);
                samples.push((value * factor) as u16);
            }
        }
    }
    samples
}

/// The PNG filter that predicts a sample from the one to its left, above it and above-left.
fn paeth(left: u8, up: u8, up_left: u8) -> u8 {
    let (left, up, up_left) = (i32::from(left), i32::from(up), i32::from(up_left));
    let estimate = left + up - up_left;
    let to_left = (estimate - left).abs();
    let to_up = (estimate - up).abs();
    let to_up_left = (estimate - up_left).abs();
    if to_left <= to_up && to_left <= to_up_left {
        left as u8
    } else if to_up <= to_up_left {
        up as u8
    } else {
        up_left as u8
    }
}

/// The CRC every PNG chunk carries, written out rather than tabulated.
///
/// A table would be faster and a table is 256 constants that a reader has to trust; this is
/// eight lines that a reader can check. A still image is decoded once, so the trade is
/// deliberate.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Produces the picture for an asset.
///
/// The renderer holds one of these, and it is the seam a decoder slots into. The default
/// implementation is procedural, so the image path has no dependency on media existing on
/// disk; [`WorkspaceResolver`] adds PNG and PPM stills for the case where it does.
pub trait PictureResolver: fmt::Debug {
    /// The picture behind an asset's URI, at a source frame.
    ///
    /// `source_frame` is the frame the clip's retime map asks for. A file-backed source
    /// holds one frame and ignores it; a procedural one uses it, which is how a generated
    /// chart moves. The parameter is here rather than in [`PictureSource::sample`] because
    /// a decoder resolves a frame once and samples it many times, and asking it to decode
    /// per pixel would be a different design.
    ///
    /// # Errors
    ///
    /// Returns a [`SourceError`] when the URI names something this resolver cannot read
    /// *and* its policy is to refuse rather than substitute. See [`MissingMedia`].
    fn resolve(
        &self,
        uri: &str,
        generated_size: (u32, u32),
        source_frame: i64,
    ) -> Result<Box<dyn PictureSource>, SourceError>;

    /// The [`PictureSource::kind`] this resolver would produce for a URI, without decoding
    /// anything.
    ///
    /// The render manifest has to name the source of a layer it is describing but not
    /// rendering, and opening the media to get that name would mean decoding a frame in
    /// order to write a word about it. So the resolver answers the question itself.
    ///
    /// This lives on the trait rather than beside it on purpose: the implementation that
    /// answers it is the implementation that would do the opening, so the label and the
    /// decision cannot drift apart. A free function next to the trait could, and did.
    ///
    /// It may cost a stat — knowing whether a file is there is the difference between
    /// `"png"` and `"offline"` — but never a decode.
    ///
    /// [`WorkspaceResolver`] has one residual gap, documented rather than hidden: a file that
    /// is present but damaged is named for its container here and comes back as a placeholder
    /// from [`PictureResolver::resolve`], because telling those apart means reading it, and
    /// this resolver has no prober in it. [`crate::MediaResolver`] closes it — a cached probe
    /// reads headers, which is not a decode, so it answers `damaged` honestly.
    fn kind(&self, uri: &str) -> &'static str;
}

/// What a resolver does about media it cannot read.
///
/// Both answers are legitimate and they belong to different moments, which is why this is a
/// policy and not a constant. The thing that is *not* legitimate is the third answer the
/// engine used to give by accident: refusing a still that was missing while quietly
/// substituting a movie that was, so that the same fact produced two behaviours depending
/// on the file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MissingMedia {
    /// Draw a labelled [`Placeholder`] and carry on.
    ///
    /// The default, and what an editor wants: a project with one unreadable shot in it
    /// still shows the other ninety-nine, and the frame says what it could not read instead
    /// of inventing pixels or refusing the whole timeline. The substitution is named in the
    /// render manifest, so it is visible to a script as well as to a person.
    #[default]
    Placeholder,
    /// Refuse the frame.
    ///
    /// What a delivery wants. A placeholder in a master is worse than a failed render,
    /// because a failed render is noticed.
    Refuse,
}

impl MissingMedia {
    /// The short stable name, for a manifest or a command line.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Placeholder => "placeholder",
            Self::Refuse => "refuse",
        }
    }
}

/// Resolves a URI to the picture behind it, and takes one declared policy about the ones it
/// cannot read.
///
/// Four URI forms resolve to real pictures: `.png` and `.ppm`/`.pnm` read a still off disk,
/// and `generated:`, `solid:` and `title:` are built from the URI itself. Everything else —
/// a movie this build has no decoder for, a still that is not where the project says it is,
/// a file whose bytes are not what its container claims — is **media this resolver cannot
/// read**, and that is one condition with one policy, [`MissingMedia`], rather than a
/// behaviour per file extension.
///
/// That uniformity is the point. The engine used to refuse a frame when a `.png` was missing
/// and quietly substitute a test chart when a `.mov` was, which meant the same fact — "the
/// picture this clip names is not available" — produced a hard failure or an invisible
/// substitution depending on the characters after the last dot. Worse, the substitute was
/// the same chart a project gets when it deliberately asks for `generated:`, so a manifest
/// could not distinguish a chart somebody wanted from one the engine fell back to.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WorkspaceResolver {
    transfer: Option<TransferFunction>,
    missing: MissingMedia,
}

impl WorkspaceResolver {
    /// A resolver that assumes the default transfer function and substitutes a labelled
    /// placeholder for media it cannot read.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            transfer: None,
            missing: MissingMedia::Placeholder,
        }
    }

    /// A resolver that reads files with a declared transfer function.
    #[must_use]
    pub const fn with_transfer(transfer: TransferFunction) -> Self {
        Self {
            transfer: Some(transfer),
            missing: MissingMedia::Placeholder,
        }
    }

    /// The same resolver with a different answer for media it cannot read.
    ///
    /// A delivery passes [`MissingMedia::Refuse`]: a placeholder in a master is worse than a
    /// failed render, because a failed render is noticed.
    #[must_use]
    pub const fn with_missing_media(self, missing: MissingMedia) -> Self {
        Self { missing, ..self }
    }

    /// The transfer function this resolver reads files with, where it was declared.
    ///
    /// `None` means [`DEFAULT_TRANSFER`] is assumed. Exposed because the media adapter reads
    /// stills itself — it knows from a probe *what* a file is, where this resolver only knows
    /// what its name claims — and it has to read them with the same declared curve, or two
    /// paths to one file would produce two pictures.
    #[must_use]
    pub const fn transfer(&self) -> Option<TransferFunction> {
        self.transfer
    }

    /// What this resolver does about media it cannot read.
    #[must_use]
    pub const fn missing_media(&self) -> MissingMedia {
        self.missing
    }

    /// The picture, or the reason there is none. The policy is applied by the caller.
    fn open(
        &self,
        uri: &str,
        generated_size: (u32, u32),
        source_frame: i64,
    ) -> Result<Box<dyn PictureSource>, SourceError> {
        let lowered = uri.to_ascii_lowercase();
        if lowered.ends_with(".ppm") || lowered.ends_with(".pnm") {
            let transfer = self.transfer.unwrap_or(DEFAULT_TRANSFER);
            // A still file holds one frame, so the source frame is deliberately ignored.
            return Ok(Box::new(Ppm::from_path(file_path(uri), transfer)?));
        }
        if lowered.ends_with(".png") {
            let transfer = self.transfer.unwrap_or(DEFAULT_TRANSFER);
            return Ok(Box::new(Png::from_path(file_path(uri), transfer)?));
        }
        let (width, height) = generated_size;
        if lowered.starts_with("generated:") {
            return Ok(Box::new(
                Generated::from_label(uri, width, height).at_frame(source_frame),
            ));
        }
        if lowered.starts_with("solid:") {
            return Ok(Box::new(Solid::from_label(uri, width, height)));
        }
        if lowered.starts_with("title:") {
            // A title is not a still: its size is the frame's, not the asset's, so it is
            // resolved at whatever the sequence asked for and the text is held in the URI.
            return Ok(Box::new(Title::from_label(uri, width, height)));
        }
        Err(SourceError::Unsupported(format!(
            "no decoder for {uri}: this build reads stills, and moving pictures are M2's"
        )))
    }
}

/// The path part of a URI, for the containers that are read off disk.
///
/// `pub(crate)` so the media adapter resolves a URI to a path exactly as the still readers
/// do: two spellings of this would be two answers about where a file is.
pub(crate) fn file_path(uri: &str) -> &str {
    uri.strip_prefix("file://").unwrap_or(uri)
}

impl PictureResolver for WorkspaceResolver {
    fn resolve(
        &self,
        uri: &str,
        generated_size: (u32, u32),
        source_frame: i64,
    ) -> Result<Box<dyn PictureSource>, SourceError> {
        match self.open(uri, generated_size, source_frame) {
            Ok(source) => Ok(source),
            Err(reason) => match self.missing {
                MissingMedia::Refuse => Err(reason),
                MissingMedia::Placeholder => Ok(Box::new(Placeholder::for_error(
                    &reason,
                    generated_size.0,
                    generated_size.1,
                ))),
            },
        }
    }

    fn kind(&self, uri: &str) -> &'static str {
        let lowered = uri.to_ascii_lowercase();
        let (expected, path_backed) = if lowered.ends_with(".png") {
            ("png", true)
        } else if lowered.ends_with(".ppm") || lowered.ends_with(".pnm") {
            ("ppm", true)
        } else if lowered.starts_with("solid:") {
            ("solid", false)
        } else if lowered.starts_with("generated:") {
            ("generated", false)
        } else if lowered.starts_with("title:") {
            ("title", false)
        } else {
            // No decoder in this build, which a stat cannot change.
            return match self.missing {
                MissingMedia::Placeholder => Placeholder::UNDECODABLE,
                MissingMedia::Refuse => "-",
            };
        };
        if !path_backed {
            return expected;
        }
        // A stat, never a decode: whether the file is there is the whole difference between
        // naming the container and naming the substitution.
        if Path::new(file_path(uri)).is_file() {
            expected
        } else {
            match self.missing {
                MissingMedia::Placeholder => Placeholder::OFFLINE,
                MissingMedia::Refuse => "-",
            }
        }
    }
}

/// The picture drawn in place of media the renderer could not read.
///
/// It is **not** a test chart, and the distinction is the reason this type exists. A chart is
/// a picture a project asked for; this is the engine reporting a fact about the project, in
/// the frame, where the person is already looking. So it is deliberately ugly — diagonal
/// hazard bands in two dark reds, which no camera produces — and it says which fact in words,
/// set in the built-in alphabet that is already in this module for titles.
///
/// Three facts, kept apart because they need different answers from whoever sees them:
///
/// | [`SourceError`] | [`PictureSource::kind`] | What it means |
/// |---|---|---|
/// | [`Io`](SourceError::Io) | `offline` | The file is not where the project says it is. Relink it. |
/// | [`Malformed`](SourceError::Malformed) | `damaged` | It is there and its bytes are not what its container claims. Replace it. |
/// | [`Unsupported`](SourceError::Unsupported) | `undecodable` | It is there and this build cannot read it. Wait for M2, or transcode. |
///
/// The reason is kept verbatim as well as classified, because "offline" is what a person
/// needs and the engine's own sentence is what a bug report needs.
#[derive(Debug, Clone)]
pub struct Placeholder {
    width: u32,
    height: u32,
    label: &'static str,
    reason: String,
    words: Title,
}

impl Placeholder {
    /// The kind of a placeholder standing in for a file that is not there.
    pub const OFFLINE: &'static str = "offline";
    /// The kind of a placeholder standing in for a file whose bytes are wrong.
    pub const DAMAGED: &'static str = "damaged";
    /// The kind of a placeholder standing in for a file this build cannot decode.
    pub const UNDECODABLE: &'static str = "undecodable";

    /// Every kind a placeholder reports, so a caller can recognise one without matching
    /// strings it wrote itself.
    pub const KINDS: [&'static str; 3] = [Self::OFFLINE, Self::DAMAGED, Self::UNDECODABLE];

    /// Whether a [`PictureSource::kind`] names a placeholder rather than a picture.
    ///
    /// This is the check a delivery gate wants over a render manifest: not "did it render"
    /// but "did any of it stand in for something".
    #[must_use]
    pub fn is_placeholder(kind: &str) -> bool {
        Self::KINDS.contains(&kind)
    }

    /// The width of a hazard band, in pixels. A power of two, so the division is exact and
    /// two tiles of one frame agree on where a band starts.
    const BAND: f32 = 32.0;

    /// The placeholder that stands in for this failure.
    ///
    /// # Panics
    ///
    /// Panics if either dimension is zero, as every source in this module does: a frame with
    /// no pixels in it is a broken request rather than an empty picture.
    #[must_use]
    pub fn for_error(error: &SourceError, width: u32, height: u32) -> Self {
        let (label, words) = match error {
            SourceError::Io(_) => (Self::OFFLINE, "MEDIA OFFLINE"),
            SourceError::Malformed(_) => (Self::DAMAGED, "MEDIA DAMAGED"),
            SourceError::Unsupported(_) => (Self::UNDECODABLE, "NO DECODER"),
        };
        Self::new(label, words, error.to_string(), width, height)
    }

    /// A placeholder with a given label and words.
    ///
    /// # Panics
    ///
    /// Panics if either dimension is zero.
    #[must_use]
    fn new(label: &'static str, words: &str, reason: String, width: u32, height: u32) -> Self {
        assert!(
            width > 0 && height > 0,
            "a placeholder needs a frame to sit on"
        );
        // A tenth of the frame's height, which is a caption rather than a title: the words
        // are a label on a picture, not the picture.
        let cap = (height as f32 / 10.0).max(1.0);
        Self {
            width,
            height,
            label,
            reason,
            words: Title::new(
                words,
                TitleFace::Mono,
                cap,
                [0.90, 0.90, 0.90],
                TitleAlign::Centre,
                width,
                height,
            ),
        }
    }

    /// The engine's own sentence about why this is here, for a manifest or a status line.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// The hazard field the words sit on.
    ///
    /// Built from addition, an exact division by a power of two and a floor, so it is the
    /// same on every machine and in every run, and a tile samples the same band as the
    /// whole frame does. `rem_euclid` is what makes that hold where a halo samples outside
    /// the frame and the coordinate is negative.
    fn ground(x: f32, y: f32) -> Pixel {
        let band = ((x + y) / Self::BAND).floor();
        if (band as i64).rem_euclid(2) == 0 {
            [0.050, 0.006, 0.006, 1.0]
        } else {
            [0.014, 0.002, 0.002, 1.0]
        }
    }
}

impl PictureSource for Placeholder {
    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn sample(&self, x: f32, y: f32) -> Pixel {
        let ground = Self::ground(x, y);
        let ink = self.words.sample(x, y);
        // Straight alpha over an opaque ground, written out rather than reaching for the
        // compositor: three exactly-rounded multiply-adds, and nothing the certified path
        // has to be told about.
        let alpha = ink[3];
        let rest = 1.0 - alpha;
        [
            ink[0] * alpha + ground[0] * rest,
            ink[1] * alpha + ground[1] * rest,
            ink[2] * alpha + ground[2] * rest,
            1.0,
        ]
    }

    fn kind(&self) -> &'static str {
        self.label
    }
}

/// A flat field: the simplest possible picture, and the one most tests want.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Solid {
    width: u32,
    height: u32,
    colour: Pixel,
}

impl Solid {
    /// A flat field of one colour.
    #[must_use]
    pub fn new(width: u32, height: u32, colour: Pixel) -> Self {
        Self {
            width,
            height,
            colour,
        }
    }

    /// A flat field whose colour comes from `solid:<r>,<g>,<b>` in eight-bit code values.
    #[must_use]
    pub fn from_label(label: &str, width: u32, height: u32) -> Self {
        let mut channels = [0.0_f32; 3];
        if let Some(rest) = label.strip_prefix("solid:") {
            for (index, part) in rest.split(',').take(3).enumerate() {
                if let Ok(code) = part.trim().parse::<u8>() {
                    channels[index] = f32::from(code) / 255.0;
                }
            }
        }
        Self::new(width, height, [channels[0], channels[1], channels[2], 1.0])
    }
}

impl PictureSource for Solid {
    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn sample(&self, _x: f32, _y: f32) -> Pixel {
        self.colour
    }

    fn kind(&self) -> &'static str {
        "solid"
    }
}

/// One polyline of a letter, in the glyph's own box: x across the glyph's width, y down from
/// the cap line (`0`) to the baseline (`1`). A negative `y` is above the cap line, which is
/// where the accents live, and a `y` past one is below the baseline, which is where a cedilla
/// lives.
type Polyline = &'static [(f32, f32)];

/// The face a title is set in.
///
/// These are **built-in faces**, not the machine's type library. A title has to render on a
/// machine with no fonts installed, inside a headless render, and byte-identically in two
/// runs, and a font file is none of those three things; so the alphabet is data in this
/// module and a face is a treatment of it — weight, width, tracking and serifs. That is a
/// deliberate limitation, and it is stated in `docs/UI.md`: there are four faces here, and
/// reading a font off the system is not one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleFace {
    /// Even weight, square proportions.
    Sans,
    /// Thinner strokes with a tick across every stroke end.
    Serif,
    /// Thin strokes on one advance, the space included.
    Mono,
    /// Heavy strokes at a little over half width.
    Condensed,
}

impl TitleFace {
    /// Every face, in the order the interface lists them.
    pub const ALL: [Self; 4] = [Self::Sans, Self::Serif, Self::Mono, Self::Condensed];

    /// The short stable name, which is what a title's URI carries.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Sans => "sans",
            Self::Serif => "serif",
            Self::Mono => "mono",
            Self::Condensed => "condensed",
        }
    }

    /// The face with this name, if there is one.
    #[must_use]
    pub fn from_key(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|face| face.key().eq_ignore_ascii_case(name.trim()))
    }

    /// Stroke width as a fraction of the type's cap height.
    const fn weight(self) -> f32 {
        match self {
            Self::Sans => 0.12,
            Self::Serif => 0.09,
            Self::Mono => 0.08,
            Self::Condensed => 0.15,
        }
    }

    /// The width of one unit of the glyph box, in units of cap height.
    const fn width_scale(self) -> f32 {
        match self {
            Self::Sans | Self::Serif => 0.84,
            Self::Mono => 0.70,
            Self::Condensed => 0.54,
        }
    }

    /// The extra space after a glyph, in glyph widths.
    const fn tracking(self) -> f32 {
        match self {
            Self::Sans | Self::Serif => 0.20,
            Self::Mono => 0.0,
            Self::Condensed => 0.16,
        }
    }

    /// Whether a stroke end carries a serif.
    const fn serifed(self) -> bool {
        matches!(self, Self::Serif)
    }

    /// One advance for every glyph, for the face that asks for it.
    const fn fixed_advance(self) -> Option<f32> {
        match self {
            // Every glyph gets the same room, and that room has to be wide enough for the
            // widest of them plus a gap. It used to be 0.80, which is narrower than `D`,
            // `O` and `R` actually draw, so a monospaced title set its wide letters on top
            // of each other. `a_glyph_never_draws_outside_its_own_advance` is the test that
            // now holds this to the alphabet instead of to a guess.
            Self::Mono => Some(1.12),
            Self::Sans | Self::Serif | Self::Condensed => None,
        }
    }
}

/// Where a line of a title sits in the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleAlign {
    /// Against the left margin.
    Left,
    /// Centred, which is what a title card wants.
    Centre,
    /// Against the right margin.
    Right,
}

impl TitleAlign {
    /// Every alignment, in the order the interface lists them.
    pub const ALL: [Self; 3] = [Self::Left, Self::Centre, Self::Right];

    /// The short stable name, which is what a title's URI carries.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Centre => "centre",
            Self::Right => "right",
        }
    }

    /// The alignment with this name, if there is one.
    #[must_use]
    pub fn from_key(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|align| align.key().eq_ignore_ascii_case(name.trim()))
    }
}

/// A character, resolved to the letter it is drawn as.
///
/// Lowercase is set as a small capital of its own letter rather than as a second alphabet:
/// one alphabet is a page of data and two is two pages, and a small capital is a treatment
/// every title card has used. The accented letters are their base letter plus a mark, the
/// inverted question and exclamation marks are their upright forms turned half around, and
/// anything else is drawn as its own letter or, failing that, as a missing-glyph box.
#[derive(Debug, Clone, Copy)]
struct Resolved {
    /// The capital this character is drawn as.
    base: char,
    /// The mark above or below it.
    mark: &'static [Polyline],
    /// Whether the whole glyph is turned half around.
    flip: bool,
    /// The height its capitals stand at, as a fraction of the line's cap height.
    scale: f32,
}

/// One glyph, measured: the segments it draws, in frame pixels, and the span of `x` a sample
/// may fall in and still touch it.
#[derive(Debug, Clone)]
struct Placed {
    /// The glyph's strokes, as `(x0, y0, x1, y1)` in frame pixels.
    segments: Vec<(f32, f32, f32, f32)>,
    /// The left edge of its advance.
    left: f32,
    /// The right edge of its advance.
    right: f32,
}

/// A title: text, set in one of four built-in faces, drawn over the frame.
///
/// A title is a **picture source**, which is what makes it ordinary: it is registered as a
/// media asset like any other, dropped on a track like any other, and it composes over what is
/// beneath it because every sample it returns carries an alpha — one inside a stroke, zero
/// outside, and a fraction along the edges, so the type is antialiased rather than jagged. A
/// sample is scene-linear like every other sample in this module, and the colour a person
/// types is sRGB code values, decoded once at the door.
///
/// The line is measured once, when the source is built, and the placed segments are kept:
/// [`PictureSource::sample`] is called once per pixel of the frame, so it may not re-measure a
/// string. Per sample it makes one comparison against the band the line occupies and then a
/// distance to the handful of segments of the one glyph that sample is over.
#[derive(Debug, Clone)]
pub struct Title {
    width: u32,
    height: u32,
    text: String,
    face: TitleFace,
    align: TitleAlign,
    size: f32,
    colour: [f32; 3],
    placed: Vec<Placed>,
    half_stroke: f32,
    band_top: f32,
    band_bottom: f32,
}

impl Title {
    /// Sets a line of text at a cap height, on a frame of a given size.
    ///
    /// The line sits on a baseline three quarters of the way down the frame, inside a margin
    /// of six per cent of its width, and `align` places it between those margins. Moving it is
    /// the clip's transform, which already exists and already animates, rather than a second
    /// kind of position.
    ///
    /// # Panics
    ///
    /// Panics if either dimension is zero. A cap height that is not a positive, finite number
    /// is a broken request rather than an empty picture, and one pixel is used instead.
    #[must_use]
    pub fn new(
        text: &str,
        face: TitleFace,
        size: f32,
        colour: [f32; 3],
        align: TitleAlign,
        width: u32,
        height: u32,
    ) -> Self {
        assert!(width > 0 && height > 0, "a title needs a frame to sit on");
        const BASELINE: f32 = 0.74;
        const MARGIN: f32 = 0.06;
        let cap = if size.is_finite() && size > 0.0 {
            size
        } else {
            1.0
        };
        let height_f = height as f32;
        let width_f = width as f32;
        let unit = cap * face.width_scale();
        let half_stroke = (cap * face.weight() / 2.0).max(0.5);
        let baseline = height_f * BASELINE;
        let margin = width_f * MARGIN;

        // The line, glyph by glyph, with the advance each one takes. A glyph's advance is its
        // own width plus the face's tracking, so a proportional face spaces an `I` and an `M`
        // differently and a monospaced one does not.
        let mut line: Vec<(Resolved, f32)> = Vec::new();
        for character in text.chars() {
            let resolved = resolve(character);
            let mut right = 0.0_f32;
            for polyline in strokes(&resolved) {
                for (x, y) in polyline {
                    right = right.max(place(&resolved, *x, *y).0);
                }
            }
            let advance = match face.fixed_advance() {
                Some(fixed) => fixed,
                None if character == ' ' => 0.40,
                None => (right + face.tracking()).max(0.34),
            };
            line.push((resolved, advance));
        }

        let total: f32 = line.iter().map(|(_, advance)| *advance).sum::<f32>() * unit;
        let mut pen = match align {
            TitleAlign::Left => margin,
            TitleAlign::Right => (width_f - margin - total).max(0.0),
            TitleAlign::Centre => (width_f - total) / 2.0,
        };

        let mut placed: Vec<Placed> = Vec::with_capacity(line.len());
        let mut band_top = baseline;
        let mut band_bottom = baseline;
        for (resolved, advance) in &line {
            let left = pen;
            let right = pen + advance * unit;
            let mut segments: Vec<(f32, f32, f32, f32)> = Vec::new();
            for polyline in strokes(resolved) {
                let points: Vec<(f32, f32)> = polyline
                    .iter()
                    .map(|&(x, y)| {
                        let (x, y) = place(resolved, x, y);
                        (left + x * unit, baseline - (1.0 - y) * cap)
                    })
                    .collect();
                for pair in points.windows(2) {
                    if let [from, to] = pair {
                        segments.push((from.0, from.1, to.0, to.1));
                        band_top = band_top.min(from.1.min(to.1));
                        band_bottom = band_bottom.max(from.1.max(to.1));
                    }
                }
                if face.serifed() {
                    // A serif is a tick laid across a stroke's end, square to the stroke
                    // itself, which is why the direction is used rather than assumed.
                    let ends = [
                        (points.first(), points.get(1)),
                        (points.last(), points.get(points.len().saturating_sub(2))),
                    ];
                    for (end, inner) in ends {
                        let (Some(end), Some(inner)) = (end, inner) else {
                            continue;
                        };
                        let (dx, dy) = (end.0 - inner.0, end.1 - inner.1);
                        let length = (dx * dx + dy * dy).sqrt();
                        let reach = cap * 0.13;
                        let (px, py) = if length <= 0.0 {
                            (0.0, reach)
                        } else {
                            (dy / length * reach, dx / length * reach)
                        };
                        segments.push((end.0 - px, end.1 + py, end.0 + px, end.1 - py));
                        band_top = band_top.min(end.1 - py.abs());
                        band_bottom = band_bottom.max(end.1 + py.abs());
                    }
                }
            }
            placed.push(Placed {
                segments,
                left,
                right,
            });
            pen = right;
        }

        Self {
            width,
            height,
            text: text.to_owned(),
            face,
            align,
            size: cap,
            colour,
            placed,
            half_stroke,
            band_top,
            band_bottom,
        }
    }

    /// A title whose text, face, size, colour and alignment come from a URI.
    ///
    /// The form is `title:<face>:<size>:<rrggbb>:<align>:<text>`, and every field but the text
    /// may be empty, in which case it takes its default — sans, a tenth of the frame's height,
    /// white, centred. The text is the remainder of the URI and may contain colons, because a
    /// title with a colon in it is not a malformed title.
    #[must_use]
    pub fn from_label(label: &str, width: u32, height: u32) -> Self {
        let rest = label.strip_prefix("title:").unwrap_or(label);
        let mut fields = rest.splitn(5, ':');
        let face = fields
            .next()
            .and_then(TitleFace::from_key)
            .unwrap_or(TitleFace::Sans);
        let size = fields
            .next()
            .and_then(|field| field.trim().parse::<f32>().ok())
            .filter(|size| size.is_finite() && *size > 0.0)
            .unwrap_or(height as f32 / 10.0);
        let colour = fields
            .next()
            .and_then(decode_hex)
            .unwrap_or([1.0, 1.0, 1.0]);
        let align = fields
            .next()
            .and_then(TitleAlign::from_key)
            .unwrap_or(TitleAlign::Centre);
        let text = fields.next().unwrap_or("");
        Self::new(text, face, size, colour, align, width, height)
    }

    /// The words the title sets.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The face it is set in.
    #[must_use]
    pub const fn face(&self) -> TitleFace {
        self.face
    }

    /// Its cap height in pixels.
    #[must_use]
    pub const fn size(&self) -> f32 {
        self.size
    }

    /// Where the line sits between the margins.
    #[must_use]
    pub const fn align(&self) -> TitleAlign {
        self.align
    }
}

impl PictureSource for Title {
    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn sample(&self, x: f32, y: f32) -> Pixel {
        // A sample outside the band the line occupies cannot touch it, and that is most of the
        // frame: one comparison rather than one distance per stroke.
        if y < self.band_top - 1.0 || y > self.band_bottom + 1.0 {
            return [0.0, 0.0, 0.0, 0.0];
        }
        let reach = self.half_stroke + 1.0;
        let glyph = self
            .placed
            .iter()
            .find(|glyph| x >= glyph.left - reach && x <= glyph.right + reach);
        let Some(glyph) = glyph else {
            return [0.0, 0.0, 0.0, 0.0];
        };
        let mut coverage = 0.0_f32;
        for (ax, ay, bx, by) in &glyph.segments {
            coverage = coverage.max(segment_coverage(x, y, *ax, *ay, *bx, *by, self.half_stroke));
            if coverage >= 1.0 {
                break;
            }
        }
        if coverage <= 0.0 {
            return [0.0, 0.0, 0.0, 0.0];
        }
        [self.colour[0], self.colour[1], self.colour[2], coverage]
    }

    fn kind(&self) -> &'static str {
        "title"
    }
}

/// How much of a sample one stroke claims, as the distance to its centre line falls away.
///
/// A stroke is a rectangle of width `2 * half`; a sample within `half` of the centre line
/// takes all of it, one exactly `half + 0.5` away takes none, and the half pixel between them
/// is the edge that keeps a diagonal legible.
fn segment_coverage(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32, half: f32) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let length_squared = dx * dx + dy * dy;
    let t = if length_squared <= 0.0 {
        0.0
    } else {
        (((px - ax) * dx + (py - ay) * dy) / length_squared).clamp(0.0, 1.0)
    };
    let (cx, cy) = (ax + dx * t, ay + dy * t);
    let (ox, oy) = (px - cx, py - cy);
    let distance = (ox * ox + oy * oy).sqrt();
    det::clamp(f64::from(half + 0.5 - distance), 0.0, 1.0) as f32
}

/// A glyph's point, in its line's own coordinates: turned half around if the character is an
/// inverted mark, then set as a small capital if it is lowercase.
fn place(resolved: &Resolved, x: f32, y: f32) -> (f32, f32) {
    let (x, y) = if resolved.flip {
        (1.0 - x, 1.0 - y)
    } else {
        (x, y)
    };
    (x, 1.0 - resolved.scale + resolved.scale * y)
}

/// Three code values from six hex digits, decoded to linear light.
///
/// A colour a person types into a field is a value on a screen, so it is sRGB by definition
/// and is decoded here rather than assumed to be light already.
fn decode_hex(field: &str) -> Option<[f32; 3]> {
    let field = field.trim().trim_start_matches('#');
    if field.len() != 6 {
        return None;
    }
    let mut out = [0.0_f32; 3];
    for (index, slot) in out.iter_mut().enumerate() {
        let pair = field.get(index * 2..index * 2 + 2)?;
        let code = u8::from_str_radix(pair, 16).ok()?;
        let value = det::clamp(f64::from(code) / 255.0, 0.0, 1.0);
        *slot = TransferFunction::Srgb.decode(value) as f32;
    }
    Some(out)
}

/// No mark.
const NO_MARK: &[Polyline] = &[];
/// An acute accent.
const ACUTE: &[Polyline] = &[&[(0.34, -0.20), (0.68, -0.02)]];
/// A grave accent.
const GRAVE: &[Polyline] = &[&[(0.32, -0.02), (0.66, -0.20)]];
/// A circumflex.
const CIRCUMFLEX: &[Polyline] = &[&[(0.28, -0.04), (0.50, -0.22), (0.72, -0.04)]];
/// A diaeresis, which is two dots.
const DIAERESIS: &[Polyline] = &[
    &[(0.36, -0.12), (0.36, -0.16)],
    &[(0.64, -0.12), (0.64, -0.16)],
];
/// A tilde, as a smooth saw rather than a curve, because every stroke here is a straight line.
const TILDE: &[Polyline] = &[&[(0.20, -0.06), (0.42, -0.18), (0.58, -0.04), (0.80, -0.16)]];
/// A cedilla, below the baseline.
const CEDILLA: &[Polyline] = &[&[(0.50, 1.00), (0.50, 1.12), (0.34, 1.20)]];

/// The character a title draws for a given one, with its mark and whether it is turned around.
///
/// The accents a Spanish title needs are here — the five vowels in both cases, `ñ`, `ç`, and
/// the two inverted marks — because an interface with a Spanish table that cannot set `AÑO`
/// is an interface with a Spanish table.
fn decompose(character: char) -> (char, &'static [Polyline], bool) {
    match character {
        'á' | 'Á' => ('A', ACUTE, false),
        'à' | 'À' => ('A', GRAVE, false),
        'â' | 'Â' => ('A', CIRCUMFLEX, false),
        'ä' | 'Ä' => ('A', DIAERESIS, false),
        'é' | 'É' => ('E', ACUTE, false),
        'è' | 'È' => ('E', GRAVE, false),
        'ê' | 'Ê' => ('E', CIRCUMFLEX, false),
        'ë' | 'Ë' => ('E', DIAERESIS, false),
        'í' | 'Í' => ('I', ACUTE, false),
        'ì' | 'Ì' => ('I', GRAVE, false),
        'î' | 'Î' => ('I', CIRCUMFLEX, false),
        'ï' | 'Ï' => ('I', DIAERESIS, false),
        'ó' | 'Ó' => ('O', ACUTE, false),
        'ò' | 'Ò' => ('O', GRAVE, false),
        'ô' | 'Ô' => ('O', CIRCUMFLEX, false),
        'ö' | 'Ö' => ('O', DIAERESIS, false),
        'ú' | 'Ú' => ('U', ACUTE, false),
        'ù' | 'Ù' => ('U', GRAVE, false),
        'û' | 'Û' => ('U', CIRCUMFLEX, false),
        'ü' | 'Ü' => ('U', DIAERESIS, false),
        'ñ' | 'Ñ' => ('N', TILDE, false),
        'ç' | 'Ç' => ('C', CEDILLA, false),
        '¿' => ('?', NO_MARK, true),
        '¡' => ('!', NO_MARK, true),
        'a'..='z' => (character.to_ascii_uppercase(), NO_MARK, false),
        other => (other, NO_MARK, false),
    }
}

/// A character, resolved.
fn resolve(character: char) -> Resolved {
    let (base, mark, flip) = decompose(character);
    Resolved {
        base,
        mark,
        flip,
        scale: if character.is_lowercase() { 0.78 } else { 1.0 },
    }
}

/// Every polyline of one glyph: its letter, then its mark when it has one.
fn strokes(resolved: &Resolved) -> Vec<Polyline> {
    let mut out: Vec<Polyline> = letter(resolved.base).to_vec();
    out.extend_from_slice(resolved.mark);
    out
}

/// The alphabet: a capital letter, a digit, or a mark of punctuation.
///
/// Anything not in it is drawn as the box printers have used for a letter they do not have
/// since type was metal, which is honest about the gap and legible in a frame.
fn letter(character: char) -> &'static [Polyline] {
    match character {
        ' ' => &[],
        'A' => &[
            &[(0.00, 1.00), (0.50, 0.00), (1.00, 1.00)],
            &[(0.20, 0.66), (0.80, 0.66)],
        ],
        'B' => &[
            &[(0.00, 0.00), (0.00, 1.00)],
            &[(0.00, 0.00), (0.62, 0.00), (0.62, 0.50), (0.00, 0.50)],
            &[(0.00, 0.50), (0.70, 0.50), (0.70, 1.00), (0.00, 1.00)],
        ],
        'C' => &[&[
            (0.95, 0.10),
            (0.50, 0.00),
            (0.05, 0.10),
            (0.00, 0.50),
            (0.05, 0.90),
            (0.50, 1.00),
            (0.95, 0.90),
        ]],
        'D' => &[
            &[(0.00, 0.00), (0.00, 1.00)],
            &[
                (0.00, 0.00),
                (0.60, 0.06),
                (0.85, 0.50),
                (0.60, 0.94),
                (0.00, 1.00),
            ],
        ],
        'E' => &[
            &[(0.92, 0.00), (0.00, 0.00), (0.00, 1.00), (0.92, 1.00)],
            &[(0.00, 0.50), (0.70, 0.50)],
        ],
        'F' => &[
            &[(0.92, 0.00), (0.00, 0.00), (0.00, 1.00)],
            &[(0.00, 0.48), (0.66, 0.48)],
        ],
        'G' => &[
            &[
                (0.95, 0.12),
                (0.50, 0.00),
                (0.06, 0.12),
                (0.00, 0.50),
                (0.06, 0.88),
                (0.50, 1.00),
                (0.95, 0.88),
                (0.95, 0.55),
            ],
            &[(0.55, 0.55), (0.95, 0.55)],
        ],
        'H' => &[
            &[(0.00, 0.00), (0.00, 1.00)],
            &[(1.00, 0.00), (1.00, 1.00)],
            &[(0.00, 0.50), (1.00, 0.50)],
        ],
        'I' => &[&[(0.50, 0.00), (0.50, 1.00)]],
        'J' => &[&[
            (0.85, 0.00),
            (0.85, 0.84),
            (0.58, 1.00),
            (0.16, 0.98),
            (0.00, 0.78),
        ]],
        'K' => &[
            &[(0.00, 0.00), (0.00, 1.00)],
            &[(0.95, 0.00), (0.05, 0.56)],
            &[(0.32, 0.40), (1.00, 1.00)],
        ],
        'L' => &[&[(0.00, 0.00), (0.00, 1.00), (0.90, 1.00)]],
        'M' => &[&[
            (0.00, 1.00),
            (0.00, 0.00),
            (0.50, 0.62),
            (1.00, 0.00),
            (1.00, 1.00),
        ]],
        'N' => &[&[(0.00, 1.00), (0.00, 0.00), (1.00, 1.00), (1.00, 0.00)]],
        'O' => &[&[
            (0.50, 0.00),
            (0.06, 0.12),
            (0.00, 0.50),
            (0.06, 0.88),
            (0.50, 1.00),
            (0.94, 0.88),
            (1.00, 0.50),
            (0.94, 0.12),
            (0.50, 0.00),
        ]],
        'P' => &[&[
            (0.00, 1.00),
            (0.00, 0.00),
            (0.64, 0.00),
            (0.72, 0.26),
            (0.64, 0.50),
            (0.00, 0.50),
        ]],
        'Q' => &[
            &[
                (0.50, 0.00),
                (0.06, 0.12),
                (0.00, 0.50),
                (0.06, 0.88),
                (0.50, 1.00),
                (0.94, 0.88),
                (1.00, 0.50),
                (0.94, 0.12),
                (0.50, 0.00),
            ],
            &[(0.58, 0.72), (1.00, 1.14)],
        ],
        'R' => &[
            &[
                (0.00, 1.00),
                (0.00, 0.00),
                (0.64, 0.00),
                (0.72, 0.26),
                (0.64, 0.50),
                (0.00, 0.50),
            ],
            &[(0.44, 0.50), (0.96, 1.00)],
        ],
        'S' => &[&[
            (0.94, 0.12),
            (0.50, 0.00),
            (0.08, 0.14),
            (0.12, 0.44),
            (0.50, 0.52),
            (0.90, 0.60),
            (0.92, 0.88),
            (0.50, 1.00),
            (0.06, 0.88),
        ]],
        'T' => &[&[(0.00, 0.00), (1.00, 0.00)], &[(0.50, 0.00), (0.50, 1.00)]],
        'U' => &[&[
            (0.00, 0.00),
            (0.00, 0.86),
            (0.50, 1.00),
            (1.00, 0.86),
            (1.00, 0.00),
        ]],
        'V' => &[&[(0.00, 0.00), (0.50, 1.00), (1.00, 0.00)]],
        'W' => &[&[
            (0.00, 0.00),
            (0.22, 1.00),
            (0.50, 0.42),
            (0.78, 1.00),
            (1.00, 0.00),
        ]],
        'X' => &[&[(0.00, 0.00), (1.00, 1.00)], &[(1.00, 0.00), (0.00, 1.00)]],
        'Y' => &[
            &[(0.00, 0.00), (0.50, 0.52), (1.00, 0.00)],
            &[(0.50, 0.52), (0.50, 1.00)],
        ],
        'Z' => &[&[(0.00, 0.00), (1.00, 0.00), (0.00, 1.00), (1.00, 1.00)]],
        '0' => &[
            &[
                (0.50, 0.00),
                (0.06, 0.12),
                (0.00, 0.50),
                (0.06, 0.88),
                (0.50, 1.00),
                (0.94, 0.88),
                (1.00, 0.50),
                (0.94, 0.12),
                (0.50, 0.00),
            ],
            &[(0.16, 0.86), (0.84, 0.14)],
        ],
        '1' => &[
            &[(0.22, 0.18), (0.50, 0.00), (0.50, 1.00)],
            &[(0.20, 1.00), (0.80, 1.00)],
        ],
        '2' => &[&[
            (0.06, 0.16),
            (0.50, 0.00),
            (0.94, 0.16),
            (0.90, 0.42),
            (0.00, 1.00),
            (1.00, 1.00),
        ]],
        '3' => &[
            &[
                (0.06, 0.14),
                (0.50, 0.00),
                (0.90, 0.16),
                (0.86, 0.42),
                (0.50, 0.50),
            ],
            &[
                (0.50, 0.50),
                (0.90, 0.60),
                (0.94, 0.86),
                (0.50, 1.00),
                (0.08, 0.88),
            ],
        ],
        '4' => &[&[(0.72, 1.00), (0.72, 0.00), (0.00, 0.68), (1.00, 0.68)]],
        '5' => &[&[
            (0.90, 0.00),
            (0.16, 0.00),
            (0.08, 0.44),
            (0.50, 0.40),
            (0.92, 0.56),
            (0.90, 0.88),
            (0.50, 1.00),
            (0.08, 0.88),
        ]],
        '6' => &[&[
            (0.88, 0.06),
            (0.50, 0.00),
            (0.10, 0.20),
            (0.04, 0.70),
            (0.50, 1.00),
            (0.92, 0.82),
            (0.86, 0.58),
            (0.40, 0.50),
            (0.06, 0.62),
        ]],
        '7' => &[&[(0.04, 0.00), (0.96, 0.00), (0.42, 1.00)]],
        '8' => &[
            &[
                (0.50, 0.00),
                (0.12, 0.16),
                (0.16, 0.44),
                (0.50, 0.50),
                (0.86, 0.42),
                (0.90, 0.14),
                (0.50, 0.00),
            ],
            &[
                (0.50, 0.50),
                (0.08, 0.62),
                (0.06, 0.90),
                (0.50, 1.00),
                (0.94, 0.90),
                (0.90, 0.60),
                (0.50, 0.50),
            ],
        ],
        '9' => &[&[
            (0.12, 0.94),
            (0.50, 1.00),
            (0.90, 0.80),
            (0.96, 0.30),
            (0.50, 0.00),
            (0.08, 0.18),
            (0.14, 0.42),
            (0.60, 0.50),
            (0.94, 0.38),
        ]],
        '.' => &[&[(0.50, 0.90), (0.50, 1.00)]],
        ',' => &[&[(0.52, 0.86), (0.42, 1.14)]],
        ':' => &[&[(0.50, 0.38), (0.50, 0.48)], &[(0.50, 0.90), (0.50, 1.00)]],
        ';' => &[&[(0.50, 0.38), (0.50, 0.48)], &[(0.52, 0.86), (0.42, 1.14)]],
        '!' => &[&[(0.50, 0.00), (0.50, 0.70)], &[(0.50, 0.90), (0.50, 1.00)]],
        '?' => &[
            &[
                (0.10, 0.18),
                (0.50, 0.00),
                (0.90, 0.18),
                (0.88, 0.42),
                (0.50, 0.60),
                (0.50, 0.70),
            ],
            &[(0.50, 0.90), (0.50, 1.00)],
        ],
        '-' => &[&[(0.14, 0.56), (0.86, 0.56)]],
        '_' => &[&[(0.00, 1.02), (1.00, 1.02)]],
        '\'' => &[&[(0.50, 0.00), (0.50, 0.22)]],
        '"' => &[&[(0.38, 0.00), (0.38, 0.22)], &[(0.66, 0.00), (0.66, 0.22)]],
        '(' => &[&[(0.72, 0.00), (0.32, 0.30), (0.32, 0.70), (0.72, 1.00)]],
        ')' => &[&[(0.28, 0.00), (0.68, 0.30), (0.68, 0.70), (0.28, 1.00)]],
        '+' => &[&[(0.50, 0.22), (0.50, 0.78)], &[(0.14, 0.50), (0.86, 0.50)]],
        '=' => &[&[(0.14, 0.42), (0.86, 0.42)], &[(0.14, 0.64), (0.86, 0.64)]],
        '/' => &[&[(0.90, 0.00), (0.10, 1.00)]],
        '*' => &[
            &[(0.50, 0.08), (0.50, 0.46)],
            &[(0.18, 0.16), (0.82, 0.38)],
            &[(0.82, 0.16), (0.18, 0.38)],
        ],
        '#' => &[
            &[(0.30, 0.00), (0.24, 1.00)],
            &[(0.70, 0.00), (0.64, 1.00)],
            &[(0.06, 0.34), (0.94, 0.34)],
            &[(0.06, 0.66), (0.94, 0.66)],
        ],
        '&' => &[&[
            (0.94, 1.00),
            (0.30, 0.46),
            (0.16, 0.60),
            (0.30, 1.00),
            (0.72, 0.72),
            (0.06, 0.26),
        ]],
        _ => &[&[
            (0.08, 0.00),
            (0.92, 0.00),
            (0.92, 1.00),
            (0.08, 1.00),
            (0.08, 0.00),
        ]],
    }
}

/// Decodes three code values into one linear-light pixel.
fn decode_channels(channels: [u16; 3], maximum: u32, transfer: TransferFunction) -> Pixel {
    let scale = f64::from(maximum);
    let mut out = [0.0_f32; 4];
    for (index, code) in channels.iter().enumerate() {
        let normalised = det::clamp(f64::from(*code) / scale, 0.0, 1.0);
        out[index] = transfer.decode(normalised) as f32;
    }
    out[3] = 1.0;
    out
}

/// A sample coordinate on one axis, clamped and rounded to a pixel index.
fn clamp_axis(value: f32, size: u32) -> u32 {
    let last = i64::from(size) - 1;
    let index = det::clamp((value - 0.5).floor() as f64, 0.0, last as f64);
    index as u32
}

/// A 64-bit hash of a label, so a generated chart is a function of the asset's name.
fn hash_label(label: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in label.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// A cursor over the ASCII header of a PPM, and over its ASCII body when the variant is
/// the plain one.
#[derive(Debug)]
struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    /// The next whitespace-delimited token, skipping comments.
    fn token(&mut self) -> Result<String, SourceError> {
        self.skip_separators()?;
        let start = self.offset;
        while self.offset < self.bytes.len() && !self.bytes[self.offset].is_ascii_whitespace() {
            self.offset += 1;
        }
        if start == self.offset {
            return Err(SourceError::Malformed(
                "a PPM header ended early".to_owned(),
            ));
        }
        let token = std::str::from_utf8(&self.bytes[start..self.offset])
            .map_err(|_| SourceError::Malformed("a PPM header is not ASCII".to_owned()))?;
        Ok(token.to_owned())
    }

    /// The next number, with a range check that a `u32` cannot overflow on.
    fn number(&mut self) -> Result<u32, SourceError> {
        let token = self.token()?;
        token
            .parse::<u32>()
            .map_err(|_| SourceError::Malformed(format!("`{token}` is not a PPM number")))
    }

    /// The next sample value, refused when it exceeds what a PPM channel can hold.
    fn sample(&mut self) -> Result<u16, SourceError> {
        let value = self.number()?;
        u16::try_from(value).map_err(|_| {
            SourceError::Malformed(format!("the sample {value} is beyond sixteen bits"))
        })
    }

    /// Skips whitespace and `#` comments, which the format allows between any two tokens.
    fn skip_separators(&mut self) -> Result<(), SourceError> {
        loop {
            while self.offset < self.bytes.len() && self.bytes[self.offset].is_ascii_whitespace() {
                self.offset += 1;
            }
            if self.offset < self.bytes.len() && self.bytes[self.offset] == b'#' {
                while self.offset < self.bytes.len() && self.bytes[self.offset] != b'\n' {
                    self.offset += 1;
                }
                continue;
            }
            if self.offset >= self.bytes.len() {
                return Err(SourceError::Malformed(
                    "a PPM header ended early".to_owned(),
                ));
            }
            return Ok(());
        }
    }

    /// Consumes exactly one separator byte, which `P6` requires before its binary body.
    fn expect_separator(&mut self) -> Result<(), SourceError> {
        if self.offset >= self.bytes.len() || !self.bytes[self.offset].is_ascii_whitespace() {
            return Err(SourceError::Malformed(
                "a P6 header must be followed by whitespace".to_owned(),
            ));
        }
        self.offset += 1;
        Ok(())
    }

    /// Everything from the cursor onwards.
    fn rest(&self) -> &'a [u8] {
        &self.bytes[self.offset..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A title, painted, as a digest — so two titles can be compared as pictures rather than
    /// as fields.
    fn painted(title: &Title) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for y in 0..title.height() {
            for x in 0..title.width() {
                let pixel = title.sample(x as f32 + 0.5, y as f32 + 0.5);
                // Quantised to eight bits on purpose: the comparison is between pictures a
                // person could see, not between two float sums that differ in the last place.
                for channel in pixel {
                    let code = det::clamp(f64::from(channel) * 255.0, 0.0, 255.0) as u8;
                    hash ^= u64::from(code);
                    hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
        }
        hash
    }

    /// How many samples of a title are more than half covered by a stroke.
    fn inked(title: &Title) -> u32 {
        let mut count = 0;
        for y in 0..title.height() {
            for x in 0..title.width() {
                if title.sample(x as f32 + 0.5, y as f32 + 0.5)[3] > 0.5 {
                    count += 1;
                }
            }
        }
        count
    }

    fn white() -> [f32; 3] {
        [1.0, 1.0, 1.0]
    }

    /// The first row of a title that carries ink, which is where its capitals stand.
    fn top_row(title: &Title) -> u32 {
        (0..title.height())
            .find(|y| {
                (0..title.width()).any(|x| title.sample(x as f32 + 0.5, *y as f32 + 0.5)[3] > 0.5)
            })
            .expect("a title with ink")
    }

    /// The row the ink's centre of mass sits on, which is what "turned half around" moves.
    fn ink_centroid(title: &Title) -> f32 {
        let mut weight = 0.0_f32;
        let mut moment = 0.0_f32;
        for y in 0..title.height() {
            for x in 0..title.width() {
                let alpha = title.sample(x as f32 + 0.5, y as f32 + 0.5)[3];
                if alpha > 0.0 {
                    weight += alpha;
                    moment += alpha * y as f32;
                }
            }
        }
        assert!(weight > 0.0, "a title with no ink has no centre of mass");
        moment / weight
    }

    /// The first column of a title that carries ink, which is where its margin put it.
    fn left_column(title: &Title) -> u32 {
        (0..title.width())
            .find(|x| {
                (0..title.height()).any(|y| title.sample(*x as f32 + 0.5, y as f32 + 0.5)[3] > 0.5)
            })
            .expect("a title with ink")
    }

    #[test]
    fn a_title_sets_type_over_a_frame_and_leaves_the_rest_of_it_alone() {
        let title = Title::new(
            "MAJESTAT",
            TitleFace::Sans,
            24.0,
            white(),
            TitleAlign::Centre,
            320,
            120,
        );
        assert_eq!(title.width(), 320);
        assert_eq!(title.height(), 120);
        assert_eq!(title.text(), "MAJESTAT");
        assert_eq!(title.kind(), "title");

        // The band the line sits in carries ink and the corners of the frame carry none, so
        // a title composes over what is under it instead of covering it.
        assert!(inked(&title) > 200, "a line of eight capitals is not empty");
        assert_eq!(
            title.sample(1.5, 1.5),
            [0.0, 0.0, 0.0, 0.0],
            "the corner is clear"
        );
        assert_eq!(title.sample(318.5, 118.5), [0.0, 0.0, 0.0, 0.0]);

        // Every drawn sample is the title's colour, and its alpha is a fraction only on an
        // edge — that is what makes the type antialiased rather than jagged.
        let mut partial = 0;
        let mut solid = 0;
        for y in 0..120 {
            for x in 0..320 {
                let pixel = title.sample(x as f32 + 0.5, y as f32 + 0.5);
                if pixel[3] > 0.0 {
                    assert_eq!([pixel[0], pixel[1], pixel[2]], white());
                    if pixel[3] >= 1.0 {
                        solid += 1;
                    } else {
                        partial += 1;
                    }
                }
            }
        }
        assert!(solid > 100, "a stroke has an inside");
        assert!(
            partial > 10,
            "and an edge, which is what the half pixel is for"
        );
    }

    #[test]
    fn the_four_faces_are_four_pictures_of_the_same_words() {
        let paint = |face| {
            painted(&Title::new(
                "TITLE",
                face,
                40.0,
                white(),
                TitleAlign::Centre,
                240,
                120,
            ))
        };
        let pictures: Vec<u64> = TitleFace::ALL.into_iter().map(paint).collect();
        for (index, first) in pictures.iter().enumerate() {
            for second in pictures.iter().skip(index + 1) {
                assert_ne!(first, second, "two faces drew the same picture");
            }
        }
        // And a face is a function of its name, in either case, because that is what a URI
        // carries.
        assert_eq!(TitleFace::from_key("Serif"), Some(TitleFace::Serif));
        assert_eq!(TitleFace::from_key("MONO"), Some(TitleFace::Mono));
        assert_eq!(TitleFace::from_key("comic"), None);
    }

    #[test]
    fn a_title_comes_from_a_uri_and_an_empty_field_takes_its_default() {
        let resolver = WorkspaceResolver::new();
        let set = resolver
            .resolve("title:serif:40:ff0000:left:HELLO", (240, 100), 0)
            .expect("a title");
        assert_eq!(set.kind(), "title");
        assert_eq!(set.width(), 240);

        let title = Title::from_label("title:serif:40:ff0000:left:HELLO", 240, 100);
        assert_eq!(title.text(), "HELLO");
        assert_eq!(title.face(), TitleFace::Serif);
        assert_eq!(title.size(), 40.0);
        assert_eq!(title.align(), TitleAlign::Left);
        // Red is decoded from the sRGB code value, so the sample is one in red and zero in
        // the two channels the person did not name.
        let ink = (0..240)
            .flat_map(|x| (0..100).map(move |y| (x, y)))
            .map(|(x, y)| title.sample(f64::from(x) as f32 + 0.5, f64::from(y) as f32 + 0.5))
            .find(|pixel| pixel[3] >= 1.0)
            .expect("a drawn sample");
        assert_eq!(ink, [1.0, 0.0, 0.0, 1.0]);

        // Every field but the text may be empty, and the text is the rest of the URI — a
        // title with a colon in it is not a malformed title.
        let default = Title::from_label("title:::::one: two", 200, 100);
        assert_eq!(default.text(), "one: two");
        assert_eq!(default.face(), TitleFace::Sans);
        assert_eq!(default.size(), 10.0, "a tenth of the frame's height");
        assert_eq!(default.align(), TitleAlign::Centre);

        let nonsense = Title::from_label("title:nonsense:not a number::::", 200, 100);
        assert_eq!(nonsense.face(), TitleFace::Sans);
        assert_eq!(nonsense.size(), 10.0);
    }

    #[test]
    fn lowercase_is_a_small_capital_and_an_accent_is_a_mark() {
        let upper = Title::new(
            "ANO",
            TitleFace::Sans,
            40.0,
            white(),
            TitleAlign::Left,
            200,
            100,
        );
        let lower = Title::new(
            "ano",
            TitleFace::Sans,
            40.0,
            white(),
            TitleAlign::Left,
            200,
            100,
        );
        // The same three letters standing shorter on the same baseline: less ink, and its top
        // lower down, because the baseline is where both of them sit.
        assert!(inked(&lower) < inked(&upper));
        assert!(
            top_row(&lower) > top_row(&upper),
            "small capitals are shorter"
        );

        // A mark sits above the cap line, so it is the topmost ink in the frame and there is
        // more of it than the bare letter had.
        let tilde = Title::new(
            "AÑO",
            TitleFace::Sans,
            40.0,
            white(),
            TitleAlign::Left,
            200,
            100,
        );
        assert!(
            top_row(&tilde) < top_row(&upper),
            "the tilde is above the capitals"
        );
        assert!(
            top_row(&tilde) <= top_row(&upper) - 2,
            "and it is drawn, not merely claimed"
        );

        // The inverted marks are the upright ones turned half around, which puts their ink
        // low in the glyph rather than high.
        let upright = Title::new(
            "?",
            TitleFace::Sans,
            40.0,
            white(),
            TitleAlign::Left,
            80,
            100,
        );
        let inverted = Title::new(
            "¿",
            TitleFace::Sans,
            40.0,
            white(),
            TitleAlign::Left,
            80,
            100,
        );
        assert_ne!(painted(&upright), painted(&inverted));
        assert!(
            ink_centroid(&inverted) > ink_centroid(&upright),
            "an inverted mark carries its weight low, with its dot at the top"
        );
    }

    #[test]
    fn a_character_the_alphabet_does_not_have_is_drawn_as_a_missing_box() {
        let known = Title::new(
            "A",
            TitleFace::Sans,
            40.0,
            white(),
            TitleAlign::Left,
            80,
            100,
        );
        let unknown = Title::new(
            "€",
            TitleFace::Sans,
            40.0,
            white(),
            TitleAlign::Left,
            80,
            100,
        );
        assert!(
            inked(&unknown) > 50,
            "a missing glyph is a box, not a blank"
        );
        assert_ne!(painted(&known), painted(&unknown));
    }

    #[test]
    fn a_title_is_the_same_picture_twice_and_moves_with_its_alignment() {
        let build = |align| Title::new("MAJ", TitleFace::Sans, 40.0, white(), align, 240, 100);
        assert_eq!(
            painted(&build(TitleAlign::Left)),
            painted(&build(TitleAlign::Left))
        );
        assert_ne!(
            painted(&build(TitleAlign::Left)),
            painted(&build(TitleAlign::Centre))
        );
        assert_ne!(
            painted(&build(TitleAlign::Right)),
            painted(&build(TitleAlign::Centre))
        );
        // The left-aligned line starts at the margin and the right-aligned one ends there,
        // with the same glyphs in both and the same amount of ink between them.
        let left = build(TitleAlign::Left);
        let right = build(TitleAlign::Right);
        assert!(left_column(&left) < left_column(&right));
        // The same glyphs, placed differently: the ink is the same to within the sub-pixel
        // where each line starts, which is a rasterisation detail and not a different line.
        let (aligned, placed) = (inked(&left) as i64, inked(&right) as i64);
        assert!((aligned - placed).abs() * 20 < aligned.max(placed));
    }

    #[test]
    fn a_generated_chart_is_a_function_of_its_seed_and_size() {
        let chart = Generated::new(64, 32, 7);
        assert_eq!(chart.width(), 64);
        assert_eq!(chart.height(), 32);
        assert_eq!(chart.seed(), 7);
        assert_eq!(chart.sample(1.5, 1.5), chart.sample(2.5, 2.5));

        // The same name gives the same chart, and different names differ.
        let first = Generated::from_label("a", 32, 16);
        let again = Generated::from_label("a", 32, 16);
        let other = Generated::from_label("b", 32, 16);
        assert_eq!(first, again);
        assert_ne!(first.seed(), other.seed());

        // Out of range sampling clamps instead of reading past the end.
        assert_eq!(chart.sample(-100.0, -100.0), chart.sample(0.5, 0.5));
        assert_eq!(chart.sample(1_000.0, 1_000.0), chart.sample(63.5, 31.5));
    }

    #[test]
    fn the_chart_has_bars_a_ramp_and_a_pattern() {
        let chart = Generated::new(80, 40, 1);
        // The top quarter is the eight colour bars, so the second one is yellow: two
        // channels up and the third at zero, which is what "saturated" means here.
        let yellow = chart.sample(15.5, 1.5);
        assert!(
            yellow[0] > 0.5 && yellow[1] > 0.5 && yellow[2] < 0.05,
            "{yellow:?}"
        );
        // Rows 10 to 20: a monotonic ramp.
        let dark = chart.sample(1.5, 12.0);
        let bright = chart.sample(78.5, 12.0);
        assert!(dark[0] < bright[0], "{dark:?} {bright:?}");
        assert!(dark[0] > 0.0 && bright[0] < 1.0);
        // The rest is a two-level checker, so it holds exactly two values per channel.
        let first = chart.sample(1.5, 30.5);
        let second = chart.sample(20.5, 30.5);
        assert_ne!(first, second);
        assert_eq!(first[3], 1.0);
    }

    #[test]
    fn a_binary_ppm_round_trips_through_its_code_values() {
        // Two pixels: black and white, at eight bits.
        let bytes = b"P6\n2 1\n255\n\x00\x00\x00\xff\xff\xff";
        let ppm = Ppm::from_bytes(bytes, TransferFunction::Linear).expect("a valid P6");
        assert_eq!(ppm.width(), 2);
        assert_eq!(ppm.height(), 1);
        assert_eq!(ppm.sample(0.5, 0.5), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(ppm.sample(1.5, 0.5), [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn a_sixteen_bit_ppm_reads_big_endian_samples() {
        // Two pixels at sixteen bits each: black, then white.
        let bytes = b"P6\n2 1\n65535\n\x00\x00\x00\x00\x00\x00\xff\xff\xff\xff\xff\xff";
        let ppm = Ppm::from_bytes(bytes, TransferFunction::Linear).expect("a valid 16-bit P6");
        assert_eq!(ppm.sample(0.5, 0.5), [0.0, 0.0, 0.0, 1.0]);
        let white = ppm.sample(1.5, 0.5);
        assert!((white[0] - 1.0).abs() < 1.0e-6, "{white:?}");
    }

    #[test]
    fn an_ascii_ppm_reads_with_comments_and_odd_spacing() {
        let bytes = b"P3\n# written by a test\n  2   1 \n255\n255 0 0\n0 255 0\n";
        let ppm = Ppm::from_bytes(bytes, TransferFunction::Linear).expect("a valid P3");
        assert_eq!(ppm.sample(0.5, 0.5), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(ppm.sample(1.5, 0.5), [0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn a_ppm_decodes_through_the_declared_transfer_function() {
        // Code 128 in the sRGB curve is 0.2158605001 in scene-linear light, and the point is
        // that it is nothing like the 0.5019607 of the code value itself.
        let bytes = b"P6\n1 1\n255\n\x80\x80\x80";
        let srgb = Ppm::from_bytes(bytes, TransferFunction::Srgb).expect("a valid P6");
        let linear = srgb.sample(0.5, 0.5)[0];
        assert!(
            (f64::from(linear) - 0.215_860_500_113_899_23).abs() < 1.0e-6,
            "{linear}"
        );
        let plain = Ppm::from_bytes(bytes, TransferFunction::Linear).expect("a valid P6");
        let untouched = plain.sample(0.5, 0.5)[0];
        assert!((f64::from(untouched) - 128.0 / 255.0).abs() < 1.0e-6);
        assert!(
            linear < untouched * 0.5,
            "the curve is doing the work, not the loader"
        );
    }

    #[test]
    fn a_malformed_ppm_is_refused_with_a_reason() {
        let short = Ppm::from_bytes(b"P6\n2 2\n255\n\x01\x02\x03", TransferFunction::Linear);
        assert!(matches!(short, Err(SourceError::Malformed(_))), "{short:?}");
        let unknown = Ppm::from_bytes(b"P7\n1 1\n255\n", TransferFunction::Linear);
        assert!(
            matches!(unknown, Err(SourceError::Unsupported(_))),
            "{unknown:?}"
        );
        let empty = Ppm::from_bytes(b"", TransferFunction::Linear);
        assert!(empty.is_err());
        let zero = Ppm::from_bytes(b"P6\n0 1\n255\n", TransferFunction::Linear);
        assert!(matches!(zero, Err(SourceError::Malformed(_))), "{zero:?}");
    }

    #[test]
    fn the_resolver_draws_what_a_uri_asks_for_and_labels_what_it_cannot_read() {
        let resolver = WorkspaceResolver::new();
        assert_eq!(resolver.missing_media(), MissingMedia::Placeholder);

        // The three forms that are built from the URI itself are pictures, not substitutes.
        let generated = resolver
            .resolve("generated:bars", (32, 16), 0)
            .expect("a chart");
        assert_eq!(generated.kind(), "generated");
        assert_eq!(generated.width(), 32);

        let solid = resolver
            .resolve("solid:255,0,0", (8, 8), 0)
            .expect("a flat field");
        assert_eq!(solid.kind(), "solid");
        assert_eq!(solid.sample(4.5, 4.5), [1.0, 0.0, 0.0, 1.0]);

        // And the two ways media can be unavailable are one condition with one policy, told
        // apart by what they say rather than by whether they render. Neither is `generated`:
        // a chart a project asked for and a stand-in for media it did not get are different
        // things, and a manifest that called them both the same word could not be believed.
        let undecodable = resolver
            .resolve("camera://card/A001C001.mxf", (64, 64), 0)
            .expect("a placeholder, because this build has no decoder for it");
        assert_eq!(undecodable.kind(), Placeholder::UNDECODABLE);

        let offline = resolver
            .resolve("clip.ppm", (64, 64), 0)
            .expect("a placeholder, because the file is not there");
        assert_eq!(offline.kind(), Placeholder::OFFLINE);

        for substitute in [&undecodable, &offline] {
            assert!(Placeholder::is_placeholder(substitute.kind()));
            assert_ne!(substitute.kind(), "generated");
            // Opaque, and something is on it: the frame reports a fact rather than going
            // black, which is the difference between an answer and a failure nobody saw.
            let pixel = substitute.sample(32.5, 32.5);
            assert_eq!(pixel[3], 1.0);
            assert!(pixel[0] > 0.0);
        }
    }

    #[test]
    fn a_delivery_policy_refuses_what_an_editor_would_stand_in_for() {
        // A placeholder in a master is worse than a failed render, because a failed render
        // is noticed. So the same resolver takes the other answer on request, and it takes
        // it for every way media can be unavailable rather than for some of them.
        let strict = WorkspaceResolver::new().with_missing_media(MissingMedia::Refuse);
        assert_eq!(strict.missing_media(), MissingMedia::Refuse);
        assert_eq!(MissingMedia::Refuse.key(), "refuse");

        assert!(matches!(
            strict.resolve("clip.ppm", (8, 8), 0),
            Err(SourceError::Io(_))
        ));
        assert!(matches!(
            strict.resolve("missing.png", (8, 8), 0),
            Err(SourceError::Io(_))
        ));
        assert!(matches!(
            strict.resolve("camera://card/A001C001.mxf", (8, 8), 0),
            Err(SourceError::Unsupported(_))
        ));

        // What it can read, it still reads.
        assert_eq!(
            strict
                .resolve("solid:0,0,255", (8, 8), 0)
                .expect("a flat field")
                .kind(),
            "solid"
        );
    }

    #[test]
    fn the_monospaced_face_gives_every_glyph_room_for_the_widest_one() {
        // Found by rendering a title and looking at it, not by a failing assertion: the
        // monospaced face gave every glyph an advance of 0.80, which is narrower than `D`,
        // `O` and `R` actually draw, so "NO DECODER" came out with its wide letters printed
        // over one another. The alphabet was right and the layout was right; nothing
        // compared a glyph's ink to the room it had been given, so a face could be
        // unreadable and still pass every test there was.
        //
        // The invariant is stated for the monospaced face only, because that is where it is
        // crisp: one advance for every glyph means that advance has to hold the widest
        // glyph in the alphabet. A proportional face measures the glyph it is about to draw,
        // and a serifed one deliberately overhangs its advance by half a stroke at each end,
        // so neither can be held to this.
        let title = Title::new(
            "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.,:-?!¿¡ÁÉÍÓÚÜÑÇ",
            TitleFace::Mono,
            40.0,
            [1.0, 1.0, 1.0],
            TitleAlign::Left,
            12_000,
            200,
        );
        for placed in &title.placed {
            for (x0, _, x1, _) in &placed.segments {
                for x in [*x0, *x1] {
                    assert!(
                        x >= placed.left && x <= placed.right,
                        "a monospaced glyph draws a stroke at {x}, outside the advance \
                         {}..{} every glyph gets, so it lands on its neighbour",
                        placed.left,
                        placed.right
                    );
                }
            }
        }

        // And it is still monospaced: one advance, the same for every glyph.
        let advances: Vec<f32> = title
            .placed
            .iter()
            .map(|placed| placed.right - placed.left)
            .collect();
        let first = advances[0];
        for advance in &advances {
            assert!(
                (advance - first).abs() < 1e-3,
                "a monospaced face must give every glyph the same advance: {advance} vs {first}"
            );
        }
    }

    #[test]
    fn a_placeholder_keeps_the_engines_own_sentence_as_well_as_a_label() {
        // "Offline" is what a person needs; the engine's own sentence is what a bug report
        // needs. Both, because a placeholder that only says "offline" has thrown away the
        // one thing that says which file and why.
        let error = SourceError::Io("file not found: shot-01.mov".to_owned());
        let placeholder = Placeholder::for_error(&error, 32, 32);
        assert_eq!(placeholder.kind(), Placeholder::OFFLINE);
        assert!(placeholder.reason().contains("shot-01.mov"));

        for (error, label) in [
            (SourceError::Io(String::new()), Placeholder::OFFLINE),
            (SourceError::Malformed(String::new()), Placeholder::DAMAGED),
            (
                SourceError::Unsupported(String::new()),
                Placeholder::UNDECODABLE,
            ),
        ] {
            assert_eq!(Placeholder::for_error(&error, 16, 16).kind(), label);
        }
    }

    #[test]
    fn a_generated_chart_moves_with_its_source_frame_and_a_solid_does_not() {
        let resolver = WorkspaceResolver::new();
        let first = resolver
            .resolve("generated:bars", (48, 32), 0)
            .expect("a chart");
        let later = resolver
            .resolve("generated:bars", (48, 32), 3)
            .expect("a chart");
        // The chart slides a pixel per frame, so the smooth ramp is read at a different
        // place and the two pictures differ.
        assert_ne!(first.sample(5.5, 12.5), later.sample(5.5, 12.5));
        // A flat field is a flat field at every frame.
        let solid = resolver
            .resolve("solid:10,20,30", (8, 8), 5)
            .expect("a flat field");
        assert_eq!(
            solid.sample(0.5, 0.5),
            [10.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0, 1.0]
        );
    }

    /// A PNG chunk: length, kind, body and the checksum the decoder verifies.
    fn chunk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        let mut checked = kind.to_vec();
        checked.extend_from_slice(body);
        out.extend_from_slice(&crc32(&checked).to_be_bytes());
        out
    }

    /// A zlib stream that stores its input, so a test image needs no compressor.
    fn stored_zlib(data: &[u8]) -> Vec<u8> {
        let mut out = vec![0x78, 0x01];
        for (index, block) in data.chunks(65_535).enumerate() {
            let last = (index + 1) * 65_535 >= data.len();
            out.push(u8::from(last));
            let length = block.len() as u16;
            out.extend_from_slice(&length.to_le_bytes());
            out.extend_from_slice(&(!length).to_le_bytes());
            out.extend_from_slice(block);
        }
        out.extend_from_slice(&adler32(data).to_be_bytes());
        out
    }

    /// The checksum a zlib stream ends with.
    fn adler32(data: &[u8]) -> u32 {
        let (mut low, mut high) = (1_u32, 0_u32);
        for byte in data {
            low = (low + u32::from(*byte)) % 65_521;
            high = (high + low) % 65_521;
        }
        high << 16 | low
    }

    /// A PNG assembled out of whole chunks, with the image data stored, not compressed.
    fn png(
        width: u32,
        height: u32,
        depth: u8,
        colour: u8,
        extras: &[Vec<u8>],
        rows: &[u8],
    ) -> Vec<u8> {
        let mut header = Vec::new();
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        header.extend_from_slice(&[depth, colour, 0, 0, 0]);
        let mut out = PNG_SIGNATURE.to_vec();
        out.extend_from_slice(&chunk(b"IHDR", &header));
        for extra in extras {
            out.extend_from_slice(extra);
        }
        out.extend_from_slice(&chunk(b"IDAT", &stored_zlib(rows)));
        out.extend_from_slice(&chunk(b"IEND", &[]));
        out
    }

    #[test]
    fn a_truecolour_png_decodes_its_code_values() {
        // Two pixels: black, then white, each row behind its own filter byte.
        let bytes = png(2, 1, 8, 2, &[], &[0, 0, 0, 0, 255, 255, 255]);
        let image = Png::from_bytes(&bytes, TransferFunction::Linear).expect("a valid PNG");
        assert_eq!(image.width(), 2);
        assert_eq!(image.height(), 1);
        assert_eq!(image.kind(), "png");
        assert_eq!(image.sample(0.5, 0.5), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(image.sample(1.5, 0.5), [1.0, 1.0, 1.0, 1.0]);
        // Out of range sampling clamps, like every other source.
        assert_eq!(image.sample(-20.0, -20.0), image.sample(0.5, 0.5));
    }

    #[test]
    fn an_alpha_channel_survives_the_decode() {
        // Opaque red, then half-transparent blue.
        let bytes = png(2, 1, 8, 6, &[], &[0, 255, 0, 0, 255, 0, 0, 255, 128]);
        let image = Png::from_bytes(&bytes, TransferFunction::Linear).expect("a valid RGBA PNG");
        assert_eq!(image.sample(0.5, 0.5)[3], 1.0);
        let half = image.sample(1.5, 0.5);
        assert_eq!(&half[..3], &[0.0, 0.0, 1.0]);
        assert!(
            (f64::from(half[3]) - 128.0 / 255.0).abs() < 1.0e-6,
            "{half:?}"
        );
    }

    #[test]
    fn an_indexed_png_reads_through_its_palette_and_transparency() {
        let palette = chunk(b"PLTE", &[255, 0, 0, 0, 0, 255]);
        let transparency = chunk(b"tRNS", &[0, 200]);
        let bytes = png(2, 1, 8, 3, &[palette, transparency], &[0, 0, 1]);
        let image = Png::from_bytes(&bytes, TransferFunction::Linear).expect("a valid palette");
        let first = image.sample(0.5, 0.5);
        assert_eq!(&first[..3], &[1.0, 0.0, 0.0]);
        assert_eq!(first[3], 0.0, "the first entry is declared transparent");
        let second = image.sample(1.5, 0.5);
        assert_eq!(&second[..3], &[0.0, 0.0, 1.0]);
        assert!((f64::from(second[3]) - 200.0 / 255.0).abs() < 1.0e-6);
    }

    #[test]
    fn a_one_bit_greyscale_png_unpacks_its_samples() {
        // Four pixels packed into one byte: white, black, white, black.
        let bytes = png(4, 1, 1, 0, &[], &[0, 0b1010_0000]);
        let image = Png::from_bytes(&bytes, TransferFunction::Linear).expect("a valid 1-bit grey");
        assert_eq!(image.sample(0.5, 0.5), [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(image.sample(1.5, 0.5), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(image.sample(2.5, 0.5), [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(image.sample(3.5, 0.5), [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn a_png_reads_through_the_transfer_function_it_is_given() {
        let bytes = png(1, 1, 8, 2, &[], &[0, 128, 128, 128]);
        let srgb = Png::from_bytes(&bytes, TransferFunction::Srgb).expect("a valid PNG");
        let linear = srgb.sample(0.5, 0.5)[0];
        assert!(
            (f64::from(linear) - 0.215_860_500_113_899_23).abs() < 1.0e-6,
            "{linear}"
        );
        let plain = Png::from_bytes(&bytes, TransferFunction::Linear).expect("a valid PNG");
        assert!((f64::from(plain.sample(0.5, 0.5)[0]) - 128.0 / 255.0).abs() < 1.0e-6);
    }

    #[test]
    fn a_malformed_png_is_refused_with_a_reason() {
        let bytes = png(2, 1, 8, 2, &[], &[0, 0, 0, 0, 255, 255, 255]);
        let truncated = Png::from_bytes(&bytes[..20], TransferFunction::Linear);
        assert!(
            matches!(truncated, Err(SourceError::Malformed(_))),
            "{truncated:?}"
        );

        // The same file with one header byte flipped. The signature (8) and the chunk header
        // (8) come first, so byte 24 is inside the header's own body: the checksum catches it.
        let mut damaged = bytes.clone();
        damaged[16 + 8] ^= 0xFF;
        let checked = Png::from_bytes(&damaged, TransferFunction::Linear);
        assert!(
            matches!(checked, Err(SourceError::Malformed(_))),
            "{checked:?}"
        );

        let not_a_png = Png::from_bytes(b"P6\n1 1\n255\n", TransferFunction::Linear);
        assert!(
            matches!(not_a_png, Err(SourceError::Malformed(_))),
            "{not_a_png:?}"
        );

        // Interlacing and the colour types this build does not read are named, not guessed.
        // The header is written here rather than by the helper, because the interlace flag is
        // the one field it does not take as a parameter.
        let mut header = Vec::new();
        header.extend_from_slice(&2_u32.to_be_bytes());
        header.extend_from_slice(&1_u32.to_be_bytes());
        header.extend_from_slice(&[8, 2, 0, 0, 1]);
        let mut interlace = PNG_SIGNATURE.to_vec();
        interlace.extend_from_slice(&chunk(b"IHDR", &header));
        interlace.extend_from_slice(&chunk(b"IDAT", &stored_zlib(&[0, 1, 2, 3, 4, 5, 6])));
        interlace.extend_from_slice(&chunk(b"IEND", &[]));
        let interlaced = Png::from_bytes(&interlace, TransferFunction::Linear);
        assert!(
            matches!(interlaced, Err(SourceError::Unsupported(_))),
            "{interlaced:?}"
        );
        let colour_type = png(2, 1, 8, 5, &[], &[0, 0, 0, 0, 0, 0, 0]);
        let unknown = Png::from_bytes(&colour_type, TransferFunction::Linear);
        assert!(
            matches!(unknown, Err(SourceError::Unsupported(_))),
            "{unknown:?}"
        );
        let depth = png(2, 1, 4, 2, &[], &[0, 0, 0, 0, 0]);
        let packed = Png::from_bytes(&depth, TransferFunction::Linear);
        assert!(
            matches!(packed, Err(SourceError::Unsupported(_))),
            "{packed:?}"
        );
    }

    #[test]
    fn the_projects_own_logo_decodes() {
        // A real file, written by a real encoder: dynamic Huffman codes, all five filters, a
        // palette-free truecolour body. It is the artwork this repository ships, so if the
        // decoder cannot read it, nothing it says about PNG matters.
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/brand/logo.png");
        let logo = Png::from_path(path, TransferFunction::Srgb).expect("the project's logo");
        assert_eq!(logo.width(), 1024);
        assert_eq!(logo.height(), 559);
        // The plate is white and the mark is ink, which is the whole of the artwork. Scanning a
        // grid rather than naming a pixel is deliberate: the mark is antialiased, so any one
        // coordinate near an edge is a mixture, and a test that named one would be testing
        // where the crown happens to be drawn rather than whether the picture decoded.
        let (mut darkest, mut brightest) = (f32::MAX, f32::MIN);
        for y in (0..logo.height()).step_by(16) {
            for x in (0..logo.width()).step_by(16) {
                let value = logo.sample(x as f32 + 0.5, y as f32 + 0.5)[0];
                darkest = darkest.min(value);
                brightest = brightest.max(value);
            }
        }
        assert!(darkest < 0.05, "no ink was decoded: {darkest}");
        assert!(brightest > 0.9, "no plate was decoded: {brightest}");
    }

    #[test]
    fn the_resolver_reads_a_png_and_says_so_in_the_manifest() {
        let bytes = png(
            3,
            2,
            8,
            2,
            &[],
            &[
                0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 0, 10, 20, 30, 40, 50, 60, 70, 80, 90,
            ],
        );
        let path = std::env::temp_dir().join("maj-resolver-still.png");
        std::fs::write(&path, &bytes).expect("a writable temporary directory");
        let resolver = WorkspaceResolver::new();
        let resolved = resolver
            .resolve(&path.to_string_lossy(), (16, 16), 7)
            .expect("a PNG on disk");
        assert_eq!(resolved.kind(), "png");
        assert_eq!((resolved.width(), resolved.height()), (3, 2));
        let _ = std::fs::remove_file(&path);

        // A file that is named .png but is not one is never a picture: the extension is a
        // claim and the signature is the fact. Which of the two answers it gets is the
        // resolver's policy, and both of them say "damaged" rather than inventing pixels.
        let liar = std::env::temp_dir().join("maj-resolver-liar.png");
        std::fs::write(&liar, b"not a picture").expect("a writable temporary directory");
        let stood_in = resolver
            .resolve(&liar.to_string_lossy(), (16, 16), 0)
            .expect("an editor carries on");
        assert_eq!(stood_in.kind(), Placeholder::DAMAGED);
        let refused = WorkspaceResolver::new()
            .with_missing_media(MissingMedia::Refuse)
            .resolve(&liar.to_string_lossy(), (16, 16), 0);
        assert!(
            matches!(refused, Err(SourceError::Malformed(_))),
            "a delivery does not: {refused:?}"
        );
        let _ = std::fs::remove_file(&liar);
    }

    #[test]
    fn the_errors_explain_themselves() {
        assert_eq!(
            SourceError::Unsupported("PFM".to_owned()).to_string(),
            "unsupported source: PFM"
        );
        assert_eq!(
            SourceError::Malformed("no size".to_owned()).to_string(),
            "malformed source: no size"
        );
        assert_eq!(
            SourceError::Io("gone".to_owned()).to_string(),
            "source could not be read: gone"
        );
    }
}
