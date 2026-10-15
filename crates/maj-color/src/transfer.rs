//! Transfer functions: how a code value relates to linear light.
//!
//! Every function here is invertible and every inverse is tested against its forward
//! direction, because a grade that cannot be undone is a grade nobody can trust. The
//! kernels use [`crate::det`] rather than the platform's `powf`, `exp` and `ln`, so the
//! same code value produces the same linear value on any machine.

use serde::{Deserialize, Serialize};

use crate::det::{exp, ln, pow_rational};

/// A transfer characteristic, from code value to linear light.
///
/// The variants are the ones a real delivery needs: sRGB for the web, BT.709 for SDR
/// broadcast, an exact power law for the many camera curves that are one, PQ for HDR10
/// and Dolby Vision, and HLG for broadcast HDR.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferFunction {
    /// No curve: the value already is linear light.
    Linear,
    /// The sRGB curve, with its linear segment near black.
    Srgb,
    /// The BT.709 camera curve (OETF), as published.
    Rec709,
    /// A pure power law with an exact rational exponent.
    Gamma {
        /// Numerator of the exponent.
        num: i32,
        /// Denominator of the exponent.
        den: i32,
    },
    /// SMPTE ST 2084, the perceptual quantiser used by HDR10 and Dolby Vision.
    Pq,
    /// ARIB STD-B67, the hybrid log-gamma curve used by broadcast HDR.
    Hlg,
}

impl TransferFunction {
    /// Short stable name, used in manifests and the interface.
    #[must_use]
    pub fn key(self) -> String {
        match self {
            Self::Linear => "linear".to_owned(),
            Self::Srgb => "srgb".to_owned(),
            Self::Rec709 => "rec709".to_owned(),
            Self::Gamma { num, den } => format!("gamma_{num}_{den}"),
            Self::Pq => "pq".to_owned(),
            Self::Hlg => "hlg".to_owned(),
        }
    }

    /// Decodes a code value in `0..=1` to linear light.
    ///
    /// Values are not clamped: a renderer that works with values above one needs to be
    /// able to pass them through, and clamping here would silently destroy highlights.
    /// NaN propagates.
    #[must_use]
    pub fn decode(self, code: f64) -> f64 {
        if code.is_nan() {
            return f64::NAN;
        }
        match self {
            Self::Linear => code,
            Self::Srgb => {
                if code <= 0.040_45 {
                    code / 12.92
                } else {
                    let shifted = (code + 0.055) / 1.055;
                    if shifted < 0.0 {
                        -pow_rational(-shifted, 12, 5)
                    } else {
                        pow_rational(shifted, 12, 5)
                    }
                }
            }
            Self::Rec709 => {
                // The OETF inverse: the linear slope near black, then the 1/0.45 power.
                if code < REC709_SLOPE * REC709_BREAK_POINT {
                    code / REC709_SLOPE
                } else {
                    let shifted = (code + REC709_OFFSET) / REC709_GAIN;
                    if shifted < 0.0 {
                        -pow_rational(-shifted, 100, 45)
                    } else {
                        pow_rational(shifted, 100, 45)
                    }
                }
            }
            Self::Gamma { num, den } => {
                if code < 0.0 {
                    -pow_rational(-code, i64::from(num), i64::from(den))
                } else {
                    pow_rational(code, i64::from(num), i64::from(den))
                }
            }
            Self::Pq => pq_decode(code),
            Self::Hlg => hlg_decode(code),
        }
    }

    /// Encodes linear light back into a code value in `0..=1` for display.
    #[must_use]
    pub fn encode(self, linear: f64) -> f64 {
        if linear.is_nan() {
            return f64::NAN;
        }
        match self {
            Self::Linear => linear,
            Self::Srgb => {
                if linear <= 0.003_130_8 {
                    linear * 12.92
                } else {
                    let root = if linear < 0.0 {
                        -pow_rational(-linear, 5, 12)
                    } else {
                        pow_rational(linear, 5, 12)
                    };
                    1.055 * root - 0.055
                }
            }
            Self::Rec709 => {
                if linear < REC709_BREAK_POINT {
                    REC709_SLOPE * linear
                } else {
                    let root = if linear < 0.0 {
                        -pow_rational(-linear, 45, 100)
                    } else {
                        pow_rational(linear, 45, 100)
                    };
                    REC709_GAIN * root - REC709_OFFSET
                }
            }
            Self::Gamma { num, den } => {
                if linear < 0.0 {
                    -pow_rational(-linear, i64::from(den), i64::from(num))
                } else {
                    pow_rational(linear, i64::from(den), i64::from(num))
                }
            }
            Self::Pq => pq_encode(linear),
            Self::Hlg => hlg_encode(linear),
        }
    }

    /// Reports whether the curve is the identity, letting a renderer skip it entirely.
    #[must_use]
    pub fn is_identity(self) -> bool {
        matches!(self, Self::Linear)
    }
}

// --- BT.709 -----------------------------------------------------------------------------

/// The slope of the linear segment of the BT.709 curve.
const REC709_SLOPE: f64 = 4.5;
/// The gain of its power segment.
const REC709_GAIN: f64 = 1.099;
/// The offset of its power segment.
const REC709_OFFSET: f64 = 0.099;

/// The linear-light value at which the two segments of the BT.709 curve meet.
///
/// The pair the recommendation publishes, 0.018 and its inverse 0.081, does not meet: the
/// power segment passes 0.018 at 0.081248, so the published curve jumps by 2.5e-4 at the
/// joint, and no inverse can round trip over a jump. The two segments cross at 0.0199989,
/// and that crossing is the break point used here, which makes the curve one continuous and
/// exactly invertible function.
///
/// The cost is confined and known: inside a band two per cent wide around the joint the
/// curve differs from the published one by at most 2.5e-4 of code value. Black and white
/// are untouched, because the published endpoints are what a grade is matched against —
/// `1.099 * 1 - 0.099` is one and zero is zero. The test
/// `the_bt709_segments_meet_without_a_step` holds both halves of this claim.
const REC709_BREAK_POINT: f64 = 0.019_998_920_658_192_233;

// --- SMPTE ST 2084 (PQ) -----------------------------------------------------------------
//
// The constants are written as exact ratios of the integers the standard publishes, so
// the exponents stay rational and the kernels stay free of libm.

/// `c1 = 3424 / 4096`.
const PQ_C1: f64 = 3424.0 / 4096.0;
/// `c2 = 2413 / 4096 * 32`.
const PQ_C2: f64 = 2413.0 / 4096.0 * 32.0;
/// `c3 = 2392 / 4096 * 32`.
const PQ_C3: f64 = 2392.0 / 4096.0 * 32.0;

/// Decodes a PQ signal in `0..=1` into linear light normalised to 10 000 cd/m².
///
/// See SMPTE ST 2084, equation for the electro-optical transfer function. `m1` is
/// `1305/8192` and `m2` is `2523/32`, both exact.
fn pq_decode(signal: f64) -> f64 {
    // A PQ signal below zero carries no light, and above one is not defined by the
    // standard; both are clamped rather than allowed to produce a complex result.
    if signal <= 0.0 {
        return 0.0;
    }
    let one_over_m2 = pow_rational(signal, 32, 2523);
    let numerator = one_over_m2 - PQ_C1;
    let denominator = PQ_C2 - PQ_C3 * one_over_m2;
    if numerator <= 0.0 || denominator <= 0.0 {
        return 0.0;
    }
    pow_rational(numerator / denominator, 8192, 1305)
}

/// Encodes linear light normalised to 10 000 cd/m² back into a PQ signal.
fn pq_encode(linear: f64) -> f64 {
    if linear <= 0.0 {
        return 0.0;
    }
    let power = pow_rational(linear, 1305, 8192);
    let numerator = PQ_C1 + PQ_C2 * power;
    let denominator = 1.0 + PQ_C3 * power;
    pow_rational(numerator / denominator, 2523, 32)
}

// --- ARIB STD-B67 (HLG) -----------------------------------------------------------------

/// The three HLG constants.
///
/// Only `a` is published; `b` and `c` are derived from it here so they cannot drift out
/// of step with the value they belong to, and so the derivation is the same on every
/// machine.
fn hlg_constants() -> (f64, f64, f64) {
    /// The published `a` of ARIB STD-B67.
    const A: f64 = 0.178_832_77;
    let b = 1.0 - 4.0 * A;
    let c = 0.5 - A * ln(4.0 * A);
    (A, b, c)
}

/// Decodes an HLG signal in `0..=1` into scene linear light.
fn hlg_decode(signal: f64) -> f64 {
    if signal <= 0.0 {
        return 0.0;
    }
    let (a, b, c) = hlg_constants();
    if signal <= 0.5 {
        signal * signal / 3.0
    } else {
        (exp((signal - c) / a) + b) / 12.0
    }
}

/// Encodes scene linear light into an HLG signal.
fn hlg_encode(linear: f64) -> f64 {
    if linear <= 0.0 {
        return 0.0;
    }
    let (a, b, c) = hlg_constants();
    if linear <= 1.0 / 12.0 {
        (3.0 * linear).sqrt()
    } else {
        let inner = 12.0 * linear - b;
        if inner <= 0.0 {
            return 0.0;
        }
        a * ln(inner) + c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every built-in curve, for the exhaustive tests.
    const ALL: [TransferFunction; 5] = [
        TransferFunction::Srgb,
        TransferFunction::Rec709,
        TransferFunction::Gamma { num: 12, den: 5 },
        TransferFunction::Pq,
        TransferFunction::Hlg,
    ];

    #[test]
    fn every_curve_round_trips_within_its_quantisation() {
        for function in ALL {
            for step in 0..=1_000 {
                let code = f64::from(step) / 1_000.0;
                let linear = function.decode(code);
                let recovered = function.encode(linear);
                assert!(
                    (recovered - code).abs() < 1.0e-9,
                    "{:?} failed to round trip at {code}: recovered {recovered}",
                    function
                );
            }
        }
    }

    #[test]
    fn every_curve_is_monotonic_and_pins_its_endpoints() {
        for function in ALL {
            let mut previous = f64::NEG_INFINITY;
            for step in 0..=1_000 {
                let code = f64::from(step) / 1_000.0;
                let linear = function.decode(code);
                assert!(
                    linear >= previous - 1.0e-15,
                    "{:?} is not monotonic at {code}",
                    function
                );
                previous = linear;
            }
            assert_eq!(function.decode(0.0), 0.0, "{:?} at black", function);
            // HLG publishes its constants to eight digits, so its curve reaches white a few
            // parts in the hundred-millionth away from one rather than exactly on it. Every
            // other curve arrives on it, and a mistake in any branch would miss by 1e-2.
            let tolerance = match function {
                TransferFunction::Hlg => 1.0e-7,
                _ => 1.0e-12,
            };
            assert!(
                (function.decode(1.0) - 1.0).abs() < tolerance,
                "{:?} at white: {}",
                function,
                function.decode(1.0)
            );
        }
    }

    #[test]
    fn the_linear_curve_is_the_identity_and_says_so() {
        let linear = TransferFunction::Linear;
        assert!(linear.is_identity());
        assert_eq!(linear.decode(0.37), 0.37);
        assert_eq!(linear.encode(0.81), 0.81);
        for function in ALL {
            assert!(!function.is_identity());
        }
    }

    #[test]
    fn the_srgb_curve_has_its_published_linear_segment() {
        let srgb = TransferFunction::Srgb;
        // Below the break point the curve is exactly a gain of 12.92.
        assert!((srgb.decode(0.02) - 0.02 / 12.92).abs() < 1.0e-15);
        // Mid grey of 0.5 decodes to 0.2140411404822326 in linear light, which is
        // `(0.555 / 1.055)^2.4` evaluated in double precision — the curve is concave in the
        // dark end, so half a code value carries well under half the light.
        assert!(
            (srgb.decode(0.5) - 0.214_041_140_482_232_6).abs() < 1.0e-12,
            "mid grey decoded to {}",
            srgb.decode(0.5)
        );
        assert!(
            srgb.decode(0.5) < 0.5,
            "the sRGB curve is concave in the dark end"
        );
    }

    #[test]
    fn the_bt709_segments_meet_without_a_step() {
        // What the recommendation publishes: a break point of 0.018 whose inverse it gives
        // as 0.081. The power segment does not pass through that pair.
        let published = REC709_GAIN * pow_rational(0.018, 45, 100) - REC709_OFFSET;
        assert!(
            (published - 0.081_247_944_035_140_46).abs() < 1.0e-12,
            "the power segment passes 0.018 at {published}"
        );
        assert!(
            (published - REC709_SLOPE * 0.018).abs() > 2.0e-4,
            "the published pair is not a step a decoder can read back through"
        );

        // The break point used instead is the crossing of the two segments. The residual is
        // the power kernel's own error, amplified by the 45th power it ends in, rather than
        // an error of the break point: it is 1e-14 of code value, a thousand million times
        // below the 16-bit quantisation the round trip above is held to.
        let left = REC709_SLOPE * REC709_BREAK_POINT;
        let right = REC709_GAIN * pow_rational(REC709_BREAK_POINT, 45, 100) - REC709_OFFSET;
        assert!(
            (left - right).abs() < 1.0e-14,
            "the segments do not meet: {left} against {right}"
        );

        // Black and white are still pinned exactly, which is what a delivery is matched on.
        assert_eq!(TransferFunction::Rec709.decode(0.0), 0.0);
        assert!((TransferFunction::Rec709.decode(1.0) - 1.0).abs() < 1.0e-12);
    }

    #[test]
    fn an_exact_gamma_two_point_two_matches_its_definition() {
        let gamma = TransferFunction::Gamma { num: 22, den: 10 };
        let decoded = gamma.decode(0.5);
        let expected = 0.5_f64.powf(2.2);
        assert!(
            (decoded - expected).abs() < 1.0e-12,
            "2.2 gamma at 0.5: {decoded} vs {expected}"
        );
    }

    #[test]
    fn pq_reaches_the_reference_white_it_is_defined_against() {
        // A PQ signal of one is 10 000 cd/m², which is the normalisation of the EOTF.
        assert!((TransferFunction::Pq.decode(1.0) - 1.0).abs() < 1.0e-12);
        // The published anchor of the curve: 100 cd/m² sits at a signal of 0.5081, which is
        // one hundredth of the peak. The quantiser exists to spend its codes on the dark
        // end, so a signal of one half is far below the half of a gamma curve.
        let hundred = TransferFunction::Pq.decode(0.508_1);
        assert!(
            (hundred - 0.01).abs() < 1.0e-4,
            "a PQ signal of 0.5081 is {hundred} of peak, not 100 cd/m²"
        );
        // The same anchor read backwards: 100 cd/m² encodes to that signal.
        assert!((TransferFunction::Pq.encode(0.01) - 0.508_1).abs() < 1.0e-4);
        let mid = TransferFunction::Pq.decode(0.5);
        assert!(
            mid > 0.0 && mid < hundred,
            "a PQ signal of one half is {mid}, which should sit below 100 cd/m²"
        );
    }

    #[test]
    fn hlg_has_its_published_break_points() {
        let hlg = TransferFunction::Hlg;
        assert!((hlg.decode(0.5) - 1.0 / 12.0).abs() < 1.0e-15);
        assert!((hlg.encode(1.0 / 12.0) - 0.5).abs() < 1.0e-15);
        // The three constants must agree with the published values.
        let (a, b, c) = hlg_constants();
        assert!((a - 0.178_832_77).abs() < 1.0e-12);
        assert!((b - 0.284_668_92).abs() < 1.0e-7, "b = {b}");
        assert!((c - 0.559_910_73).abs() < 1.0e-7, "c = {c}");
    }

    #[test]
    fn curves_are_well_behaved_outside_the_unit_interval_and_report_nan() {
        for function in ALL {
            assert!(function.decode(f64::NAN).is_nan(), "{:?}", function);
            assert!(function.encode(f64::NAN).is_nan(), "{:?}", function);
            // A highlight above one must survive the round trip rather than being
            // clamped, or a grade would lose everything above diffuse white.
            let extended = function.decode(1.0) * 1.5;
            let recovered = function.encode(extended);
            assert!(
                recovered > 1.0,
                "{:?} destroyed the extended range: {recovered}",
                function
            );
        }
    }

    #[test]
    fn keys_are_stable_and_distinct() {
        let keys: Vec<String> = ALL
            .iter()
            .map(|function| function.key())
            .chain(std::iter::once(TransferFunction::Linear.key()))
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(keys.len(), sorted.len(), "two curves share a key");
        assert_eq!(
            TransferFunction::Gamma { num: 12, den: 5 }.key(),
            "gamma_12_5"
        );
        assert_eq!(TransferFunction::Srgb.key(), "srgb");
    }
}
