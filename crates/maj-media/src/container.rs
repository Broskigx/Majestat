//! The closed vocabulary of containers.

use std::path::Path;

/// What kind of container a file is, from a closed vocabulary.
///
/// This is an enum rather than a string because of a property of the seam it feeds:
/// `maj_render::PictureResolver::kind` returns `&'static str`, so a decoder **cannot** hand
/// back a container name a library gave it. The manifest's vocabulary therefore stays
/// finite and documented, and a container nobody has mapped is [`Unknown`](ContainerKind::Unknown)
/// and labels `undecodable` rather than leaking a string out of a C library into a document
/// format.
///
/// Adding a variant is a deliberate act with a visible diff, which is the point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainerKind {
    /// QuickTime (`.mov`): ISO base media with the `qt  ` brand.
    QuickTime,
    /// ISO base media (`.mp4`, `.m4v`).
    Mp4,
    /// Material Exchange Format (`.mxf`).
    Mxf,
    /// Matroska and WebM (`.mkv`, `.webm`).
    Matroska,
    /// RIFF AVI (`.avi`).
    Avi,
    /// A numbered run of stills addressed as one stream.
    ImageSequence,
    /// RIFF WAVE (`.wav`).
    Wav,
    /// A single PNG still.
    Png,
    /// A single Netpbm still (`.ppm`, `.pnm`).
    Ppm,
    /// Readable by no decoder in this build.
    Unknown,
}

impl ContainerKind {
    /// Every container this build names, in a stable order.
    ///
    /// Pinned as a constant so a test can prove the vocabulary is closed and that every
    /// member of it has a distinct key.
    pub const ALL: [Self; 10] = [
        Self::QuickTime,
        Self::Mp4,
        Self::Mxf,
        Self::Matroska,
        Self::Avi,
        Self::ImageSequence,
        Self::Wav,
        Self::Png,
        Self::Ppm,
        Self::Unknown,
    ];

    /// The short stable name, for a render manifest.
    ///
    /// A `const fn` over a closed enum, which is what makes it safe to hand to a trait that
    /// returns `&'static str`. [`Unknown`](ContainerKind::Unknown) answers `undecodable`,
    /// which is a placeholder label rather than a container name, deliberately: a file whose
    /// container has no mapping is exactly the thing that label was minted for.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::QuickTime => "mov",
            Self::Mp4 => "mp4",
            Self::Mxf => "mxf",
            Self::Matroska => "mkv",
            Self::Avi => "avi",
            Self::ImageSequence => "sequence",
            Self::Wav => "wav",
            Self::Png => "png",
            Self::Ppm => "ppm",
            Self::Unknown => "undecodable",
        }
    }

    /// Whether this container can carry a moving picture.
    ///
    /// A `.wav` cannot, and that is not a failure: a probe of one succeeds and reports an
    /// audio stream. The distinction matters to a caller deciding whether the absence of a
    /// video stream is a pathology or the nature of the file.
    #[must_use]
    pub const fn carries_video(self) -> bool {
        match self {
            Self::QuickTime
            | Self::Mp4
            | Self::Mxf
            | Self::Matroska
            | Self::Avi
            | Self::ImageSequence
            | Self::Png
            | Self::Ppm => true,
            Self::Wav | Self::Unknown => false,
        }
    }

    /// What a file's extension *claims* it is.
    ///
    /// A claim, never a conclusion: a probe reads the bytes and may disagree, which is the
    /// whole point of the corpus's wrong-extension case. This exists so a prober can pick
    /// which parser to try first, not so it can answer without looking.
    #[must_use]
    pub fn claimed_by_extension(path: &Path) -> Self {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);
        match extension.as_deref() {
            Some("mov") => Self::QuickTime,
            Some("mp4" | "m4v") => Self::Mp4,
            Some("mxf") => Self::Mxf,
            Some("mkv" | "webm") => Self::Matroska,
            Some("avi") => Self::Avi,
            Some("wav") => Self::Wav,
            Some("png") => Self::Png,
            Some("ppm" | "pnm" | "pgm") => Self::Ppm,
            _ => Self::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_container_has_a_distinct_key() {
        let keys: BTreeSet<&str> = ContainerKind::ALL.iter().map(|kind| kind.key()).collect();
        assert_eq!(
            keys.len(),
            ContainerKind::ALL.len(),
            "two containers share a manifest key, so a manifest would be ambiguous"
        );
    }

    #[test]
    fn an_unmapped_container_labels_undecodable_rather_than_naming_itself() {
        assert_eq!(ContainerKind::Unknown.key(), "undecodable");
        assert!(!ContainerKind::Unknown.carries_video());
    }

    #[test]
    fn the_extension_is_a_claim_and_the_vocabulary_stays_closed() {
        assert_eq!(
            ContainerKind::claimed_by_extension(Path::new("a/b/shot.MOV")),
            ContainerKind::QuickTime
        );
        assert_eq!(
            ContainerKind::claimed_by_extension(Path::new("shot.braw")),
            ContainerKind::Unknown
        );
        assert_eq!(
            ContainerKind::claimed_by_extension(Path::new("shot")),
            ContainerKind::Unknown
        );
    }
}
