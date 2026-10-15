# `maj-media`: the surface, designed against the seam

`maj-media` is layer 2 and today it is twenty-two lines: a role, a layer and a test that
pins both. This document is its surface — the types, the traits, where each one may be
named, and the four constraints that decided their shapes. It is written before the
decoder because the seam it has to fit already exists and is already tested, and a surface
designed after an FFmpeg binding has been written is a surface shaped by FFmpeg.

Nothing here is implemented yet. What is settled is what gets built in M2 and what it is
allowed to look like.

## 1. The seam, as it actually is

The renderer already does not know where pixels come from. `maj-render` holds a
`PictureResolver` and that is the whole contract (`crates/maj-render/src/source.rs`):

```rust
pub trait PictureResolver: fmt::Debug {
    fn resolve(
        &self,
        uri: &str,
        generated_size: (u32, u32),
        source_frame: i64,
    ) -> Result<Box<dyn PictureSource>, SourceError>;

    fn kind(&self, uri: &str) -> &'static str;
}
```

`resolve` is where a decoder goes. `kind` is what labels the manifest. Four properties of
that signature are design inputs rather than details, and three of them constrain
`maj-media` more than FFmpeg does:

* **`source_frame: i64` is a frame index, not a timestamp.** The retime map has already
  resolved what the clip is asking for. So `maj-media`'s video reader is addressed by frame
  index, and mapping an index to a container timestamp is the decoder's problem, exactly
  once, inside layer 2. No float seconds cross this boundary, in either direction.
* **`kind` returns `&'static str`.** A decoder cannot hand back a container name FFmpeg
  gave it. The vocabulary of kinds is therefore a closed enum in the workspace with a
  `const fn key(self) -> &'static str`, and an unrecognised container is an existing
  `Placeholder::UNDECODABLE`, not a new string. §4.
* **`resolve` takes `&self`.** A decoder is stateful — it holds an open file, a position
  and a codec context — so the adapter needs interior mutability. §7.
* **`resolve` returns a fresh `Box<dyn PictureSource>` per frame, and `PictureSource` is a
  sampler** (`sample(x, y) -> Pixel`, in scene-linear light). So a decoded frame is
  resolved once and sampled many times, which is what the trait's own documentation says
  it was shaped for. The decoder session must outlive the box; the box must not own the
  session. §7.

`SourceError` has exactly three variants and `Placeholder::for_error` maps them onto the
three labels P0.3 established:

| `SourceError` | Placeholder label | What it means |
|---|---|---|
| `Io` | `offline` | The file is not where the project says it is |
| `Malformed` | `damaged` | The bytes are not what the container claims |
| `Unsupported` | `undecodable` | This build has no decoder for it |

**`maj-media`'s error type must map onto those three and add no fourth.** P0.3's whole
point was that media which cannot be read is one condition with one policy; a decoder that
invented a fourth failure mode would reopen it. §3.

## 2. The layering problem, and where the adapter lives

`PictureResolver` is defined in `maj-render`, which is **layer 4**. `maj-media` is **layer
2**. Dependencies point downward only, so:

> **`maj-media` cannot implement `PictureResolver`.** That would require layer 2 to depend
> on layer 4.

This is not an obstacle; it is the architecture working. The split it forces is the right
one anyway:

```
layer 4  maj-render   MediaResolver: PictureResolver   <- the adapter lives here
                      ^ depends on maj-media, maj-color
layer 2  maj-media    trait VideoReader, DecodedFrame  <- the decoder lives here
                      ^ depends on maj-time, maj-core; FFmpeg is private
```

`maj-media` publishes a decoder vocabulary in Majestat's own types and knows nothing about
renderers, samplers or scene-linear light. `maj-render` gains an optional dependency on
`maj-media` and a `MediaResolver` that implements `PictureResolver` by delegating to it —
wrapping a `DecodedFrame` in a type that implements `PictureSource`, and doing the colour
conversion with `maj-color` (layer 3), which is where that arithmetic belongs and where it
is already on the certified path.

So the decoder never produces a `Pixel` and the renderer never names a codec. The
conversion between the two worlds happens in one adapter, in one crate, and it is the only
place that has to know both vocabularies.

`WorkspaceResolver` is not replaced. `MediaResolver` composes it: `generated:`, `solid:`,
`title:`, `.png` and `.ppm` keep resolving exactly as they do now and keep their tests, and
the new decoder answers only for what `WorkspaceResolver` currently calls `Unsupported`.
The existing `MissingMedia` policy still sits at the outside, so `--strict-media` keeps
working unchanged over a strictly larger set of readable media.

## 3. Errors

```rust
/// Why media could not be read. Maps onto maj-render's three labels and adds no fourth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaError {
    /// The file could not be opened or read.            -> offline
    Io(String),
    /// The bytes are not what the container claims.     -> damaged
    Malformed(String),
    /// No decoder in this build reads it.               -> undecodable
    Unsupported(String),
}
```

Three variants, the same three as `SourceError`, deliberately. The adapter's conversion is
total and loses nothing, and no `maj-media` failure can fail to be one of the three things
a placeholder can say.

Decode is full of conditions that are *not* errors and must not become them: a frame that
needs more packets, a stream that ends, a seek that landed before the keyframe it needed.
Those are states of the reader, handled inside layer 2, and they never reach a caller as an
error — a reader that returns `Io` because it has not fed the decoder enough packets yet
would make a hazard placeholder out of a working file.

One condition deserves naming because it is the one the project already recorded as a debt:
a file that is **present but damaged**. `PictureResolver::kind` costs a stat and cannot tell
a damaged file from a readable one without reading it, so today a damaged still is named for
its container in the manifest while the frame gets a `damaged` placeholder. `maj-media`'s
probe is what repays that debt: `kind` can ask for a *cached* probe result and answer
`damaged` honestly, without a decode. §4.

## 4. Probe: what a file is, without decoding it

Probe is the first thing built and the thing most of M2's gate rests on. It opens a file,
reads its headers and answers in Majestat's types.

```rust
/// Everything known about a file after reading its headers and no frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaProbe {
    pub container: ContainerKind,
    pub streams: Vec<StreamInfo>,
    /// The decoder build that produced this, so a cache entry cannot outlive its decoder.
    pub decoder: DecoderIdentity,
}

/// The closed vocabulary of containers, because `PictureResolver::kind` is `&'static str`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerKind {
    QuickTime, Mp4, Mxf, Matroska, Avi, ImageSequence, Wav,
    /// Readable by no decoder in this build.
    Unknown,
}

impl ContainerKind {
    pub const fn key(self) -> &'static str { /* "mov", "mp4", "mxf", ... */ }
}
```

`ContainerKind::key` is what feeds `MediaResolver::kind`, which is why it is a `const fn`
over a closed enum: the manifest's vocabulary stays finite and documented, and a container
nobody has mapped is `Unknown` and labels `undecodable` rather than leaking a string from a
library into a document format.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamInfo {
    Video(VideoStream),
    Audio(AudioStream),
    /// Present, counted, not read: timecode, data and subtitle tracks.
    Other { index: usize, role: StreamRole },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoStream {
    pub index: usize,
    pub codec: CodecKind,
    /// Coded size, before rotation and before the sample aspect is applied.
    pub coded_size: (u32, u32),
    /// Pixel aspect as an exact ratio. Never an f64.
    pub sample_aspect: Rational,
    /// The timebase the project will address this stream in.
    pub timebase: Timebase,
    /// The stream's own timebase, for the index-to-timestamp map.
    pub stream_timebase: Rational,
    /// Frame count when the container states it or it can be derived exactly.
    pub frames: Option<u64>,
    pub rate: FrameRateKind,
    pub rotation: Rotation,
    pub field_order: FieldOrder,
    pub picture: PictureFormat,
}

/// Rotation as a quarter-turn count. A rotation is not an angle, so it is not a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation { None, Quarter, Half, ThreeQuarters }

/// Constant or variable, and variable is a different kind of stream, not a flag on one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameRateKind {
    Constant(Timebase),
    /// Each frame's presentation time, in the stream's own timebase. Exact integers.
    Variable { timestamps: Vec<i64> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldOrder { Progressive, TopFirst, BottomFirst, SingleTop, SingleBottom }

/// How samples are laid out and what they mean. No conversion is implied by naming it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PictureFormat {
    pub layout: PlaneLayout,     // Yuv420, Yuv422, Yuv444, Rgb, ...
    pub depth: BitDepth,         // Eight, Ten, Twelve, Sixteen
    pub range: SampleRange,      // Limited | Full
    pub matrix: ColourMatrix,    // Bt709 | Bt601 | Bt2020Ncl | ...
    pub primaries: ColourPrimaries,
    pub transfer: TransferFunction,  // maj_color's, which is layer 3 -> allowed
    pub chroma_siting: ChromaSiting,
}
```

Three of M2's gate axes are in those types on purpose, because the gate is "variable frame
rate, interlacing and rotation handled correctly" and a thing that is not in the model
cannot be handled correctly:

* **Variable frame rate is a different `FrameRateKind`, not a boolean.** A VFR stream
  carries its per-frame timestamps, as exact integers in the stream's timebase, so the
  index-to-timestamp map is a lookup rather than a multiplication that is wrong for every
  frame after the first.
* **Interlacing is a declared field order**, so the renderer can refuse or de-interlace
  deliberately. Silently treating a field as a frame is the failure mode this prevents.
* **Rotation is a quarter-turn count, not degrees.** Four cases, exact, no angle
  arithmetic, and no chance of a 90 that is really 89.999.

`PictureFormat` declares colour metadata and converts nothing. `maj-media` has no opinion
about scene-linear light; it says what the samples are and lets layer 3 and 4 decide.

`DecoderIdentity` is what makes the probe cacheable and the render reproducible:

```rust
/// Which decoder produced an answer. Part of every cache key and of the render manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoderIdentity {
    pub backend: &'static str,   // "ffmpeg"
    pub version: String,         // "9.0.2"
    /// Hash of the library's own configuration string, per docs/FFMPEG.md §4.
    pub configuration: [u8; 32],
}
```

## 5. Reading frames

```rust
/// A source of decoded video frames, addressed by frame index.
pub trait VideoReader: fmt::Debug {
    fn stream(&self) -> &VideoStream;

    /// The frame at `index`, decoding and seeking as needed.
    ///
    /// Addressed by index because that is what the retime map produces. Timestamps,
    /// keyframe distance, packet feeding and reordering are this reader's problem.
    fn frame(&mut self, index: i64) -> Result<&DecodedFrame, MediaError>;
}

/// One decoded picture, in the format it was decoded in. Nothing is converted.
#[derive(Debug)]
pub struct DecodedFrame {
    format: PictureFormat,
    coded_size: (u32, u32),
    /// The index asked for, so a caller can prove it got what it asked for.
    index: i64,
    planes: Vec<Plane>,
}

#[derive(Debug)]
pub struct Plane {
    pub stride: usize,
    pub rows: usize,
    pub bytes: Vec<u8>,
}
```

`frame` returns a borrow, so the common case — resolve once, sample a million times —
allocates nothing per sample and the reader keeps ownership of its buffer.

Returning the frame *in its decoded format*, unconverted, is the load-bearing decision:

* **YUV-to-RGB and transfer decoding stay on the certified path.** They are arithmetic in
  the workspace, under ADR 0006's rules about reduction order and rounding, done by
  `maj-color` in the layer-4 adapter — not by swscale, whose results are reproducible for a
  build and not across builds, which is exactly the property Tier 2 cannot have. This is
  why `docs/FFMPEG.md` disables swscale, and the two decisions have to stay in step.
* **A ten-bit frame is never rounded to eight on the way in.** Integer rounding happens
  once, at the declared output boundary (ADR 0006), and a conversion in the decoder would
  be a second one nobody declared.

The rest of the surface, in the same shape and built in this order:

```rust
pub trait MediaBackend: fmt::Debug {
    fn identity(&self) -> &DecoderIdentity;
    fn probe(&self, path: &Path) -> Result<MediaProbe, MediaError>;
    fn open_video(&self, path: &Path, stream: usize)
        -> Result<Box<dyn VideoReader>, MediaError>;
    /// Hardware decode is capability-tested, never assumed (ADR 0005).
    fn hardware(&self) -> HardwareSupport;
}
```

* **Audio** (`AudioReader`, integer sample blocks, no resampling — same reasoning as
  swscale) is declared here and implemented in M9, except for what waveforms need.
* **Waveforms** are integer peak-and-RMS pairs per bucket, with the bucket size an exact
  divisor of the timebase so two zoom levels agree, and they are a product of probe-time
  streaming rather than of decode.
* **Thumbnails** are `frame()` plus the adapter, with no separate path, because a second
  path is a second set of colour decisions.

## 6. Hardware decode, and the certified path

ADR 0005 says hardware decode is capability-tested and not assumed zero-copy, and ADR 0006
says the certified path has fixed arithmetic. Together those force a rule worth stating
plainly:

> **Hardware decode is Tier 1 only. A certified render decodes in software.**

A hardware decoder's output depends on a driver and a GPU, so it cannot be part of a claim
that the same log and the same media produce the same bytes. Playback uses it; export and
`--verify` do not, and the manifest says which was used — which is the same labelling rule
Tier 1 already lives under.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardwareSupport {
    /// What the machine actually answered when asked, not what it is expected to do.
    pub d3d11va: bool,
    pub dxva2: bool,
}
```

The surface bridge to `maj-gpu` is **not** in this design: it is layer 3, it arrives with
M4, and promising it here would be promising a measurement nobody has taken. What M2 owes
is the measurement ADR 0005 asked for — the decoder-surface-to-texture cost, with the
software fallback designed first.

## 7. The adapter, and two shapes it is forced into

In `maj-render`:

```rust
pub struct MediaResolver<B: MediaBackend> {
    stills: WorkspaceResolver,
    backend: B,
    /// Open readers, keyed by URI. Behind a RefCell because `resolve` takes `&self`.
    readers: RefCell<HashMap<String, Box<dyn VideoReader>>>,
    /// Probe results, so `kind` can answer `damaged` without a decode.
    probes: RefCell<HashMap<String, Result<MediaProbe, MediaError>>>,
}

impl<B: MediaBackend> PictureResolver for MediaResolver<B> { /* ... */ }
```

Two consequences of the existing signature, recorded rather than discovered later:

* **`resolve(&self)` means interior mutability.** A decoder is stateful and the trait hands
  out a shared reference, so the reader cache is a `RefCell`. That is sound today:
  `PictureResolver` carries no `Send`/`Sync` bound and the renderer, including
  `render_tiled`, is single-threaded. **If `PictureResolver` ever gains `Send + Sync`** —
  which parallel tiles or M13's distributed render would want — this becomes a `Mutex` or a
  reader per thread, and that is a change to this type and not to the trait.
* **The box returned by `resolve` cannot borrow the reader**, because `resolve` returns an
  owned `Box<dyn PictureSource>` while the reader lives in the cache behind a `RefCell`. So
  the adapter converts: it decodes into the reader's buffer, then produces an owned,
  already-converted scene-linear sampler for the frame. That is one conversion and one
  allocation per resolved frame, not per sample, and it is where `maj-color` is applied.
  The alternative — handing out a borrow — would require changing `PictureResolver`, and
  the seam is not worth bending for an allocation that happens once a frame.

`kind` gets strictly better and stays `&'static str`: a cached probe lets it answer
`ContainerKind::key` for a readable file, `damaged` for one whose headers do not parse,
`offline` for one that is not there, and `undecodable` for a container with no mapping — all
without a decode, which repays the debt recorded in `.planning/STATE.md`.

## 8. `unsafe`, which is the real decision in M2

`[workspace.lints.rust] unsafe_code = "forbid"`, and `maj-win32` is the single crate that
relaxes it (ADR 0013). **FFmpeg is a C library and binding it requires `unsafe`.** There is
no version of M2 that does not confront this, so it is better confronted as a decision than
as a diff.

Three shapes, and they are genuinely different:

1. **Relax the lint in `maj-media`.** Smallest diff, worst shape: the crate with the
   vocabulary, the probe logic, the timestamp arithmetic and the waveform buckets becomes a
   crate where `unsafe` is legal everywhere, and the one property that made `maj-win32`'s
   exception reviewable — *all of the unsafe is in one small crate that does one thing* —
   is lost.
2. **A leaf FFI crate.** `maj-media-sys` at layer 2: declarations, RAII wrappers, and
   nothing else — no probe logic, no model types, no decisions. It is the only crate
   besides `maj-win32` with `unsafe_code = "allow"`, and `maj-media` stays `forbid` and
   contains everything worth reviewing for correctness. The exception stays the kind of
   exception ADR 0013 made: narrow, visible, and about a platform edge.
3. **Out of process.** A helper executable owns FFmpeg; `maj-media` talks to it over a
   pipe and contains no `unsafe` at all. This is strictly the most robust: M2's gate is "a
   hundred-file corpus with zero crashes", and a malformed file that crashes libavcodec
   would kill a helper rather than the editor — which is also what M15 wants from crash
   containment. It costs per-frame latency and makes the hardware-decode surface bridge
   much harder, since a GPU surface would have to cross a process boundary.

**Recommended: (2), with (3) kept as a backend rather than a rewrite.** The `MediaBackend`
trait in §5 is what makes that possible — an out-of-process backend is another
implementation of it, so the robustness escape hatch stays available without being paid for
up front, and the choice can be made against M2's measured crash rate instead of against a
prediction.

**This needs an ADR, and it is not written here.** Relaxing a workspace-wide `forbid` is a
project-level decision of the same weight as ADR 0013, and this document's job is to show
that the design has a place for the answer, not to pick it unilaterally.

## 9. What M2 builds, in order

Each step is usable and measurable on its own, which is what keeps the phase from being one
large unverifiable landing.

1. **The vocabulary** — every type in §3 and §4, with no backend behind them. Compiles,
   tests, no FFmpeg. The layering and the `&'static str` closure are provable here.
2. **The `unsafe` decision** — ADR, then `maj-media-sys` as a leaf with the pinned library
   behind it, and `docs/FFMPEG.md` §4's three checks as its first test. The gate before any
   decode is that the loaded library proves it is the LGPL build.
3. **Probe** — `MediaBackend::probe` over the corpus. The measurement is the corpus itself:
   a hundred files, zero crashes, and rotation, field order and VFR read correctly off each
   one. This is most of M2's gate and it needs no frames.
4. **`MediaResolver` in `maj-render`** — the adapter, software decode, `maj-color` doing the
   conversion. The measurement is a frame of real footage rendered to a still, and a
   manifest naming the container instead of `undecodable`. This is the moment the viewer
   stops being empty.
5. **Exactness** — the index-to-timestamp map under VFR, seeking to a frame and getting
   *that* frame, and three renders of one revision producing identical bytes with a decoder
   in the path. Determinism with a decoder in it is a stronger claim than the current one
   and needs its own test, not a reuse of M1's.
6. **Hardware decode, measured** — `HardwareSupport` answered by capability test, the cost
   recorded, Tier 1 only, software fallback on every path.

Waveforms and thumbnails follow 3 and 4 and need nothing new. Audio decode is declared in 3
and implemented in M9.

## 10. The constraints, and where each is answered

| Constraint | Where it is honoured |
|---|---|
| Rational time, never floats | `Rational`, `Timebase`, integer `pts`, `Rotation` as quarter-turns, `FrameRateKind::Variable` as exact timestamps, integer waveform buckets — §4, §5 |
| FFmpeg types never above layer 2 | FFmpeg is private to a leaf `maj-media-sys`; `maj-media` publishes only Majestat types and re-exports nothing — §2, §8 |
| `unsafe` only in `maj-win32` | Not satisfiable with an in-process C binding; the narrowest answer is a second leaf-crate exception, and it needs an ADR — §8 |
| The certified path (ADR 0006) | No swscale and no swresample; conversion by `maj-color` in the layer-4 adapter; no rounding in the decoder; hardware decode is Tier 1 only; `DecoderIdentity` in the manifest and in every cache key — §4, §5, §6 |
| One condition, one policy (P0.3) | `MediaError` has the same three variants as `SourceError` and adds no fourth; `MissingMedia` and `--strict-media` keep working unchanged over a larger set of readable media — §2, §3 |
| Licence hygiene | `docs/FFMPEG.md`: the pinned LGPL build, the three runtime checks, the obligations |
