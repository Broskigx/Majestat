//! How a layer meets the frame and the layers under it: placement, blending, masks.
//!
//! Three decisions live here, and all three are part of the certified path rather than
//! implementation detail.
//!
//! **Placement is inverted, not forward-mapped.** The renderer walks the *output* pixels
//! and asks what source coordinate each one came from, so no output pixel can be left
//! unvisited and no source pixel can be written twice. The transform of
//! [`maj_core::Transform`] is applied in the order the format declares — scale about the
//! anchor, then clockwise rotation, then translation — and inverted by reversing that
//! order.
//!
//! **Blending is non-premultiplied and exact.** Every mode is a closed-form expression in
//! multiplication, division, comparison and square root. There is no `pow`, no `exp` and no
//! lookup table, because a fast approximation here would make two machines disagree about a
//! pixel, and the whole point of the certified path is that they do not.
//!
//! **Masks are evaluated in source-normalised coordinates**, which is where the model
//! defines them, so a mask survives a change of sequence resolution. The coverage it
//! produces is multiplied into the layer's alpha *before* the effects run, so a blurred
//! layer has a blurred mask edge rather than a hard one.

use maj_color::det;
use maj_core::{BlendMode, Mask, MaskShape, Rational, Transform};

use crate::framebuffer::Pixel;

/// Blends two non-premultiplied colours with one mode, channel by channel.
///
/// The result is not clamped: the mode's value is what it is, and clamping belongs to the
/// single rounding at the output boundary. Clamping here would make [`BlendMode::Add`] a
/// different function depending on where it sits in a chain.
#[must_use]
pub fn blend(mode: BlendMode, source: Pixel, backdrop: Pixel) -> Pixel {
    let mut out = [0.0_f32; 4];
    for channel in 0..3 {
        out[channel] = blend_channel(mode, source[channel], backdrop[channel]);
    }
    out[3] = source[3];
    out
}

/// One channel of one blend mode.
///
/// The formulas are the ones the W3C compositing specification publishes, which are the
/// ones every compositor implements, so a designer's expectation transfers.
#[must_use]
pub fn blend_channel(mode: BlendMode, source: f32, backdrop: f32) -> f32 {
    match mode {
        BlendMode::Normal => source,
        BlendMode::Add => source + backdrop,
        BlendMode::Multiply => source * backdrop,
        BlendMode::Screen => 1.0 - (1.0 - source) * (1.0 - backdrop),
        BlendMode::Overlay => {
            if backdrop <= 0.5 {
                2.0 * source * backdrop
            } else {
                1.0 - 2.0 * (1.0 - source) * (1.0 - backdrop)
            }
        }
        BlendMode::Darken => {
            if source < backdrop {
                source
            } else {
                backdrop
            }
        }
        BlendMode::Lighten => {
            if source > backdrop {
                source
            } else {
                backdrop
            }
        }
        BlendMode::Difference => (source - backdrop).abs(),
        BlendMode::HardLight => {
            if source <= 0.5 {
                2.0 * source * backdrop
            } else {
                1.0 - 2.0 * (1.0 - source) * (1.0 - backdrop)
            }
        }
        BlendMode::SoftLight => {
            if source <= 0.5 {
                backdrop - (1.0 - 2.0 * source) * backdrop * (1.0 - backdrop)
            } else {
                let curve = if backdrop <= 0.25 {
                    ((16.0 * backdrop - 12.0) * backdrop + 4.0) * backdrop
                } else {
                    backdrop.sqrt()
                };
                backdrop + (2.0 * source - 1.0) * (curve - backdrop)
            }
        }
    }
}

/// Composites a layer over a backdrop: the blend mode, weighted by alpha and opacity.
///
/// This is source-over with a blend function, which is the same thing every other
/// compositor does:
///
/// ```text
/// alpha        = source alpha * opacity
/// out rgb      = backdrop rgb + alpha * (B(source rgb, backdrop rgb) - backdrop rgb)
/// out alpha    = alpha + backdrop alpha * (1 - alpha)
/// ```
///
/// With no blend mode of its own this reduces to the plain `lerp` that a cross-fade needs,
/// which is why a transition is not a special case anywhere in the renderer.
#[must_use]
pub fn composite(mode: BlendMode, opacity: f32, source: Pixel, backdrop: Pixel) -> Pixel {
    let alpha = source[3] * det::clamp(f64::from(opacity), 0.0, 1.0) as f32;
    let blended = blend(mode, source, backdrop);
    let mut out = [0.0_f32; 4];
    for channel in 0..3 {
        out[channel] = backdrop[channel] + alpha * (blended[channel] - backdrop[channel]);
    }
    out[3] = alpha + backdrop[3] * (1.0 - alpha);
    out
}

/// A layer's placement resolved to numbers, ready to invert per pixel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    source_width: f64,
    source_height: f64,
    frame_width: f64,
    frame_height: f64,
    anchor_x: f64,
    anchor_y: f64,
    translate_x: f64,
    translate_y: f64,
    scale_x: f64,
    scale_y: f64,
    cos: f64,
    sin: f64,
}

impl Placement {
    /// Resolves a model transform for a source read into a frame.
    ///
    /// Returns `None` when either scale is zero, because such a layer covers no area and
    /// its inverse is not a function. Reporting that as "nothing to draw" is honest;
    /// dividing by zero and drawing garbage is not.
    #[must_use]
    pub fn new(
        transform: Transform,
        source_size: (u32, u32),
        frame_size: (u32, u32),
    ) -> Option<Self> {
        let scale_x = rational_to_f64(transform.scale_x());
        let scale_y = rational_to_f64(transform.scale_y());
        if scale_x == 0.0 || scale_y == 0.0 {
            return None;
        }
        let (sine, cosine) = det::sin_cos(det::radians(rational_to_f64(transform.rotation())));
        Some(Self {
            source_width: f64::from(source_size.0),
            source_height: f64::from(source_size.1),
            frame_width: f64::from(frame_size.0),
            frame_height: f64::from(frame_size.1),
            anchor_x: rational_to_f64(transform.anchor_x()) * f64::from(source_size.0),
            anchor_y: rational_to_f64(transform.anchor_y()) * f64::from(source_size.1),
            translate_x: rational_to_f64(transform.x()),
            translate_y: rational_to_f64(transform.y()),
            scale_x,
            scale_y,
            cos: cosine,
            sin: sine,
        })
    }

    /// The source coordinate that lands on a frame coordinate.
    ///
    /// The frame coordinate is in pixel units with the centre of pixel zero at `0.5`, and
    /// the translation is measured from the centre of the frame with positive `y` downward,
    /// exactly as the format declares.
    #[must_use]
    pub fn to_source(&self, frame_x: f64, frame_y: f64) -> (f64, f64) {
        let rel_x = frame_x - self.frame_width * 0.5 - self.translate_x;
        let rel_y = frame_y - self.frame_height * 0.5 - self.translate_y;
        // R(-rotation) * S(1/scale), because the forward order is scale, then rotate.
        let rotated_x = rel_x * self.cos + rel_y * self.sin;
        let rotated_y = -rel_x * self.sin + rel_y * self.cos;
        (
            self.anchor_x + rotated_x / self.scale_x,
            self.anchor_y + rotated_y / self.scale_y,
        )
    }

    /// Where the frame's top-left corner came from, for the culling test.
    #[must_use]
    pub fn source_bounds(&self) -> (f64, f64, f64, f64) {
        let corners = [
            self.to_source(0.0, 0.0),
            self.to_source(self.frame_width, 0.0),
            self.to_source(0.0, self.frame_height),
            self.to_source(self.frame_width, self.frame_height),
        ];
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for (x, y) in corners {
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
        (min_x, min_y, max_x, max_y)
    }

    /// Whether any part of the source can land inside the frame.
    ///
    /// The test is conservative in both directions: it uses the corners of the frame only,
    /// which is exact for an affine map, and it widens the source rectangle by half a pixel
    /// so a layer whose edge falls between two samples is not culled.
    #[must_use]
    pub fn touches_frame(&self) -> bool {
        let (min_x, min_y, max_x, max_y) = self.source_bounds();
        max_x >= -0.5
            && max_y >= -0.5
            && min_x <= self.source_width + 0.5
            && min_y <= self.source_height + 0.5
    }

    /// The source's own size, for the manifest.
    #[must_use]
    pub fn source_size(&self) -> (u32, u32) {
        (self.source_width as u32, self.source_height as u32)
    }
}

/// Multiplies the coverage of every mask into one number in `0..=1`.
///
/// `u` and `v` are source-normalised coordinates: `0, 0` is the top-left corner of the
/// source frame and `1, 1` its bottom-right, which is the space [`Mask`] is defined in.
/// Masks intersect, so the coverages multiply; an inverted mask contributes its complement.
#[must_use]
pub fn mask_coverage(masks: &[Mask], u: f64, v: f64) -> f32 {
    let mut coverage = 1.0_f64;
    for mask in masks {
        let opacity = rational_to_f64(mask.opacity());
        if opacity <= 0.0 {
            continue;
        }
        let (center_x, center_y) = mask.center();
        let (width, height) = mask.size();
        let (sine, cosine) = det::sin_cos(det::radians(rational_to_f64(mask.rotation())));
        let half_width = rational_to_f64(width) * 0.5;
        let half_height = rational_to_f64(height) * 0.5;
        if half_width <= 0.0 || half_height <= 0.0 {
            continue;
        }
        let offset_x = u - rational_to_f64(center_x);
        let offset_y = v - rational_to_f64(center_y);
        // Undo the mask's own rotation, so the shape test is axis-aligned.
        let local_x = offset_x * cosine + offset_y * sine;
        let local_y = -offset_x * sine + offset_y * cosine;
        let normalised_x = (local_x / half_width).abs();
        let normalised_y = (local_y / half_height).abs();
        let distance = match mask.shape() {
            MaskShape::Rectangle => normalised_x.max(normalised_y),
            MaskShape::Ellipse => {
                (normalised_x * normalised_x + normalised_y * normalised_y).sqrt()
            }
        };
        let feather = det::clamp(rational_to_f64(mask.feather()), 0.0, 1.0);
        // The feather is a length in the frame, while `distance` is measured in half-sizes,
        // so the band has to be converted before it is used: a feather equal to the shape's
        // small half-size ramps across the whole shape. Dividing by the shape size is what
        // makes the softness independent of how large the mask is, which is the property a
        // designer expects when they scale a mask up.
        let softness = if feather <= 0.0 {
            0.0
        } else {
            feather / half_width.min(half_height)
        };
        let shape = if softness <= 0.0 {
            if distance <= 1.0 { 1.0 } else { 0.0 }
        } else {
            // The soft band lies inside the shape, so the mask still covers exactly what
            // its geometry says it covers and the falloff eats into it.
            det::clamp((1.0 - distance) / softness, 0.0, 1.0)
        };
        let shape = if mask.is_inverted() {
            1.0 - shape
        } else {
            shape
        };
        coverage *= shape * opacity;
    }
    coverage as f32
}

/// A rational as the `f64` the renderer computes with.
///
/// The conversion is exact for every value a project stores (a numerator and denominator
/// inside 32 bits) up to the 53 bits of a double, and it is the only place where the exact
/// model becomes an approximate one — deliberately, at the boundary of the arithmetic.
#[must_use]
pub fn rational_to_f64(value: Rational) -> f64 {
    value.num() as f64 / value.den() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use maj_core::MaskId;

    /// A mask identifier for the tests, allocated the way the project allocates one.
    fn mask_id(raw: u64) -> MaskId {
        MaskId::new(raw)
    }

    #[test]
    fn every_blend_mode_is_a_closed_form_with_the_expected_landmarks() {
        // White and black are the fixed points each mode is defined by.
        for mode in BlendMode::ALL {
            let knobs = [
                (1.0_f32, 1.0_f32),
                (1.0, 0.5),
                (0.5, 1.0),
                (0.5, 0.5),
                (0.25, 0.75),
            ];
            for (source, backdrop) in knobs {
                let value = blend_channel(mode, source, backdrop);
                assert!(value.is_finite(), "{mode:?} produced {value}");
                assert!(
                    (-1.0..=2.0).contains(&value),
                    "{mode:?} left the working range at ({source}, {backdrop}): {value}"
                );
            }
        }

        assert_eq!(blend_channel(BlendMode::Normal, 0.3, 0.7), 0.3);
        assert_eq!(blend_channel(BlendMode::Multiply, 0.5, 0.5), 0.25);
        assert_eq!(blend_channel(BlendMode::Screen, 0.5, 0.5), 0.75);
        assert_eq!(blend_channel(BlendMode::Darken, 0.3, 0.7), 0.3);
        assert_eq!(blend_channel(BlendMode::Lighten, 0.3, 0.7), 0.7);
        assert!((blend_channel(BlendMode::Difference, 0.3, 0.7) - 0.4).abs() < 1.0e-7);
        // Overlay branches on the backdrop, HardLight on the source: they are each other
        // with the operands swapped.
        assert_eq!(
            blend_channel(BlendMode::Overlay, 0.3, 0.7),
            blend_channel(BlendMode::HardLight, 0.7, 0.3)
        );
        // Multiply darkens and Screen brightens, which is what their names promise.
        assert!(blend_channel(BlendMode::Multiply, 0.8, 0.8) < 0.8);
        assert!(blend_channel(BlendMode::Screen, 0.2, 0.2) > 0.2);
        // Soft light is symmetric about half, and moves slowly.
        assert!((blend_channel(BlendMode::SoftLight, 0.5, 0.37) - 0.37).abs() < 1.0e-7);
        assert!(blend_channel(BlendMode::SoftLight, 0.75, 0.5) > 0.5);
        assert!(blend_channel(BlendMode::SoftLight, 0.25, 0.5) < 0.5);
    }

    #[test]
    fn an_opaque_normal_layer_replaces_the_backdrop_and_a_transparent_one_does_not() {
        let opaque = [0.25, 0.5, 0.75, 1.0];
        let backdrop = [1.0, 0.0, 0.5, 1.0];
        assert_eq!(composite(BlendMode::Normal, 1.0, opaque, backdrop), opaque);

        let clear = [0.25, 0.5, 0.75, 0.0];
        assert_eq!(composite(BlendMode::Normal, 1.0, clear, backdrop), backdrop);

        // Half alpha lands exactly halfway, and opacity scales that alpha.
        let half = [0.0, 0.0, 0.0, 0.5];
        let mixed = composite(BlendMode::Normal, 1.0, half, [1.0, 1.0, 1.0, 1.0]);
        assert!((mixed[0] - 0.5).abs() < 1.0e-7, "{mixed:?}");
        let quarter = composite(BlendMode::Normal, 0.5, half, [1.0, 1.0, 1.0, 1.0]);
        assert!((quarter[0] - 0.75).abs() < 1.0e-7, "{quarter:?}");
        assert!(
            (quarter[3] - 1.0).abs() < 1.0e-7,
            "an opaque backdrop stays opaque"
        );
    }

    #[test]
    fn a_multiply_layer_over_a_white_backdrop_is_its_own_picture() {
        let source = [0.4, 0.6, 0.8, 0.5];
        let backdrop = [1.0, 1.0, 1.0, 1.0];
        let out = composite(BlendMode::Multiply, 1.0, source, backdrop);
        // Source-over with a multiply function, over an opaque white backdrop: the result is
        // the backdrop plus half of the difference between the product and the backdrop.
        // A multiply layer over white therefore darkens by half of what an opaque one would.
        assert!((out[0] - 0.7).abs() < 1.0e-6, "{out:?}");
        assert!((out[1] - 0.8).abs() < 1.0e-6, "{out:?}");
        assert!(
            (out[3] - 1.0).abs() < 1.0e-6,
            "the backdrop stays opaque: {out:?}"
        );
    }

    #[test]
    fn an_identity_placement_maps_the_pixel_grid_onto_itself() {
        let placement =
            Placement::new(Transform::identity(), (64, 32), (64, 32)).expect("a placement");
        assert!(placement.touches_frame());
        for (x, y) in [(0.5, 0.5), (32.5, 16.5), (63.5, 31.5), (10.5, 7.5)] {
            assert_eq!(placement.to_source(x, y), (x, y), "at {x}, {y}");
        }
    }

    #[test]
    fn an_integer_translation_moves_the_source_by_whole_pixels() {
        let transform = Transform::new(
            Rational::from_integer(4),
            Rational::from_integer(-3),
            Rational::from_integer(1),
            Rational::from_integer(1),
            Rational::from_integer(0),
            Rational::from_reduced(1, 2),
            Rational::from_reduced(1, 2),
        )
        .expect("a transform");
        let placement = Placement::new(transform, (32, 32), (32, 32)).expect("a placement");
        // A frame pixel 4 to the right reads a source pixel 4 to the left, and vice versa.
        assert_eq!(placement.to_source(20.5, 10.5), (16.5, 13.5));
    }

    #[test]
    fn a_scale_of_two_reads_every_other_source_pixel() {
        let transform = Transform::new(
            Rational::from_integer(0),
            Rational::from_integer(0),
            Rational::from_integer(2),
            Rational::from_integer(2),
            Rational::from_integer(0),
            Rational::from_reduced(1, 2),
            Rational::from_reduced(1, 2),
        )
        .expect("a transform");
        let placement = Placement::new(transform, (64, 64), (128, 128)).expect("a placement");
        // The frame is twice the source, so a frame pixel maps to half a source pixel.
        assert_eq!(placement.to_source(32.0, 32.0), (16.0, 16.0));
    }

    #[test]
    fn a_quarter_turn_maps_the_frame_corners_onto_the_source_corners() {
        let transform = Transform::new(
            Rational::from_integer(0),
            Rational::from_integer(0),
            Rational::from_integer(1),
            Rational::from_integer(1),
            Rational::from_integer(90),
            Rational::from_reduced(1, 2),
            Rational::from_reduced(1, 2),
        )
        .expect("a transform");
        let placement = Placement::new(transform, (100, 50), (50, 100)).expect("a placement");
        // Rotating the source a quarter turn clockwise brings its bottom-left corner to the
        // frame's top-left, and its top-right corner to the frame's bottom-right.
        let (x, y) = placement.to_source(0.0, 0.0);
        assert!((x - 0.0).abs() < 1.0e-9, "{x}");
        assert!((y - 50.0).abs() < 1.0e-9, "{y}");
        let (x, y) = placement.to_source(50.0, 100.0);
        assert!((x - 100.0).abs() < 1.0e-9, "{x}");
        assert!((y - 0.0).abs() < 1.0e-9, "{y}");
        // The source's own centre is the frame's centre, because the anchor is the centre.
        let (x, y) = placement.to_source(25.0, 50.0);
        assert!(
            (x - 50.0).abs() < 1.0e-9 && (y - 25.0).abs() < 1.0e-9,
            "{x}, {y}"
        );
    }

    #[test]
    fn a_zero_scale_never_becomes_a_transform_so_there_is_no_inverse_to_take() {
        // The model refuses a collapsed scale where the value is written, so the renderer
        // can never be handed one. `Placement::new` still refuses it, because a renderer
        // that divides by a zero scale would produce NaN rather than an invisible layer.
        let collapsed = Transform::new(
            Rational::from_integer(0),
            Rational::from_integer(0),
            Rational::from_integer(0),
            Rational::from_integer(1),
            Rational::from_integer(0),
            Rational::from_reduced(1, 2),
            Rational::from_reduced(1, 2),
        );
        assert!(collapsed.is_err(), "a zero scale is not a transform");
        let mirrored = Transform::new(
            Rational::from_integer(0),
            Rational::from_integer(0),
            Rational::from_integer(-1),
            Rational::from_integer(1),
            Rational::from_integer(0),
            Rational::from_reduced(1, 2),
            Rational::from_reduced(1, 2),
        );
        assert!(mirrored.is_err(), "and neither is a negative one");
        assert!(Placement::new(Transform::identity(), (32, 32), (32, 32)).is_some());
    }

    #[test]
    fn a_layer_far_outside_the_frame_is_culled() {
        let transform = Transform::new(
            Rational::from_integer(4_000),
            Rational::from_integer(0),
            Rational::from_integer(1),
            Rational::from_integer(1),
            Rational::from_integer(0),
            Rational::from_reduced(1, 2),
            Rational::from_reduced(1, 2),
        )
        .expect("a transform");
        let placement = Placement::new(transform, (32, 32), (32, 32)).expect("a placement");
        assert!(!placement.touches_frame());
    }

    #[test]
    fn a_full_mask_covers_and_a_zero_sized_one_does_not() {
        let full = Mask::new(mask_id(1), MaskShape::Rectangle).expect("a mask");
        assert_eq!(mask_coverage(&[full], 0.5, 0.5), 1.0);
        assert_eq!(mask_coverage(&[], 0.5, 0.5), 1.0);

        let half = Mask::with_geometry(
            mask_id(2),
            MaskShape::Rectangle,
            Rational::from_reduced(1, 2),
            Rational::from_reduced(1, 2),
            Rational::from_reduced(1, 2),
            Rational::from_reduced(1, 2),
            Rational::from_integer(0),
            Rational::from_integer(0),
            Rational::from_integer(1),
            false,
        )
        .expect("a mask");
        // Inside the middle half, fully covered; outside it, not covered at all.
        assert_eq!(mask_coverage(&[half], 0.5, 0.5), 1.0);
        assert_eq!(mask_coverage(&[half], 0.5, 0.9), 0.0);
        assert_eq!(mask_coverage(&[half], 0.9, 0.5), 0.0);
    }

    #[test]
    fn an_inverted_mask_is_the_complement_and_masks_intersect() {
        let inverted = Mask::with_geometry(
            mask_id(3),
            MaskShape::Rectangle,
            Rational::from_reduced(1, 2),
            Rational::from_reduced(1, 2),
            Rational::from_reduced(1, 2),
            Rational::from_reduced(1, 2),
            Rational::from_integer(0),
            Rational::from_integer(0),
            Rational::from_integer(1),
            true,
        )
        .expect("a mask");
        assert_eq!(mask_coverage(&[inverted], 0.5, 0.5), 0.0);
        assert_eq!(mask_coverage(&[inverted], 0.05, 0.05), 1.0);

        let left = Mask::with_geometry(
            mask_id(4),
            MaskShape::Rectangle,
            Rational::from_reduced(1, 4),
            Rational::from_reduced(1, 2),
            Rational::from_reduced(1, 2),
            Rational::from_integer(1),
            Rational::from_integer(0),
            Rational::from_integer(0),
            Rational::from_integer(1),
            false,
        )
        .expect("a mask");
        let top = Mask::with_geometry(
            mask_id(5),
            MaskShape::Rectangle,
            Rational::from_reduced(1, 2),
            Rational::from_reduced(1, 4),
            Rational::from_integer(1),
            Rational::from_reduced(1, 2),
            Rational::from_integer(0),
            Rational::from_integer(0),
            Rational::from_integer(1),
            false,
        )
        .expect("a mask");
        // The intersection is the top-left quadrant and nothing else.
        assert_eq!(mask_coverage(&[left, top], 0.25, 0.25), 1.0);
        assert_eq!(mask_coverage(&[left, top], 0.75, 0.25), 0.0);
        assert_eq!(mask_coverage(&[left, top], 0.25, 0.75), 0.0);
        assert_eq!(mask_coverage(&[left, top], 0.75, 0.75), 0.0);
    }

    #[test]
    fn an_ellipse_mask_rounds_its_corners() {
        let ellipse = Mask::with_geometry(
            mask_id(6),
            MaskShape::Ellipse,
            Rational::from_reduced(1, 2),
            Rational::from_reduced(1, 2),
            Rational::from_integer(1),
            Rational::from_integer(1),
            Rational::from_integer(0),
            Rational::from_integer(0),
            Rational::from_integer(1),
            false,
        )
        .expect("a mask");
        assert_eq!(mask_coverage(&[ellipse], 0.5, 0.5), 1.0);
        // Just inside the right edge on the horizontal axis is covered...
        assert_eq!(mask_coverage(&[ellipse], 0.999, 0.5), 1.0);
        // ...but the corner is not, which is the difference from a rectangle.
        assert_eq!(mask_coverage(&[ellipse], 0.999, 0.999), 0.0);
    }

    #[test]
    fn a_feathered_mask_ramps_inside_its_own_edge() {
        let feathered = Mask::with_geometry(
            mask_id(7),
            MaskShape::Rectangle,
            Rational::from_reduced(1, 2),
            Rational::from_reduced(1, 2),
            Rational::from_integer(1),
            Rational::from_integer(1),
            Rational::from_integer(0),
            Rational::from_reduced(1, 2),
            Rational::from_integer(1),
            false,
        )
        .expect("a mask");
        // The band is the outer half of the shape's half-size: full coverage at the
        // centre, zero at the edge, and halfway through the band at the halfway point.
        assert_eq!(mask_coverage(&[feathered], 0.5, 0.5), 1.0);
        assert_eq!(mask_coverage(&[feathered], 1.0, 0.5), 0.0);
        let mid = mask_coverage(&[feathered], 0.75, 0.5);
        assert!((mid - 0.5).abs() < 1.0e-6, "coverage {mid}");
    }

    #[test]
    fn a_mask_opacity_scales_its_coverage() {
        let half_opacity = Mask::with_geometry(
            mask_id(8),
            MaskShape::Rectangle,
            Rational::from_reduced(1, 2),
            Rational::from_reduced(1, 2),
            Rational::from_integer(1),
            Rational::from_integer(1),
            Rational::from_integer(0),
            Rational::from_integer(0),
            Rational::from_reduced(1, 2),
            false,
        )
        .expect("a mask");
        assert!((mask_coverage(&[half_opacity], 0.5, 0.5) - 0.5).abs() < 1.0e-6);
    }
}
