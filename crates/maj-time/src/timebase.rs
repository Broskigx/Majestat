use core::fmt;

use crate::{Rational, TimeError};

/// Frames per second expressed exactly, plus the drop-frame capability of a rate.
///
/// A timebase is the pair "how many frames per second" and "which timecode
/// conventions that rate supports". Rates that are not whole numbers are stored as
/// their exact ratio, so `29.97` is `30000/1001` and never a rounded decimal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "Rational", into = "Rational"))]
pub struct Timebase(Rational);

impl Timebase {
    /// 23.976 fps — `24000/1001`. The standard film rate for NTSC delivery.
    pub const FPS_23_976: Self = Self(Rational::from_reduced(24_000, 1_001));
    /// 24 fps — `24/1`. Theatrical film rate.
    pub const FPS_24: Self = Self(Rational::from_reduced(24, 1));
    /// 25 fps — `25/1`. PAL and 50 Hz regions.
    pub const FPS_25: Self = Self(Rational::from_reduced(25, 1));
    /// 29.97 fps — `30000/1001`. NTSC. Supports drop-frame timecode.
    pub const FPS_29_97: Self = Self(Rational::from_reduced(30_000, 1_001));
    /// 30 fps — `30/1`.
    pub const FPS_30: Self = Self(Rational::from_reduced(30, 1));
    /// 50 fps — `50/1`.
    pub const FPS_50: Self = Self(Rational::from_reduced(50, 1));
    /// 59.94 fps — `60000/1001`. Supports drop-frame timecode.
    pub const FPS_59_94: Self = Self(Rational::from_reduced(60_000, 1_001));
    /// 60 fps — `60/1`.
    pub const FPS_60: Self = Self(Rational::from_reduced(60, 1));

    /// Builds a timebase from an exact rate.
    ///
    /// Returns [`TimeError::NonPositiveTimebase`] for a rate of zero or below.
    ///
    /// ```
    /// use maj_time::{Rational, Timebase};
    /// let tb = Timebase::new(Rational::new(30_000, 1_001).unwrap()).unwrap();
    /// assert_eq!(tb.nominal_fps(), 30);
    /// assert_eq!(tb.drop_frame_frames(), Some(2));
    /// ```
    pub fn new(rate: Rational) -> Result<Self, TimeError> {
        if !rate.is_positive() {
            return Err(TimeError::NonPositiveTimebase);
        }
        Ok(Self(rate))
    }

    /// The exact rate as a rational.
    #[must_use]
    pub const fn rate(self) -> Rational {
        self.0
    }

    /// Numerator of the exact rate.
    #[must_use]
    pub const fn numerator(self) -> i64 {
        self.0.num()
    }

    /// Denominator of the exact rate.
    #[must_use]
    pub const fn denominator(self) -> i64 {
        self.0.den()
    }

    /// Nearest whole number of frames per second, rounded half up.
    ///
    /// This is the divisor used to split a frame count into
    /// hours, minutes, seconds and frames. `30000/1001` yields `30`.
    #[must_use]
    pub fn nominal_fps(self) -> u32 {
        let num = self.0.num();
        let den = self.0.den();
        let rounded = (num + den / 2) / den;
        u32::try_from(rounded).unwrap_or(u32::MAX)
    }

    /// Number of timecode frames skipped each minute when drop-frame is used.
    ///
    /// Returns `Some(2)` for `30000/1001`, `Some(4)` for `60000/1001`, and `None`
    /// for every other rate — those rates only have non-drop timecode.
    #[must_use]
    pub const fn drop_frame_frames(self) -> Option<u32> {
        match (self.0.num(), self.0.den()) {
            (30_000, 1_001) => Some(2),
            (60_000, 1_001) => Some(4),
            _ => None,
        }
    }

    /// Reports whether this rate is an integer number of frames per second.
    #[must_use]
    pub const fn is_integer_rate(self) -> bool {
        self.0.is_integer()
    }
}

impl fmt::Display for Timebase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{} fps", self.0.num(), self.0.den())
    }
}

impl TryFrom<Rational> for Timebase {
    type Error = TimeError;

    fn try_from(value: Rational) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Timebase> for Rational {
    fn from(value: Timebase) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_rates_round_to_their_nominal_fps() {
        assert_eq!(Timebase::FPS_23_976.nominal_fps(), 24);
        assert_eq!(Timebase::FPS_29_97.nominal_fps(), 30);
        assert_eq!(Timebase::FPS_59_94.nominal_fps(), 60);
        assert_eq!(Timebase::FPS_25.nominal_fps(), 25);
        assert_eq!(Timebase::FPS_50.nominal_fps(), 50);
    }

    #[test]
    fn only_ntsc_rates_support_drop_frame() {
        assert_eq!(Timebase::FPS_29_97.drop_frame_frames(), Some(2));
        assert_eq!(Timebase::FPS_59_94.drop_frame_frames(), Some(4));
        assert_eq!(Timebase::FPS_25.drop_frame_frames(), None);
        assert_eq!(Timebase::FPS_24.drop_frame_frames(), None);
        assert_eq!(Timebase::FPS_30.drop_frame_frames(), None);
    }

    #[test]
    fn rejects_a_non_positive_rate() {
        let zero = Timebase::new(Rational::from_integer(0));
        assert_eq!(zero, Err(TimeError::NonPositiveTimebase));
        let negative = Timebase::new(Rational::from_integer(-25));
        assert_eq!(negative, Err(TimeError::NonPositiveTimebase));
    }

    #[test]
    fn accepts_a_non_standard_rate() {
        let tb = Timebase::new(Rational::new(12_500, 1_000).unwrap()).unwrap();
        assert_eq!(tb.nominal_fps(), 13);
        assert_eq!(tb.drop_frame_frames(), None);
        assert!(!tb.is_integer_rate());
    }

    #[test]
    fn display_names_the_exact_rate() {
        assert_eq!(Timebase::FPS_23_976.to_string(), "24000/1001 fps");
        assert_eq!(Timebase::FPS_25.to_string(), "25/1 fps");
    }
}
