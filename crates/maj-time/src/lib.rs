//! # maj-time — layer 0
//!
//! Rational time, timecode and frame addressing for Majestat.
//!
//! This crate is the bottom of the stack and has **zero dependencies**. It is pure
//! arithmetic: no I/O, no allocation-heavy work, no clocks, no randomness.
//!
//! ## Why rational time
//!
//! 23.976 fps is `24000/1001`, not `23.976`. Real-time code that stores positions as
//! floating point drifts: a 3-hour timeline at 23.976 fps accumulates visible
//! sub-frame error, and no floating-point representation of `1001/24000` is exact.
//! Majestat addresses every position with an integer frame count under an exact
//! rational [`Timebase`], so a given project revision always means the same frames.
//! See `docs/adr/0003-rational-time.md` and `docs/DETERMINISM.md`.
//!
//! ## Division semantics
//!
//! Every operation that divides uses **floor** (`div_euclid`) semantics, never
//! truncation-toward-zero. Truncation makes retiming behave differently on either
//! side of frame zero, which is a determinism hazard. [`RetimeMap`] is documented
//! against floor semantics and property-tested for monotonicity.
//!
//! ## Layout
//!
//! - [`Rational`] — exact rational number with a canonical reduced form.
//! - [`Timebase`] — frames per second plus the drop-frame capability of a rate.
//! - [`Timecode`] / [`TimecodeMode`] — SMPTE non-drop and drop-frame conversion.
//! - [`FrameRange`] — half-open frame interval `[start, end)`.
//! - [`RetimeMap`] — mapping between timeline frames and source frames.
//! - [`testing`] — a deterministic PRNG used by the property tests.

mod error;
mod range;
mod rational;
mod retime;
pub mod testing;
mod timebase;
mod timecode;

pub use error::TimeError;
pub use range::FrameRange;
pub use rational::Rational;
pub use retime::RetimeMap;
pub use timebase::Timebase;
pub use timecode::{Timecode, TimecodeMode};
