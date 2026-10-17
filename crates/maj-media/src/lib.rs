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

//! # What is built here, and what is not
//!
//! M2a is the vocabulary and the probe path, with **no FFI at all**: the types of §3 and §4 of
//! that design, a backend that reads container headers in Rust ([`NativeBackend`]), and a
//! cache in front of it so a question about a file costs a stat and not a read. Three things
//! follow from that order, and they are the reason for it:
//!
//! * The layering is provable before a decoder exists. This crate names no renderer and no
//!   layer-3 type, and the closed [`ContainerKind`] vocabulary is what a manifest will say.
//! * The debt `PictureResolver::kind` has been carrying — a present-but-damaged file named
//!   for its container while the frame beside it said `damaged` — is repaid by [`ProbeCache`]
//!   without a decoder being written.
//! * The `unsafe` decision stays M2b's, made against a measured crash rate over the corpus
//!   rather than against a prediction.
//!
//! No frame is decoded by anything in this crate. [`MediaBackend::open_video`] refuses with
//! `undecodable`, which has always been the honest label for a build with no decoder in it.

mod cache;
mod container;
pub mod corpus;
mod decode;
mod error;
mod native;
mod picture;
mod probe;
mod stream;

pub use cache::ProbeCache;
pub use container::ContainerKind;
pub use decode::{DecodedFrame, Plane, VideoReader};
pub use error::MediaError;
pub use native::NativeBackend;
pub use picture::{
    BitDepth, ChromaSiting, ColourMatrix, ColourPrimaries, PictureFormat, PlaneLayout, SampleRange,
    TransferCharacteristics,
};
pub use probe::{DecoderIdentity, HardwareSupport, MediaBackend, MediaProbe};
pub use stream::{
    AudioStream, CodecKind, FieldOrder, FrameRateKind, Rotation, StreamInfo, StreamRole,
    VideoStream,
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
