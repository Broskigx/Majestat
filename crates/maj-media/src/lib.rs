//! `maj-media` — media I/O: probe, demux, decode, seek, waveforms, thumbnails
//!
//! **Layer 2.** See `docs/ARCHITECTURE.md` for the layering rule.
//!
//! Scaffolded in M0; implemented in M2, and **its surface is designed**: `docs/MEDIA.md`
//! carries the types, the traits and the four constraints that decided their shapes,
//! written against the seam that already exists rather than against an FFmpeg binding.
//!
//! Four things about that design are worth knowing from here, because they are properties
//! of the architecture rather than of the decoder:
//!
//! * **This crate cannot implement `maj_render::PictureResolver`.** That trait is defined
//!   at layer 4 and dependencies point downward only, so the adapter lives in `maj-render`
//!   and this crate publishes a decoder vocabulary that knows nothing about renderers.
//! * **Nothing is converted here.** A decoded frame comes back in the format it was
//!   decoded in, with its colour metadata declared and unapplied. The YUV-to-RGB matrix and
//!   the transfer function are arithmetic on the certified path, so they belong to
//!   `maj-color` under ADR 0006 — which is also why the pinned FFmpeg build has no
//!   `swscale` in it (`docs/FFMPEG.md`).
//! * **Media that cannot be read stays one condition with one policy.** This crate's error
//!   type has the same three variants `maj_render::SourceError` has — offline, damaged,
//!   undecodable — and adds no fourth, because P0.3 settled that and a decoder is the
//!   obvious place to reopen it by accident.
//! * **Binding a C library needs `unsafe`, which this workspace forbids.** The narrowest
//!   answer is a leaf FFI crate that holds nothing but declarations, leaving this crate
//!   `forbid`. That relaxes a workspace-wide lint, so it needs an ADR of its own weight
//!   and is deliberately not decided in a module comment. See `docs/MEDIA.md` §8.

//!
//! What *is* here, since P1: [`mod@measure`], which hashes a file and finds it again by hash.
//! That needs no decoder, and it is what lets a project survive a drive letter changing.
//! The probe, the demuxer and the decoder are still to come.

pub mod measure;

pub use measure::{
    Found, Measurement, MediaError, Search, Wanted, find_by_hash, measure, measure_uri, uri_of,
    url_to_path,
};

/// Human-readable statement of this crate's responsibility.
pub const ROLE: &str = "media I/O: probe, demux, decode, seek, waveforms, thumbnails";

/// Layer in the Majestat architecture, from `docs/ARCHITECTURE.md`.
pub const LAYER: u8 = 2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_its_role_and_layer() {
        assert!(!ROLE.is_empty(), "every crate must declare its role");
        assert_eq!(LAYER, 2);
    }
}
