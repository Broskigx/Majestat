//! Sixteen-bit binary PPM: the reference intermediate, and the format that needs no library.
//!
//! PPM is not a delivery format. It has no metadata, no alpha and no standard way to say
//! what curve its samples are on. It is here because it is the one image container that can
//! be written and read with nothing but arithmetic, which makes it the format a determinism
//! gate and a diff tool can rely on.
//!
//! Two things this writer refuses to do quietly:
//!
//! * **Drop alpha.** A PPM holds three channels. A frame with anything less than a fully
//!   opaque alpha is refused rather than flattened behind a colour the writer invented.
//! * **Guess the curve.** The samples are the code values of the declared transfer function,
//!   and the section header of the file is the only place that declaration exists, so the
//!   manifest has to travel with it.
//!
//! Samples are sixteen bits, big-endian, and the header is written in the one canonical form
//! the format allows: `P6`, the dimensions, the maximum, and a single newline before the
//! binary body.

use maj_color::TransferFunction;
use maj_render::Framebuffer;

use crate::ExportError;

/// The scale PPM's sixteen-bit variant declares in its header.
pub const MAXIMUM: u16 = u16::MAX;

/// The header of a sixteen-bit PPM of the given size.
#[must_use]
pub fn header(width: u32, height: u32) -> String {
    format!("P6\n{width} {height}\n{}\n", MAXIMUM)
}

/// Encodes a frame as a sixteen-bit binary PPM.
///
/// # Errors
///
/// Returns [`ExportError::TransparentPixel`] when any pixel is not fully opaque, because
/// dropping alpha silently would turn a composite into a different picture.
pub fn encode(frame: &Framebuffer, transfer: TransferFunction) -> Result<Vec<u8>, ExportError> {
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            if let Some(pixel) = frame.pixel(x, y) {
                if pixel[3] < 1.0 {
                    return Err(ExportError::TransparentPixel {
                        x,
                        y,
                        alpha: pixel[3],
                    });
                }
            }
        }
    }
    let header = header(frame.width(), frame.height());
    let codes = frame.to_code_values(transfer, maj_render::BitDepth::Sixteen);
    let mut out = Vec::with_capacity(header.len() + codes.len() / 4 * 3 * 2);
    out.extend_from_slice(header.as_bytes());
    for pixel in codes.chunks_exact(4) {
        for channel in &pixel[..3] {
            out.extend_from_slice(&channel.to_be_bytes());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use maj_render::{BitDepth, Pixel};

    fn opaque(width: u32, height: u32, colour: Pixel) -> Framebuffer {
        Framebuffer::new(width, height, colour)
    }

    #[test]
    fn the_header_is_the_one_canonical_form() {
        assert_eq!(header(3, 2), "P6\n3 2\n65535\n");
        // A P6 body begins immediately after exactly one newline.
        assert!(header(1, 1).ends_with('\n'));
    }

    #[test]
    fn the_body_is_the_pixels_most_significant_byte_first() {
        let mut frame = opaque(2, 1, [0.0, 0.0, 0.0, 1.0]);
        frame.set_pixel(0, 0, [1.0, 0.5, 0.0, 1.0]);
        let bytes = encode(&frame, TransferFunction::Linear).expect("an opaque frame");
        let header_length = header(2, 1).len();
        assert_eq!(&bytes[..header_length], header(2, 1).as_bytes());
        let body = &bytes[header_length..];
        assert_eq!(
            body.len(),
            2 * 3 * 2,
            "two pixels, three channels, two bytes"
        );
        assert_eq!(&body[0..2], &[0xFF, 0xFF], "red at the full scale");
        assert_eq!(&body[2..4], &[0x80, 0x00], "green at the half");
        assert_eq!(&body[4..6], &[0x00, 0x00], "blue at zero");
        assert_eq!(
            &body[6..12],
            &[0, 0, 0, 0, 0, 0],
            "the second pixel is black"
        );
    }

    #[test]
    fn the_curve_is_applied_at_the_boundary() {
        // The nearest `f32` to the linear light code 128 decodes to, which is the precision
        // a frame holds.
        let frame = opaque(1, 1, [0.214_041_14, 0.0, 0.0, 1.0]);
        let plain = encode(&frame, TransferFunction::Linear).expect("an opaque frame");
        let srgb = encode(&frame, TransferFunction::Srgb).expect("an opaque frame");
        let header_length = header(1, 1).len();
        let plain_red =
            u16::from_be_bytes(plain[header_length..header_length + 2].try_into().unwrap());
        let srgb_red =
            u16::from_be_bytes(srgb[header_length..header_length + 2].try_into().unwrap());
        assert_eq!(
            plain_red, 14_027,
            "linear light, scaled by 65535 and rounded once"
        );
        assert!(
            (i32::from(srgb_red) - 32_768).abs() <= 1,
            "the sRGB curve puts mid grey at the middle of the range, and it landed on {srgb_red}"
        );
    }

    #[test]
    fn a_transparent_frame_is_refused_rather_than_flattened() {
        let clear = opaque(1, 1, [0.5, 0.5, 0.5, 0.0]);
        let error = encode(&clear, TransferFunction::Linear).expect_err("alpha cannot be dropped");
        assert_eq!(
            error,
            ExportError::TransparentPixel {
                x: 0,
                y: 0,
                alpha: 0.0
            }
        );

        let mut half = opaque(2, 1, [0.5, 0.5, 0.5, 1.0]);
        half.set_pixel(1, 0, [0.0, 0.0, 0.0, 0.5]);
        assert!(matches!(
            encode(&half, TransferFunction::Linear),
            Err(ExportError::TransparentPixel { x: 1, y: 0, .. })
        ));
    }

    #[test]
    fn the_same_frame_encodes_to_the_same_bytes() {
        let frame = opaque(4, 3, [0.25, 0.5, 0.75, 1.0]);
        let first = encode(&frame, TransferFunction::Rec709).expect("an opaque frame");
        let second = encode(&frame, TransferFunction::Rec709).expect("an opaque frame");
        assert_eq!(first, second);
        assert_eq!(first.len(), header(4, 3).len() + 4 * 3 * 3 * 2);
    }

    #[test]
    fn the_writer_agrees_with_the_frame_about_the_declared_depth() {
        // Sixteen bits is the only depth this format carries, and the reader in
        // `maj-render` reads it back to the same linear light.
        let mut frame = opaque(1, 1, [0.0, 0.0, 0.0, 1.0]);
        frame.set_pixel(0, 0, [0.5, 0.25, 0.125, 1.0]);
        let codes = frame.to_code_values(TransferFunction::Linear, BitDepth::Sixteen);
        let bytes = encode(&frame, TransferFunction::Linear).expect("an opaque frame");
        let body = &bytes[header(1, 1).len()..];
        for channel in 0..3 {
            let value = u16::from_be_bytes(body[channel * 2..channel * 2 + 2].try_into().unwrap());
            assert_eq!(value, codes[channel]);
        }
    }
}
