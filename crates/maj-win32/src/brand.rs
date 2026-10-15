//! The brand, as this application uses it: the owner's logo, decoded once at build time.
//!
//! `assets/brand/logo.png` is the only source of the artwork (`assets/brand/README.md` records its
//! bytes and its hash). This module holds no pixels of its own: `build.rs` decodes that file,
//! crops the emblem out of the plate's margins, resamples it to the sizes the interface and the
//! operating system ask for, and writes the result into the build directory, where it is included
//! below. The consequence worth stating plainly is that **the shipping binary carries pixels and
//! no PNG decoder** — and that the sizes in it are the sizes that are used, rather than a
//! megabyte of source artwork scaled down at run time.
//!
//! Two consumers, one mark:
//!
//! * [`icons`] hands Windows the two icons a window class wants — the taskbar's and the title
//!   bar's — built from the emblem rather than from a resource, because there is no resource
//!   compiler on this host and no `.ico` to link (ADR 0013).
//! * [`seal`] draws the same emblem into the interface, where the reference frame this interface
//!   follows also carries it: the corner of the strip that changes pages.
//!
//! What the code here deliberately does **not** do is re-colour the artwork. The supplied logo is
//! ink on a plate, and the plate is part of it: inverting it, keying it out or reducing it to the
//! brand's gold would all be design decisions the owner has not made. `assets/brand/README.md`
//! keeps that question open and `bash assets/brand/check-page.sh` shows every candidate.

use crate::win32::{self, Colour, Hdc, Rect};

include!(concat!(env!("OUT_DIR"), "/brand.rs"));

/// The two icons a window class wants, from the emblem: the large one and the small one.
///
/// `None` when the operating system refuses a bitmap, which leaves the window with whatever icon
/// it had rather than with a failure — an icon is not worth refusing to start over.
pub(crate) fn icons() -> Option<(*mut std::ffi::c_void, *mut std::ffi::c_void)> {
    let large = win32::icon_from_bgra(32, &ICON_32_BGRA)?;
    let small = win32::icon_from_bgra(16, &ICON_16_BGRA)?;
    Some((large, small))
}

/// What the build made of the logo, in one line, for the application to report.
///
/// It is evidence rather than decoration: it names the source artwork's size and the crop taken
/// out of it, so a person reading `--self-check` can see which file was decoded and that the
/// emblem was found inside it rather than assumed to fill it.
#[must_use]
pub(crate) fn summary() -> String {
    format!(
        "brand: logo {}x{} decoded at build time, emblem {} px cropped to a {} px seal and the \
         16 and 32 px icons",
        SOURCE.0, SOURCE.1, EMBLEM.2, SEAL_SIZE
    )
}

/// Draws the emblem set in `ink`, on `ground`, with no plate.
///
/// The rectangle's shape is the caller's business: the emblem is square and the blit is
/// smoothed, so a caller that gives it a square gets the mark undistorted.
///
/// The supplied logo is ink on white, and a white plate on a dark interface reads as a sticker
/// stuck to the window rather than as the application's mark. So the plate is not drawn: the
/// emblem's own coverage ([`SEAL_ALPHA`]) chooses, per pixel, how much of `ink` to lay over
/// `ground`, which keeps the artwork's antialiased edges instead of cutting them to a threshold.
///
/// The compositing happens here, into a small buffer, rather than through `AlphaBlend`. Both
/// bars this is drawn on are a flat fill or a gradient between two colours eleven code values
/// apart, so mixing against one of them gives the same pixels an alpha blend would, without a
/// DIB section having to exist for the length of a paint. `ground` is what the caller filled
/// behind it; passing the wrong one shows up as a faint halo, which is why the two call sites
/// pass the colour they just drew with.
pub(crate) fn seal_tinted(hdc: Hdc, rect: Rect, ink: Colour, ground: Colour) {
    if rect.is_empty() {
        return;
    }
    let (ink_r, ink_g, ink_b) = ink.parts();
    let (ground_r, ground_g, ground_b) = ground.parts();
    let mix = |ink: u8, ground: u8, coverage: u8| -> u8 {
        let coverage = u32::from(coverage);
        let value = u32::from(ink) * coverage + u32::from(ground) * (255 - coverage);
        ((value + 127) / 255).min(255) as u8
    };
    let mut pixels = Vec::with_capacity(SEAL_ALPHA.len() * 4);
    for coverage in SEAL_ALPHA {
        // BGRA, top-down: what every 32-bit bitmap in this crate is.
        pixels.push(mix(ink_b, ground_b, coverage));
        pixels.push(mix(ink_g, ground_g, coverage));
        pixels.push(mix(ink_r, ground_r, coverage));
        pixels.push(255);
    }
    win32::blit_smooth(hdc, rect, SEAL_SIZE, SEAL_SIZE, &pixels);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How much of the pixel at a coordinate is ink.
    fn ink_at(x: u32, y: u32) -> u32 {
        u32::from(SEAL_ALPHA[(y * SEAL_SIZE + x) as usize])
    }

    #[test]
    fn the_generated_mask_is_a_square_of_coverage_and_the_icons_are_opaque() {
        assert_eq!(SEAL_ALPHA.len(), (SEAL_SIZE * SEAL_SIZE) as usize);
        assert_eq!(ICON_16_BGRA.len(), 16 * 16 * 4);
        assert_eq!(ICON_32_BGRA.len(), 32 * 32 * 4);
        assert!(
            ICON_16_BGRA
                .chunks_exact(4)
                .chain(ICON_32_BGRA.chunks_exact(4))
                .all(|pixel| pixel[3] == 255),
            "an icon carries the artwork as supplied, plate and all"
        );
    }

    #[test]
    fn the_emblem_crop_lies_inside_the_source_artwork() {
        let (x, y, side) = EMBLEM;
        assert!(side > 0);
        assert!(
            x + side <= SOURCE.0 && y + side <= SOURCE.1,
            "the crop {EMBLEM:?} does not fit in {SOURCE:?}"
        );
        // The supplied plate is wider than it is tall, so a square crop of the emblem cannot be
        // the whole file; and the emblem is a mark, not a hairline.
        assert!(side < SOURCE.0);
        assert!(side > SOURCE.1 / 2);
    }

    #[test]
    fn the_mask_is_a_mark_rather_than_an_empty_or_a_filled_square() {
        // The corners carry no ink, because the mark is centred with a margin around it. On the
        // mask this is the plate having become nothing at all rather than having become white,
        // which is the whole point of drawing from coverage: there is no plate to read as a
        // sticker on a dark bar.
        for (x, y) in [(0, 0), (SEAL_SIZE - 1, 0), (0, SEAL_SIZE - 1)] {
            assert!(
                ink_at(x, y) < 24,
                "the corner ({x}, {y}) should be bare, and its coverage is {}",
                ink_at(x, y)
            );
        }

        let ink = (0..SEAL_SIZE)
            .flat_map(|y| (0..SEAL_SIZE).map(move |x| (x, y)))
            .filter(|(x, y)| ink_at(*x, *y) > 128)
            .count();
        let total = (SEAL_SIZE * SEAL_SIZE) as usize;
        assert!(
            ink * 100 / total > 2 && ink * 100 / total < 60,
            "{ink} of {total} pixels are ink, which is not the shape of a crown and a sword"
        );

        // The crop is taken about the emblem's own centre, so the mark must sit in the middle of
        // the square rather than in a corner of it: this is what a broken bounding box looks like.
        let (mut left, mut right) = (SEAL_SIZE, 0);
        for x in 0..SEAL_SIZE {
            for y in 0..SEAL_SIZE {
                if ink_at(x, y) > 128 {
                    left = left.min(x);
                    right = right.max(x);
                }
            }
        }
        let centre = (left + right) / 2;
        let middle = SEAL_SIZE / 2;
        assert!(
            centre.abs_diff(middle) <= 2,
            "the mark's centre is at {centre} and the seal's is at {middle}"
        );
    }

    #[test]
    fn the_mask_keeps_the_soft_edge_the_artwork_has() {
        // A threshold would have been easier and would have left the mark with jagged edges at
        // 48 pixels. So there must be pixels that are partly ink: not many, because the mark is
        // mostly solid, but enough to be an edge rather than a staircase.
        let partial = SEAL_ALPHA
            .iter()
            .filter(|coverage| (24..=231).contains(*coverage))
            .count();
        assert!(
            partial > 32,
            "only {partial} pixels of the mask are a partial coverage, which is a threshold \
             rather than an edge"
        );
    }
}
