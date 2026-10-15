//! Deterministic transcendental functions.
//!
//! IEEE-754 requires addition, subtraction, multiplication, division and square root to
//! be correctly rounded, so those are already bit-identical on every platform. `exp`,
//! `ln`, `pow`, `sin` and `cos` are **not**: they are library functions, and different
//! libm implementations differ in the last bits. A colour pipeline that calls `powf`
//! therefore cannot promise the same bytes on two machines.
//!
//! Everything here is built from multiplication, division and addition only, so the
//! certified path (ADR 0006) has nothing left to disagree about. Accuracy is a few
//! units in the last place, far below the 1/65535 quantisation of a 16-bit
//! intermediate. The tests compare against the platform's libm, which is the one place
//! libm is allowed: as a reference to be checked against, never as part of the
//! certified computation.

// The standard constants are taken from the language rather than typed out here. A platform
// gets *functions* wrong, not doubles: these are correctly rounded literals in `core`, the
// same bits on every machine, and taking them from there removes the chance of a transposed
// digit in a hand-copied approximation of pi. The split `pi / 2` further down is a different
// matter, because no library publishes it in two words.
pub use std::f64::consts::{FRAC_PI_2, LN_2, PI, TAU};

/// Square root. IEEE-754 requires correct rounding, so the hardware instruction is
/// already portable and needs no replacement kernel.
#[must_use]
pub fn sqrt(value: f64) -> f64 {
    value.sqrt()
}

/// Raises to a non-negative integer power by repeated squaring.
#[must_use]
pub fn powi(mut base: f64, mut exponent: u64) -> f64 {
    let mut result = 1.0_f64;
    while exponent > 0 {
        if exponent & 1 == 1 {
            result *= base;
        }
        base *= base;
        exponent >>= 1;
    }
    result
}

/// Multiplies by two raised to `exponent`, by repeated scaling.
///
/// Repeated scaling rather than bit manipulation keeps the function independent of the
/// floating-point layout. Exponents in a colour pipeline are small.
#[must_use]
pub fn scale_by_pow2(mut value: f64, mut exponent: i64) -> f64 {
    while exponent > 0 {
        value *= 2.0;
        exponent -= 1;
    }
    while exponent < 0 {
        value *= 0.5;
        exponent += 1;
    }
    value
}

/// Natural exponential.
///
/// The argument is reduced to `k * ln 2 + r` with `|r| <= ln 2 / 2`, the series is
/// evaluated on `r`, and the result is scaled by `2^k`.
#[must_use]
pub fn exp(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x > 709.0 {
        return f64::INFINITY;
    }
    if x < -745.0 {
        return 0.0;
    }
    let k = (x / LN_2).round();
    let r = x - k * LN_2;
    let mut term = 1.0_f64;
    let mut sum = 1.0_f64;
    let mut n = 1_u32;
    while n <= 14 {
        term *= r / f64::from(n);
        sum += term;
        n += 1;
    }
    scale_by_pow2(sum, k as i64)
}

/// Natural logarithm.
///
/// The argument is split into `m * 2^e` with `m` in `[1, 2)`, and `ln m` is evaluated
/// with the `atanh` series, which converges geometrically in that range.
#[must_use]
pub fn ln(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x.is_infinite() {
        return f64::INFINITY;
    }
    let mut exponent: i64 = 0;
    let mut mantissa = x;
    while mantissa >= 2.0 {
        mantissa *= 0.5;
        exponent += 1;
    }
    while mantissa < 1.0 {
        mantissa *= 2.0;
        exponent -= 1;
    }
    // ln(m) = 2 * atanh(z) with z = (m - 1) / (m + 1), and |z| <= 1/3.
    let z = (mantissa - 1.0) / (mantissa + 1.0);
    let z_squared = z * z;
    let mut term = z;
    let mut sum = 0.0_f64;
    let mut n = 0_u32;
    while n <= 12 {
        sum += term / f64::from(2 * n + 1);
        term *= z_squared;
        n += 1;
    }
    2.0 * sum + (exponent as f64) * LN_2
}

/// The `n`-th root of a value.
///
/// The argument is split into `m * 2^e` with `m` in `[1, 2)`, and the answer is assembled
/// from `m^(1/n)` and a power of two. Working on a mantissa in `[1, 2)` keeps every
/// intermediate bounded: the naive form of this decomposition needs `2^r` for the
/// remainder of the exponent division, which overflows a double as soon as `n` passes
/// 1024 — and the PQ curve asks for 1305, 2523 and 8192. The power-of-two factor is
/// therefore evaluated from its logarithm.
///
/// `m^(1/n)` starts at `exp(ln(m) / n)` and is then pushed to the fixed point of the
/// Newton map. The starting point only has to be close; the fixed point is what fixes the
/// answer, and it is a deterministic function of `m` and `n` because every step is a
/// multiplication, a division or an addition.
#[must_use]
pub fn nth_root(value: f64, n: u64) -> f64 {
    if n == 0 {
        return f64::NAN;
    }
    if n == 1 {
        return value;
    }
    if value == 0.0 {
        return 0.0;
    }
    if value < 0.0 {
        // A real root exists only for an odd order.
        if n % 2 == 0 {
            return f64::NAN;
        }
        return -nth_root(-value, n);
    }
    if value.is_infinite() {
        return f64::INFINITY;
    }
    if n == 2 {
        return sqrt(value);
    }

    // value = mantissa * 2^exponent, with the mantissa in [1, 2).
    let mut mantissa = value;
    let mut exponent: i64 = 0;
    while mantissa >= 2.0 {
        mantissa *= 0.5;
        exponent += 1;
    }
    while mantissa < 1.0 {
        mantissa *= 2.0;
        exponent -= 1;
    }

    // exponent / n = quotient + remainder / n, so 2^exponent = 2^quotient * 2^(remainder / n).
    let order = n as f64;
    let quotient = exponent.div_euclid(n as i64);
    let remainder = exponent.rem_euclid(n as i64) as f64;

    let mut estimate = exp(ln(mantissa) / order);
    for _ in 0..8 {
        let denominator = powi(estimate, n - 1);
        if denominator == 0.0 || !denominator.is_finite() {
            break;
        }
        // y_{k+1} = ((n - 1) * y_k + m / y_k^(n - 1)) / n
        let next = ((order - 1.0) * estimate + mantissa / denominator) / order;
        if next == estimate {
            break;
        }
        estimate = next;
    }

    // The scaling, which stays finite because its exponent is bounded by one.
    estimate * exp((quotient as f64 + remainder / order) * LN_2)
}

/// Raises a value to an exact rational power `num / den`.
///
/// The `den`-th root is refined to its Newton fixed point, then raised to `num` by
/// repeated squaring. The alternative, `exp(num/den * ln(x))`, applies both transcendental
/// functions to a large argument and multiplies their errors by `num/den`, which matters
/// because this function dominates every transfer function in a grade.
#[must_use]
pub fn pow_rational(base: f64, num: i64, den: i64) -> f64 {
    if den == 0 {
        return f64::NAN;
    }
    let (num, den) = if den < 0 { (-num, -den) } else { (num, den) };
    if base < 0.0 {
        // A negative base is real only for an odd numerator and an odd denominator.
        if den % 2 == 1 && num % 2 == 1 {
            return -pow_rational(-base, num, den);
        }
        return f64::NAN;
    }
    if num == 0 {
        return 1.0;
    }
    if base == 0.0 {
        return if num > 0 { 0.0 } else { f64::INFINITY };
    }
    if base.is_infinite() {
        return if num > 0 { f64::INFINITY } else { 0.0 };
    }

    let negative_exponent = num < 0;
    let power = num.unsigned_abs();
    let root = if den == 1 {
        base
    } else {
        nth_root(base, den as u64)
    };
    let magnitude = if power == 1 { root } else { powi(root, power) };
    if negative_exponent {
        1.0 / magnitude
    } else {
        magnitude
    }
}

/// `pi / 2`, with the low 33 bits of its significand cleared.
///
/// Clearing those bits is what makes `quadrant * PIO2_HI` exact: the product then needs at
/// most 20 + 20 significand bits, which a double holds without rounding, so the leading
/// cancellation below costs nothing.
const PIO2_HI: f64 = f64::from_bits(0x3FF9_21FB_5440_0000);

/// `pi / 2 - PIO2_HI`, to the full width of a double.
///
/// The pair carries `pi / 2` to about 86 bits. That is what keeps the reduction accurate
/// when the remainder is far smaller than the argument, which is the case that decides
/// whether a rotated layer lands on the right pixel. A single-word `pi / 2` is wrong there
/// by `quadrant` times its own last bit, and the error grows with the angle.
const PIO2_LO: f64 = 6.077_100_506_506_192e-11;

/// `2 / pi`, used only to pick which quadrant `x` falls in.
const TWO_OVER_PI: f64 = std::f64::consts::FRAC_2_PI;

/// Reduces an angle to a quadrant index and a remainder in `[-pi/4, pi/4]`.
///
/// This is Cody-Waite reduction: `quadrant * pi / 2` is subtracted one word of the constant
/// at a time, so the leading bits cancel exactly and the remainder keeps its own precision.
/// Any integer quadrant gives a consistent answer, so choosing the neighbouring quadrant on
/// a boundary costs nothing — the index moves by one and the remainder by `pi / 2`.
fn reduce_quadrant(x: f64) -> (i64, f64) {
    let quadrant = (x * TWO_OVER_PI).round();
    let remainder = (x - quadrant * PIO2_HI) - quadrant * PIO2_LO;
    (quadrant as i64, remainder)
}

/// Taylor series for sine, terminated after the `r^17` term.
///
/// Valid for `|r| <= pi/4`, where the first omitted term is below `1e-16`.
fn sin_series(r: f64) -> f64 {
    let r_squared = r * r;
    let mut term = r;
    let mut sum = r;
    let mut index = 0_u32;
    while index < 8 {
        term *= -r_squared / f64::from((2 * index + 2) * (2 * index + 3));
        sum += term;
        index += 1;
    }
    sum
}

/// Taylor series for cosine, terminated after the `r^16` term.
fn cos_series(r: f64) -> f64 {
    let r_squared = r * r;
    let mut term = 1.0_f64;
    let mut sum = 1.0_f64;
    let mut index = 0_u32;
    while index < 8 {
        term *= -r_squared / f64::from((2 * index + 1) * (2 * index + 2));
        sum += term;
        index += 1;
    }
    sum
}

/// Sine of an angle in radians.
#[must_use]
pub fn sin(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    let (quadrant, remainder) = reduce_quadrant(x);
    match quadrant.rem_euclid(4) {
        0 => sin_series(remainder),
        1 => cos_series(remainder),
        2 => -sin_series(remainder),
        _ => -cos_series(remainder),
    }
}

/// Cosine of an angle in radians.
#[must_use]
pub fn cos(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    let (quadrant, remainder) = reduce_quadrant(x);
    match quadrant.rem_euclid(4) {
        0 => cos_series(remainder),
        1 => -sin_series(remainder),
        2 => -cos_series(remainder),
        _ => sin_series(remainder),
    }
}

/// Sine and cosine together, sharing one argument reduction.
///
/// The renderer asks for both when it builds a rotation, and sharing the reduction keeps
/// the pair consistent: `sin^2 + cos^2` is exactly one to within rounding, which a pair
/// computed from two independent reductions does not guarantee.
#[must_use]
pub fn sin_cos(x: f64) -> (f64, f64) {
    if !x.is_finite() {
        return (f64::NAN, f64::NAN);
    }
    let (quadrant, remainder) = reduce_quadrant(x);
    let sine = sin_series(remainder);
    let cosine = cos_series(remainder);
    match quadrant.rem_euclid(4) {
        0 => (sine, cosine),
        1 => (cosine, -sine),
        2 => (-sine, -cosine),
        _ => (-cosine, sine),
    }
}

/// Converts degrees to radians, exactly as far as the conversion allows.
#[must_use]
pub fn radians(degrees: f64) -> f64 {
    degrees * (PI / 180.0)
}

/// Clamps a value into an inclusive range, propagating NaN unchanged.
#[must_use]
pub fn clamp(value: f64, low: f64, high: f64) -> f64 {
    if value.is_nan() {
        return value;
    }
    if value < low {
        low
    } else if value > high {
        high
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Relative difference against a reference, which is how libm is used here.
    fn relative_error(ours: f64, reference: f64) -> f64 {
        if reference == 0.0 {
            return (ours - reference).abs();
        }
        ((ours - reference) / reference).abs()
    }

    const TOLERANCE: f64 = 1.0e-13;

    #[test]
    fn exp_matches_the_platform_reference() {
        for step in -140..=140 {
            let x = f64::from(step) * 0.05;
            let ours = exp(x);
            let reference = x.exp();
            assert!(
                relative_error(ours, reference) < TOLERANCE,
                "exp({x}): {ours} vs {reference}"
            );
        }
        assert_eq!(exp(0.0), 1.0);
    }

    #[test]
    fn ln_matches_the_platform_reference() {
        let mut value = 0.01_f64;
        while value < 1.0e6 {
            let ours = ln(value);
            let reference = value.ln();
            assert!(
                relative_error(ours, reference) < TOLERANCE,
                "ln({value}): {ours} vs {reference}"
            );
            value *= 1.37;
        }
        assert_eq!(ln(1.0), 0.0);
    }

    #[test]
    fn exp_and_ln_are_inverses_within_rounding() {
        for step in -60..=60 {
            let x = f64::from(step) * 0.1;
            let round_trip = ln(exp(x));
            assert!(
                (round_trip - x).abs() < 1.0e-12,
                "ln(exp({x})) = {round_trip}"
            );
        }
    }

    #[test]
    fn integer_powers_are_exact_and_roots_are_accurate() {
        assert_eq!(powi(2.0, 10), 1024.0);
        assert_eq!(powi(3.0, 0), 1.0);
        assert_eq!(powi(1.5, 4), 5.0625);

        assert!((nth_root(27.0, 3) - 3.0).abs() < 1.0e-12);
        assert!((nth_root(1024.0, 10) - 2.0).abs() < 1.0e-12);
        assert_eq!(nth_root(0.0, 5), 0.0);
        assert!(
            nth_root(-8.0, 2).is_nan(),
            "an even root of a negative is undefined"
        );
        assert!((nth_root(-8.0, 3) + 2.0).abs() < 1.0e-12);
    }

    #[test]
    fn rational_powers_match_the_platform_for_the_exponents_a_grade_uses() {
        // 2.4 is the sRGB decode exponent and 1/2.2 the Rec.709 one.
        for step in 0..=100 {
            let base = f64::from(step) * 0.01;
            let ours = pow_rational(base, 12, 5);
            let reference = base.powf(2.4);
            assert!(
                relative_error(ours, reference) < 1.0e-12,
                "x^2.4 at {base}: {ours} vs {reference}"
            );
        }
        assert!((pow_rational(4.0, 1, 2) - 2.0).abs() < 1.0e-15);
        assert!((pow_rational(8.0, 1, 3) - 2.0).abs() < 1.0e-12);
        assert!((pow_rational(2.0, -1, 1) - 0.5).abs() < 1.0e-15);
        assert_eq!(pow_rational(0.0, 1, 3), 0.0);
        assert_eq!(pow_rational(5.0, 0, 3), 1.0);
    }

    #[test]
    fn sine_and_cosine_match_the_platform_reference() {
        let mut x = -12.0_f64;
        while x <= 12.0 {
            assert!(
                relative_error(sin(x), x.sin()) < TOLERANCE,
                "sin({x}): {} vs {}",
                sin(x),
                x.sin()
            );
            assert!(
                relative_error(cos(x), x.cos()) < TOLERANCE,
                "cos({x}): {} vs {}",
                cos(x),
                x.cos()
            );
            x += 0.017;
        }
    }

    #[test]
    fn sine_and_cosine_share_one_reduction_and_stay_on_the_unit_circle() {
        for step in -180..=180 {
            let degrees = f64::from(step);
            let (sine, cosine) = sin_cos(radians(degrees));
            let magnitude = sine * sine + cosine * cosine;
            assert!(
                (magnitude - 1.0).abs() < 1.0e-12,
                "sin^2 + cos^2 = {magnitude} at {degrees} degrees"
            );
        }
        let (sine, cosine) = sin_cos(0.0);
        assert!(sine.abs() < 1.0e-15);
        assert!((cosine - 1.0).abs() < 1.0e-15);
    }

    #[test]
    fn spanning_scales_by_powers_of_two() {
        assert_eq!(scale_by_pow2(1.0, 10), 1024.0);
        assert_eq!(scale_by_pow2(1024.0, -10), 1.0);
        assert_eq!(scale_by_pow2(3.0, 0), 3.0);
    }

    #[test]
    fn clamping_propagates_nan_and_saturates_outside_the_range() {
        assert_eq!(clamp(2.0, 0.0, 1.0), 1.0);
        assert_eq!(clamp(-2.0, 0.0, 1.0), 0.0);
        assert_eq!(clamp(0.5, 0.0, 1.0), 0.5);
        assert!(clamp(f64::NAN, 0.0, 1.0).is_nan());
    }

    #[test]
    fn non_finite_inputs_are_reported_rather_than_invented() {
        assert!(exp(f64::NAN).is_nan());
        assert!(ln(-1.0).is_nan());
        assert_eq!(ln(0.0), f64::NEG_INFINITY);
        assert!(sin(f64::NAN).is_nan());
        assert!(cos(f64::INFINITY).is_nan());
        assert_eq!(exp(1000.0), f64::INFINITY);
        assert_eq!(exp(-1000.0), 0.0);
    }
}
