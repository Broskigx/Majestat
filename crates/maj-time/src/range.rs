use core::fmt;

use crate::TimeError;

/// A half-open interval of frames, `[start, end)`.
///
/// Half-open is the only representation that composes without off-by-one bugs: two
/// adjacent ranges `[0, 10)` and `[10, 20)` tile `[0, 20)` exactly and share no
/// frame. Every clip trim, transition and export window in Majestat is a
/// `FrameRange`.
///
/// `start` may be negative (timelines are addressable before frame zero), but `end`
/// is never less than `start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "RangeRepr", into = "RangeRepr"))]
pub struct FrameRange {
    start: i64,
    end: i64,
}

/// Wire form of [`FrameRange`].
///
/// Deserialisation re-runs the ordering check, so `end < start` can never be loaded.
#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RangeRepr {
    start: i64,
    end: i64,
}

impl FrameRange {
    /// Builds `[start, end)`.
    ///
    /// Returns [`TimeError::InvertedRange`] when `end < start`. `end == start` is
    /// allowed and produces an empty range.
    ///
    /// ```
    /// use maj_time::FrameRange;
    /// let r = FrameRange::new(0, 100).unwrap();
    /// assert_eq!(r.len(), 100);
    /// assert!(r.contains(99));
    /// assert!(!r.contains(100), "the end frame is exclusive");
    /// ```
    pub fn new(start: i64, end: i64) -> Result<Self, TimeError> {
        if end < start {
            return Err(TimeError::InvertedRange { start, end });
        }
        Ok(Self { start, end })
    }

    /// Builds `[start, start + len)`.
    ///
    /// Returns [`TimeError::InvertedRange`] when `len` is negative and
    /// [`TimeError::Overflow`] when the sum exceeds `i64`.
    pub fn from_start_len(start: i64, len: i64) -> Result<Self, TimeError> {
        let end = start.checked_add(len).ok_or(TimeError::Overflow)?;
        Self::new(start, end)
    }

    /// An empty range positioned at `frame`.
    #[must_use]
    pub const fn empty_at(frame: i64) -> Self {
        Self {
            start: frame,
            end: frame,
        }
    }

    /// Builds a range from bounds the caller has already ordered.
    ///
    /// Crate-internal: used by retiming, where the bounds come from a monotonic
    /// mapping rather than from user input.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if `end < start`.
    #[must_use]
    pub(crate) const fn from_ordered_bounds(start: i64, end: i64) -> Self {
        debug_assert!(end >= start, "bounds must be ordered");
        Self { start, end }
    }

    /// The inclusive start frame.
    #[must_use]
    pub const fn start(self) -> i64 {
        self.start
    }

    /// The exclusive end frame.
    #[must_use]
    pub const fn end(self) -> i64 {
        self.end
    }

    /// Number of frames covered.
    #[must_use]
    pub const fn len(self) -> i64 {
        self.end - self.start
    }

    /// Reports whether the range covers no frames.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.end == self.start
    }

    /// Reports whether `frame` is inside the range.
    #[must_use]
    pub const fn contains(self, frame: i64) -> bool {
        frame >= self.start && frame < self.end
    }

    /// Reports whether `other` lies entirely inside this range.
    #[must_use]
    pub const fn contains_range(self, other: Self) -> bool {
        other.start >= self.start && other.end <= self.end
    }

    /// Reports whether the two ranges share at least one frame.
    ///
    /// Empty ranges never intersect anything, which keeps this consistent with
    /// [`FrameRange::intersection`] returning `None`.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        !self.is_empty() && !other.is_empty() && self.start < other.end && other.start < self.end
    }

    /// Reports whether `other` starts exactly where this range ends, or vice versa.
    #[must_use]
    pub const fn is_adjacent(self, other: Self) -> bool {
        self.end == other.start || other.end == self.start
    }

    /// The shared frames, if any.
    #[must_use]
    pub fn intersection(self, other: Self) -> Option<Self> {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);
        if end <= start {
            None
        } else {
            Some(Self { start, end })
        }
    }

    /// The smallest range containing both inputs.
    ///
    /// This is a hull, not a set union: the gap between two disjoint ranges is
    /// included.
    #[must_use]
    pub const fn union_hull(self, other: Self) -> Self {
        Self {
            start: if self.start < other.start {
                self.start
            } else {
                other.start
            },
            end: if self.end > other.end {
                self.end
            } else {
                other.end
            },
        }
    }

    /// Moves the range by `delta` frames.
    pub fn shifted(self, delta: i64) -> Result<Self, TimeError> {
        let start = self.start.checked_add(delta).ok_or(TimeError::Overflow)?;
        let end = self.end.checked_add(delta).ok_or(TimeError::Overflow)?;
        Ok(Self { start, end })
    }

    /// Clamps a frame into `[start, end]`, saturating rather than wrapping.
    #[must_use]
    pub const fn clamp(self, frame: i64) -> i64 {
        if frame < self.start {
            self.start
        } else if frame > self.end {
            self.end
        } else {
            frame
        }
    }
}

impl fmt::Display for FrameRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}, {})", self.start, self.end)
    }
}

#[cfg(feature = "serde")]
impl From<FrameRange> for RangeRepr {
    fn from(value: FrameRange) -> Self {
        Self {
            start: value.start,
            end: value.end,
        }
    }
}

#[cfg(feature = "serde")]
impl TryFrom<RangeRepr> for FrameRange {
    type Error = TimeError;

    fn try_from(value: RangeRepr) -> Result<Self, Self::Error> {
        Self::new(value.start, value.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_is_end_minus_start() {
        let r = FrameRange::new(10, 25).unwrap();
        assert_eq!(r.len(), 15);
        assert!(!r.is_empty());
    }

    #[test]
    fn empty_ranges_are_allowed_at_any_position() {
        let r = FrameRange::new(7, 7).unwrap();
        assert!(r.is_empty());
        assert_eq!(r.len(), 0);
        assert!(!r.contains(7));
    }

    #[test]
    fn rejects_an_inverted_range() {
        assert_eq!(
            FrameRange::new(5, 4),
            Err(TimeError::InvertedRange { start: 5, end: 4 })
        );
    }

    #[test]
    fn adjacent_ranges_tile_without_overlap() {
        let a = FrameRange::new(0, 10).unwrap();
        let b = FrameRange::new(10, 20).unwrap();
        assert!(a.is_adjacent(b));
        assert!(!a.intersects(b));
        assert_eq!(a.intersection(b), None);
        assert_eq!(a.union_hull(b), FrameRange::new(0, 20).unwrap());
        assert_eq!(a.len() + b.len(), a.union_hull(b).len());
    }

    #[test]
    fn intersects_agrees_with_intersection() {
        let empty = FrameRange::new(5, 5).unwrap();
        let span = FrameRange::new(0, 10).unwrap();
        assert!(!empty.intersects(span), "an empty range shares no frame");
        assert!(!span.intersects(empty));
        assert_eq!(empty.intersection(span), None);

        let cases = [
            (
                FrameRange::new(0, 10).unwrap(),
                FrameRange::new(10, 20).unwrap(),
            ),
            (
                FrameRange::new(0, 10).unwrap(),
                FrameRange::new(9, 20).unwrap(),
            ),
            (
                FrameRange::new(0, 10).unwrap(),
                FrameRange::new(0, 10).unwrap(),
            ),
            (
                FrameRange::new(-5, 5).unwrap(),
                FrameRange::new(4, 9).unwrap(),
            ),
        ];
        for (a, b) in cases {
            assert_eq!(
                a.intersects(b),
                a.intersection(b).is_some(),
                "intersects and intersection disagree for {a} and {b}"
            );
        }
    }

    #[test]
    fn one_shared_frame_counts_as_intersecting() {
        let a = FrameRange::new(0, 10).unwrap();
        let b = FrameRange::new(9, 20).unwrap();
        assert!(a.intersects(b));
        assert_eq!(a.intersection(b), Some(FrameRange::new(9, 10).unwrap()));
    }

    #[test]
    fn intersection_is_commutative() {
        let a = FrameRange::new(0, 10).unwrap();
        let b = FrameRange::new(4, 20).unwrap();
        assert_eq!(a.intersection(b), b.intersection(a));
    }

    #[test]
    fn shift_preserves_length_and_reports_overflow() {
        let r = FrameRange::new(0, 10).unwrap();
        assert_eq!(r.shifted(-5).unwrap(), FrameRange::new(-5, 5).unwrap());
        let extreme = FrameRange::new(i64::MAX - 1, i64::MAX).unwrap();
        assert_eq!(extreme.shifted(1), Err(TimeError::Overflow));
    }

    #[test]
    fn negative_start_frames_are_addressable() {
        let r = FrameRange::new(-100, -50).unwrap();
        assert_eq!(r.len(), 50);
        assert!(r.contains(-100));
        assert!(r.contains(-51));
        assert!(!r.contains(-50));
    }

    #[test]
    fn from_start_len_rejects_a_negative_length() {
        assert_eq!(
            FrameRange::from_start_len(10, -1),
            Err(TimeError::InvertedRange { start: 10, end: 9 })
        );
    }

    #[test]
    fn clamp_saturates_at_both_bounds() {
        let r = FrameRange::new(0, 10).unwrap();
        assert_eq!(r.clamp(-1), 0);
        assert_eq!(r.clamp(10), 10);
        assert_eq!(r.clamp(4), 4);
    }
}
