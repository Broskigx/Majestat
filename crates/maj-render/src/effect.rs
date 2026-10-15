//! The ten effects the certified path proves, and the halo each one needs.
//!
//! Every variant of [`maj_core::EffectKind`] is implemented here, because the model says so
//! out loud: a project that uses one of these renders the same in the reference renderer as
//! in a graded output. Nothing in this module calls a platform transcendental. The two that
//! need one — exposure and gamma — go through [`maj_color::det`], which builds its `exp` and
//! its roots from multiplication, division and addition, so two machines agree on the last
//! bit of every pixel.
//!
//! Effects are applied in **frame space**, which is what the model's own wording requires:
//! a vignette darkens "toward the frame corners" and grain is measured on the frame, so both
//! must know where the tile they are working on sits inside the whole frame, not just where
//! it sits inside itself. [`EffectContext`] is that knowledge, and it is why a blurred and a
//! grained render are identical whether the frame was rendered in one tile or in sixty-four.
//!
//! Each effect declares its [`spatial_support`]: how many pixels around a sample it reads.
//! A tile rendered with at least that much halo around it produces exactly the pixels the
//! whole-frame pass would have produced, and the renderer uses that number to size its
//! tiles. Getting this wrong shows up as a seam, which is why there is a test that renders
//! the same frame two ways and compares the digests.

use maj_color::det;
use maj_core::{CoreError, EffectInstance, EffectKind, Param, Rational};

use crate::framebuffer::{Framebuffer, Pixel};

/// The linear value the contrast pivot and the exposure reference are measured against.
///
/// 0.18 is the diffuse white of an 18 per cent grey card — the value a cinematographer
/// meters for, and therefore the only defensible place to put a contrast pivot.
pub const MID_GREY: f32 = 0.18;

/// Rec.709 luminance weights, for saturation and hue rotation.
pub const LUMA_WEIGHTS: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// The maximum blur radius the tiling policy is prepared to halo for.
pub const MAX_BLUR_RADIUS: u32 = 256;

/// Where a buffer sits inside the frame it belongs to.
///
/// A tile knows its own size, but an effect that is defined on the frame — a vignette, a
/// grain field — needs the frame's size and the tile's origin as well. Passing that
/// explicitly is what keeps a tiled render bit-identical to an untiled one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectContext {
    /// Width of the whole frame.
    pub frame_width: u32,
    /// Height of the whole frame.
    pub frame_height: u32,
    /// Column of the buffer's left edge inside the frame.
    pub origin_x: i64,
    /// Row of the buffer's top edge inside the frame.
    pub origin_y: i64,
}

impl EffectContext {
    /// The context of a buffer that is the whole frame.
    #[must_use]
    pub const fn whole_frame(frame_width: u32, frame_height: u32) -> Self {
        Self {
            frame_width,
            frame_height,
            origin_x: 0,
            origin_y: 0,
        }
    }

    /// Whether the buffer is the entire frame.
    #[must_use]
    pub fn is_whole_frame(&self, width: u32, height: u32) -> bool {
        self.origin_x == 0
            && self.origin_y == 0
            && width == self.frame_width
            && height == self.frame_height
    }
}

/// How many pixels around a sample an effect reads.
///
/// The renderer sizes its halo by the sum of the supports in a layer's chain (see
/// [`chain_halo`]), so this has to be the truth rather than an estimate. A separable box
/// blur of radius `r` applied twice spreads over `2r`, and the unsharp mask reads one pixel
/// in each direction.
#[must_use]
pub fn spatial_support(kind: EffectKind, params: &[Param]) -> u32 {
    match kind {
        EffectKind::Blur => 2 * blur_radius(params),
        EffectKind::Sharpen => 1,
        EffectKind::Exposure
        | EffectKind::Brightness
        | EffectKind::Contrast
        | EffectKind::Saturation
        | EffectKind::Gamma
        | EffectKind::HueRotate
        | EffectKind::Vignette
        | EffectKind::Grain => 0,
    }
}

/// The halo a layer's effect chain needs, which is the **sum** of its supports.
///
/// Not the maximum, which is the mistake this function exists to prevent. Effects run in
/// order, so a sharpen reading one pixel of a blurred picture reads pixels that were
/// themselves built from six pixels away: to get the sharpen's output right at the edge of
/// a tile, the tile needs the blur's margin *plus* the sharpen's. Summing the supports is
/// exactly that arithmetic, and a chain whose halo is understated shows up as a seam.
#[must_use]
pub fn chain_halo(effects: &[EffectInstance]) -> u32 {
    effects
        .iter()
        .filter(|effect| effect.is_enabled())
        .map(|effect| spatial_support(effect.kind(), effect.params()))
        .sum()
}

/// Applies one effect to a buffer.
///
/// # Errors
///
/// Returns [`CoreError::InvalidArgument`] when a parameter the effect needs is missing or
/// of the wrong variant. A project round-tripped through validation cannot reach that state,
/// and a caller that built an effect by hand finds out immediately rather than silently
/// rendering the default.
pub fn apply(
    effect: &EffectInstance,
    frame: &mut Framebuffer,
    context: EffectContext,
) -> Result<(), CoreError> {
    if !effect.is_enabled() {
        return Ok(());
    }
    let kind = effect.kind();
    let params = effect.params();
    match kind {
        EffectKind::Exposure => {
            let stops = number(params, "stops", 0.0);
            // 2^stops, from the deterministic exponential rather than a platform `powf`.
            let gain = det::exp(stops * det::LN_2) as f32;
            map_rgb(frame, |channel| channel * gain);
        }
        EffectKind::Brightness => {
            let amount = number(params, "amount", 0.0) as f32;
            map_rgb(frame, |channel| channel + amount);
        }
        EffectKind::Contrast => {
            let amount = number(params, "amount", 1.0) as f32;
            map_rgb(frame, |channel| (channel - MID_GREY) * amount + MID_GREY);
        }
        EffectKind::Saturation => {
            let amount = number(params, "amount", 1.0) as f32;
            let luma = LUMA_WEIGHTS;
            map_pixel(frame, |pixel| {
                let grey = luma[0] * pixel[0] + luma[1] * pixel[1] + luma[2] * pixel[2];
                [
                    grey + (pixel[0] - grey) * amount,
                    grey + (pixel[1] - grey) * amount,
                    grey + (pixel[2] - grey) * amount,
                    pixel[3],
                ]
            });
        }
        EffectKind::Gamma => {
            // The exponent is the reciprocal of the declared value, so a value above one
            // lifts the mid-tones. Both parts stay exact integers: no rounded decimal and
            // no platform `powf` reaches the kernel.
            let (num, den) = rational_reciprocal(params, "value")?;
            map_rgb(frame, |channel| {
                det::pow_rational(signed_power(channel), num, den) as f32
            });
        }
        EffectKind::HueRotate => {
            let degrees = number(params, "degrees", 0.0);
            if degrees == 0.0 {
                // The YIQ round trip is not an exact identity, so the identity is taken
                // here rather than approximated: a rotation of nothing must not alter a bit.
                return Ok(());
            }
            let (sine, cosine) = det::sin_cos(det::radians(degrees));
            let (sine, cosine) = (sine as f32, cosine as f32);
            map_pixel(frame, |pixel| {
                let y = 0.299 * pixel[0] + 0.587 * pixel[1] + 0.114 * pixel[2];
                let i = 0.596 * pixel[0] - 0.274 * pixel[1] - 0.322 * pixel[2];
                let q = 0.211 * pixel[0] - 0.523 * pixel[1] + 0.312 * pixel[2];
                let rotated_i = i * cosine + q * sine;
                let rotated_q = -i * sine + q * cosine;
                [
                    y + 0.956 * rotated_i + 0.621 * rotated_q,
                    y - 0.272 * rotated_i - 0.647 * rotated_q,
                    y - 1.106 * rotated_i + 1.703 * rotated_q,
                    pixel[3],
                ]
            });
        }
        EffectKind::Blur => {
            let radius = blur_radius(params);
            if radius > 0 {
                box_blur(frame, radius);
                box_blur(frame, radius);
            }
        }
        EffectKind::Sharpen => {
            let amount = number(params, "amount", 0.5) as f32;
            if amount != 0.0 {
                let blurred = blurred_three_by_three(frame);
                for y in 0..frame.height() {
                    for x in 0..frame.width() {
                        let index = (y as usize) * (frame.width() as usize) + x as usize;
                        let sharp = frame.pixels()[index];
                        let soft = blurred.pixels()[index];
                        let mut out = [0.0_f32; 4];
                        for (channel, value) in out.iter_mut().enumerate().take(3) {
                            *value = sharp[channel] + amount * (sharp[channel] - soft[channel]);
                        }
                        out[3] = sharp[3];
                        frame.set_pixel(x, y, out);
                    }
                }
            }
        }
        EffectKind::Vignette => {
            let amount = number(params, "amount", 0.5) as f32;
            let feather = det::clamp(number(params, "feather", 0.5), 0.0, 1.0) as f32;
            let centre_x = f64::from(context.frame_width) * 0.5;
            let centre_y = f64::from(context.frame_height) * 0.5;
            let corner = (centre_x * centre_x + centre_y * centre_y).sqrt();
            let inner = 1.0 - f64::from(feather);
            for y in 0..frame.height() {
                for x in 0..frame.width() {
                    let frame_x = (context.origin_x + i64::from(x)) as f64 + 0.5;
                    let frame_y = (context.origin_y + i64::from(y)) as f64 + 0.5;
                    let dx = (frame_x - centre_x) / corner;
                    let dy = (frame_y - centre_y) / corner;
                    let distance = (dx * dx + dy * dy).sqrt();
                    let ramp = if feather <= 0.0 {
                        if distance >= 1.0 { 1.0 } else { 0.0 }
                    } else {
                        det::clamp((distance - inner) / f64::from(feather), 0.0, 1.0)
                    };
                    let gain = (1.0 - f64::from(amount) * ramp) as f32;
                    let mut pixel = frame.pixel(x, y).unwrap_or([0.0, 0.0, 0.0, 1.0]);
                    for channel in pixel.iter_mut().take(3) {
                        *channel *= gain;
                    }
                    frame.set_pixel(x, y, pixel);
                }
            }
        }
        EffectKind::Grain => {
            let amount = number(params, "amount", 0.125) as f32;
            let seed = integer(params, "seed", 1);
            if amount != 0.0 {
                for y in 0..frame.height() {
                    for x in 0..frame.width() {
                        let frame_x = context.origin_x + i64::from(x);
                        let frame_y = context.origin_y + i64::from(y);
                        let noise = grain_value(frame_x, frame_y, seed);
                        let mut pixel = frame.pixel(x, y).unwrap_or([0.0, 0.0, 0.0, 1.0]);
                        for (channel, value) in pixel.iter_mut().enumerate().take(3) {
                            *value += amount * noise * channel_weight(channel);
                        }
                        frame.set_pixel(x, y, pixel);
                    }
                }
            }
        }
    }
    Ok(())
}

/// The radius a blur effect was asked for, clamped to what the tiling policy supports.
#[must_use]
pub fn blur_radius(params: &[Param]) -> u32 {
    let radius = number(params, "radius", 4.0);
    if !radius.is_finite() || radius <= 0.0 {
        return 0;
    }
    (radius + 0.5).min(f64::from(MAX_BLUR_RADIUS)) as u32
}

/// A separable box blur, one pass, with the edge clamped to the frame.
///
/// The sum over the kernel window is taken in a fixed order and accumulated in double
/// precision. That is slower than a sliding sum, deliberately: a sliding sum accumulates
/// rounding differently depending on where the window started, so the same pixel would come
/// out differently in a tile than in a whole-frame pass, and the tiling test exists to make
/// that impossible rather than merely unlikely.
fn box_blur(frame: &mut Framebuffer, radius: u32) {
    let width = frame.width();
    let height = frame.height();
    let span = radius as i64;
    let mut horizontal = Framebuffer::new(width, height, [0.0, 0.0, 0.0, 0.0]);
    for y in 0..height {
        for x in 0..width {
            let mut sum = [0.0_f64; 4];
            let centre = i64::from(x);
            for offset in -span..=span {
                let tap = frame.sample_clamped(centre + offset, i64::from(y));
                for channel in 0..4 {
                    sum[channel] += f64::from(tap[channel]);
                }
            }
            let divisor = (2 * span + 1) as f64;
            horizontal.set_pixel(
                x,
                y,
                [
                    (sum[0] / divisor) as f32,
                    (sum[1] / divisor) as f32,
                    (sum[2] / divisor) as f32,
                    (sum[3] / divisor) as f32,
                ],
            );
        }
    }
    for y in 0..height {
        for x in 0..width {
            let mut sum = [0.0_f64; 4];
            let centre = i64::from(y);
            for offset in -span..=span {
                let tap = horizontal.sample_clamped(i64::from(x), centre + offset);
                for channel in 0..4 {
                    sum[channel] += f64::from(tap[channel]);
                }
            }
            let divisor = (2 * span + 1) as f64;
            frame.set_pixel(
                x,
                y,
                [
                    (sum[0] / divisor) as f32,
                    (sum[1] / divisor) as f32,
                    (sum[2] / divisor) as f32,
                    (sum[3] / divisor) as f32,
                ],
            );
        }
    }
}

/// A three-by-three box blur, for the unsharp mask.
fn blurred_three_by_three(frame: &Framebuffer) -> Framebuffer {
    let width = frame.width();
    let height = frame.height();
    let mut out = Framebuffer::new(width, height, [0.0, 0.0, 0.0, 0.0]);
    for y in 0..height {
        for x in 0..width {
            let mut sum = [0.0_f64; 4];
            for dy in -1..=1_i64 {
                for dx in -1..=1_i64 {
                    let tap = frame.sample_clamped(i64::from(x) + dx, i64::from(y) + dy);
                    for channel in 0..4 {
                        sum[channel] += f64::from(tap[channel]);
                    }
                }
            }
            out.set_pixel(
                x,
                y,
                [
                    (sum[0] / 9.0) as f32,
                    (sum[1] / 9.0) as f32,
                    (sum[2] / 9.0) as f32,
                    (sum[3] / 9.0) as f32,
                ],
            );
        }
    }
    out
}

/// A grain sample in `-1..=1`, a pure function of the pixel and the seed.
///
/// A hash rather than a random number generator: a generator carries state, and state makes
/// the value of a pixel depend on how many pixels were visited before it, which is exactly
/// what breaks a tiled render. The mixer is the finaliser splitmix64 uses.
#[must_use]
pub fn grain_value(x: i64, y: i64, seed: i64) -> f32 {
    let mut value = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (seed as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^= value >> 31;
    // The top 23 bits scaled to [0, 1), then to [-1, 1): a power of two, so the mapping is
    // exact and every sample is strictly inside the range.
    let unit = (value >> 41) as f32 / 8_388_608.0;
    unit * 2.0 - 1.0
}

/// Grain weight per channel, so the grain is not monochrome noise.
fn channel_weight(channel: usize) -> f32 {
    const WEIGHTS: [f32; 3] = [1.0, 0.9, 0.8];
    WEIGHTS[channel]
}

/// Applies a function to every colour channel of every pixel, leaving alpha alone.
fn map_rgb(frame: &mut Framebuffer, mut function: impl FnMut(f32) -> f32) {
    let width = frame.width();
    let height = frame.height();
    for y in 0..height {
        for x in 0..width {
            let mut pixel = frame.pixel(x, y).unwrap_or([0.0, 0.0, 0.0, 1.0]);
            for channel in pixel.iter_mut().take(3) {
                *channel = function(*channel);
            }
            frame.set_pixel(x, y, pixel);
        }
    }
}

/// Applies a function to every pixel.
fn map_pixel(frame: &mut Framebuffer, mut function: impl FnMut(Pixel) -> Pixel) {
    let width = frame.width();
    let height = frame.height();
    for y in 0..height {
        for x in 0..width {
            let pixel = frame.pixel(x, y).unwrap_or([0.0, 0.0, 0.0, 1.0]);
            frame.set_pixel(x, y, function(pixel));
        }
    }
}

/// A numeric parameter, falling back to the declared default.
fn number(params: &[Param], key: &str, fallback: f64) -> f64 {
    params
        .iter()
        .find(|param| param.key() == key)
        .and_then(|param| param.value().as_rational())
        .map_or(fallback, |value| value.num() as f64 / value.den() as f64)
}

/// A whole-number parameter.
fn integer(params: &[Param], key: &str, fallback: i64) -> i64 {
    params
        .iter()
        .find(|param| param.key() == key)
        .and_then(|param| param.value().as_integer())
        .unwrap_or(fallback)
}

/// The exact rational value of a numeric parameter, or the declared default.
fn rational(params: &[Param], key: &str, fallback: Rational) -> Rational {
    params
        .iter()
        .find(|param| param.key() == key)
        .and_then(|param| param.value().as_rational())
        .unwrap_or(fallback)
}

/// The reciprocal of a parameter's exact rational value, as numerator and denominator.
///
/// # Errors
///
/// Returns [`CoreError::InvalidArgument`] for a value that is not positive, because the
/// reciprocal of zero is not a number and a negative exponent is not what the schema
/// declares.
fn rational_reciprocal(params: &[Param], key: &str) -> Result<(i64, i64), CoreError> {
    let value = rational(params, key, Rational::from_integer(1));
    if !value.is_positive() {
        return Err(CoreError::invalid("an exponent must be greater than zero"));
    }
    Ok((value.den(), value.num()))
}

/// A channel as the base of a rational power, keeping the sign out of the kernel.
fn signed_power(channel: f32) -> f64 {
    if channel.is_finite() {
        f64::from(channel)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maj_core::{EffectId, ParamValue};

    /// An effect instance with one parameter set the way a test needs it.
    fn effect(kind: EffectKind, values: &[(&str, ParamValue)]) -> EffectInstance {
        let mut instance = EffectInstance::new(EffectId::new(1), kind);
        for (key, value) in values {
            instance
                .set_param(key, value.clone())
                .expect("a declared parameter");
        }
        instance
    }

    fn rational(value: i64) -> ParamValue {
        ParamValue::Rational(maj_core::Rational::from_integer(value))
    }

    fn rational_pair(num: i64, den: i64) -> ParamValue {
        ParamValue::Rational(maj_core::Rational::from_reduced(num, den))
    }

    fn frame(width: u32, height: u32, colour: Pixel) -> Framebuffer {
        Framebuffer::new(width, height, colour)
    }

    #[test]
    fn exposure_multiplies_by_powers_of_two() {
        let mut buffer = frame(2, 1, [0.25, 0.25, 0.25, 1.0]);
        let context = EffectContext::whole_frame(2, 1);
        apply(
            &effect(EffectKind::Exposure, &[("stops", rational(1))]),
            &mut buffer,
            context,
        )
        .expect("applies");
        let pixel = buffer.pixel(0, 0).expect("a pixel");
        assert!((pixel[0] - 0.5).abs() < 1.0e-6, "{pixel:?}");

        // A stop down halves, and zero stops is the identity to the bit.
        let mut down = frame(2, 1, [0.5, 0.0, 0.0, 1.0]);
        apply(
            &effect(EffectKind::Exposure, &[("stops", rational(-1))]),
            &mut down,
            context,
        )
        .expect("applies");
        assert_eq!(down.pixel(0, 0).expect("a pixel")[0], 0.25);

        let mut identity = frame(2, 1, [0.3, 0.4, 0.5, 1.0]);
        let before = identity.digest();
        apply(
            &effect(EffectKind::Exposure, &[("stops", rational(0))]),
            &mut identity,
            context,
        )
        .expect("applies");
        assert_eq!(
            identity.digest(),
            before,
            "zero stops must not touch the bits"
        );
    }

    #[test]
    fn brightness_offsets_and_contrast_pivots_about_mid_grey() {
        let context = EffectContext::whole_frame(1, 1);
        let mut buffer = frame(1, 1, [MID_GREY, 0.0, 0.0, 1.0]);
        apply(
            &effect(EffectKind::Contrast, &[("amount", rational_pair(2, 1))]),
            &mut buffer,
            context,
        )
        .expect("applies");
        assert!(
            (buffer.pixel(0, 0).expect("a pixel")[0] - MID_GREY).abs() < 1.0e-7,
            "the pivot is unmoved by a contrast change"
        );

        let mut bright = frame(1, 1, [0.1, 0.1, 0.1, 1.0]);
        apply(
            &effect(EffectKind::Brightness, &[("amount", rational_pair(1, 4))]),
            &mut bright,
            context,
        )
        .expect("applies");
        assert!((bright.pixel(0, 0).expect("a pixel")[0] - 0.35).abs() < 1.0e-6);

        // Contrast above one pushes away from the pivot, in both directions.
        let mut dark = frame(1, 1, [0.09, 0.0, 0.0, 1.0]);
        apply(
            &effect(EffectKind::Contrast, &[("amount", rational_pair(2, 1))]),
            &mut dark,
            context,
        )
        .expect("applies");
        assert!(dark.pixel(0, 0).expect("a pixel")[0] < 0.09);
    }

    #[test]
    fn saturation_leaves_grey_alone_and_doubles_a_primary() {
        let context = EffectContext::whole_frame(1, 1);
        let mut grey = frame(1, 1, [0.4, 0.4, 0.4, 1.0]);
        apply(
            &effect(EffectKind::Saturation, &[("amount", rational_pair(2, 1))]),
            &mut grey,
            context,
        )
        .expect("applies");
        let pixel = grey.pixel(0, 0).expect("a pixel");
        assert!((pixel[0] - 0.4).abs() < 1.0e-6, "{pixel:?}");
        assert!((pixel[1] - 0.4).abs() < 1.0e-6, "{pixel:?}");

        // Zero saturation is the Rec.709 luminance, which is the documented definition.
        let mut colour = frame(1, 1, [1.0, 0.0, 0.0, 1.0]);
        apply(
            &effect(EffectKind::Saturation, &[("amount", rational(0))]),
            &mut colour,
            context,
        )
        .expect("applies");
        let grey_value = colour.pixel(0, 0).expect("a pixel")[0];
        assert!(
            (grey_value - LUMA_WEIGHTS[0]).abs() < 1.0e-6,
            "{grey_value}"
        );
    }

    #[test]
    fn gamma_of_one_is_the_identity_and_half_brightens() {
        let context = EffectContext::whole_frame(1, 1);
        let mut identity = frame(1, 1, [0.25, 0.5, 0.75, 1.0]);
        let before = identity.digest();
        apply(
            &effect(EffectKind::Gamma, &[("value", rational(1))]),
            &mut identity,
            context,
        )
        .expect("applies");
        assert_eq!(identity.digest(), before, "a gamma of one must be exact");

        let mut lifted = frame(1, 1, [0.25, 0.0, 0.0, 1.0]);
        apply(
            &effect(EffectKind::Gamma, &[("value", rational(2))]),
            &mut lifted,
            context,
        )
        .expect("applies");
        let pixel = lifted.pixel(0, 0).expect("a pixel");
        assert!((pixel[0] - 0.5).abs() < 1.0e-6, "{pixel:?}");
    }

    #[test]
    fn a_half_turn_twice_returns_the_colour_it_started_from() {
        let context = EffectContext::whole_frame(1, 1);
        // Zero degrees is the identity, and the schema caps a rotation at half a turn, so a
        // full turn is two applications of the largest one the model allows.
        let mut untouched = frame(1, 1, [0.6, 0.3, 0.1, 1.0]);
        let original = untouched.digest();
        apply(
            &effect(EffectKind::HueRotate, &[("degrees", rational(0))]),
            &mut untouched,
            context,
        )
        .expect("applies");
        assert_eq!(
            untouched.digest(),
            original,
            "zero degrees must not touch the bits"
        );

        let mut buffer = frame(1, 1, [0.6, 0.3, 0.1, 1.0]);
        let before = buffer.pixel(0, 0).expect("a pixel");
        let half = effect(EffectKind::HueRotate, &[("degrees", rational(180))]);
        apply(&half, &mut buffer, context).expect("applies");
        apply(&half, &mut buffer, context).expect("applies");
        let after = buffer.pixel(0, 0).expect("a pixel");
        for channel in 0..3 {
            // The published YIQ matrix pair carries three decimal places, so a half turn
            // taken twice returns the colour to within that rounding — about a thousandth
            // of the smallest channel — and no closer. The exact identity is the zero-degree
            // case above, which takes a fast path.
            assert!(
                (after[channel] - before[channel]).abs() < 3.0e-3,
                "channel {channel}: {before:?} became {after:?}"
            );
        }

        // A third of a turn moves red toward green, and preserves luma to within the
        // round-trip error of the YIQ pair.
        let mut turned = frame(1, 1, [1.0, 0.0, 0.0, 1.0]);
        apply(
            &effect(EffectKind::HueRotate, &[("degrees", rational(120))]),
            &mut turned,
            context,
        )
        .expect("applies");
        let pixel = turned.pixel(0, 0).expect("a pixel");
        assert!(pixel[1] > pixel[0], "{pixel:?}");
        let luma_before = 0.299;
        let luma_after = 0.299 * pixel[0] + 0.587 * pixel[1] + 0.114 * pixel[2];
        assert!((luma_after - luma_before).abs() < 1.0e-3, "{pixel:?}");
    }

    #[test]
    fn a_blur_preserves_a_flat_field_and_spreads_an_impulse() {
        let context = EffectContext::whole_frame(16, 1);
        let mut flat = frame(16, 1, [0.5, 0.25, 0.0, 1.0]);
        apply(
            &effect(EffectKind::Blur, &[("radius", rational(3))]),
            &mut flat,
            context,
        )
        .expect("applies");
        for x in 0..16 {
            let pixel = flat.pixel(x, 0).expect("a pixel");
            assert!((pixel[0] - 0.5).abs() < 1.0e-6, "at {x}: {pixel:?}");
            assert!((pixel[1] - 0.25).abs() < 1.0e-6, "at {x}: {pixel:?}");
        }

        let mut impulse = frame(15, 1, [0.0, 0.0, 0.0, 1.0]);
        impulse.set_pixel(7, 0, [1.0, 1.0, 1.0, 1.0]);
        apply(
            &effect(EffectKind::Blur, &[("radius", rational(1))]),
            &mut impulse,
            context,
        )
        .expect("applies");
        let centre = impulse.pixel(7, 0).expect("a pixel")[0];
        let near = impulse.pixel(8, 0).expect("a pixel")[0];
        let far = impulse.pixel(0, 0).expect("a pixel")[0];
        assert!(
            centre > near && near > far,
            "the impulse must spread: {centre}, {near}, {far}"
        );
        assert!(centre < 1.0 && near > 0.0, "and it must be conserved-ish");
        // Two box passes of radius one reach two pixels, and no further.
        assert_eq!(far, 0.0, "the kernel has a bounded support");

        // A radius of zero must be exact rather than nearly exact.
        let mut untouched = frame(8, 8, [0.1, 0.2, 0.3, 1.0]);
        let before = untouched.digest();
        apply(
            &effect(EffectKind::Blur, &[("radius", rational(0))]),
            &mut untouched,
            context,
        )
        .expect("applies");
        assert_eq!(untouched.digest(), before);
    }

    #[test]
    fn a_blur_of_a_blur_is_wider_than_one_pass() {
        let context = EffectContext::whole_frame(41, 1);
        let mut once = frame(41, 1, [0.0, 0.0, 0.0, 1.0]);
        once.set_pixel(20, 0, [1.0, 1.0, 1.0, 1.0]);
        let mut twice = once.clone();
        let blur = effect(EffectKind::Blur, &[("radius", rational(2))]);
        apply(&blur, &mut once, context).expect("applies");
        apply(&blur, &mut twice, context).expect("applies");
        apply(&blur, &mut twice, context).expect("applies");

        // Each pass flattens the peak and pushes it further out.
        let peak_once = once.pixel(20, 0).expect("a pixel")[0];
        let peak_twice = twice.pixel(20, 0).expect("a pixel")[0];
        assert!(
            peak_twice < peak_once && peak_once < 1.0,
            "{peak_once} then {peak_twice}"
        );
        // One pass reaches four pixels; the second reaches eight, so the far tail moves.
        assert_eq!(
            once.pixel(26, 0).expect("a pixel")[0],
            0.0,
            "one pass is bounded"
        );
        assert!(
            twice.pixel(26, 0).expect("a pixel")[0] > 0.0,
            "two passes reach further"
        );
    }

    #[test]
    fn sharpening_keeps_a_flat_field_and_raises_an_edge() {
        let context = EffectContext::whole_frame(8, 1);
        let mut flat = frame(8, 1, [0.5, 0.5, 0.5, 1.0]);
        apply(
            &effect(EffectKind::Sharpen, &[("amount", rational(1))]),
            &mut flat,
            context,
        )
        .expect("applies");
        for x in 0..8 {
            let pixel = flat.pixel(x, 0).expect("a pixel");
            assert!((pixel[0] - 0.5).abs() < 1.0e-6, "at {x}: {pixel:?}");
        }

        let mut edge = frame(8, 1, [0.0, 0.0, 0.0, 1.0]);
        for x in 4..8 {
            edge.set_pixel(x, 0, [1.0, 1.0, 1.0, 1.0]);
        }
        let before = edge.pixel(4, 0).expect("a pixel")[0];
        apply(
            &effect(EffectKind::Sharpen, &[("amount", rational(1))]),
            &mut edge,
            context,
        )
        .expect("applies");
        let after = edge.pixel(4, 0).expect("a pixel")[0];
        assert!(
            after > before,
            "the light side of an edge gains: {before} then {after}"
        );
        let dark_side = edge.pixel(3, 0).expect("a pixel")[0];
        assert!(
            dark_side <= 0.0,
            "the dark side of an edge undershoots: {dark_side}"
        );
    }

    #[test]
    fn a_vignette_darkens_toward_the_corners_and_keeps_the_centre() {
        let context = EffectContext::whole_frame(9, 9);
        let mut buffer = frame(9, 9, [1.0, 1.0, 1.0, 1.0]);
        apply(
            &effect(
                EffectKind::Vignette,
                &[("amount", rational(1)), ("feather", rational(1))],
            ),
            &mut buffer,
            context,
        )
        .expect("applies");
        let centre = buffer.pixel(4, 4).expect("a pixel")[0];
        let corner = buffer.pixel(0, 0).expect("a pixel")[0];
        assert!(
            (centre - 1.0).abs() < 1.0e-6,
            "the centre is untouched: {centre}"
        );
        assert!(corner < 0.2, "the corner is darkened: {corner}");
        let mid = buffer.pixel(2, 4).expect("a pixel")[0];
        assert!(
            mid < centre && mid > corner,
            "the falloff is monotonic: {mid}"
        );

        // Alpha is not a picture, so a vignette must not make the frame transparent.
        assert_eq!(buffer.pixel(0, 0).expect("a pixel")[3], 1.0);
    }

    #[test]
    fn grain_is_reproducible_from_the_seed_and_framed_by_the_offset() {
        let context = EffectContext::whole_frame(16, 16);
        let first = effect(
            EffectKind::Grain,
            &[
                ("amount", rational_pair(1, 8)),
                ("seed", ParamValue::Integer(7)),
            ],
        );
        let mut a = frame(16, 16, [0.5, 0.5, 0.5, 1.0]);
        apply(&first, &mut a, context).expect("applies");
        let mut b = frame(16, 16, [0.5, 0.5, 0.5, 1.0]);
        apply(&first, &mut b, context).expect("applies");
        assert_eq!(
            a.digest(),
            b.digest(),
            "the same seed must give the same field"
        );

        let mut other_seed = frame(16, 16, [0.5, 0.5, 0.5, 1.0]);
        apply(
            &effect(
                EffectKind::Grain,
                &[
                    ("amount", rational_pair(1, 8)),
                    ("seed", ParamValue::Integer(8)),
                ],
            ),
            &mut other_seed,
            context,
        )
        .expect("applies");
        assert_ne!(
            a.digest(),
            other_seed.digest(),
            "a different seed is a different field"
        );

        // The same pixel gets the same grain whatever tile it lands in.
        assert_eq!(grain_value(12, 3, 7), grain_value(12, 3, 7));
        assert_ne!(grain_value(12, 3, 7), grain_value(13, 3, 7));
        for x in 0..64 {
            let value = grain_value(x, 0, 1);
            assert!((-1.0..1.0).contains(&value), "{value}");
        }
    }

    #[test]
    fn the_declared_support_matches_what_the_effect_reads() {
        let blur = effect(EffectKind::Blur, &[("radius", rational(4))]);
        assert_eq!(spatial_support(blur.kind(), blur.params()), 8);
        let sharpen = effect(EffectKind::Sharpen, &[("amount", rational(1))]);
        assert_eq!(spatial_support(sharpen.kind(), sharpen.params()), 1);
        let exposure = effect(EffectKind::Exposure, &[("stops", rational(1))]);
        assert_eq!(spatial_support(exposure.kind(), exposure.params()), 0);
        assert_eq!(chain_halo(&[exposure.clone(), blur.clone()]), 8);

        // Every effect declares a support and starts enabled. The halo a *disabled*
        // effect contributes is checked where the switch can actually be thrown — the
        // renderer's own tests, through `Command::SetEffectEnabled`.
        for kind in EffectKind::ALL {
            let instance = EffectInstance::new(EffectId::new(1), kind);
            assert!(instance.is_enabled(), "{kind:?} must start enabled");
            let support = spatial_support(kind, instance.params());
            assert!(support <= 2 * MAX_BLUR_RADIUS, "{kind:?}");
        }

        // A chain's halo is the sum of its parts: the second effect reads the first's
        // output, so both margins are needed, not the larger one.
        let vignette = effect(EffectKind::Vignette, &[("amount", rational(1))]);
        assert_eq!(chain_halo(&[blur.clone(), sharpen.clone()]), 9);
        assert_eq!(chain_halo(&[vignette, blur, sharpen]), 9);
    }

    #[test]
    fn every_effect_runs_with_its_defaults_and_leaves_the_frame_finite() {
        let context = EffectContext::whole_frame(8, 8);
        for kind in EffectKind::ALL {
            let mut buffer = frame(8, 8, [0.4, 0.3, 0.2, 1.0]);
            let defaults = EffectInstance::new(EffectId::new(3), kind);
            apply(&defaults, &mut buffer, context).expect("the defaults must apply");
            assert!(buffer.is_finite(), "{kind:?} produced a non-finite value");
            assert!(buffer.peak() < 8.0, "{kind:?} produced {}", buffer.peak());
        }
    }
}
