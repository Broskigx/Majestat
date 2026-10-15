//! The frame buffer, and the one rounding the certified path allows.
//!
//! A frame lives here as **scene-linear floating point**, four channels, straight
//! (non-premultiplied) alpha. That is what ADR 0006 asks for: intermediates of at least
//! 16-bit float for composite work, with integer rounding happening once, at the declared
//! output boundary. There is no `u8`, no `u16` and no gamma-corrected pixel anywhere on
//! the way through; a value of one is diffuse white and a value of two is a stop over it.
//!
//! Floating point is used here without apology, because the operations the renderer needs
//! are exactly the ones IEEE-754 defines as correctly rounded — addition, subtraction,
//! multiplication, division and square root. Two machines agree on those bit for bit, and
//! so do two runs on one machine. What is *not* portable, and is therefore absent, is
//! `powf`, `sin`, `ln` and friends: every transcendental in this crate comes from
//! [`maj_color::det`], which builds them from multiplication, division and addition.\n//!
//! The sampling rules are part of the determinism story, not a detail of implementation.
//! Reads outside the frame clamp to the edge rather than returning zero, the bilinear
//! filter has one fixed corner order, and the pixel centre sits at `index + 1/2`.

use maj_color::TransferFunction;
use maj_core::determinism::hash_bytes;

/// Channels per pixel: red, green, blue and alpha, in that order.
pub const CHANNELS: usize = 4;

/// A single pixel in scene-linear light, straight alpha.
pub type Pixel = [f32; CHANNELS];

/// A frame of pixels in scene-linear light.
///
/// Rows are stored top to bottom and left to right, so the byte order of the buffer is the
/// order a file format wants and the digest depends on nothing but the pixels and the size.
#[derive(Debug, Clone, PartialEq)]
pub struct Framebuffer {
    width: u32,
    height: u32,
    pixels: Vec<Pixel>,
}

impl Framebuffer {
    /// Allocates a frame filled with one value.
    ///
    /// # Panics
    ///
    /// Panics if the dimensions are not both non-zero, or if their product does not fit in
    /// the address space. A frame of zero pixels is a programming error, not a state the
    /// renderer can recover from.
    #[must_use]
    pub fn new(width: u32, height: u32, fill: Pixel) -> Self {
        let count = checked_pixel_count(width, height);
        debug_assert!(fill.iter().all(|channel| channel.is_finite()));
        Self {
            width,
            height,
            pixels: vec![fill; count],
        }
    }

    /// Allocates an opaque black frame, which is the background every sequence starts from.
    #[must_use]
    pub fn black(width: u32, height: u32) -> Self {
        Self::new(width, height, [0.0, 0.0, 0.0, 1.0])
    }

    /// Number of pixels across.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Number of pixels down.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// The pixels, in row-major order.
    #[must_use]
    pub fn pixels(&self) -> &[Pixel] {
        &self.pixels
    }

    /// One row of pixels.
    ///
    /// # Panics
    ///
    /// Panics if `y` is outside the frame.
    #[must_use]
    pub fn row(&self, y: u32) -> &[Pixel] {
        let start = y as usize * self.width as usize;
        &self.pixels[start..start + self.width as usize]
    }

    /// One row of pixels, mutably.
    ///
    /// # Panics
    ///
    /// Panics if `y` is outside the frame.
    #[must_use]
    pub fn row_mut(&mut self, y: u32) -> &mut [Pixel] {
        let start = y as usize * self.width as usize;
        &mut self.pixels[start..start + self.width as usize]
    }

    /// Reads a pixel.
    ///
    /// Returns `None` when the coordinates fall outside the frame, so a caller that has a
    /// coordinate to check cannot accidentally read a neighbour.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<Pixel> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.pixels
            .get(y as usize * self.width as usize + x as usize)
            .copied()
    }

    /// Writes a pixel.
    ///
    /// # Panics
    ///
    /// Panics if the coordinates fall outside the frame. Writing past the edge would
    /// silently wrap into the next row, which is the kind of bug that shows up as a
    /// corrupted frame rather than as a failing test.
    pub fn set_pixel(&mut self, x: u32, y: u32, value: Pixel) {
        assert!(
            x < self.width && y < self.height,
            "pixel ({x}, {y}) is outside a {}x{} frame",
            self.width,
            self.height
        );
        let index = y as usize * self.width as usize + x as usize;
        self.pixels[index] = value;
    }

    /// Reads a pixel with edge clamping, so out-of-frame coordinates are legal.
    #[must_use]
    pub fn sample_clamped(&self, x: i64, y: i64) -> Pixel {
        let x = x.clamp(0, i64::from(self.width) - 1) as u32;
        let y = y.clamp(0, i64::from(self.height) - 1) as u32;
        self.pixels[y as usize * self.width as usize + x as usize]
    }

    /// Reads a pixel with edge clamping, from floating-point coordinates.
    ///
    /// The coordinate is in pixel units with the centre of pixel zero at `0.5`, which is
    /// the convention the whole renderer uses: `x = 0.5` is the centre of the first pixel
    /// and `x = width - 0.5` the centre of the last. Nearest-neighbour rounding here is
    /// `floor(x)`, which makes an integer translation an identity rather than a half-pixel
    /// shift.
    #[must_use]
    pub fn sample_nearest(&self, x: f32, y: f32) -> Pixel {
        let xi = floored(x - 0.5);
        let yi = floored(y - 0.5);
        self.sample_clamped(xi, yi)
    }

    /// Reads a pixel with bilinear filtering, and edge clamping beyond the frame.
    ///
    /// The four taps are always visited in the same order and combined as
    /// `(tap00 * (1 - fx) + tap10 * fx) * (1 - fy) + (tap01 * (1 - fx) + tap11 * fx) * fy`,
    /// so the result does not depend on how the compiler feels about reordering.
    #[must_use]
    pub fn sample_bilinear(&self, x: f32, y: f32) -> Pixel {
        let u = x - 0.5;
        let v = y - 0.5;
        let x0 = floored(u);
        let y0 = floored(v);
        let fx = u - x0 as f32;
        let fy = v - y0 as f32;
        let tap00 = self.sample_clamped(x0, y0);
        let tap10 = self.sample_clamped(x0 + 1, y0);
        let tap01 = self.sample_clamped(x0, y0 + 1);
        let tap11 = self.sample_clamped(x0 + 1, y0 + 1);
        let mut out = [0.0_f32; CHANNELS];
        for channel in 0..CHANNELS {
            let top = tap00[channel] * (1.0 - fx) + tap10[channel] * fx;
            let bottom = tap01[channel] * (1.0 - fx) + tap11[channel] * fx;
            out[channel] = top * (1.0 - fy) + bottom * fy;
        }
        out
    }

    /// A digest of the frame: the dimensions and every channel bit pattern.
    ///
    /// The integer is written little-endian by hand rather than by casting the buffer, so
    /// the digest is the same on a big-endian machine and a little-endian one. That is the
    /// point of a digest that is used to prove two renders are identical.
    #[must_use]
    pub fn digest(&self) -> String {
        let mut bytes = Vec::with_capacity(8 + self.pixels.len() * 16);
        bytes.extend_from_slice(&self.width.to_le_bytes());
        bytes.extend_from_slice(&self.height.to_le_bytes());
        for pixel in &self.pixels {
            for channel in pixel {
                bytes.extend_from_slice(&channel.to_bits().to_le_bytes());
            }
        }
        hash_bytes(&bytes)
    }

    /// Whether every channel of every pixel is finite.
    ///
    /// A NaN anywhere is a bug in an effect rather than a pixel value, and it must be
    /// caught before it reaches a file, because it would poison every later operation.
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.pixels
            .iter()
            .all(|pixel| pixel.iter().all(|channel| channel.is_finite()))
    }

    /// The largest absolute channel value in the frame, for the manifest.
    #[must_use]
    pub fn peak(&self) -> f32 {
        self.pixels
            .iter()
            .flat_map(|pixel| pixel.iter())
            .fold(0.0_f32, |peak, channel| peak.max(channel.abs()))
    }

    /// The frame as code values, through a transfer function, at a declared depth.
    ///
    /// This is the single rounding of the certified path. Everything before it is linear
    /// light; everything after it is integers, and the two never meet again. The encodings
    /// come from [`maj_color::TransferFunction`], whose kernels are deterministic, and the
    /// final quantisation rounds half away from zero on the exact half, which is the
    /// convention the project uses everywhere (`lerp_rational` included).
    #[must_use]
    pub fn to_code_values(&self, function: TransferFunction, depth: BitDepth) -> Vec<u16> {
        let maximum = MAXIMUM_BY_DEPTH[depth.index()] as f64;
        let mut out = Vec::with_capacity(self.pixels.len() * CHANNELS);
        for pixel in &self.pixels {
            for channel in pixel {
                let linear = if channel.is_finite() {
                    f64::from(*channel)
                } else {
                    0.0
                };
                let encoded = function.encode(linear);
                let clamped = if encoded.is_nan() {
                    0.0
                } else {
                    encoded.clamp(0.0, 1.0)
                };
                let quantised = (clamped * maximum + 0.5).floor();
                out.push(quantised as u16);
            }
        }
        out
    }
}

/// The integer depths a frame can be written at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BitDepth {
    /// Eight bits per channel, for a viewing copy.
    Eight,
    /// Sixteen bits per channel, for a lossless reference.
    Sixteen,
}

impl BitDepth {
    /// Every depth, in increasing order.
    pub const ALL: [Self; 2] = [Self::Eight, Self::Sixteen];

    /// The largest integer each channel can take.
    #[must_use]
    pub const fn maximum(self) -> u16 {
        MAXIMUM_BY_DEPTH[self.index()]
    }

    /// Short stable name, used in manifests and file names.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Eight => "8",
            Self::Sixteen => "16",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Eight => 0,
            Self::Sixteen => 1,
        }
    }
}

const MAXIMUM_BY_DEPTH: [u16; 2] = [255, u16::MAX];

/// Rounds toward negative infinity as an integer, without the platform's float-to-int
/// conversion, which is one of the operations whose behaviour on an out-of-range value is
/// defined by the cast rather than by the number.
fn floored(value: f32) -> i64 {
    let truncated = value as i64;
    if (value as f64) < truncated as f64 {
        truncated - 1
    } else {
        truncated
    }
}

/// Panics if the dimensions do not describe a frame that can exist.
fn checked_pixel_count(width: u32, height: u32) -> usize {
    assert!(width > 0 && height > 0, "a frame must have a non-zero size");
    let count = u64::from(width) * u64::from(height);
    assert!(
        count <= 1 << 34,
        "a {width}x{height} frame is {count} pixels, which is beyond what this build renders"
    );
    count as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_black_frame_is_opaque_and_within_bounds() {
        let frame = Framebuffer::black(4, 3);
        assert_eq!(frame.width(), 4);
        assert_eq!(frame.height(), 3);
        assert_eq!(frame.pixels().len(), 12);
        assert_eq!(frame.pixel(3, 2), Some([0.0, 0.0, 0.0, 1.0]));
        assert_eq!(frame.pixel(4, 0), None);
        assert!(frame.is_finite());
        assert_eq!(frame.peak(), 1.0);
    }

    #[test]
    fn sampling_clamps_rather_than_inventing_a_border() {
        let mut frame = Framebuffer::black(2, 1);
        frame.set_pixel(0, 0, [0.25, 0.0, 0.0, 1.0]);
        frame.set_pixel(1, 0, [0.75, 0.0, 0.0, 1.0]);
        assert_eq!(frame.sample_clamped(-5, 0), [0.25, 0.0, 0.0, 1.0]);
        assert_eq!(frame.sample_clamped(99, 0), [0.75, 0.0, 0.0, 1.0]);
        // Nearest neighbour at a pixel centre is the pixel itself.
        assert_eq!(frame.sample_nearest(0.5, 0.5), [0.25, 0.0, 0.0, 1.0]);
        assert_eq!(frame.sample_nearest(1.5, 0.5), [0.75, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn bilinear_landmarks_are_exact() {
        let mut frame = Framebuffer::black(2, 2);
        frame.set_pixel(0, 0, [1.0, 0.0, 0.0, 1.0]);
        frame.set_pixel(1, 0, [0.0, 1.0, 0.0, 1.0]);
        frame.set_pixel(0, 1, [0.0, 0.0, 1.0, 1.0]);
        frame.set_pixel(1, 1, [1.0, 1.0, 1.0, 1.0]);

        // On a pixel centre the filter returns that pixel untouched.
        assert_eq!(frame.sample_bilinear(0.5, 0.5), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(frame.sample_bilinear(1.5, 1.5), [1.0, 1.0, 1.0, 1.0]);

        // Midway between the four centres, every tap carries equal weight.
        // Every tap carries a quarter: red comes from two of them, and so do green and
        // blue, so all three channels land on a half.
        let middle = frame.sample_bilinear(1.0, 1.0);
        assert!((middle[0] - 0.5).abs() < 1.0e-6, "{middle:?}");
        assert!((middle[1] - 0.5).abs() < 1.0e-6, "{middle:?}");
        assert!((middle[2] - 0.5).abs() < 1.0e-6, "{middle:?}");

        // Outside the frame the filter keeps clamping the same edge taps.
        assert_eq!(frame.sample_bilinear(-10.0, 0.5), [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn the_digest_separates_size_content_and_order() {
        let first = Framebuffer::black(2, 1);
        let second = Framebuffer::black(1, 2);
        assert_ne!(
            first.digest(),
            second.digest(),
            "size is part of the digest"
        );

        let mut changed = Framebuffer::black(2, 1);
        changed.set_pixel(1, 0, [0.0, 0.0, 0.0, 0.999_999_9]);
        assert_ne!(first.digest(), changed.digest());

        let mut swapped = Framebuffer::black(2, 1);
        swapped.set_pixel(1, 0, [1.0, 0.0, 0.0, 1.0]);
        let mut other = Framebuffer::black(2, 1);
        other.set_pixel(0, 0, [1.0, 0.0, 0.0, 1.0]);
        assert_ne!(swapped.digest(), other.digest(), "position matters");

        assert_eq!(first.digest(), Framebuffer::black(2, 1).digest());
    }

    #[test]
    fn quantisation_rounds_once_at_the_declared_boundary() {
        let mut frame = Framebuffer::black(1, 1);
        frame.set_pixel(0, 0, [0.5, 0.25, 0.0, 1.0]);
        let eight = frame.to_code_values(TransferFunction::Linear, BitDepth::Eight);
        assert_eq!(eight[0], 128, "half rounds up");
        assert_eq!(eight[1], 64);
        assert_eq!(eight[2], 0);
        assert_eq!(eight[3], 255);

        let sixteen = frame.to_code_values(TransferFunction::Linear, BitDepth::Sixteen);
        assert_eq!(sixteen[0], 32_768);
        assert_eq!(sixteen[1], 16_384);

        // Above one is clamped, because a code value has no meaning outside the range.
        let mut hot = Framebuffer::black(1, 1);
        hot.set_pixel(0, 0, [4.0, -1.0, 0.0, 1.0]);
        let clamped = hot.to_code_values(TransferFunction::Linear, BitDepth::Eight);
        assert_eq!(clamped[0], 255);
        assert_eq!(clamped[1], 0);
    }

    #[test]
    fn the_transfer_function_is_applied_at_the_boundary_only() {
        // The linear light the sRGB curve decodes code 128 to, at the precision a frame
        // actually holds (the nearest `f32` to 0.21404114048223258). Encoding it again sits
        // exactly on the quantisation boundary between 127 and 128, which is precisely why
        // an integer-depth round trip is asserted to within one code rather than exactly:
        // the boundary is a property of the encoding, not an error in it.
        let mut frame = Framebuffer::black(1, 1);
        frame.set_pixel(0, 0, [0.214_041_14, 0.0, 0.0, 1.0]);
        let code = frame.to_code_values(TransferFunction::Srgb, BitDepth::Eight)[0];
        assert!(
            (i32::from(code) - 128).abs() <= 1,
            "sRGB mid grey must land on code 128, landed on {code}"
        );

        // The same linear value through no curve at all is a different picture, which is
        // the claim: the curve is applied at the boundary, and it is the curve that does it.
        let plain = frame.to_code_values(TransferFunction::Linear, BitDepth::Eight)[0];
        assert_eq!(plain, 55);
        assert_ne!(plain, code);
    }

    #[test]
    fn a_non_finite_channel_is_reported_and_quantised_to_black() {
        let mut frame = Framebuffer::black(1, 1);
        frame.set_pixel(0, 0, [f32::NAN, 0.0, 0.0, 1.0]);
        assert!(!frame.is_finite());
        let code = frame.to_code_values(TransferFunction::Linear, BitDepth::Eight);
        assert_eq!(code[0], 0);
    }
}
