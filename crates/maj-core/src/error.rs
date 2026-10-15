use core::fmt;

use maj_time::TimeError;

/// Errors produced by the project model, the command log and the `.maj` format.
///
/// Nothing in `maj-core` panics on malformed input. A corrupt or hand-edited project
/// document must fail loudly at the boundary instead of reaching the engine with a
/// broken invariant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// An entity referenced by a command or a document does not exist.
    NotFound {
        /// Entity kind, such as `asset` or `clip`.
        kind: &'static str,
        /// Raw identifier that could not be resolved.
        id: u64,
    },
    /// An entity was created with an identifier that is already in use.
    Duplicate {
        /// Entity kind.
        kind: &'static str,
        /// Raw identifier that collided.
        id: u64,
    },
    /// A positional insertion referred to a slot outside the container.
    IndexOutOfRange {
        /// Requested index.
        index: usize,
        /// Current length of the container.
        len: usize,
    },
    /// A command or constructor was given a value that violates a model rule.
    InvalidArgument {
        /// Explanation of the violated rule.
        reason: String,
    },
    /// The document was written by a schema version this build cannot read.
    SchemaUnsupported {
        /// Version found in the document.
        found: u32,
        /// Version this build writes.
        supported: u32,
    },
    /// The document carries no `schema_version` field at all.
    SchemaMissing,
    /// The document is not a Majestat project.
    MagicMismatch {
        /// Magic string found in the document.
        found: String,
    },
    /// Layer-0 time arithmetic rejected a value in the document.
    Time(TimeError),
    /// The JSON document could not be parsed or produced.
    Json(String),
    /// The project file could not be read or written.
    Io(String),
}

impl CoreError {
    /// Builds a [`CoreError::NotFound`].
    #[must_use]
    pub fn not_found(kind: &'static str, id: u64) -> Self {
        Self::NotFound { kind, id }
    }

    /// Builds a [`CoreError::Duplicate`].
    #[must_use]
    pub fn duplicate(kind: &'static str, id: u64) -> Self {
        Self::Duplicate { kind, id }
    }

    /// Builds a [`CoreError::IndexOutOfRange`].
    #[must_use]
    pub fn index_out_of_range(index: usize, len: usize) -> Self {
        Self::IndexOutOfRange { index, len }
    }

    /// Builds a [`CoreError::InvalidArgument`] from anything string-like.
    #[must_use]
    pub fn invalid(reason: impl Into<String>) -> Self {
        Self::InvalidArgument {
            reason: reason.into(),
        }
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { kind, id } => write!(f, "no {kind} with identifier {id}"),
            Self::Duplicate { kind, id } => write!(f, "{kind} identifier {id} is already in use"),
            Self::IndexOutOfRange { index, len } => {
                write!(f, "index {index} is outside a container of length {len}")
            }
            Self::InvalidArgument { reason } => write!(f, "invalid argument: {reason}"),
            Self::SchemaUnsupported { found, supported } => write!(
                f,
                "project schema version {found} is newer than the supported version {supported}"
            ),
            Self::SchemaMissing => write!(f, "project document has no schema_version"),
            Self::MagicMismatch { found } => {
                write!(f, "`{found}` is not a Majestat project")
            }
            Self::Time(err) => write!(f, "time: {err}"),
            Self::Json(err) => write!(f, "json: {err}"),
            Self::Io(err) => write!(f, "io: {err}"),
        }
    }
}

impl core::error::Error for CoreError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Time(err) => Some(err),
            _ => None,
        }
    }
}

impl From<TimeError> for CoreError {
    fn from(value: TimeError) -> Self {
        Self::Time(value)
    }
}

impl From<serde_json::Error> for CoreError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value.to_string())
    }
}

impl From<std::io::Error> for CoreError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_renders_a_message_and_a_kind() {
        let cases = [
            CoreError::not_found("clip", 7),
            CoreError::duplicate("asset", 2),
            CoreError::index_out_of_range(9, 3),
            CoreError::invalid("speed must be positive"),
            CoreError::SchemaUnsupported {
                found: 99,
                supported: 1,
            },
            CoreError::SchemaMissing,
            CoreError::MagicMismatch {
                found: "OTHER".into(),
            },
            CoreError::Time(TimeError::Overflow),
            CoreError::Json("bad".into()),
            CoreError::Io("gone".into()),
        ];
        for case in cases {
            assert!(!case.to_string().is_empty(), "{case:?} has no message");
        }
    }

    #[test]
    fn time_errors_are_exposed_as_the_source() {
        let err: CoreError = TimeError::Overflow.into();
        assert!(core::error::Error::source(&err).is_some());
        assert!(core::error::Error::source(&CoreError::SchemaMissing).is_none());
    }
}
