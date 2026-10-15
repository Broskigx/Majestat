//! `maj-color` — colour management: transfer functions, matrices, LUTs, deterministic math
//!
//! **Layer 3.** See `docs/ARCHITECTURE.md` for the layering rule.
//!
//! ## What this crate is responsible for
//!
//! Turning code values into linear light and back, and doing it in a way that produces
//! the same bytes on any machine. Two pieces:
//!
//! * [`det`] — `exp`, `ln`, `pow`, `sin` and `cos` built from multiplication, division
//!   and addition, because the platform's libm is not bit-reproducible across
//!   implementations and the certified render path (ADR 0006) cannot tolerate that.
//! * [`TransferFunction`] — sRGB, BT.709, exact power laws, ST 2084 (PQ) and HLG.
//!
//! OpenColorIO configuration, ACES 2.0 transforms, DCTL and the full set of colour
//! spaces arrive in M8; the pieces here are the ones the renderer needs before then,
//! and they are the pieces that must be deterministic for every later layer to be.

/// Human-readable statement of this crate's responsibility.
pub const ROLE: &str = "colour management: OpenColorIO, ACES, display transforms, scopes";

/// Layer in the Majestat architecture, from `docs/ARCHITECTURE.md`.
pub const LAYER: u8 = 3;

pub mod det;
mod transfer;

pub use transfer::TransferFunction;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_its_role_and_layer() {
        assert!(!ROLE.is_empty(), "every crate must declare its role");
        assert_eq!(LAYER, 3);
    }
}
