//! Reading frames: the surface, declared here and implemented in M2b.
//!
//! Nothing in this module decodes anything yet, and the shapes are not provisional: they are
//! what the seam in `maj-render` requires, and `docs/MEDIA.md` §5 records why each one is the
//! shape it is. Declaring them in M2a is what lets the layering and the vocabulary be proved
//! before an FFI crate exists.

use std::fmt;

use crate::error::MediaError;
use crate::picture::PictureFormat;
use crate::stream::VideoStream;

/// One plane of a decoded picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plane {
    /// Bytes per row, which may exceed the row's own width.
    pub stride: usize,
    /// Rows in this plane.
    pub rows: usize,
    /// The samples, row-major, `stride` bytes per row.
    pub bytes: Vec<u8>,
}

impl Plane {
    /// Whether the buffer is large enough for the geometry it declares.
    ///
    /// Checked rather than assumed, and with checked arithmetic: a stride times a row count
    /// is exactly the multiplication that overflows on malformed input, and this crate's gate
    /// is a corpus with zero crashes in it.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        self.stride
            .checked_mul(self.rows)
            .is_some_and(|needed| self.bytes.len() >= needed)
    }
}

/// One decoded picture, in the format it was decoded in. Nothing is converted.
///
/// Returning the frame unconverted is the load-bearing decision of the whole decode path.
/// YUV-to-RGB and transfer decoding are arithmetic on the certified path, done by `maj-color`
/// in the layer-4 adapter under ADR 0006's rules about reduction order and rounding — not by
/// `swscale`, whose results are reproducible for a build and not across builds, which is
/// exactly the property Tier 2 cannot have. And a ten-bit frame is never rounded to eight on
/// the way in: rounding happens once, at the declared output boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFrame {
    format: PictureFormat,
    coded_size: (u32, u32),
    index: i64,
    planes: Vec<Plane>,
}

impl DecodedFrame {
    /// A frame from its planes.
    ///
    /// # Errors
    ///
    /// [`MediaError::Malformed`] when a plane's buffer is smaller than the geometry it
    /// declares. A decoder that handed out a short plane would be a decoder whose consumers
    /// index past the end of a buffer, so the check is here rather than at every use.
    pub fn new(
        format: PictureFormat,
        coded_size: (u32, u32),
        index: i64,
        planes: Vec<Plane>,
    ) -> Result<Self, MediaError> {
        if let Some(position) = planes.iter().position(|plane| !plane.is_consistent()) {
            return Err(MediaError::Malformed(format!(
                "decoded plane {position} is shorter than its declared geometry"
            )));
        }
        Ok(Self {
            format,
            coded_size,
            index,
            planes,
        })
    }

    /// How the samples are laid out and what they mean.
    #[must_use]
    pub const fn format(&self) -> &PictureFormat {
        &self.format
    }

    /// Coded size, before rotation and before the sample aspect is applied.
    #[must_use]
    pub const fn coded_size(&self) -> (u32, u32) {
        self.coded_size
    }

    /// The index that was asked for, so a caller can prove it got what it asked for.
    #[must_use]
    pub const fn index(&self) -> i64 {
        self.index
    }

    /// The planes, in layout order.
    #[must_use]
    pub fn planes(&self) -> &[Plane] {
        &self.planes
    }
}

/// A source of decoded video frames, addressed by frame index.
pub trait VideoReader: fmt::Debug {
    /// The stream this reader reads.
    fn stream(&self) -> &VideoStream;

    /// The frame at `index`, decoding and seeking as needed.
    ///
    /// Addressed by index because that is what the retime map produces: `PictureResolver`
    /// hands the renderer a `source_frame: i64`, so no float seconds cross this boundary in
    /// either direction. Timestamps, keyframe distance, packet feeding and reordering are
    /// this reader's problem, solved exactly once, inside layer 2.
    ///
    /// Returns a borrow, so the common case — resolve once, sample a million times —
    /// allocates nothing per sample and the reader keeps ownership of its buffer.
    ///
    /// # Errors
    ///
    /// The three conditions in [`MediaError`] and no fourth. A frame that needs more packets,
    /// a stream that ended, a seek that landed before the keyframe it needed: those are
    /// states of this reader, not errors, and a reader that reported them as
    /// [`MediaError::Io`] would make a hazard placeholder out of a working file.
    fn frame(&mut self, index: i64) -> Result<&DecodedFrame, MediaError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picture::{BitDepth, PlaneLayout};

    fn format() -> PictureFormat {
        PictureFormat::undeclared(PlaneLayout::Yuv420, BitDepth::Eight)
    }

    #[test]
    fn a_frame_keeps_the_index_it_was_asked_for() {
        let plane = Plane {
            stride: 4,
            rows: 2,
            bytes: vec![0; 8],
        };
        let frame = DecodedFrame::new(format(), (4, 2), 17, vec![plane]).expect("consistent");
        assert_eq!(frame.index(), 17);
        assert_eq!(frame.coded_size(), (4, 2));
        assert_eq!(frame.planes().len(), 1);
        assert_eq!(frame.format().depth, BitDepth::Eight);
    }

    #[test]
    fn a_plane_shorter_than_its_geometry_is_refused_rather_than_trusted() {
        let plane = Plane {
            stride: 4,
            rows: 2,
            bytes: vec![0; 7],
        };
        assert!(!plane.is_consistent());
        let refused = DecodedFrame::new(format(), (4, 2), 0, vec![plane]);
        assert!(matches!(refused, Err(MediaError::Malformed(_))));
    }

    #[test]
    fn a_stride_that_overflows_is_refused_rather_than_wrapping() {
        let plane = Plane {
            stride: usize::MAX,
            rows: 2,
            bytes: Vec::new(),
        };
        assert!(!plane.is_consistent());
    }
}
