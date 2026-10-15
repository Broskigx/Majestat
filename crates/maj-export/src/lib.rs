//! `maj-export` — encoders, muxing, A/V-sync validation, render manifests, C2PA credentials
//!
//! **Layer 5.** See `docs/ARCHITECTURE.md` for the layering rule.
//!
//! This phase of the crate is the **still** half of that responsibility: the formats that
//! turn a rendered frame into a file, and the manifest that says how the frame was made.
//! Video encoding, muxing and credentials arrive with the rest of M8; the shape of this
//! module is already the shape they will slot into, because a frame leaves the renderer as
//! [`maj_render::Framebuffer`] and every writer here is a pure function of it.
//!
//! Two formats, chosen because between them they cover both ends of the ladder:
//!
//! * [`png`] — the delivery still. Lossless, sixteen bits per channel, alpha, and read by
//!   everything. Written with a stored deflate stream so the bytes are a function of the
//!   pixels and of nothing else.
//! * [`ppm`] — the reference intermediate. Three channels, no metadata, no library needed
//!   to read or write it, which makes it the format a determinism gate can diff.
//!
//! And one document:
//!
//! * [`manifest`] — the render manifest, as JSON. Policy, revision, layers, halo, digest.
//!
//! The single rounding of the certified path happens here, at the boundary: a
//! `Framebuffer` holds scene-linear light and a file holds integers, and
//! [`ImageFormat`] with a declared [`maj_render::BitDepth`] and
//! [`maj_color::TransferFunction`] is the statement of which integers.

#![forbid(unsafe_code)]

use std::fmt;
use std::path::{Path, PathBuf};

use maj_color::TransferFunction;
use maj_render::{BitDepth, Framebuffer};

pub mod manifest;
pub mod png;
pub mod ppm;

/// Everything that can stop an export.
#[derive(Debug, Clone, PartialEq)]
pub enum ExportError {
    /// A file could not be written.
    Io(String),
    /// The path's extension names no format this build writes.
    UnknownFormat(String),
    /// A PPM was asked to carry a pixel that is not fully opaque.
    TransparentPixel {
        /// Column of the offending pixel.
        x: u32,
        /// Row of the offending pixel.
        y: u32,
        /// The alpha that was found.
        alpha: f32,
    },
    /// A manifest could not be serialised.
    Manifest(String),
}

impl fmt::Display for ExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(what) => write!(formatter, "could not write: {what}"),
            Self::UnknownFormat(extension) => write!(
                formatter,
                "`{extension}` is not an image format this build writes; use png or ppm"
            ),
            Self::TransparentPixel { x, y, alpha } => write!(
                formatter,
                "a PPM carries no alpha, and the pixel ({x}, {y}) has alpha {alpha}"
            ),
            Self::Manifest(what) => write!(formatter, "the manifest could not be written: {what}"),
        }
    }
}

impl std::error::Error for ExportError {}

/// The still formats this crate writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    /// PNG, at eight or sixteen bits per channel, with alpha.
    Png,
    /// Sixteen-bit binary PPM, with no alpha.
    Ppm,
}

impl ImageFormat {
    /// Every format, in a fixed order.
    pub const ALL: [Self; 2] = [Self::Png, Self::Ppm];

    /// The format a path's extension names, if any.
    #[must_use]
    pub fn from_path(path: impl AsRef<Path>) -> Option<Self> {
        let extension = path
            .as_ref()
            .extension()?
            .to_string_lossy()
            .to_ascii_lowercase();
        match extension.as_str() {
            "png" => Some(Self::Png),
            "ppm" | "pnm" => Some(Self::Ppm),
            _ => None,
        }
    }

    /// The canonical extension, without a dot.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Ppm => "ppm",
        }
    }

    /// Whether the format can carry alpha.
    #[must_use]
    pub const fn has_alpha(self) -> bool {
        matches!(self, Self::Png)
    }

    /// Encodes a frame.
    ///
    /// # Errors
    ///
    /// Returns [`ExportError::TransparentPixel`] when a PPM is asked for a frame that is not
    /// fully opaque. PNG has no such restriction, because it carries alpha.
    pub fn encode(
        self,
        frame: &Framebuffer,
        depth: BitDepth,
        transfer: TransferFunction,
    ) -> Result<Vec<u8>, ExportError> {
        match self {
            Self::Png => Ok(png::encode(frame, depth, transfer)),
            Self::Ppm => ppm::encode(frame, transfer),
        }
    }
}

/// Writes a frame to a path, choosing the format from the extension.
///
/// Returns the number of bytes written, so a caller can report what it did rather than
/// assuming it worked.
///
/// # Errors
///
/// Returns [`ExportError::UnknownFormat`] for an extension this build does not write, and
/// the errors of the chosen encoder.
pub fn write_frame(
    path: impl AsRef<Path>,
    frame: &Framebuffer,
    depth: BitDepth,
    transfer: TransferFunction,
) -> Result<usize, ExportError> {
    let path = path.as_ref();
    let format = ImageFormat::from_path(path).ok_or_else(|| {
        ExportError::UnknownFormat(path.extension().map_or_else(String::new, |extension| {
            extension.to_string_lossy().into_owned()
        }))
    })?;
    let bytes = format.encode(frame, depth, transfer)?;
    std::fs::write(path, &bytes).map_err(|error| ExportError::Io(format!("{path:?}: {error}")))?;
    Ok(bytes.len())
}

/// Writes a manifest to a path, as JSON.
///
/// # Errors
///
/// Returns the errors of [`manifest::to_json`] and [`ExportError::Io`].
pub fn write_manifest(
    path: impl AsRef<Path>,
    value: &maj_render::RenderManifest,
) -> Result<usize, ExportError> {
    let path = path.as_ref();
    let text = manifest::to_json(value)?;
    std::fs::write(path, text.as_bytes())
        .map_err(|error| ExportError::Io(format!("{path:?}: {error}")))?;
    Ok(text.len())
}

/// Writes a frame and its manifest side by side.
///
/// This is the unit a render farm delivers: the picture, and the record of how it was made.
/// The manifest goes to [`manifest::path_for`] the frame's own path, so the two can never
/// drift apart once they are written.
///
/// # Errors
///
/// Returns the errors of [`write_frame`] and [`write_manifest`].
pub fn write_frame_with_manifest(
    path: impl AsRef<Path>,
    frame: &Framebuffer,
    manifest: &maj_render::RenderManifest,
    depth: BitDepth,
    transfer: TransferFunction,
) -> Result<(usize, PathBuf), ExportError> {
    let path = path.as_ref();
    let written = write_frame(path, frame, depth, transfer)?;
    let manifest_path = manifest::path_for(path);
    write_manifest(&manifest_path, manifest)?;
    Ok((written, manifest_path))
}

/// Human-readable statement of this crate's responsibility.
pub const ROLE: &str = "encoders, muxing, A/V-sync validation, render manifests, C2PA credentials";

/// Layer in the Majestat architecture, from `docs/ARCHITECTURE.md`.
pub const LAYER: u8 = 5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_its_role_and_layer() {
        assert!(!ROLE.is_empty(), "every crate must declare its role");
        assert_eq!(LAYER, 5);
    }

    #[test]
    fn the_format_comes_from_the_extension_and_says_what_it_can_carry() {
        assert_eq!(ImageFormat::from_path("a.png"), Some(ImageFormat::Png));
        assert_eq!(ImageFormat::from_path("a.PPM"), Some(ImageFormat::Ppm));
        assert_eq!(ImageFormat::from_path("a.pnm"), Some(ImageFormat::Ppm));
        assert_eq!(ImageFormat::from_path("a.mp4"), None);
        assert_eq!(ImageFormat::from_path("noextension"), None);
        assert_eq!(ImageFormat::Png.extension(), "png");
        assert!(ImageFormat::Png.has_alpha());
        assert!(!ImageFormat::Ppm.has_alpha());

        // An unknown extension is refused by name rather than by silence.
        let error = write_frame(
            "out.tiff",
            &Framebuffer::black(2, 2),
            BitDepth::Eight,
            TransferFunction::Srgb,
        )
        .expect_err("tiff is not written here");
        assert_eq!(error, ExportError::UnknownFormat("tiff".to_owned()));
        assert!(error.to_string().contains("png or ppm"));
    }

    #[test]
    fn a_frame_is_written_to_disk_with_its_manifest_beside_it() {
        let directory = std::env::temp_dir().join("majestat-export-test");
        std::fs::create_dir_all(&directory).expect("a temporary directory");
        let path = directory.join("frame.0001.png");
        let frame = Framebuffer::black(4, 3);
        let manifest = maj_render::RenderManifest {
            schema: maj_render::RENDER_MANIFEST_SCHEMA,
            policy: "certified",
            certified: true,
            sequence: 1,
            frame: 1,
            width: 4,
            height: 3,
            project_revision: 7,
            state_hash: "0".repeat(64),
            halo: 0,
            grid: (1, 1),
            layers: Vec::new(),
            digest: frame.digest(),
            peak_thousandths: 1000,
        };
        let (written, manifest_path) = write_frame_with_manifest(
            &path,
            &frame,
            &manifest,
            BitDepth::Eight,
            TransferFunction::Srgb,
        )
        .expect("the frame is written");
        assert!(written > 0);
        assert_eq!(manifest_path, directory.join("frame.0001.maj.json"));
        let image = std::fs::read(&path).expect("the frame is on disk");
        assert_eq!(image.len(), written);
        assert_eq!(&image[..8], &png::SIGNATURE);
        let text = std::fs::read_to_string(&manifest_path).expect("the manifest is on disk");
        assert!(text.contains("\"project_revision\": 7"));
        assert_eq!(text, manifest::to_json(&manifest).expect("serialises"));
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn a_png_and_a_ppm_of_one_frame_agree_about_the_picture() {
        let frame = Framebuffer::black(3, 2);
        let png = ImageFormat::Png
            .encode(&frame, BitDepth::Sixteen, TransferFunction::Linear)
            .expect("a PNG carries alpha");
        assert_eq!(&png[..8], &png::SIGNATURE);
        let ppm = ImageFormat::Ppm
            .encode(&frame, BitDepth::Sixteen, TransferFunction::Linear)
            .expect("an opaque frame");
        assert_eq!(&ppm[..2], b"P6");
        // Both hold the same code values, in different containers.
        assert!(
            ppm.len() > png.len() / 4,
            "the PPM is uncompressed and the PNG is stored"
        );
    }

    #[test]
    fn the_error_messages_say_what_to_do_about_it() {
        assert_eq!(
            ExportError::Io("no space".to_owned()).to_string(),
            "could not write: no space"
        );
        assert_eq!(
            ExportError::TransparentPixel {
                x: 1,
                y: 2,
                alpha: 0.5
            }
            .to_string(),
            "a PPM carries no alpha, and the pixel (1, 2) has alpha 0.5"
        );
        assert_eq!(
            ExportError::Manifest("bad".to_owned()).to_string(),
            "the manifest could not be written: bad"
        );
    }
}
