//! Why media could not be read: three conditions, and deliberately no fourth.

use std::fmt;

/// Why media could not be read.
///
/// The variants are the three `maj_render::SourceError` has, in the same order and with the
/// same meanings, because P0.3 settled that media which cannot be read is **one condition
/// with one policy**. A decoder is the obvious place to reopen that by accident — a fourth
/// variant here would be a fourth placeholder label, and then a frame could report
/// something the policy has no answer for.
///
/// | Variant | Placeholder label | What it means |
/// |---|---|---|
/// | [`Io`](MediaError::Io) | `offline` | The file is not where the project says it is |
/// | [`Malformed`](MediaError::Malformed) | `damaged` | The bytes are not what the container claims |
/// | [`Unsupported`](MediaError::Unsupported) | `undecodable` | No decoder in this build reads it |
///
/// What is **not** here is as deliberate as what is. Decoding is full of conditions that are
/// not failures: a frame that needs more packets, a stream that ends, a seek that landed
/// before the keyframe it needed. Those are states of a reader, handled inside layer 2, and
/// a reader that returned [`Io`](MediaError::Io) because it had not fed the decoder enough
/// packets yet would make a hazard placeholder out of a working file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaError {
    /// The file could not be opened or read.
    Io(String),
    /// The bytes are not what the container claims.
    Malformed(String),
    /// No decoder in this build reads it.
    Unsupported(String),
}

impl MediaError {
    /// The short stable name of the condition, for a manifest or a log line.
    ///
    /// These are the three `maj_render::Placeholder` labels, spelled here so a caller at
    /// layer 2 can name the condition without depending on layer 4.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Io(_) => "offline",
            Self::Malformed(_) => "damaged",
            Self::Unsupported(_) => "undecodable",
        }
    }

    /// The engine's own sentence about the condition.
    ///
    /// Kept verbatim as well as classified: `offline` is what a person needs, and the
    /// sentence is what a bug report needs.
    #[must_use]
    pub fn reason(&self) -> &str {
        match self {
            Self::Io(what) | Self::Malformed(what) | Self::Unsupported(what) => what,
        }
    }
}

impl fmt::Display for MediaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(what) => write!(formatter, "media could not be read: {what}"),
            Self::Malformed(what) => write!(formatter, "malformed media: {what}"),
            Self::Unsupported(what) => write!(formatter, "unsupported media: {what}"),
        }
    }
}

impl std::error::Error for MediaError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_labels_are_the_three_placeholder_labels() {
        assert_eq!(MediaError::Io(String::new()).label(), "offline");
        assert_eq!(MediaError::Malformed(String::new()).label(), "damaged");
        assert_eq!(
            MediaError::Unsupported(String::new()).label(),
            "undecodable"
        );
    }

    #[test]
    fn the_sentence_survives_classification() {
        let error = MediaError::Malformed("moov ends inside a box header".to_owned());
        assert_eq!(error.reason(), "moov ends inside a box header");
        assert!(error.to_string().contains("moov ends inside a box header"));
    }
}
