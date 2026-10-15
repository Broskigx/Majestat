use core::fmt;

use crate::{FrameRange, Rational, TimeError};

/// Maps timeline frames onto source frames for one clip.
///
/// The mapping is affine with a *rational* slope:
///
/// ```text
/// source_at(t) = source_start + floor((t - timeline_start) * speed)
/// ```
///
/// `speed` is exact, so `1/2` (slow motion) and `2/1` (fast motion) do not drift.
/// All division floors toward negative infinity, so behaviour is identical on both
/// sides of frame zero (see the crate docs).
///
/// ## Round-trip limits
///
/// `timeline_at(source_at(t))` returns `t` exactly when `speed` is a whole number
/// of frames per timeline frame (`1`, `2`, `3`, ...) or [`RetimeMap::is_identity`].
/// For fractional speeds the mapping is many-to-one in one direction by
/// construction, so callers should rely on **monotonicity** (property-tested) rather
/// than exact inversion. Keyframed and ramped retimes arrive in M6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetimeMap {
    source_start: i64,
    timeline_start: i64,
    speed: Rational,
}

/// Saturating conversion from the exact `i128` domain back to `i64`.
///
/// Saturation, not wrapping, keeps the result deterministic even for absurd inputs.
fn saturate(value: i128) -> i64 {
    i64::try_from(value).unwrap_or(if value < 0 { i64::MIN } else { i64::MAX })
}

impl RetimeMap {
    /// Builds a mapping.
    ///
    /// Returns [`TimeError::NonPositiveSpeed`] unless `speed > 0`.
    ///
    /// ```
    /// use maj_time::{Rational, RetimeMap};
    /// let map = RetimeMap::new(100, 0, Rational::from_integer(2)).unwrap();
    /// assert_eq!(map.source_at(0), 100);
    /// assert_eq!(map.source_at(3), 106);
    /// ```
    pub fn new(source_start: i64, timeline_start: i64, speed: Rational) -> Result<Self, TimeError> {
        if !speed.is_positive() {
            return Err(TimeError::NonPositiveSpeed {
                num: speed.num(),
                den: speed.den(),
            });
        }
        Ok(Self {
            source_start,
            timeline_start,
            speed,
        })
    }

    /// A one-to-one mapping with no offset.
    #[must_use]
    pub const fn identity() -> Self {
        Self {
            source_start: 0,
            timeline_start: 0,
            speed: Rational::from_reduced(1, 1),
        }
    }

    /// A one-to-one mapping whose timeline position starts at `timeline_start`.
    #[must_use]
    pub const fn identity_at(timeline_start: i64) -> Self {
        Self {
            source_start: timeline_start,
            timeline_start,
            speed: Rational::from_reduced(1, 1),
        }
    }

    /// First source frame addressed by this clip.
    #[must_use]
    pub const fn source_start(self) -> i64 {
        self.source_start
    }

    /// First timeline frame addressed by this clip.
    #[must_use]
    pub const fn timeline_start(self) -> i64 {
        self.timeline_start
    }

    /// Exact playback speed as source frames per timeline frame.
    #[must_use]
    pub const fn speed(self) -> Rational {
        self.speed
    }

    /// Reports whether the mapping is one-to-one and unshifted.
    #[must_use]
    pub fn is_identity(self) -> bool {
        self.speed.is_integer() && self.speed.num() == 1 && self.source_start == self.timeline_start
    }

    /// Reports whether the mapping is one-to-one in both directions.
    ///
    /// True when the speed is a whole number of frames per timeline frame.
    #[must_use]
    pub const fn is_one_to_one(self) -> bool {
        self.speed.is_integer()
    }

    /// Reports whether this mapping could have come from a valid project.
    ///
    /// Deserialising a project can produce a [`RetimeMap`] that never passed through
    /// [`RetimeMap::new`], so the load path validates with this before use.
    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.speed.is_positive()
    }

    /// Source frame shown at `timeline_frame`.
    ///
    /// Floors toward negative infinity, so the mapping is monotonic and exact at
    /// frame boundaries.
    #[must_use]
    pub fn source_at(self, timeline_frame: i64) -> i64 {
        debug_assert!(self.is_valid(), "RetimeMap used without validation");
        let delta = i128::from(timeline_frame) - i128::from(self.timeline_start);
        let offset =
            (delta * i128::from(self.speed.num())).div_euclid(i128::from(self.speed.den()));
        saturate(i128::from(self.source_start) + offset)
    }

    /// Timeline frame whose source is `source_frame`, floored.
    ///
    /// The exact inverse of [`RetimeMap::source_at`] only when
    /// [`RetimeMap::is_one_to_one`] holds.
    #[must_use]
    pub fn timeline_at(self, source_frame: i64) -> i64 {
        debug_assert!(self.is_valid(), "RetimeMap used without validation");
        let delta = i128::from(source_frame) - i128::from(self.source_start);
        let offset =
            (delta * i128::from(self.speed.den())).div_euclid(i128::from(self.speed.num()));
        saturate(i128::from(self.timeline_start) + offset)
    }

    /// The source frames needed to render a timeline range.
    ///
    /// Half-open in, half-open out. For an empty timeline range the result is an
    /// empty range positioned at the current source frame.
    #[must_use]
    pub fn source_range_for(self, timeline: FrameRange) -> FrameRange {
        let first = self.source_at(timeline.start());
        if timeline.is_empty() {
            return FrameRange::empty_at(first);
        }
        let last = self.source_at(timeline.end() - 1);
        FrameRange::from_ordered_bounds(first.min(last), first.max(last).saturating_add(1))
    }

    /// The timeline frames covered by a source range.
    ///
    /// Returns `None` when the source range is empty.
    #[must_use]
    pub fn timeline_range_for(self, source: FrameRange) -> Option<FrameRange> {
        if source.is_empty() {
            return None;
        }
        let start = self.timeline_at(source.start());
        let end = self.timeline_at(source.end() - 1).saturating_add(1);
        Some(FrameRange::from_ordered_bounds(start, end.max(start)))
    }
}

impl fmt::Display for RetimeMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "timeline {} -> source {} @ {}x",
            self.timeline_start, self.source_start, self.speed
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_round_trips_every_frame() {
        let map = RetimeMap::identity();
        assert!(map.is_identity());
        assert!(map.is_one_to_one());
        for frame in [-100, -1, 0, 1, 997, 10_000] {
            assert_eq!(map.source_at(frame), frame);
            assert_eq!(map.timeline_at(frame), frame);
        }
    }

    #[test]
    fn integer_speed_round_trips_exactly() {
        let map = RetimeMap::new(500, 100, Rational::from_integer(2)).unwrap();
        for timeline in [100, 101, 250, 1_000] {
            let source = map.source_at(timeline);
            assert_eq!(map.timeline_at(source), timeline);
        }
        assert_eq!(map.source_at(100), 500);
        assert_eq!(map.source_at(101), 502);
        assert_eq!(
            map.source_at(99),
            498,
            "the mapping must work before its start"
        );
    }

    #[test]
    fn half_speed_advances_one_source_frame_every_two_timeline_frames() {
        let map = RetimeMap::new(0, 0, Rational::new(1, 2).unwrap()).unwrap();
        assert_eq!(map.source_at(0), 0);
        assert_eq!(map.source_at(1), 0);
        assert_eq!(map.source_at(2), 1);
        assert_eq!(map.source_at(3), 1);
        assert_eq!(map.source_at(4), 2);
        assert!(!map.is_one_to_one());
    }

    #[test]
    fn ntsc_speed_stays_exact() {
        // 29.97 -> 23.976 is a real, and genuinely non-integer, retime.
        let speed = Rational::new(24_000, 30_000).unwrap();
        let map = RetimeMap::new(0, 0, speed).unwrap();
        assert_eq!(map.source_at(0), 0);
        assert_eq!(map.source_at(5), 4, "floor(5 * 0.8) = 4");
        assert_eq!(map.source_at(10), 8);
        assert_eq!(map.source_at(499), 399);
    }

    #[test]
    fn mapping_is_monotonic_everywhere_including_below_zero() {
        let map = RetimeMap::new(1_000, 200, Rational::new(3, 7).unwrap()).unwrap();
        let mut previous = map.source_at(-500);
        for timeline in -499..2_000 {
            let current = map.source_at(timeline);
            assert!(
                current >= previous,
                "monotonicity broke at {timeline}: {current} < {previous}"
            );
            previous = current;
        }
    }

    #[test]
    fn floors_toward_negative_infinity() {
        let map = RetimeMap::new(0, 0, Rational::new(1, 2).unwrap()).unwrap();
        assert_eq!(map.source_at(-1), -1, "floor(-0.5) must be -1, not 0");
        assert_eq!(map.source_at(-2), -1);
        assert_eq!(map.source_at(-3), -2);
    }

    #[test]
    fn rejects_non_positive_speeds() {
        assert!(matches!(
            RetimeMap::new(0, 0, Rational::from_integer(0)),
            Err(TimeError::NonPositiveSpeed { .. })
        ));
        assert!(matches!(
            RetimeMap::new(0, 0, Rational::from_integer(-2)),
            Err(TimeError::NonPositiveSpeed { .. })
        ));
    }

    #[test]
    fn source_range_covers_the_last_timeline_frame() {
        let map = RetimeMap::identity();
        let timeline = FrameRange::new(10, 20).unwrap();
        assert_eq!(
            map.source_range_for(timeline),
            FrameRange::new(10, 20).unwrap()
        );
    }

    #[test]
    fn source_range_of_a_slow_clip_is_shorter_than_the_timeline_range() {
        let map = RetimeMap::new(0, 0, Rational::new(1, 2).unwrap()).unwrap();
        let timeline = FrameRange::new(0, 10).unwrap();
        assert_eq!(
            map.source_range_for(timeline),
            FrameRange::new(0, 5).unwrap()
        );
    }

    #[test]
    fn empty_timeline_range_maps_to_an_empty_source_range() {
        let map = RetimeMap::new(50, 0, Rational::from_integer(2)).unwrap();
        let empty = FrameRange::new(3, 3).unwrap();
        let source = map.source_range_for(empty);
        assert!(source.is_empty());
        assert_eq!(source.start(), 56);
    }

    #[test]
    fn timeline_range_for_reports_none_for_an_empty_source() {
        let map = RetimeMap::identity();
        assert_eq!(map.timeline_range_for(FrameRange::new(5, 5).unwrap()), None);
    }

    #[test]
    fn timeline_range_for_is_the_inverse_of_source_range_for_when_one_to_one() {
        let map = RetimeMap::new(300, 40, Rational::from_integer(3)).unwrap();
        let timeline = FrameRange::new(40, 60).unwrap();
        let source = map.source_range_for(timeline);
        assert_eq!(map.timeline_range_for(source), Some(timeline));
    }

    #[test]
    fn display_describes_the_mapping() {
        let map = RetimeMap::new(100, 0, Rational::new(1, 2).unwrap()).unwrap();
        assert_eq!(map.to_string(), "timeline 0 -> source 100 @ 1/2x");
    }
}
