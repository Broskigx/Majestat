//! `maj-playback` — deadline-driven playback: scheduling, frame pools, audio-clock A/V sync
//!
//! **Layer 4.** See `docs/ARCHITECTURE.md` for the layering rule.
//!
//! Scaffolded in M0; implemented in the phase named in `.planning/ROADMAP.md`.

/// Human-readable statement of this crate's responsibility.
pub const ROLE: &str = "deadline-driven playback: scheduling, frame pools, audio-clock A/V sync";

/// Layer in the Majestat architecture, from `docs/ARCHITECTURE.md`.
pub const LAYER: u8 = 4;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_its_role_and_layer() {
        assert!(!ROLE.is_empty(), "every crate must declare its role");
        assert_eq!(LAYER, 4);
    }
}
