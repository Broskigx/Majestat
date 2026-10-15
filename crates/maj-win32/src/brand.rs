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

use crate::win32::{self, Hdc, Rect};

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

/// Draws the seal — the emblem, cropped out of the plate — into a rectangle.
///
/// The rectangle's shape is the caller's business: the seal is square and the blit is smoothed,
/// so a caller that gives it a square gets the mark undistorted.
pub(crate) fn seal(hdc: Hdc, rect: Rect) {
    if rect.is_empty() {
        return;
    }
    win32::blit_smooth(hdc, rect, SEAL_SIZE, SEAL_SIZE, &SEAL_BGRA);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The luminance of the pixel at a coordinate of the seal, as the eye weighs it.
    fn luma(x: u32, y: u32) -> u32 {
        let base = ((y * SEAL_SIZE + x) * 4) as usize;
        let blue = u32::from(SEAL_BGRA[base]);
        let green = u32::from(SEAL_BGRA[base + 1]);
        let red = u32::from(SEAL_BGRA[base + 2]);
        (299 * red + 587 * green + 114 * blue) / 1000
    }

    #[test]
    fn the_generated_seal_is_a_square_of_opaque_bgra() {
        assert_eq!(SEAL_BGRA.len(), (SEAL_SIZE * SEAL_SIZE * 4) as usize);
        assert!(
            SEAL_BGRA.chunks_exact(4).all(|pixel| pixel[3] == 255),
            "every pixel of the seal is opaque: the logo includes its plate"
        );
        assert_eq!(ICON_16_BGRA.len(), 16 * 16 * 4);
        assert_eq!(ICON_32_BGRA.len(), 32 * 32 * 4);
        assert!(
            ICON_16_BGRA
                .chunks_exact(4)
                .chain(ICON_32_BGRA.chunks_exact(4))
                .all(|pixel| pixel[3] == 255)
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
    fn the_seal_is_a_mark_on_a_plate_rather_than_an_empty_square() {
        // The plate's corners are paper, because the mark is centred with a margin around it.
        for (x, y) in [(0, 0), (SEAL_SIZE - 1, 0), (0, SEAL_SIZE - 1)] {
            assert!(
                luma(x, y) > 200,
                "the corner ({x}, {y}) should be the plate, and it is {}",
                luma(x, y)
            );
        }

        let ink = (0..SEAL_SIZE)
            .flat_map(|y| (0..SEAL_SIZE).map(move |x| (x, y)))
            .filter(|(x, y)| luma(*x, *y) < 128)
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
                if luma(x, y) < 128 {
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
}
