//! `maj-render` — offline deterministic renderer: same graph as preview, offline policy
//!
//! **Layer 4.** See `docs/ARCHITECTURE.md` for the layering rule.
//!
//! This crate is the **reference renderer**: a CPU implementation of the certified path
//! (Tier 2, ADR 0006) that turns a project and a timeline frame into pixels, and that the
//! GPU path is later checked against. It is deliberately single-threaded and deliberately
//! boring. Every number that reaches a pixel is either an exactly rounded IEEE-754
//! operation (addition, subtraction, multiplication, division, square root) or comes from
//! [`maj_color::det`], which builds its transcendentals from those. There is no `powf`, no
//! `sinf`, no lookup table with an interpolated entry, and no library that hides one.
//!
//! ## What it does
//!
//! `maj-core` says what a frame *is*; this crate says what it *looks like*:
//!
//! * [`Framebuffer`] holds one frame of scene-linear light, straight alpha, so a value of
//!   one is diffuse white and two is a stop over it. Integer rounding happens once, at the
//!   output boundary, and never between effects.
//! * [`render_frame`] resolves an animation at a timeline frame, reads each layer through a
//!   [`PictureSource`], inverts the layer's transform, multiplies in the mask coverage,
//!   runs the effect chain, and composites the layer over what is beneath it.
//! * [`render_tiled`] produces the same frame through a grid of tiles with a halo, which is
//!   how the offline path will eventually parallelise. That it agrees with the untiled
//!   render is not an aspiration but a test.
//! * [`RenderManifest`] records what was rendered, with which policy and what digest, so a
//!   render can be attributed and compared rather than merely trusted.
//! * [`source::Title`] sets type over a frame in four built-in faces, from an alphabet in
//!   this crate, because a title has to render on a machine with no fonts on it.
//!
//! Two things are deliberately absent. There is no file I/O — a frame is handed to
//! `maj-export` or to a viewer. And there is no clock, no thread and no global state, so
//! two runs agree and a test can be deterministic without a fixture.

#![forbid(unsafe_code)]

pub mod compose;
pub mod effect;
pub mod framebuffer;
pub mod render;
pub mod source;

pub use compose::{Placement, blend, blend_channel, composite, mask_coverage, rational_to_f64};
pub use effect::{EffectContext, apply, chain_halo, spatial_support};
pub use framebuffer::{BitDepth, CHANNELS, Framebuffer, Pixel};
pub use render::{
    FrameGeometry, LayerRecord, RENDER_MANIFEST_SCHEMA, RenderError, RenderManifest, RenderPolicy,
    RenderReport, RenderRequest, RenderTask, Window, frame_halo, render_frame, render_tiled,
    transition_pixel,
};
pub use source::{
    Generated, MissingMedia, PictureResolver, PictureSource, Placeholder, Png, Ppm, Solid,
    SourceError, Title, TitleAlign, TitleFace, WorkspaceResolver,
};

/// Human-readable statement of this crate's responsibility.
pub const ROLE: &str = "offline deterministic renderer: same graph as preview, offline policy";

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
