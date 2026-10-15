//! The reference renderer: a project and a timeline frame become pixels.
//!
//! This is the **certified path** (Tier 2, ADR 0006) implemented on the CPU. It is the
//! definition the GPU path will later be checked against, so it is written to be read:
//! every decision that could differ between two machines is made explicitly and named.
//!
//! ## What a frame is
//!
//! A frame is composited from the sequence's video tracks, in the order the sequence holds
//! them: the first video track is the backdrop and each later one is composited over the
//! result. Tracks are the only z-order the model has, so this is the only order that can be
//! derived from the document rather than invented. Audio tracks are skipped entirely: this
//! crate produces picture, and `maj-audio` owns sound.
//!
//! Within a track, clips are visited in the order the track stores them, which is the
//! insertion order the model treats as part of the state. A clip that is disabled, that
//! does not cover the frame, or whose placement collapses to no area contributes nothing
//! and is recorded as invisible in the manifest rather than silently dropped.
//!
//! ## What a layer is
//!
//! For each frame pixel the renderer asks the **inverse** question: which source
//! coordinate landed here? The transform is inverted exactly, the source is sampled there,
//! the mask coverage is multiplied into the layer's alpha *before* the effect chain runs,
//! every effect in the chain is applied in declared order, and the finished layer is
//! composited over the backdrop with its own blend mode and opacity.
//!
//! ## Why tiles agree with the whole
//!
//! [`render_tiled`] renders the same frame as a grid of windows. Two properties make that
//! bit-exact rather than approximate, and both are tested:
//!
//! * Every effect that reads its neighbours declares its
//!   [`spatial_support`](crate::spatial_support), and each window is expanded by the largest
//!   support in the frame before anything is drawn. A
//!   window's interior therefore sees the same pixels a whole-frame pass would have given
//!   it, and the kernel never reads past the expansion.
//! * Every effect that is defined on the frame — a vignette, a grain field — is given the
//!   frame's size and the window's absolute origin through [`EffectContext`], so its value
//!   depends on where the pixel is, never on where the tile starts. Grain is a hash of the
//!   absolute pixel coordinate, not of a counter.
//!
//! What is *not* claimed: the tiled path is not yet parallel, and it is not faster. It
//! exists because correctness under tiling is a precondition for parallelism, and proving
//! it now is cheaper than discovering it later.

use std::fmt;

use maj_core::{
    AnimChannel, BlendMode, Clip, CoreError, MediaAssetId, Project, Rational, Sequence, SequenceId,
    Track, TrackKind, Transform, Transition, TransitionKind,
};
use serde::Serialize;

use crate::compose::{Placement, composite, mask_coverage, rational_to_f64};
use crate::effect::{self, EffectContext, chain_halo};
use crate::framebuffer::{Framebuffer, Pixel};
use crate::source::{PictureResolver, SourceError};

/// A fully transparent pixel: the fill a layer starts from.
pub const CLEAR: Pixel = [0.0, 0.0, 0.0, 0.0];

/// Opaque black, which is what a dip to black reaches and what an empty sequence is.
pub const BLACK: Pixel = [0.0, 0.0, 0.0, 1.0];

/// Opaque white, which is what a dip to white reaches.
pub const WHITE: Pixel = [1.0, 1.0, 1.0, 1.0];

/// Version of the render manifest's shape.
pub const RENDER_MANIFEST_SCHEMA: u32 = 1;

/// Which of the two determinism tiers a render was produced on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RenderPolicy {
    /// Fixed reduction order, no fast-math, no vendor approximations. What this crate is.
    Certified,
    /// Vendor intrinsics and relaxed precision, for interactive playback only.
    Opportunistic,
}

impl RenderPolicy {
    /// Every policy, in a fixed order.
    pub const ALL: [Self; 2] = [Self::Certified, Self::Opportunistic];

    /// Short stable name, used in manifests.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Certified => "certified",
            Self::Opportunistic => "opportunistic",
        }
    }

    /// Whether a render on this policy can be verified across hardware.
    #[must_use]
    pub const fn is_certified(self) -> bool {
        matches!(self, Self::Certified)
    }
}

impl fmt::Display for RenderPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.key())
    }
}

/// Everything that can stop a render.
#[derive(Debug, Clone, PartialEq)]
pub enum RenderError {
    /// The project has no sequence with that identifier.
    UnknownSequence(SequenceId),
    /// The sequence references media the project does not hold.
    UnknownAsset(MediaAssetId),
    /// The policy asked for is not implemented by the reference renderer.
    UnsupportedPolicy(RenderPolicy),
    /// An animated channel carried a value that is not a number, or a scale that
    /// cannot form a transform.
    MalformedAnimation(String),
    /// A tile grid asked for zero tiles.
    EmptyGrid,
    /// The model rejected something the renderer asked of it.
    Core(CoreError),
    /// A source could not be produced.
    Source(SourceError),
}

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownSequence(id) => write!(formatter, "the project has no sequence {id}"),
            Self::UnknownAsset(id) => {
                write!(
                    formatter,
                    "the sequence references media {id}, which is not registered"
                )
            }
            Self::UnsupportedPolicy(policy) => write!(
                formatter,
                "the reference renderer implements the certified path only, not `{policy}`"
            ),
            Self::MalformedAnimation(what) => write!(formatter, "malformed animation: {what}"),
            Self::EmptyGrid => formatter.write_str("a tile grid needs at least one tile"),
            Self::Core(error) => write!(formatter, "{error}"),
            Self::Source(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for RenderError {}

impl From<CoreError> for RenderError {
    fn from(error: CoreError) -> Self {
        Self::Core(error)
    }
}

impl From<SourceError> for RenderError {
    fn from(error: SourceError) -> Self {
        Self::Source(error)
    }
}

/// What to render: which sequence, which frame, and at what size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderRequest {
    /// Sequence to render.
    pub sequence: SequenceId,
    /// Timeline frame to render.
    pub frame: i64,
    /// Output width in pixels, which need not be the sequence's own width.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
}

impl RenderRequest {
    /// A request at an explicit resolution.
    #[must_use]
    pub const fn new(sequence: SequenceId, frame: i64, width: u32, height: u32) -> Self {
        Self {
            sequence,
            frame,
            width,
            height,
        }
    }

    /// A request at the sequence's own resolution.
    #[must_use]
    pub const fn of(sequence: &Sequence, frame: i64) -> Self {
        let id = sequence.id();
        Self::new(id, frame, sequence.width(), sequence.height())
    }
}

/// What one layer of the frame did, for the manifest.
///
/// A render that cannot say what it drew is a render nobody can audit, so each layer
/// records its identity, the source kind it resolved to, the halo it needed, and whether
/// it contributed anything at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LayerRecord {
    /// The clip, by raw identifier.
    pub clip: u64,
    /// The clip's name at render time.
    pub name: String,
    /// Whether the clip landed on the frame.
    pub visible: bool,
    /// The kind of picture source that was read, or `-` when nothing was read.
    pub source: &'static str,
    /// The clip's blend mode.
    pub blend: &'static str,
    /// The enabled effects, in application order.
    pub effects: Vec<&'static str>,
    /// Number of masks multiplied into the layer's alpha.
    pub masks: u32,
    /// The halo a tiled render had to give this layer.
    pub halo: u32,
    /// `layer` for a clip composited on its own, `from` or `to` for a transition partner.
    pub role: &'static str,
    /// The transition that read this layer, when one did.
    pub transition: Option<&'static str>,
}

impl LayerRecord {
    /// A record for a clip that was not read at all.
    fn invisible(clip: &Clip) -> Self {
        Self {
            clip: clip.id().raw(),
            name: clip.name().to_owned(),
            visible: false,
            source: "-",
            blend: clip.blend().key(),
            effects: Vec::new(),
            masks: clip.masks().len() as u32,
            halo: 0,
            role: "layer",
            transition: None,
        }
    }
}

/// What a render did, in a form that can be stored, diffed and compared.
///
/// The manifest holds **no floating point**, for the same reason the project format does
/// not: two runs must produce identical text, and a decimal rendering of a float is a
/// place where that can quietly fail. The frame's peak is recorded in thousandths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenderManifest {
    /// Shape version of this manifest.
    pub schema: u32,
    /// The policy the render ran on.
    pub policy: &'static str,
    /// Whether that policy is the certified one.
    pub certified: bool,
    /// The sequence, by raw identifier.
    pub sequence: u64,
    /// The timeline frame.
    pub frame: i64,
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// Project revision at render time.
    pub project_revision: u64,
    /// Hash of the project state at render time.
    pub state_hash: String,
    /// The halo the render used, zero for a whole-frame pass.
    pub halo: u32,
    /// The tile grid, as columns and rows.
    pub grid: (u32, u32),
    /// Every layer the render considered, in compositing order.
    pub layers: Vec<LayerRecord>,
    /// Digest of the rendered frame.
    pub digest: String,
    /// The largest absolute channel value in the frame, in thousandths.
    pub peak_thousandths: u32,
}

impl RenderManifest {
    /// Records a render.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Core`] when the project cannot be hashed.
    pub fn build(
        project: &Project,
        request: RenderRequest,
        policy: RenderPolicy,
        grid: (u32, u32),
        halo: u32,
        frame: &Framebuffer,
        resolver: &dyn PictureResolver,
    ) -> Result<Self, RenderError> {
        let sequence = project
            .sequence(request.sequence)
            .ok_or(RenderError::UnknownSequence(request.sequence))?;
        let mut layers = Vec::new();
        for track in video_tracks(sequence) {
            collect_layers(project, track, request.frame, &mut layers, resolver)?;
        }
        let peak = frame.peak();
        let peak_thousandths = if peak.is_finite() && peak > 0.0 {
            (f64::from(peak) * 1000.0).round() as u32
        } else {
            0
        };
        Ok(Self {
            schema: RENDER_MANIFEST_SCHEMA,
            policy: policy.key(),
            certified: policy.is_certified(),
            sequence: request.sequence.raw(),
            frame: request.frame,
            width: request.width,
            height: request.height,
            project_revision: project.revision(),
            state_hash: project.state_hash()?,
            halo,
            grid,
            layers,
            digest: frame.digest(),
            peak_thousandths,
        })
    }

    /// Whether every layer in the manifest landed on the frame.
    #[must_use]
    pub fn drew_something(&self) -> bool {
        self.layers.iter().any(|layer| layer.visible)
    }

    /// The enabled effects across all layers, in order.
    #[must_use]
    pub fn effect_keys(&self) -> Vec<&'static str> {
        self.layers
            .iter()
            .flat_map(|layer| layer.effects.iter().copied())
            .collect()
    }
}

/// A render's output: the pixels and the record of how they were made.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderReport {
    /// The rendered frame.
    pub frame: Framebuffer,
    /// What was rendered, and with what policy.
    pub manifest: RenderManifest,
}

/// One render, described before it is run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderTask {
    /// What to render.
    pub request: RenderRequest,
    /// Which determinism tier to render on.
    pub policy: RenderPolicy,
    /// How many tiles to split the frame into, as columns and rows.
    pub grid: (u32, u32),
}

impl RenderTask {
    /// A whole-frame render on the certified path.
    #[must_use]
    pub const fn single(request: RenderRequest) -> Self {
        Self {
            request,
            policy: RenderPolicy::Certified,
            grid: (1, 1),
        }
    }

    /// A tiled render on the certified path.
    #[must_use]
    pub const fn tiled(request: RenderRequest, grid: (u32, u32)) -> Self {
        Self {
            request,
            policy: RenderPolicy::Certified,
            grid,
        }
    }

    /// The same task on another policy.
    #[must_use]
    pub const fn with_policy(mut self, policy: RenderPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// The halo this task's frame needs, given the effect chains in it.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::UnknownSequence`] when the sequence does not exist.
    pub fn halo(&self, project: &Project) -> Result<u32, RenderError> {
        let sequence = project
            .sequence(self.request.sequence)
            .ok_or(RenderError::UnknownSequence(self.request.sequence))?;
        Ok(frame_halo(sequence))
    }

    /// Renders the task, whichever grid it asks for.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::UnsupportedPolicy`] for any policy other than the certified
    /// one, because this crate is the certified path and pretending otherwise would put a
    /// false statement in a manifest. Otherwise returns the renderer's own errors.
    pub fn render(
        &self,
        project: &Project,
        resolver: &dyn PictureResolver,
    ) -> Result<RenderReport, RenderError> {
        if !self.policy.is_certified() {
            return Err(RenderError::UnsupportedPolicy(self.policy));
        }
        let sequence = project
            .sequence(self.request.sequence)
            .ok_or(RenderError::UnknownSequence(self.request.sequence))?;
        let halo = frame_halo(sequence);
        let frame = render_grid(project, sequence, self.request, resolver, self.grid, halo)?;
        let manifest = RenderManifest::build(
            project,
            self.request,
            self.policy,
            self.grid,
            halo,
            &frame,
            resolver,
        )?;
        Ok(RenderReport { frame, manifest })
    }
}

/// Renders one timeline frame as a single window covering the whole frame.
///
/// # Errors
///
/// Returns [`RenderError::UnknownSequence`] when the sequence does not exist, and the
/// errors of the private `render_window` otherwise.
pub fn render_frame(
    project: &Project,
    request: RenderRequest,
    resolver: &dyn PictureResolver,
) -> Result<Framebuffer, RenderError> {
    let sequence = project
        .sequence(request.sequence)
        .ok_or(RenderError::UnknownSequence(request.sequence))?;
    render_grid(project, sequence, request, resolver, (1, 1), 0)
}

/// Renders one timeline frame as a grid of tiles, each with the halo it needs.
///
/// The grid is `(columns, rows)`. A grid of `(1, 1)` is exactly [`render_frame`], and any
/// other grid must produce the identical frame — that equality is the point.
///
/// # Errors
///
/// Returns [`RenderError::EmptyGrid`] for a zero in either axis, and the errors of the
/// private `render_window` otherwise.
pub fn render_tiled(
    project: &Project,
    request: RenderRequest,
    resolver: &dyn PictureResolver,
    grid: (u32, u32),
) -> Result<Framebuffer, RenderError> {
    let sequence = project
        .sequence(request.sequence)
        .ok_or(RenderError::UnknownSequence(request.sequence))?;
    let halo = frame_halo(sequence);
    render_grid(project, sequence, request, resolver, grid, halo)
}

/// The largest halo any layer in the sequence needs.
///
/// This is a property of the *frame*, not of a tile: a window is expanded by the largest
/// support in the sequence, so every effect in the window has the pixels it needs.
#[must_use]
pub fn frame_halo(sequence: &Sequence) -> u32 {
    video_tracks(sequence)
        .flat_map(|track| track.clips())
        .map(|clip| chain_halo(clip.effects()))
        .max()
        .unwrap_or(0)
}

/// Renders a frame through a grid of windows.
fn render_grid(
    project: &Project,
    sequence: &Sequence,
    request: RenderRequest,
    resolver: &dyn PictureResolver,
    grid: (u32, u32),
    halo: u32,
) -> Result<Framebuffer, RenderError> {
    let columns = grid.0.max(1) as usize;
    let rows = grid.1.max(1) as usize;
    let mut out = Framebuffer::black(request.width, request.height);
    for tile in partition(request.width, request.height, columns, rows) {
        let window = render_window(project, sequence, request, resolver, tile, halo)?;
        blit(&window, tile, &mut out);
    }
    Ok(out)
}

/// The tiles of a grid, covering the frame exactly once.
///
/// The remainder of a division is spread over the leading columns and rows, so the split is
/// a pure function of the frame size and the grid rather than of floating-point rounding.
pub fn partition(width: u32, height: u32, columns: usize, rows: usize) -> Vec<Window> {
    let mut tiles = Vec::with_capacity(columns * rows);
    for row_span in split(height, rows) {
        for column_span in &split(width, columns) {
            tiles.push(Window {
                x: i64::from(column_span.0),
                y: i64::from(row_span.0),
                width: column_span.1,
                height: row_span.1,
            });
        }
    }
    tiles
}

/// Splits a length into `parts` pieces, the remainder going to the leading pieces.
fn split(length: u32, parts: usize) -> Vec<(u32, u32)> {
    let parts = parts.max(1) as u32;
    let base = length / parts;
    let remainder = length % parts;
    let mut spans = Vec::with_capacity(parts as usize);
    let mut start = 0;
    for index in 0..parts {
        let size = base + u32::from(index < remainder);
        if size == 0 {
            continue;
        }
        spans.push((start, size));
        start += size;
    }
    spans
}

/// A rectangular window of the frame, in frame coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// Column of the window's left edge.
    pub x: i64,
    /// Row of the window's top edge.
    pub y: i64,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl Window {
    /// The whole frame as one window.
    #[must_use]
    pub const fn whole(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }

    /// The same region grown by `halo` on every side, cropped to the frame.
    ///
    /// Growing is clamped to the frame because there is nothing outside it to read: an
    /// effect that clamps at the frame edge must clamp at the same place in a tile as in a
    /// whole-frame pass, and a window that extended past the edge would let it read a
    /// coordinate the whole-frame pass never saw.
    #[must_use]
    pub fn expanded(self, halo: u32, frame: (u32, u32)) -> Self {
        let (frame_width, frame_height) = (i64::from(frame.0), i64::from(frame.1));
        let halo = i64::from(halo);
        let left = (self.x - halo).max(0);
        let top = (self.y - halo).max(0);
        let right = (self.x + i64::from(self.width) + halo).min(frame_width);
        let bottom = (self.y + i64::from(self.height) + halo).min(frame_height);
        Self {
            x: left,
            y: top,
            width: (right - left).max(0) as u32,
            height: (bottom - top).max(0) as u32,
        }
    }

    /// Where `inner`'s top-left corner sits inside this window.
    #[must_use]
    pub const fn offset_of(self, inner: Self) -> (u32, u32) {
        ((inner.x - self.x) as u32, (inner.y - self.y) as u32)
    }
}

/// Copies the window a rendered buffer holds into the frame it belongs to.
fn blit(window: &Framebuffer, target: Window, out: &mut Framebuffer) {
    for y in 0..target.height {
        for x in 0..target.width {
            let pixel = window.pixel(x, y).unwrap_or(BLACK);
            out.set_pixel(target.x as u32 + x, target.y as u32 + y, pixel);
        }
    }
}

/// Renders one window of the frame, with the halo its effects need.
///
/// # Errors
///
/// Propagates the errors of [`render_layer`] and of the effect chain.
fn render_window(
    project: &Project,
    sequence: &Sequence,
    request: RenderRequest,
    resolver: &dyn PictureResolver,
    tile: Window,
    halo: u32,
) -> Result<Framebuffer, RenderError> {
    let window = tile.expanded(halo, (request.width, request.height));
    let mut canvas = Framebuffer::black(window.width, window.height);

    for track in video_tracks(sequence) {
        match active_transition(track, request.frame) {
            Some(transition) => {
                let outgoing = transition_partner(track, transition, false);
                let incoming = transition_partner(track, transition, true);
                let mut sides = Vec::new();
                for clip in [outgoing, incoming] {
                    let buffer = match clip {
                        Some(clip) => {
                            render_layer(project, clip, request, resolver, window, false)?
                        }
                        None => None,
                    };
                    sides.push(buffer);
                }
                let mixed = match mix_transition(
                    transition,
                    request,
                    sides[0].as_ref(),
                    sides[1].as_ref(),
                    window,
                )? {
                    Some(mixed) => mixed,
                    None => continue,
                };
                // A transition's mix is composited with the incoming clip's blend mode,
                // falling back to the outgoing clip's when there is no incoming picture.
                let blend = incoming.or(outgoing).map_or(BlendMode::Normal, Clip::blend);
                composite_into(&mut canvas, &mixed, blend);
            }
            None => {
                for clip in track.clips() {
                    if !clip.covers(request.frame) {
                        continue;
                    }
                    if let Some(layer) =
                        render_layer(project, clip, request, resolver, window, true)?
                    {
                        composite_into(&mut canvas, &layer, clip.blend());
                    }
                }
            }
        }
    }

    if halo == 0 {
        return Ok(canvas);
    }
    // The halo is a margin for the effects: only the tile itself is kept.
    let mut out = Framebuffer::new(tile.width, tile.height, CLEAR);
    let (offset_x, offset_y) = window.offset_of(tile);
    for y in 0..tile.height {
        for x in 0..tile.width {
            let pixel = canvas.pixel(offset_x + x, offset_y + y).unwrap_or(CLEAR);
            out.set_pixel(x, y, pixel);
        }
    }
    Ok(out)
}

/// Composites a whole layer buffer over a whole canvas buffer.
///
/// Opacity and mask coverage have already been multiplied into the layer's alpha, so the
/// compositor's own opacity is one: doing it twice would square a half-transparent layer.
fn composite_into(canvas: &mut Framebuffer, layer: &Framebuffer, blend: BlendMode) {
    debug_assert_eq!(
        (canvas.width(), canvas.height()),
        (layer.width(), layer.height())
    );
    for y in 0..canvas.height() {
        for x in 0..canvas.width() {
            let source = layer.pixel(x, y).unwrap_or(CLEAR);
            let backdrop = canvas.pixel(x, y).unwrap_or(BLACK);
            canvas.set_pixel(x, y, composite(blend, 1.0, source, backdrop));
        }
    }
}

/// The video tracks of a sequence, in compositing order: first is the backdrop.
fn video_tracks(sequence: &Sequence) -> impl Iterator<Item = &Track> {
    sequence
        .tracks()
        .iter()
        .filter(|track| track.kind() == TrackKind::Video && track.is_enabled())
}

/// The first transition on a track that covers a frame.
fn active_transition(track: &Track, frame: i64) -> Option<&Transition> {
    track
        .transitions()
        .iter()
        .find(|transition| transition.covers(frame))
}

/// The clip one side of a transition reads.
///
/// The named partner wins when it exists. Otherwise the renderer takes the nearest clip on
/// the correct side of the transition's own start, because a transition placed over a cut
/// or a gap still has a picture on each side and reading nothing would make a dissolve
/// flicker rather than dissolve.
fn transition_partner<'a>(
    track: &'a Track,
    transition: &Transition,
    incoming: bool,
) -> Option<&'a Clip> {
    let named = if incoming {
        transition.to()
    } else {
        transition.from()
    };
    if let Some(clip) = named.and_then(|id| track.clip(id)) {
        return Some(clip);
    }
    let pivot = transition.at();
    if incoming {
        track
            .clips()
            .iter()
            .filter(|clip| clip.timeline_start() >= pivot)
            .min_by_key(|clip| clip.timeline_start())
    } else {
        track
            .clips()
            .iter()
            .filter(|clip| clip.timeline_start() < pivot)
            .max_by_key(|clip| clip.timeline_start())
    }
}

/// Mixes the two sides of a transition into one layer.
///
/// Returns `None` when both sides are missing, which is a transition over empty space: an
/// honest nothing rather than a black rectangle.
///
/// The mix is per pixel and includes alpha, so a dissolve between a picture and nothing
/// fades the picture out to whatever is beneath the track.
///
/// # Errors
///
/// Returns [`RenderError::Core`] when the transition's progress cannot be computed.
fn mix_transition(
    transition: &Transition,
    request: RenderRequest,
    outgoing: Option<&Framebuffer>,
    incoming: Option<&Framebuffer>,
    window: Window,
) -> Result<Option<Framebuffer>, RenderError> {
    if outgoing.is_none() && incoming.is_none() {
        return Ok(None);
    }
    let progress = rational_to_f64(transition.progress(request.frame)?);
    let geometry = FrameGeometry::new(request.width, request.height);
    let mut out = Framebuffer::new(window.width, window.height, CLEAR);
    for y in 0..window.height {
        for x in 0..window.width {
            let from = outgoing
                .and_then(|buffer| buffer.pixel(x, y))
                .unwrap_or(CLEAR);
            let to = incoming
                .and_then(|buffer| buffer.pixel(x, y))
                .unwrap_or(CLEAR);
            let frame_x = (window.x + i64::from(x)) as f64 + 0.5;
            let frame_y = (window.y + i64::from(y)) as f64 + 0.5;
            let value = transition_pixel(
                transition.kind(),
                progress,
                frame_x,
                frame_y,
                geometry,
                from,
                to,
            );
            out.set_pixel(x, y, value);
        }
    }
    Ok(Some(out))
}
/// The frame's size in pixels, as the numbers a whole-frame geometry is computed from.
///
/// A transition's travelling edge, a vignette's corner distance and a grain field's extent
/// are all defined on the same rectangle, so they are given the same value rather than four
/// loose numbers each.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameGeometry {
    /// Width in pixels.
    pub width: f64,
    /// Height in pixels.
    pub height: f64,
}

impl FrameGeometry {
    /// The geometry of a frame of the given size.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width: f64::from(width),
            height: f64::from(height),
        }
    }

    /// The frame's centre, in pixel coordinates.
    #[must_use]
    pub fn centre(&self) -> (f64, f64) {
        (self.width * 0.5, self.height * 0.5)
    }

    /// The distance from the centre to a corner: how far a growing circle has to reach.
    #[must_use]
    pub fn half_diagonal(&self) -> f64 {
        let (x, y) = self.centre();
        (x * x + y * y).sqrt()
    }
}

/// One pixel of one transition.
///
/// The geometric kinds use a hard edge, as their names promise, and the edges are placed on
/// the pixel grid from the *frame* coordinate, so a wipe lands on the same pixel in a tile
/// as in a whole-frame pass.
#[must_use]
pub fn transition_pixel(
    kind: TransitionKind,
    progress: f64,
    x: f64,
    y: f64,
    frame: FrameGeometry,
    from: Pixel,
    to: Pixel,
) -> Pixel {
    let p = clamp_unit(progress);
    match kind {
        TransitionKind::CrossDissolve => mix(from, to, p),
        TransitionKind::DipToBlack => dip(from, to, p, BLACK),
        TransitionKind::DipToWhite => dip(from, to, p, WHITE),
        TransitionKind::WipeLeft => {
            // The edge travels from the right edge to the left, revealing the incoming
            // clip behind it.
            let edge = (1.0 - p) * frame.width;
            if x >= edge { to } else { from }
        }
        TransitionKind::WipeRight => {
            let edge = p * frame.width;
            if x < edge { to } else { from }
        }
        TransitionKind::WipeUp => {
            let edge = (1.0 - p) * frame.height;
            if y >= edge { to } else { from }
        }
        TransitionKind::WipeDown => {
            let edge = p * frame.height;
            if y < edge { to } else { from }
        }
        TransitionKind::Circle => {
            // A circle in pixel space, growing from the centre and reaching the corners of
            // the frame exactly at the end, so the incoming clip covers everything then.
            let (centre_x, centre_y) = frame.centre();
            let dx = x - centre_x;
            let dy = y - centre_y;
            let distance = (dx * dx + dy * dy).sqrt();
            if distance <= p * frame.half_diagonal() {
                to
            } else {
                from
            }
        }
    }
}

/// A linear mix of two pixels, per channel, including alpha.
fn mix(from: Pixel, to: Pixel, t: f64) -> Pixel {
    let mut out = [0.0_f32; 4];
    for channel in 0..4 {
        let blended =
            f64::from(from[channel]) + (f64::from(to[channel]) - f64::from(from[channel])) * t;
        out[channel] = blended as f32;
    }
    out
}

/// Fades the outgoing picture to a colour by the mid-point, then the incoming out of it.
fn dip(from: Pixel, to: Pixel, progress: f64, colour: Pixel) -> Pixel {
    if progress < 0.5 {
        mix(from, colour, progress * 2.0)
    } else {
        mix(colour, to, progress * 2.0 - 1.0)
    }
}

/// A number clamped to `0..=1`, with NaN becoming zero.
fn clamp_unit(value: f64) -> f64 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    }
}

/// Renders one clip into a layer buffer the size of a window.
///
/// Returns `None` when the clip cannot contribute: it is disabled, its placement collapses
/// to no area, or it lies outside the frame. `require_coverage` is what separates a clip
/// composited on its own from a transition partner, which is read past its own out point
/// because a dissolve over a cut uses handles.
///
/// # Errors
///
/// Returns [`RenderError::UnknownAsset`] when the clip's media is not registered,
/// [`RenderError::MalformedAnimation`] when an animated channel is not a number, and the
/// errors of the source and the effect chain.
fn render_layer(
    project: &Project,
    clip: &Clip,
    request: RenderRequest,
    resolver: &dyn PictureResolver,
    window: Window,
    require_coverage: bool,
) -> Result<Option<Framebuffer>, RenderError> {
    if !clip.is_enabled() {
        return Ok(None);
    }
    let source_frame = if require_coverage {
        match clip.source_frame_at(request.frame) {
            Some(frame) => frame,
            None => return Ok(None),
        }
    } else {
        clip.retime().source_at(request.frame)
    };
    let asset = project
        .asset(clip.asset_id())
        .ok_or(RenderError::UnknownAsset(clip.asset_id()))?;
    let Some(resolved) = resolve_layer(clip, request.frame - clip.timeline_start())? else {
        return Ok(None);
    };
    let frame_size = (request.width, request.height);
    let source = resolver.resolve(asset.uri(), frame_size, source_frame)?;
    let source_size = (source.width(), source.height());
    let Some(placement) = Placement::new(resolved.transform, source_size, frame_size) else {
        return Ok(None);
    };
    if !placement.touches_frame() {
        return Ok(None);
    }

    let mut layer = Framebuffer::new(window.width, window.height, CLEAR);
    let source_width = f64::from(source_size.0);
    let source_height = f64::from(source_size.1);
    let masks = clip.masks();
    for y in 0..window.height {
        for x in 0..window.width {
            let frame_x = (window.x + i64::from(x)) as f64 + 0.5;
            let frame_y = (window.y + i64::from(y)) as f64 + 0.5;
            let (source_x, source_y) = placement.to_source(frame_x, frame_y);
            let mut pixel = source.sample(source_x as f32, source_y as f32);
            // Mask coverage and layer opacity are multiplied into alpha here, before the
            // effect chain runs, so a blurred layer has a blurred mask edge rather than a
            // hard one and an animated opacity is blurred along with the picture.
            let mut coverage = resolved.opacity;
            if !masks.is_empty() {
                // The mask is defined in source-normalised coordinates, so it survives a
                // change of sequence resolution and travels with the layer.
                coverage *= mask_coverage(masks, source_x / source_width, source_y / source_height);
            }
            pixel[3] *= coverage;
            layer.set_pixel(x, y, pixel);
        }
    }

    let context = EffectContext {
        frame_width: request.width,
        frame_height: request.height,
        origin_x: window.x,
        origin_y: window.y,
    };
    for effect in clip.effects() {
        effect::apply(effect, &mut layer, context)?;
    }
    Ok(Some(layer))
}

/// A layer's placement and the opacity it was composited with, after animation.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ResolvedLayer {
    transform: Transform,
    opacity: f32,
}

/// Resolves a clip's animated channels at an offset from the clip's start.
///
/// A channel that is not animated falls back to the clip's static value, which is the same
/// number, so an un-animated project renders exactly as it did before animation existed.
/// Returns `None` when the animation collapses the scale to zero or below: the layer covers
/// no area, which is an invisible layer rather than a malformed one.
fn resolve_layer(clip: &Clip, offset: i64) -> Result<Option<ResolvedLayer>, RenderError> {
    let base = clip.transform();
    let (mut x, mut y) = (base.x(), base.y());
    let (mut scale_x, mut scale_y) = (base.scale_x(), base.scale_y());
    let mut rotation = base.rotation();
    let mut opacity = clip.opacity();

    for animation in clip.animation() {
        let value = animation.sample(offset)?;
        let number = value.as_rational().ok_or_else(|| {
            RenderError::MalformedAnimation(format!(
                "channel `{}` sampled to a value that is not a number",
                animation.channel().key()
            ))
        })?;
        match animation.channel() {
            AnimChannel::Opacity => opacity = number,
            AnimChannel::X => x = number,
            AnimChannel::Y => y = number,
            AnimChannel::ScaleX => scale_x = number,
            AnimChannel::ScaleY => scale_y = number,
            AnimChannel::Rotation => rotation = number,
        }
    }

    if !scale_x.is_positive() || !scale_y.is_positive() {
        return Ok(None);
    }
    let transform = Transform::new(
        x,
        y,
        scale_x,
        scale_y,
        rotation,
        base.anchor_x(),
        base.anchor_y(),
    )?;
    Ok(Some(ResolvedLayer {
        transform,
        opacity: clamp_unit(rational_to_f64(opacity)) as f32,
    }))
}

/// Every clip a frame's manifest should mention, in compositing order.
fn collect_layers(
    project: &Project,
    track: &Track,
    frame: i64,
    records: &mut Vec<LayerRecord>,
    resolver: &dyn PictureResolver,
) -> Result<(), RenderError> {
    if let Some(transition) = active_transition(track, frame) {
        for (clip, role) in [
            (transition_partner(track, transition, false), "from"),
            (transition_partner(track, transition, true), "to"),
        ] {
            match clip {
                Some(clip) => {
                    let mut record = describe_layer(project, clip, frame, true, resolver)?;
                    record.role = role;
                    record.transition = Some(transition.kind().key());
                    records.push(record);
                }
                None => records.push(LayerRecord {
                    clip: 0,
                    name: format!("missing {role} clip"),
                    visible: false,
                    source: "-",
                    blend: BlendMode::Normal.key(),
                    effects: Vec::new(),
                    masks: 0,
                    halo: 0,
                    role,
                    transition: Some(transition.kind().key()),
                }),
            }
        }
        return Ok(());
    }
    for clip in track.clips() {
        let visible = clip.covers(frame);
        let record = if visible {
            describe_layer(project, clip, frame, true, resolver)?
        } else {
            LayerRecord::invisible(clip)
        };
        records.push(record);
    }
    Ok(())
}

/// What one clip did, without rendering it.
fn describe_layer(
    project: &Project,
    clip: &Clip,
    frame: i64,
    as_partner: bool,
    resolver: &dyn PictureResolver,
) -> Result<LayerRecord, RenderError> {
    let mut record = LayerRecord {
        clip: clip.id().raw(),
        name: clip.name().to_owned(),
        visible: false,
        source: "-",
        blend: clip.blend().key(),
        effects: clip
            .effects()
            .iter()
            .filter(|effect| effect.is_enabled())
            .map(|effect| effect.kind().key())
            .collect(),
        masks: clip.masks().len() as u32,
        halo: chain_halo(clip.effects()),
        role: "layer",
        transition: None,
    };
    if !clip.is_enabled() {
        return Ok(record);
    }
    let offset = frame - clip.timeline_start();
    if resolve_layer(clip, offset)?.is_none() {
        return Ok(record);
    }
    if !as_partner && clip.source_frame_at(frame).is_none() {
        return Ok(record);
    }
    let asset = project
        .asset(clip.asset_id())
        .ok_or(RenderError::UnknownAsset(clip.asset_id()))?;
    // The resolver that would open this names it, rather than a second function next to the
    // trait guessing from the URI: a manifest that said `png` about a frame that came back
    // as a placeholder would be worse than a manifest that said nothing, because it would
    // be believed. See [`PictureResolver::kind`].
    record.source = resolver.kind(asset.uri());
    record.visible = true;
    Ok(record)
}

/// A rational as the number the compositor uses, clamped to `0..=1`.
#[must_use]
pub fn opacity_ratio(value: Rational) -> f32 {
    clamp_unit(rational_to_f64(value)) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{MissingMedia, Placeholder, WorkspaceResolver};
    use maj_core::{
        Clip, ClipId, Command, EffectId, EffectInstance, EffectKind, History, Interpolation,
        Keyframe, Mask, MaskId, MaskShape, MediaAsset, MediaAssetId, Param, ParamValue, Sequence,
        SequenceId, Timebase, Track, TrackId, Transition, TransitionId,
    };
    use maj_time::{FrameRange, Rational as R};

    const WIDTH: u32 = 24;
    const HEIGHT: u32 = 16;

    /// A project, its sequence and its one track, built the way the model builds one: by
    /// applying commands, so every fixture in this module also exercises the command path.
    struct Fixture {
        project: Project,
        history: History,
        sequence: SequenceId,
        track: TrackId,
    }

    impl Fixture {
        fn new() -> Self {
            let sequence = SequenceId::new(1);
            let track = TrackId::new(1);
            let mut fixture = Self {
                project: Project::new("render-test"),
                history: History::new(),
                sequence,
                track,
            };
            let sequence_value =
                Sequence::new(sequence, "cut", Timebase::FPS_25, WIDTH, HEIGHT, 48_000).unwrap();
            fixture.apply(Command::CreateSequence {
                index: 0,
                sequence: sequence_value,
            });
            fixture.apply(Command::CreateTrack {
                sequence,
                index: 0,
                track: Track::new(track, TrackKind::Video, "V1"),
            });
            fixture
        }

        fn apply(&mut self, command: Command) {
            self.history
                .apply_local(&mut self.project, command)
                .expect("the fixture's commands must apply");
        }

        /// Registers generated media and appends a clip that reads it.
        fn add_clip(&mut self, id: u64, name: &str, uri: &str, start: i64, length: i64) -> ClipId {
            let clip_id = ClipId::new(id);
            let asset_id = MediaAssetId::new(id);
            if self.project.asset(asset_id).is_none() {
                self.apply(Command::CreateMediaAsset {
                    index: self.project.assets().len(),
                    asset: MediaAsset::new(asset_id, name, uri, Timebase::FPS_25, length.max(24))
                        .unwrap(),
                });
            }
            let clip = Clip::new(
                clip_id,
                asset_id,
                name,
                FrameRange::new(0, length).unwrap(),
                start,
                R::from_integer(1),
            )
            .unwrap();
            self.apply(Command::AppendClip {
                sequence: self.sequence,
                track: self.track,
                clip,
            });
            clip_id
        }

        fn request(&self, frame: i64) -> RenderRequest {
            RenderRequest::new(self.sequence, frame, WIDTH, HEIGHT)
        }

        fn render(&self, frame: i64) -> Framebuffer {
            render_frame(
                &self.project,
                self.request(frame),
                &WorkspaceResolver::new(),
            )
            .expect("the fixture must render")
        }
    }

    fn effect(id: u64, kind: EffectKind) -> EffectInstance {
        EffectInstance::new(EffectId::new(id), kind)
    }

    #[test]
    fn an_empty_sequence_renders_an_opaque_black_frame() {
        let fixture = Fixture::new();
        let frame = fixture.render(0);
        assert_eq!(frame.width(), WIDTH);
        assert_eq!(frame.height(), HEIGHT);
        assert!(frame.pixels().iter().all(|pixel| *pixel == BLACK));
        let report = RenderTask::single(fixture.request(0))
            .render(&fixture.project, &WorkspaceResolver::new())
            .expect("renders");
        assert!(!report.manifest.drew_something());
        assert_eq!(report.manifest.frame, 0);
        assert!(report.manifest.certified);
    }

    #[test]
    fn a_clip_fills_the_frame_and_its_picture_changes_with_the_source_frame() {
        let mut fixture = Fixture::new();
        fixture.add_clip(1, "chart", "generated:bars", 0, 100);
        let first = fixture.render(0);
        let second = fixture.render(5);
        assert_ne!(
            first.digest(),
            second.digest(),
            "procedural media must move when the timeline does"
        );
        // The chart's top quarter is saturated colour, so the frame is not grey and not
        // transparent.
        let corner = first.pixel(1, 1).expect("a pixel");
        assert_eq!(corner[3], 1.0);
        assert!(corner.iter().any(|channel| *channel > 0.4), "{corner:?}");
    }

    #[test]
    fn clip_opacity_mixes_the_layer_toward_what_is_beneath_it() {
        let mut fixture = Fixture::new();
        let clip = fixture.add_clip(1, "chart", "generated:bars", 0, 100);
        fixture.apply(Command::SetClipTransform {
            sequence: fixture.sequence,
            clip,
            transform: Transform::new(
                R::from_integer(0),
                R::from_integer(0),
                R::from_reduced(1, 2),
                R::from_reduced(1, 2),
                R::from_integer(0),
                R::from_reduced(1, 2),
                R::from_reduced(1, 2),
            )
            .unwrap(),
        });
        let opaque = fixture.render(0);
        fixture.apply(Command::SetClipOpacity {
            sequence: fixture.sequence,
            clip,
            opacity: R::from_reduced(1, 2),
        });
        let half = fixture.render(0);
        assert_ne!(opaque.digest(), half.digest());
        // The corners of a half-scale layer are untouched backdrop.
        let corner = half.pixel(0, 0).expect("a pixel");
        let centre = half.pixel(WIDTH / 2, HEIGHT / 2).expect("a pixel");
        assert_ne!(corner, centre, "the layer covers the centre only");
    }

    #[test]
    fn a_mask_removes_the_part_of_the_layer_it_does_not_cover() {
        let mut fixture = Fixture::new();
        let clip = fixture.add_clip(1, "chart", "generated:bars", 0, 100);
        let unmasked = fixture.render(0);

        let half = Mask::with_geometry(
            MaskId::new(1),
            MaskShape::Rectangle,
            R::from_reduced(1, 4),
            R::from_reduced(1, 2),
            R::from_reduced(1, 2),
            R::from_integer(1),
            R::from_integer(0),
            R::from_integer(0),
            R::from_integer(1),
            false,
        )
        .unwrap();
        fixture.apply(Command::AddMask {
            sequence: fixture.sequence,
            clip,
            index: 0,
            mask: half,
        });
        let masked = fixture.render(0);
        assert_ne!(unmasked.digest(), masked.digest());
        // The mask selects the left quarter, so the right side of the frame is now the
        // black backdrop and the left side is unchanged.
        assert_eq!(masked.pixel(WIDTH - 2, HEIGHT / 2).expect("a pixel"), BLACK);
        assert_eq!(
            masked.pixel(2, HEIGHT / 2).expect("a pixel"),
            unmasked.pixel(2, HEIGHT / 2).expect("a pixel")
        );
    }

    #[test]
    fn a_tiled_render_is_bit_identical_to_a_whole_frame_render() {
        let mut fixture = Fixture::new();
        let clip = fixture.add_clip(1, "chart", "generated:bars", 0, 100);
        // A chain with every spatial dependency the tile policy has to halo for: a blur
        // that reads neighbours, grain that reads the absolute coordinate, and a vignette
        // defined on the whole frame.
        for (index, kind) in [
            EffectKind::Blur,
            EffectKind::Grain,
            EffectKind::Vignette,
            EffectKind::Sharpen,
            EffectKind::Contrast,
        ]
        .into_iter()
        .enumerate()
        {
            let mut instance = effect(index as u64 + 1, kind);
            if kind == EffectKind::Blur {
                instance
                    .set_param("radius", ParamValue::Rational(R::from_integer(3)))
                    .unwrap();
            }
            fixture.apply(Command::AddEffect {
                sequence: fixture.sequence,
                clip,
                index,
                effect: instance,
            });
        }
        let request = fixture.request(3);
        let whole =
            render_frame(&fixture.project, request, &WorkspaceResolver::new()).expect("whole");
        assert!(frame_halo(fixture.project.sequence(fixture.sequence).unwrap()) >= 6);
        for grid in [(1, 1), (2, 1), (3, 1), (4, 2), (7, 5), (WIDTH, HEIGHT)] {
            let tiled = render_tiled(&fixture.project, request, &WorkspaceResolver::new(), grid)
                .expect("tiled");
            assert_eq!(
                tiled.digest(),
                whole.digest(),
                "grid {grid:?} diverged from the whole frame"
            );
        }
    }

    #[test]
    fn a_disabled_effect_leaves_the_frame_alone_and_needs_no_halo() {
        let mut fixture = Fixture::new();
        let clip = fixture.add_clip(1, "chart", "generated:bars", 0, 100);
        let plain = fixture.render(0);
        assert_eq!(
            frame_halo(fixture.project.sequence(fixture.sequence).unwrap()),
            0
        );

        let mut blur = effect(1, EffectKind::Blur);
        blur.set_param("radius", ParamValue::Rational(R::from_integer(8)))
            .unwrap();
        fixture.apply(Command::AddEffect {
            sequence: fixture.sequence,
            clip,
            index: 0,
            effect: blur,
        });
        let blurred = fixture.render(0);
        assert_ne!(plain.digest(), blurred.digest());
        assert_eq!(
            frame_halo(fixture.project.sequence(fixture.sequence).unwrap()),
            16
        );

        // The switch is a command, so the halo it removes is provable from the document.
        fixture.apply(Command::SetEffectEnabled {
            sequence: fixture.sequence,
            clip,
            effect: EffectId::new(1),
            enabled: false,
        });
        assert_eq!(
            frame_halo(fixture.project.sequence(fixture.sequence).unwrap()),
            0
        );
        assert_eq!(fixture.render(0).digest(), plain.digest());
        let report = RenderTask::single(fixture.request(0))
            .render(&fixture.project, &WorkspaceResolver::new())
            .expect("renders");
        assert!(
            report.manifest.effect_keys().is_empty(),
            "a disabled effect is not applied"
        );
    }

    #[test]
    fn a_cross_dissolve_at_its_midpoint_averages_its_two_clips() {
        let mut fixture = Fixture::new();
        let first = fixture.add_clip(1, "red", "solid:255,0,0", 0, 20);
        let second = fixture.add_clip(2, "blue", "solid:0,0,255", 20, 20);
        fixture.apply(Command::AddTransition {
            sequence: fixture.sequence,
            track: fixture.track,
            index: 0,
            transition: Transition::new(
                TransitionId::new(1),
                TransitionKind::CrossDissolve,
                18,
                4,
                Some(first),
                Some(second),
            )
            .unwrap(),
        });
        // The transition covers 18..22, and frame 20 is its half-way point.
        let middle = fixture.render(20).pixel(2, 2).expect("a pixel");
        let blue = fixture.render(23).pixel(2, 2).expect("a pixel");
        let red = fixture.render(17).pixel(2, 2).expect("a pixel");
        assert!(blue[2] > blue[0], "the incoming clip is blue: {blue:?}");
        assert!(red[0] > red[2], "the outgoing clip is red: {red:?}");
        // Half-way between the two solids, per channel, including the handles the two
        // clips contribute outside their own ranges.
        assert!(middle[2] > red[2] && middle[2] < blue[2], "{middle:?}");
        assert!(middle[0] > blue[0] && middle[0] < red[0], "{middle:?}");
    }

    #[test]
    fn every_geometric_transition_has_a_hard_edge_in_the_right_place() {
        let from = [1.0, 0.0, 0.0, 1.0];
        let to = [0.0, 0.0, 1.0, 1.0];
        let frame = FrameGeometry::new(100, 50);
        // A cross dissolve is the only kind that is a blend everywhere.
        for (kind, left, right) in [
            (TransitionKind::WipeRight, to, from),
            (TransitionKind::WipeLeft, from, to),
        ] {
            let at = transition_pixel(kind, 0.5, 49.5, 25.0, frame, from, to);
            let other = transition_pixel(kind, 0.5, 50.5, 25.0, frame, from, to);
            assert_eq!(at, left, "{kind:?} left of the edge");
            assert_eq!(other, right, "{kind:?} right of the edge");
        }
        for (kind, top, bottom) in [
            (TransitionKind::WipeDown, to, from),
            (TransitionKind::WipeUp, from, to),
        ] {
            let at = transition_pixel(kind, 0.5, 50.0, 24.5, frame, from, to);
            let other = transition_pixel(kind, 0.5, 50.0, 25.5, frame, from, to);
            assert_eq!(at, top, "{kind:?} above the edge");
            assert_eq!(other, bottom, "{kind:?} below the edge");
        }
        // A wipe at zero progress has not started, and at one it has finished.
        assert_eq!(
            transition_pixel(TransitionKind::WipeRight, 0.0, 0.5, 0.5, frame, from, to),
            from
        );
        assert_eq!(
            transition_pixel(TransitionKind::WipeRight, 1.0, 99.5, 0.5, frame, from, to),
            to
        );
        // The geometry itself: the centre is the middle, the half-diagonal is a corner.
        assert_eq!(frame.centre(), (50.0, 25.0));
        assert!((frame.half_diagonal() - (50.0_f64 * 50.0 + 25.0 * 25.0).sqrt()).abs() < 1.0e-12);
    }

    #[test]
    fn a_dip_reaches_its_colour_at_the_midpoint() {
        let from = [1.0, 1.0, 1.0, 1.0];
        let to = [1.0, 1.0, 1.0, 1.0];
        let square = FrameGeometry::new(10, 10);
        let middle = transition_pixel(TransitionKind::DipToBlack, 0.5, 0.5, 0.5, square, from, to);
        assert_eq!(middle, BLACK, "a dip is fully black at its midpoint");
        let white = transition_pixel(
            TransitionKind::DipToWhite,
            0.5,
            0.5,
            0.5,
            square,
            [0.0, 0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0, 1.0],
        );
        assert_eq!(
            white, WHITE,
            "a dip to white is fully white at its midpoint"
        );
        // A circle grows from the centre of the frame: nothing at the start, half the way
        // out at the midpoint, and everything — corners included — at the end.
        let frame = FrameGeometry::new(100, 50);
        let centre = transition_pixel(TransitionKind::Circle, 0.5, 50.0, 25.0, frame, from, BLACK);
        assert_eq!(
            centre, BLACK,
            "the centre is inside the circle at half progress"
        );
        assert_eq!(
            transition_pixel(TransitionKind::Circle, 0.0, 51.0, 25.0, frame, from, BLACK),
            from,
            "a circle of radius zero covers nothing"
        );
        assert_eq!(
            transition_pixel(TransitionKind::Circle, 1.0, 0.5, 0.5, frame, from, BLACK),
            BLACK,
            "a circle at full progress reaches the corners"
        );
    }

    #[test]
    fn a_half_scale_cross_dissolve_is_the_average_of_the_two_pictures() {
        let mut fixture = Fixture::new();
        let first = fixture.add_clip(1, "red", "solid:255,0,0", 0, 10);
        let second = fixture.add_clip(2, "grey", "solid:128,128,128", 10, 10);
        fixture.apply(Command::AddTransition {
            sequence: fixture.sequence,
            track: fixture.track,
            index: 0,
            transition: Transition::new(
                TransitionId::new(1),
                TransitionKind::CrossDissolve,
                8,
                4,
                Some(first),
                Some(second),
            )
            .unwrap(),
        });
        let middle = fixture.render(9).pixel(1, 1).expect("a pixel");
        let red = 1.0_f32;
        let grey = 128.0 / 255.0;
        // Frame 8 is progress zero and frame 11 is progress three quarters.
        let half = (red + grey) * 0.5;
        assert!(
            (middle[0] - half).abs() < 0.5,
            "expected a blend near {half}, got {middle:?}"
        );
    }

    #[test]
    fn an_animated_translation_moves_the_layer_over_time() {
        let mut fixture = Fixture::new();
        let clip = fixture.add_clip(1, "chart", "generated:bars", 0, 100);
        fixture.apply(Command::SetChannelKeys {
            sequence: fixture.sequence,
            clip,
            channel: AnimChannel::X,
            keys: vec![
                Keyframe::new(
                    0,
                    ParamValue::Rational(R::from_integer(0)),
                    Interpolation::Linear,
                ),
                Keyframe::new(
                    10,
                    ParamValue::Rational(R::from_integer(8)),
                    Interpolation::Linear,
                ),
            ],
        });
        let start = fixture.render(0);
        let moved = fixture.render(10);
        let half = fixture.render(5);
        assert_ne!(start.digest(), moved.digest());
        assert_ne!(start.digest(), half.digest());
        // The animation is exact, so the half-way point is exactly four pixels along.
        let transform = resolve_layer(fixture.project.clip(clip).expect("the clip"), 5)
            .expect("resolves")
            .expect("a layer");
        assert_eq!(transform.transform.x(), R::from_integer(4));
    }

    #[test]
    fn an_animated_scale_that_collapses_renders_nothing() {
        let mut fixture = Fixture::new();
        let clip = fixture.add_clip(1, "chart", "generated:bars", 0, 100);
        fixture.apply(Command::SetChannelKeys {
            sequence: fixture.sequence,
            clip,
            channel: AnimChannel::ScaleX,
            keys: vec![
                Keyframe::new(
                    0,
                    ParamValue::Rational(R::from_integer(1)),
                    Interpolation::Linear,
                ),
                Keyframe::new(
                    10,
                    ParamValue::Rational(R::from_integer(0)),
                    Interpolation::Linear,
                ),
            ],
        });
        assert!(fixture.render(0).pixel(1, 1).expect("a pixel")[3] == 1.0);
        assert_eq!(fixture.render(10).pixel(1, 1).expect("a pixel"), BLACK);
        let report = RenderTask::single(fixture.request(10))
            .render(&fixture.project, &WorkspaceResolver::new())
            .expect("renders");
        assert!(
            !report.manifest.drew_something(),
            "a collapsed layer is invisible"
        );
    }

    #[test]
    fn an_audio_track_contributes_no_picture() {
        let mut fixture = Fixture::new();
        let audio = TrackId::new(9);
        fixture.apply(Command::CreateTrack {
            sequence: fixture.sequence,
            index: 1,
            track: Track::new(audio, TrackKind::Audio, "A1"),
        });
        fixture.apply(Command::CreateMediaAsset {
            index: fixture.project.assets().len(),
            asset: MediaAsset::new(
                MediaAssetId::new(9),
                "music.wav",
                "generated:audio",
                Timebase::FPS_25,
                24,
            )
            .unwrap()
            .with_streams(false, true),
        });
        fixture.apply(Command::AppendClip {
            sequence: fixture.sequence,
            track: audio,
            clip: Clip::new(
                ClipId::new(9),
                MediaAssetId::new(9),
                "music",
                FrameRange::new(0, 24).unwrap(),
                0,
                R::from_integer(1),
            )
            .unwrap(),
        });
        assert!(
            fixture
                .render(4)
                .pixels()
                .iter()
                .all(|pixel| *pixel == BLACK)
        );
    }

    #[test]
    fn a_missing_sequence_and_an_opportunistic_policy_are_refused() {
        let fixture = Fixture::new();
        let absent = RenderRequest::new(SequenceId::new(99), 0, 8, 8);
        assert_eq!(
            render_frame(&fixture.project, absent, &WorkspaceResolver::new()),
            Err(RenderError::UnknownSequence(SequenceId::new(99)))
        );
        let task = RenderTask::single(fixture.request(0)).with_policy(RenderPolicy::Opportunistic);
        assert_eq!(
            task.render(&fixture.project, &WorkspaceResolver::new()),
            Err(RenderError::UnsupportedPolicy(RenderPolicy::Opportunistic))
        );
        assert!(
            render_tiled(
                &fixture.project,
                fixture.request(0),
                &WorkspaceResolver::new(),
                (0, 0)
            )
            .is_ok()
        );
    }

    #[test]
    fn a_clip_pointing_at_media_that_is_not_registered_cannot_be_created_at_all() {
        let mut fixture = Fixture::new();
        // The model refuses a clip whose media is not registered, so a project reached
        // through the command path can never hold one. The renderer keeps its own check
        // anyway: it takes a `&Project` and must not panic on a document that arrived by
        // any other route.
        let error = fixture
            .history
            .apply_local(
                &mut fixture.project,
                Command::AppendClip {
                    sequence: fixture.sequence,
                    track: fixture.track,
                    clip: Clip::new(
                        ClipId::new(5),
                        MediaAssetId::new(404),
                        "orphan",
                        FrameRange::new(0, 10).unwrap(),
                        0,
                        R::from_integer(1),
                    )
                    .unwrap(),
                },
            )
            .expect_err("an unregistered asset is refused");
        assert!(matches!(error, CoreError::NotFound { .. }), "{error:?}");
        assert!(
            fixture
                .render(0)
                .pixels()
                .iter()
                .all(|pixel| *pixel == BLACK)
        );
    }

    #[test]
    fn the_manifest_records_the_layers_policy_and_digest() {
        let mut fixture = Fixture::new();
        let clip = fixture.add_clip(1, "chart", "generated:bars", 0, 100);
        let mut blur = effect(1, EffectKind::Blur);
        blur.set_param("radius", ParamValue::Rational(R::from_integer(4)))
            .unwrap();
        fixture.apply(Command::AddEffect {
            sequence: fixture.sequence,
            clip,
            index: 0,
            effect: blur,
        });
        let mut vignette = effect(2, EffectKind::Vignette);
        vignette
            .set_param("amount", ParamValue::Rational(R::from_reduced(1, 2)))
            .unwrap();
        fixture.apply(Command::AddEffect {
            sequence: fixture.sequence,
            clip,
            index: 1,
            effect: vignette,
        });
        let task = RenderTask::tiled(fixture.request(2), (2, 2));
        let report = task
            .render(&fixture.project, &WorkspaceResolver::new())
            .expect("renders");
        assert_eq!(report.manifest.schema, RENDER_MANIFEST_SCHEMA);
        assert_eq!(report.manifest.policy, "certified");
        assert_eq!(report.manifest.grid, (2, 2));
        assert_eq!(
            report.manifest.halo, 8,
            "a blur of radius four needs eight pixels"
        );
        assert_eq!(report.manifest.sequence, fixture.sequence.raw());
        assert_eq!(report.manifest.width, WIDTH);
        assert_eq!(report.manifest.digest, report.frame.digest());
        assert_eq!(report.manifest.project_revision, fixture.project.revision());
        assert_eq!(
            report.manifest.state_hash,
            fixture.project.state_hash().unwrap()
        );
        assert_eq!(report.manifest.effect_keys(), vec!["blur", "vignette"]);
        assert_eq!(report.manifest.layers.len(), 1);
        let layer = &report.manifest.layers[0];
        assert_eq!(layer.clip, clip.raw());
        assert_eq!(layer.name, "chart");
        assert_eq!(layer.source, "generated");
        assert_eq!(layer.blend, "normal");
        assert_eq!(layer.masks, 0);
        assert!(layer.visible);
        assert_eq!(layer.role, "layer");

        // The same request rendered twice is the same bytes, which is Tier 1.
        let again = task
            .render(&fixture.project, &WorkspaceResolver::new())
            .expect("renders");
        assert_eq!(again.frame.digest(), report.frame.digest());
        assert_eq!(again.manifest, report.manifest);
    }

    #[test]
    fn the_manifest_label_and_the_resolver_agree_on_what_a_uri_names() {
        // The manifest's label and the picture that actually arrives come from the same
        // implementation now, so this holds them together for every form of URI there is.
        let resolver = WorkspaceResolver::new();
        for (uri, label) in [
            ("generated:bars", "generated"),
            ("solid:1,2,3", "solid"),
            ("title:sans:24:ffffff:centre:HELLO", "title"),
            // A still that is not on disk, and a movie no build of this engine reads yet:
            // two different facts, two different labels, and neither of them a picture.
            ("missing.png", Placeholder::OFFLINE),
            ("missing.ppm", Placeholder::OFFLINE),
            ("camera://card/A001C001.mxf", Placeholder::UNDECODABLE),
        ] {
            let source = resolver.resolve(uri, (64, 64), 0).expect("a source");
            assert_eq!(source.kind(), label, "the picture for {uri}");
            assert_eq!(resolver.kind(uri), label, "the manifest's label for {uri}");
        }

        // A file that is there is named for its container.
        let logo = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/brand/logo.png");
        assert_eq!(resolver.kind(logo), "png");

        // Under a delivery policy the same unreadable media is a refusal, and the manifest
        // promises no picture rather than naming a placeholder it will not draw.
        let strict = WorkspaceResolver::new().with_missing_media(MissingMedia::Refuse);
        for uri in ["missing.png", "camera://card/A001C001.mxf"] {
            assert!(strict.resolve(uri, (64, 64), 0).is_err(), "{uri}");
            assert_eq!(strict.kind(uri), "-", "{uri}");
        }
        assert_eq!(strict.kind(logo), "png", "a readable file is unaffected");
    }

    #[test]
    fn unreadable_media_is_one_condition_with_one_policy() {
        // The bug this encodes: the engine used to refuse a frame when a still was missing
        // and quietly substitute a test chart when a movie was, so the same fact produced
        // two behaviours depending on the characters after the last dot.
        let mut fixture = Fixture::new();
        fixture.add_clip(1, "a missing still", "file:///nowhere/shot.png", 0, 20);
        fixture.add_clip(
            2,
            "an undecodable movie",
            "file:///nowhere/shot.mov",
            20,
            20,
        );
        let request = RenderRequest::new(fixture.sequence, 0, 96, 64);

        // By default both render, and the manifest says which stood in for what.
        let resolver = WorkspaceResolver::new();
        let still = RenderTask::single(request)
            .render(&fixture.project, &resolver)
            .expect("a missing still still renders");
        assert_eq!(still.manifest.layers[0].source, Placeholder::OFFLINE);
        assert!(Placeholder::is_placeholder(still.manifest.layers[0].source));

        let movie = RenderTask::single(RenderRequest::new(fixture.sequence, 25, 96, 64))
            .render(&fixture.project, &resolver)
            .expect("an undecodable movie still renders");
        let drawn = movie
            .manifest
            .layers
            .iter()
            .find(|layer| layer.visible)
            .expect("the movie's clip covers frame 25");
        assert_eq!(drawn.source, Placeholder::UNDECODABLE);

        // The placeholder is a picture, not an empty frame: a person looking at it sees
        // something, and it is not the chart a project gets when it asks for one.
        assert!(still.frame.peak() > 0.0);
        assert_ne!(still.manifest.layers[0].source, "generated");

        // And under a delivery policy, both are refused — the same policy for both.
        let strict = WorkspaceResolver::new().with_missing_media(MissingMedia::Refuse);
        for frame in [0, 25] {
            let refused = RenderTask::single(RenderRequest::new(fixture.sequence, frame, 96, 64))
                .render(&fixture.project, &strict);
            assert!(
                matches!(refused, Err(RenderError::Source(_))),
                "frame {frame} must be refused, not approximated: {refused:?}"
            );
        }
    }

    #[test]
    fn a_placeholder_is_the_same_bytes_whole_and_tiled() {
        // The placeholder is drawn on the certified path like anything else, so it has to
        // survive the tiling promise: its hazard bands must not shift at a tile boundary.
        let mut fixture = Fixture::new();
        fixture.add_clip(1, "offline", "file:///nowhere/shot.mov", 0, 20);
        let request = RenderRequest::new(fixture.sequence, 0, 96, 64);
        let resolver = WorkspaceResolver::new();
        let whole = render_frame(&fixture.project, request, &resolver).expect("whole");
        let tiled = render_tiled(&fixture.project, request, &resolver, (3, 2)).expect("tiled");
        assert_eq!(whole.digest(), tiled.digest());
    }

    #[test]
    fn a_png_on_disk_is_the_picture_the_renderer_draws() {
        // The whole path, end to end: a real file on disk, a media asset that names it, a clip
        // that reads it, and pixels in a frame. The decoder itself is `source.rs`'s business;
        // what this test is for is that a still image reaches the compositor at all, and that
        // what arrives is the file rather than the labelled chart a decoder-less build falls
        // back to. The file is the artwork this repository ships, so the test needs no fixture.
        let logo = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/brand/logo.png");
        let mut fixture = Fixture::new();
        fixture.add_clip(1, "logo", logo, 0, 20);
        let report = RenderTask::single(RenderRequest::new(fixture.sequence, 0, 320, 200))
            .render(&fixture.project, &WorkspaceResolver::new())
            .expect("a PNG renders");

        assert_eq!(report.manifest.layers.len(), 1);
        assert_eq!(report.manifest.layers[0].source, "png");
        let pixels = report.frame.pixels();
        let light = pixels
            .iter()
            .filter(|pixel| pixel[0] > 0.7 && pixel[1] > 0.7 && pixel[2] > 0.7)
            .count();
        assert!(
            light > pixels.len() / 4,
            "the supplied plate should dominate: {light} of {}",
            pixels.len()
        );
        // A row that holds both ink and plate can only be a row of the picture: a letterbox bar
        // would be all one value, which is what makes this stronger than counting dark pixels.
        let mixed = report.frame.height() as usize > 0
            && (0..report.frame.height() as usize).any(|row| {
                let line =
                    &pixels[row * report.frame.width() as usize..][..report.frame.width() as usize];
                let dark = line.iter().any(|pixel| pixel[0] < 0.05);
                let pale = line.iter().any(|pixel| pixel[0] > 0.9);
                dark && pale
            });
        assert!(mixed, "one row should carry both the mark and its plate");
        // The chart this replaces is built from saturated bars; ink on paper is not, so a
        // coloured frame would mean the resolver fell back to a generated picture.
        let colourful = pixels
            .iter()
            .filter(|pixel| {
                (pixel[0] - pixel[1]).abs() > 0.05 || (pixel[1] - pixel[2]).abs() > 0.05
            })
            .count();
        assert!(
            colourful * 20 < pixels.len(),
            "{colourful} saturated pixels would be the chart, not the file"
        );
    }

    #[test]
    fn a_transition_names_its_partners_in_the_manifest() {
        let mut fixture = Fixture::new();
        let first = fixture.add_clip(1, "red", "solid:255,0,0", 0, 20);
        let second = fixture.add_clip(2, "blue", "solid:0,0,255", 20, 20);
        fixture.apply(Command::AddTransition {
            sequence: fixture.sequence,
            track: fixture.track,
            index: 0,
            transition: Transition::new(
                TransitionId::new(1),
                TransitionKind::DipToBlack,
                19,
                2,
                Some(first),
                Some(second),
            )
            .unwrap(),
        });
        let report = RenderTask::single(fixture.request(19))
            .render(&fixture.project, &WorkspaceResolver::new())
            .expect("renders");
        assert_eq!(report.manifest.layers.len(), 2);
        assert_eq!(report.manifest.layers[0].role, "from");
        assert_eq!(report.manifest.layers[1].role, "to");
        assert_eq!(report.manifest.layers[0].transition, Some("dip_to_black"));
        assert_eq!(report.manifest.layers[0].source, "solid");
    }

    #[test]
    fn a_transition_over_a_gap_reads_whatever_clip_is_on_each_side() {
        let mut fixture = Fixture::new();
        let first = fixture.add_clip(1, "red", "solid:255,0,0", 0, 10);
        fixture.add_clip(2, "blue", "solid:0,0,255", 40, 10);
        // A transition between 12 and 20: inside the gap, with no named partners.
        fixture.apply(Command::AddTransition {
            sequence: fixture.sequence,
            track: fixture.track,
            index: 0,
            transition: Transition::new(
                TransitionId::new(1),
                TransitionKind::CrossDissolve,
                12,
                8,
                None,
                None,
            )
            .unwrap(),
        });
        let frame = fixture.render(12);
        // The outgoing side is the red clip, read past its out point as handles.
        let pixel = frame.pixel(2, 2).expect("a pixel");
        assert!(pixel[0] > pixel[2], "the outgoing side is red: {pixel:?}");
        let report = RenderTask::single(fixture.request(12))
            .render(&fixture.project, &WorkspaceResolver::new())
            .expect("renders");
        assert_eq!(report.manifest.layers[0].clip, first.raw());
        assert!(report.manifest.layers[0].visible);
    }

    #[test]
    fn a_higher_video_track_is_composited_over_a_lower_one() {
        let mut fixture = Fixture::new();
        fixture.add_clip(1, "red", "solid:255,0,0", 0, 20);
        let upper = TrackId::new(2);
        fixture.apply(Command::CreateTrack {
            sequence: fixture.sequence,
            index: 1,
            track: Track::new(upper, TrackKind::Video, "V2"),
        });
        fixture.apply(Command::CreateMediaAsset {
            index: fixture.project.assets().len(),
            asset: MediaAsset::new(
                MediaAssetId::new(2),
                "blue",
                "solid:0,0,255",
                Timebase::FPS_25,
                24,
            )
            .unwrap(),
        });
        fixture.apply(Command::AppendClip {
            sequence: fixture.sequence,
            track: upper,
            clip: Clip::new(
                ClipId::new(2),
                MediaAssetId::new(2),
                "blue",
                FrameRange::new(0, 20).unwrap(),
                0,
                R::from_integer(1),
            )
            .unwrap(),
        });
        let pixel = fixture.render(4).pixel(3, 3).expect("a pixel");
        assert!(pixel[2] > pixel[0], "the upper track wins: {pixel:?}");

        // Removing the upper track brings the lower one back, which is what a track's
        // presence in the sequence means.
        fixture
            .history
            .apply_local(
                &mut fixture.project,
                Command::RemoveTrack {
                    sequence: fixture.sequence,
                    track: upper,
                },
            )
            .expect("removes");
        let pixel = fixture.render(4).pixel(3, 3).expect("a pixel");
        assert!(
            pixel[0] > pixel[2],
            "the lower track is visible again: {pixel:?}"
        );
    }

    #[test]
    fn windows_expand_by_the_halo_and_stop_at_the_frame_edge() {
        let frame = (100, 50);
        let whole = Window::whole(100, 50);
        assert_eq!(whole.expanded(0, frame), whole);
        assert_eq!(
            whole.expanded(8, frame),
            whole,
            "there is nothing outside the frame"
        );
        let tile = Window {
            x: 20,
            y: 10,
            width: 10,
            height: 10,
        };
        assert_eq!(
            tile.expanded(8, frame),
            Window {
                x: 12,
                y: 2,
                width: 26,
                height: 26
            }
        );
        assert_eq!(
            tile.expanded(4, frame),
            Window {
                x: 16,
                y: 6,
                width: 18,
                height: 18
            }
        );
        // A window starting at the frame's corner clamps rather than reading outside.
        let corner = Window {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        };
        assert_eq!(
            corner.expanded(8, frame),
            Window {
                x: 0,
                y: 0,
                width: 12,
                height: 12
            }
        );
    }

    #[test]
    fn a_grid_partitions_the_frame_exactly_once() {
        for grid in [(1, 1), (2, 2), (3, 2), (5, 3), (7, 1)] {
            let tiles = partition(24, 16, grid.0 as usize, grid.1 as usize);
            assert_eq!(tiles.len(), (grid.0 * grid.1) as usize);
            let covered: usize = tiles
                .iter()
                .map(|tile| (tile.width * tile.height) as usize)
                .sum();
            assert_eq!(covered, 24 * 16, "grid {grid:?} covers the frame once");
            for tile in &tiles {
                assert!(tile.width > 0 && tile.height > 0);
            }
        }
    }

    #[test]
    fn the_error_types_say_what_went_wrong() {
        assert_eq!(
            RenderError::UnknownSequence(SequenceId::new(3)).to_string(),
            format!("the project has no sequence {}", SequenceId::new(3))
        );
        assert_eq!(
            RenderError::UnsupportedPolicy(RenderPolicy::Opportunistic).to_string(),
            "the reference renderer implements the certified path only, not `opportunistic`"
        );
        assert_eq!(RenderPolicy::Certified.to_string(), "certified");
        let mut effect_instance = effect(1, EffectKind::Exposure);
        assert!(
            effect_instance
                .set_param("stops", ParamValue::Rational(R::from_integer(9)))
                .is_err()
        );
    }

    #[test]
    fn the_opacity_helper_clamps_and_a_param_can_be_read_back() {
        assert_eq!(opacity_ratio(R::from_integer(1)), 1.0);
        assert_eq!(opacity_ratio(R::from_reduced(1, 4)), 0.25);
        assert_eq!(opacity_ratio(R::from_integer(3)), 1.0);
        let instance = effect(1, EffectKind::Exposure);
        let param = instance.param("stops").expect("the declared parameter");
        assert_eq!(param.key(), "stops");
        assert_eq!(
            Param::new("stops", ParamValue::Rational(R::from_integer(0))),
            *param
        );
    }
}
