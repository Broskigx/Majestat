use core::fmt;

/// Errors produced by layer-0 time arithmetic.
///
/// Every fallible operation in this crate reports through this type. Nothing in
/// `maj-time` panics on bad input; callers decide how to surface a failure, which
/// keeps malformed project data from taking down a render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeError {
    /// A rational was constructed with a zero denominator.
    ZeroDenominator,
    /// A deserialised rational was not in canonical form.
    ///
    /// Canonical means reduced with a positive denominator. Rejecting anything else
    /// at the boundary stops a hand-edited project file from reaching the engine
    /// with a ratio that would compare differently from an equivalent literal.
    NonCanonical {
        /// Numerator as found in the document.
        num: i64,
        /// Denominator as found in the document.
        den: i64,
    },
    /// The exact result does not fit in the underlying 64-bit representation.
    Overflow,
    /// A frame range was constructed with `end < start`.
    InvertedRange {
        /// Requested inclusive start frame.
        start: i64,
        /// Requested exclusive end frame.
        end: i64,
    },
    /// A timebase was constructed with a rate of zero or below.
    NonPositiveTimebase,
    /// Drop-frame timecode was requested for a rate that does not define it.
    ///
    /// Only `30000/1001` and `60000/1001` have a standard drop-frame rule.
    DropFrameUnsupported {
        /// Numerator of the offending rate.
        num: i64,
        /// Denominator of the offending rate.
        den: i64,
    },
    /// A timecode field is out of range for its position.
    InvalidTimecodeField {
        /// Which field was rejected: `hours`, `minutes`, `seconds` or `frames`.
        field: &'static str,
        /// The rejected value.
        value: i64,
    },
    /// A timecode cannot represent a negative position.
    ///
    /// Negative positions are expressed with [`crate::FrameRange`], not timecode.
    NegativeFrames {
        /// The rejected frame number.
        frames: i64,
    },
    /// A retime speed was not strictly positive.
    ///
    /// Reverse playback and freeze frames are modelled in M6, not here.
    NonPositiveSpeed {
        /// Numerator of the rejected speed.
        num: i64,
        /// Denominator of the rejected speed.
        den: i64,
    },
    /// A timecode string could not be parsed.
    TimecodeParse {
        /// The offending input, trimmed.
        input: String,
    },
}

impl fmt::Display for TimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroDenominator => write!(f, "rational denominator must not be zero"),
            Self::NonCanonical { num, den } => {
                write!(f, "rational {num}/{den} is not in canonical reduced form")
            }
            Self::Overflow => write!(f, "exact time arithmetic overflowed 64 bits"),
            Self::InvertedRange { start, end } => {
                write!(f, "frame range end ({end}) precedes start ({start})")
            }
            Self::NonPositiveTimebase => write!(f, "timebase rate must be greater than zero"),
            Self::DropFrameUnsupported { num, den } => {
                write!(f, "drop-frame timecode is not defined for rate {num}/{den}")
            }
            Self::InvalidTimecodeField { field, value } => {
                write!(f, "timecode field `{field}` is out of range: {value}")
            }
            Self::NegativeFrames { frames } => {
                write!(f, "timecode cannot represent negative frame {frames}")
            }
            Self::NonPositiveSpeed { num, den } => {
                write!(f, "retime speed must be greater than zero, got {num}/{den}")
            }
            Self::TimecodeParse { input } => {
                write!(f, "could not parse `{input}` as HH:MM:SS:FF or HH:MM:SS;FF")
            }
        }
    }
}

impl core::error::Error for TimeError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_renders_a_message() {
        let cases = [
            TimeError::ZeroDenominator,
            TimeError::NonCanonical { num: 2, den: 4 },
            TimeError::Overflow,
            TimeError::InvertedRange { start: 5, end: 1 },
            TimeError::NonPositiveTimebase,
            TimeError::DropFrameUnsupported { num: 25, den: 1 },
            TimeError::InvalidTimecodeField {
                field: "frames",
                value: 40,
            },
            TimeError::NegativeFrames { frames: -1 },
            TimeError::NonPositiveSpeed { num: 0, den: 1 },
            TimeError::TimecodeParse {
                input: "nope".into(),
            },
        ];
        for case in cases {
            assert!(!case.to_string().is_empty(), "{case:?} has no message");
        }
    }
}
