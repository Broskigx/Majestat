use core::{cmp::Ordering, fmt};

use crate::error::TimeError;

/// Exact rational number kept in canonical form.
///
/// Canonical form means the denominator is strictly positive, the numerator and
/// denominator are coprime, and zero is always `0/1`. Because the form is unique,
/// [`Rational`] can derive `PartialEq` and `Eq` and still compare exactly:
/// `24000/1001` and `48000/2002` are the same value and compare equal.
///
/// Arithmetic is computed in `i128` and stored back in `i64`, so intermediate
/// overflow cannot silently wrap. A result that genuinely exceeds `i64` returns
/// [`TimeError::Overflow`] instead of saturating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(try_from = "RationalRepr", into = "RationalRepr")
)]
pub struct Rational {
    num: i64,
    den: i64,
}

/// Wire form of [`Rational`].
///
/// Deserialisation goes through [`Rational::from_repr`], so a document cannot smuggle
/// in an unreduced ratio or a negative denominator.
#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RationalRepr {
    num: i64,
    den: i64,
}

/// Euclid's algorithm on unsigned magnitudes.
///
/// Magnitudes are handled as `u128` because `|i64::MIN|` and `|i128::MIN|` do not fit
/// in their own signed types. `gcd(0, x) == x`, so a zero numerator still reduces.
const fn gcd_u128(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

impl Rational {
    /// Builds a rational from a simplified ratio, with the sign on the numerator.
    ///
    /// Returns [`TimeError::ZeroDenominator`] if `den` is zero and
    /// [`TimeError::Overflow`] if negating the denominator overflows.
    ///
    /// ```
    /// use maj_time::Rational;
    /// let r = Rational::new(48_000, 2_002).unwrap();
    /// assert_eq!(r.num(), 24_000);
    /// assert_eq!(r.den(), 1_001);
    /// ```
    pub fn new(num: i64, den: i64) -> Result<Self, TimeError> {
        Self::from_i128(i128::from(num), i128::from(den))
    }

    /// Builds a whole number as a rational.
    ///
    /// ```
    /// use maj_time::Rational;
    /// assert_eq!(Rational::from_integer(24).to_f64(), 24.0);
    /// ```
    #[must_use]
    pub const fn from_integer(value: i64) -> Self {
        Self { num: value, den: 1 }
    }

    /// Builds a ratio the caller has already reduced.
    ///
    /// Use this only for `const` rate tables such as
    /// [`crate::Timebase::FPS_23_976`], where the values are written by hand and are
    /// known to be coprime with a positive denominator. Everywhere else use
    /// [`Rational::new`], which reduces for you.
    ///
    /// # Panics
    ///
    /// Panics if `den` is not strictly positive.
    #[must_use]
    pub const fn from_reduced(num: i64, den: i64) -> Self {
        assert!(den > 0, "rational denominator must be positive");
        Self { num, den }
    }

    /// Canonicalises an `i128` ratio into `i64` bounds.
    ///
    /// Reduction is performed entirely on unsigned magnitudes. Doing the division in
    /// signed space would risk an overflow panic (debug) or a silent wrap (release)
    /// for magnitudes such as `|i64::MIN|`, which no signed 64-bit type can hold.
    fn from_i128(num: i128, den: i128) -> Result<Self, TimeError> {
        if den == 0 {
            return Err(TimeError::ZeroDenominator);
        }
        let negative = (num < 0) != (den < 0);
        let numerator = num.unsigned_abs();
        let denominator = den.unsigned_abs();
        let divisor = gcd_u128(numerator, denominator);
        debug_assert!(divisor != 0, "a non-zero denominator cannot reduce to zero");
        let reduced_num = numerator / divisor;
        let reduced_den = denominator / divisor;

        let den_out = i64::try_from(reduced_den).map_err(|_| TimeError::Overflow)?;
        let num_out = if negative {
            i64::try_from(-(reduced_num as i128)).map_err(|_| TimeError::Overflow)?
        } else {
            i64::try_from(reduced_num).map_err(|_| TimeError::Overflow)?
        };
        Ok(Self {
            num: num_out,
            den: den_out,
        })
    }

    /// Numerator of the reduced ratio.
    #[must_use]
    pub const fn num(self) -> i64 {
        self.num
    }

    /// Denominator of the reduced ratio, always strictly positive.
    #[must_use]
    pub const fn den(self) -> i64 {
        self.den
    }

    /// Reports whether the value is a whole number.
    #[must_use]
    pub const fn is_integer(self) -> bool {
        self.den == 1
    }

    /// Reports whether the value is exactly zero.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.num == 0
    }

    /// Reports whether the value is strictly greater than zero.
    #[must_use]
    pub const fn is_positive(self) -> bool {
        self.num > 0
    }

    /// Reports whether the value is strictly less than zero.
    #[must_use]
    pub const fn is_negative(self) -> bool {
        self.num < 0
    }

    /// Approximates the value as `f64`.
    ///
    /// This is for display and heuristics only. Never use it to address a frame.
    #[must_use]
    pub fn to_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    /// Exact addition.
    ///
    /// `i128` intermediates are always checked: an unchecked product of two `i64`
    /// values can reach `2^126`, and their sum can leave the `i128` range entirely.
    /// A wrap here would be invisible until it corrupted a frame number.
    pub fn checked_add(self, other: Self) -> Result<Self, TimeError> {
        let left = i128::from(self.num)
            .checked_mul(i128::from(other.den))
            .ok_or(TimeError::Overflow)?;
        let right = i128::from(other.num)
            .checked_mul(i128::from(self.den))
            .ok_or(TimeError::Overflow)?;
        let num = left.checked_add(right).ok_or(TimeError::Overflow)?;
        let den = i128::from(self.den)
            .checked_mul(i128::from(other.den))
            .ok_or(TimeError::Overflow)?;
        Self::from_i128(num, den)
    }

    /// Exact subtraction.
    ///
    /// Checked for the same reason as [`Rational::checked_add`].
    pub fn checked_sub(self, other: Self) -> Result<Self, TimeError> {
        let left = i128::from(self.num)
            .checked_mul(i128::from(other.den))
            .ok_or(TimeError::Overflow)?;
        let right = i128::from(other.num)
            .checked_mul(i128::from(self.den))
            .ok_or(TimeError::Overflow)?;
        let num = left.checked_sub(right).ok_or(TimeError::Overflow)?;
        let den = i128::from(self.den)
            .checked_mul(i128::from(other.den))
            .ok_or(TimeError::Overflow)?;
        Self::from_i128(num, den)
    }

    /// Exact multiplication.
    ///
    /// Checked for the same reason as [`Rational::checked_add`].
    pub fn checked_mul(self, other: Self) -> Result<Self, TimeError> {
        let num = i128::from(self.num)
            .checked_mul(i128::from(other.num))
            .ok_or(TimeError::Overflow)?;
        let den = i128::from(self.den)
            .checked_mul(i128::from(other.den))
            .ok_or(TimeError::Overflow)?;
        Self::from_i128(num, den)
    }

    /// Exact division.
    ///
    /// Returns [`TimeError::ZeroDenominator`] when `other` is zero.
    pub fn checked_div(self, other: Self) -> Result<Self, TimeError> {
        if other.is_zero() {
            return Err(TimeError::ZeroDenominator);
        }
        let num = i128::from(self.num)
            .checked_mul(i128::from(other.den))
            .ok_or(TimeError::Overflow)?;
        let den = i128::from(self.den)
            .checked_mul(i128::from(other.num))
            .ok_or(TimeError::Overflow)?;
        Self::from_i128(num, den)
    }

    /// Exact reciprocal.
    ///
    /// Returns [`TimeError::ZeroDenominator`] for zero.
    pub fn checked_recip(self) -> Result<Self, TimeError> {
        let one = Self::from_integer(1);
        one.checked_div(self)
    }

    /// Floors the value to a whole number, toward negative infinity.
    ///
    /// Floor, not truncation: `-3/2` floors to `-2`. See the crate docs on
    /// division semantics.
    pub fn floor(self) -> Result<i64, TimeError> {
        if self.den == 1 {
            return Ok(self.num);
        }
        i64::try_from(i128::from(self.num).div_euclid(i128::from(self.den)))
            .map_err(|_| TimeError::Overflow)
    }

    /// Reports whether the value lies strictly between zero and one.
    #[must_use]
    pub const fn is_proper_fraction(self) -> bool {
        self.num != 0 && self.num.unsigned_abs() < self.den.unsigned_abs()
    }

    /// Rebuilds a rational from a stored numerator and denominator, enforcing
    /// canonical form.
    ///
    /// ```
    /// use maj_time::{Rational, TimeError};
    /// assert_eq!(
    ///     Rational::from_repr(2, 4),
    ///     Err(TimeError::NonCanonical { num: 2, den: 4 }),
    /// );
    /// assert_eq!(Rational::from_repr(1, 2).unwrap().den(), 2);
    /// ```
    pub fn from_repr(num: i64, den: i64) -> Result<Self, TimeError> {
        let rebuilt = Self::new(num, den)?;
        if rebuilt.num != num || rebuilt.den != den {
            return Err(TimeError::NonCanonical { num, den });
        }
        Ok(rebuilt)
    }
}

#[cfg(feature = "serde")]
impl From<Rational> for RationalRepr {
    fn from(value: Rational) -> Self {
        Self {
            num: value.num,
            den: value.den,
        }
    }
}

#[cfg(feature = "serde")]
impl TryFrom<RationalRepr> for Rational {
    type Error = TimeError;

    fn try_from(value: RationalRepr) -> Result<Self, Self::Error> {
        Self::from_repr(value.num, value.den)
    }
}

impl Ord for Rational {
    fn cmp(&self, other: &Self) -> Ordering {
        // Products of two i64 magnitudes stay inside i128 (at most 2^126), so this
        // comparison is exact without any float involvement.
        let lhs = i128::from(self.num) * i128::from(other.den);
        let rhs = i128::from(other.num) * i128::from(self.den);
        lhs.cmp(&rhs)
    }
}

impl PartialOrd for Rational {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == 1 {
            write!(f, "{}", self.num)
        } else {
            write!(f, "{}/{}", self.num, self.den)
        }
    }
}

impl TryFrom<(i64, i64)> for Rational {
    type Error = TimeError;

    fn try_from(value: (i64, i64)) -> Result<Self, Self::Error> {
        Self::new(value.0, value.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduces_to_canonical_form() {
        let r = Rational::new(48_000, 2_002).unwrap();
        assert_eq!((r.num(), r.den()), (24_000, 1_001));
    }

    #[test]
    fn moves_the_sign_onto_the_numerator() {
        let r = Rational::new(1, -2).unwrap();
        assert_eq!((r.num(), r.den()), (-1, 2));
    }

    #[test]
    fn zero_is_always_one_over_one() {
        let r = Rational::new(0, -7).unwrap();
        assert_eq!((r.num(), r.den()), (0, 1));
        assert!(r.is_zero());
    }

    #[test]
    fn rejects_a_zero_denominator() {
        assert_eq!(Rational::new(3, 0), Err(TimeError::ZeroDenominator));
    }

    #[test]
    fn equal_values_compare_equal_after_reduction() {
        assert_eq!(
            Rational::new(24_000, 1_001).unwrap(),
            Rational::new(48_000, 2_002).unwrap()
        );
    }

    #[test]
    fn reports_overflow_instead_of_wrapping() {
        let big = Rational::from_integer(i64::MAX);
        assert_eq!(
            big.checked_add(Rational::from_integer(1)),
            Err(TimeError::Overflow)
        );
        assert_eq!(big.checked_mul(big), Err(TimeError::Overflow));
    }

    #[test]
    fn floors_toward_negative_infinity() {
        let neg = Rational::new(-3, 2).unwrap();
        assert_eq!(neg.floor().unwrap(), -2, "floor(-1.5) must be -2, not -1");
        let pos = Rational::new(3, 2).unwrap();
        assert_eq!(pos.floor().unwrap(), 1);
        assert_eq!(Rational::new(-4, 2).unwrap().floor().unwrap(), -2);
    }

    #[test]
    fn division_by_zero_is_reported() {
        let one = Rational::from_integer(1);
        assert_eq!(
            one.checked_div(Rational::from_integer(0)),
            Err(TimeError::ZeroDenominator)
        );
        assert_eq!(
            Rational::from_integer(0).checked_recip(),
            Err(TimeError::ZeroDenominator)
        );
    }

    #[test]
    fn ordering_is_exact_not_float_based() {
        let a = Rational::new(1, 3).unwrap();
        let b = Rational::new(333_333_333, 1_000_000_000).unwrap();
        assert!(a > b, "1/3 must exceed 0.333333333 exactly");
    }

    #[test]
    fn display_shows_whole_numbers_bare() {
        assert_eq!(Rational::from_integer(24).to_string(), "24");
        assert_eq!(
            Rational::new(24_000, 1_001).unwrap().to_string(),
            "24000/1001"
        );
    }
}
