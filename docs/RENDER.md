# The renderer

`maj-render` is the reference renderer: the definition of what a frame *is*, on the
certified path, in portable arithmetic. This document says what it does and why, in the
order the pixels are built.

The one-sentence version: **a frame is a rectangle of scene-linear light, built by asking
each output pixel where it came from, and every choice a machine could disagree about is
made explicitly and named.**

## The pipeline

```
project + frame number
  ↓  sequence → its video tracks, in the order the sequence holds them
  ↓  per track: the active transition names its two partners, or each covering clip
  ↓  per layer:   animation → transform → placement → source sample → mask coverage
  ↓              effects, in declared order → composite over the backdrop
  ↓  the window is cropped out of the halo
frame of scene-linear light  →  one rounding, at the export boundary  →  integers
```

### 1. Tracks are the only z-order there is

The model has no per-clip depth, so the sequence's own track order is the compositing
order: the first video track is the backdrop and each later one is composited over the
result. Audio tracks are skipped entirely — this crate produces picture, and `maj-audio`
owns sound. A track that is disabled contributes nothing.

Within a track, clips are visited in the order the track stores them, which is the insertion
order the model treats as part of the state.

### 2. Placement is inverted, not forward-mapped

For every output pixel the renderer asks **which source coordinate landed here**. That is
the only formulation in which no output pixel is left unvisited, no source pixel is written
twice, and a rotated or scaled layer needs no scattering pass.

The model's `Transform` is applied scale → rotation → translation, so the inverse is
translation → rotation → scale, taken about the anchor:

```text
rel        = pixel − frame_centre − translate
rotated    = R(−θ) · rel
source     = anchor · source_size + rotated / scale
```

Rotation is clockwise on screen, which in a y-down coordinate system is the matrix
`[[cos, sin], [−sin, cos]]` applied to the inverse. A scale of zero cannot appear here: the
model refuses to build such a transform, and `Placement::new` refuses it again so a
renderer can never divide by zero into a NaN frame.

### 3. Coordinates, exactly

* A pixel's **centre is at `index + 1/2`**. `x = 0.5` is the centre of the first column, so
  an integer translation is an identity rather than a half-pixel shift.
* Sampling outside a picture **clamps to its edge**. A source that faded to transparency
  where the renderer looks for it would hide a framing bug.
* Bilinear taps are always visited in the same order and combined as
  `(a·(1−fx) + b·fx)·(1−fy) + (c·(1−fx) + d·fx)·fy`, so the result does not depend on a
  compiler's reassociation.
* Neutral rounding is **half away from zero**, everywhere, once.

### 4. Masks multiply into alpha before the effects

Mask geometry is **normalised to the source frame**: a centre of `1/2, 1/2` is the middle of
the picture whatever the sequence resolution. Coverage is multiplied into the layer's alpha
*before* the effect chain runs, which is what makes a blurred layer have a blurred mask edge
instead of the hard edge of a mask applied after the fact. Masks intersect, so their
coverages multiply; an inverted mask contributes its complement. A feather is a length in
the frame, divided by the shape's smaller half-size, so scaling a mask up scales its softness
with it.

Layer opacity is multiplied into alpha at the same moment, for the same reason, which is why
the compositor's own opacity is always one: applying it twice would square a half-transparent
layer.

### 5. Effects are frame-space and declare their reach

The ten effects of `EffectKind` are all implemented here, so a project that uses one renders
the same in this renderer and in a graded output.

Two of them need a platform transcendental and therefore do not call one: exposure uses
`2^stops` from `maj_color::det`, which builds it from multiplication and division, and gamma
uses an exact rational exponent rather than a rounded decimal. A gamma of one and an exposure
of zero stops are byte-identical to no effect at all, and a hue rotation of zero degrees
takes a fast path rather than a YIQ round trip that is merely close.

The two that depend on *where* a pixel is — the vignette's corner distance and the grain
field — are handed the frame's size and the window's absolute origin. Grain is a hash of the
absolute coordinate and the seed, not of a counter, because a counter makes a pixel's value
depend on how many pixels were visited before it.

Sharpen divides by nine with a divisor taken in double precision, and the box blur sums its
kernel in a fixed order rather than sliding a window, deliberately: a sliding sum accumulates
rounding differently depending on where it started, so the same pixel would come out
differently in a tile than in a whole-frame pass.

### 6. Transitions

A transition covers the half-open interval `[at, at + duration)` and names its partners. The
named partner wins; otherwise the renderer takes the nearest clip on the correct side of the
transition's own start, because a transition placed over a cut or a gap still has a picture
on each side and reading nothing would make a dissolve flicker instead of dissolve.

A partner is read **past its own out point** — a dissolve over a cut uses handles — so the
source frame comes from the retime map rather than from the clip's coverage.

| Kind | Behaviour |
|---|---|
| Cross dissolve | A per-channel blend, alpha included |
| Dip to black / white | The outgoing picture fades to the colour by the midpoint, the incoming one out of it. Alpha is mixed too, so a picture on a lower track stays visible: a dip is a property of its track, and a full-frame dip needs the dip on the topmost track |
| Wipe left / right | A hard edge travelling in the named direction, revealing the incoming clip behind it |
| Wipe up / down | The same on the vertical axis |
| Circle | A circle growing from the centre in pixel space, reaching the corners exactly at the end, so the incoming clip covers everything at the last frame |

Progress is exact: the first frame of the transition is `0/1` and the last is
`(duration − 1)/duration`, so there is no discontinuity at the hand-off to the incoming clip.
The mix is composited with the incoming clip's blend mode, falling back to the outgoing
clip's when there is no incoming picture.

### 7. Animation is sampled, not simulated

Each animatable channel is sampled at an offset in frames from the clip's start. The values
are `Rational`, the interpolation is `Hold` or exact `Linear`, and a channel that is not
animated falls back to the clip's static value — which is the same number, so an un-animated
project renders exactly as it did before animation existed. An animated scale that collapses
to zero means the layer covers no area: the renderer treats it as an invisible layer and says
so in the manifest rather than refusing the frame.

## Tiling, and why a tile equals the whole

`render_frame` renders the frame as one window. `render_tiled` renders it as a grid of
windows and must produce **the same bits**. Two properties make that true:

1. **A window is expanded by the sum of the supports of the effects in the chain.** Sum, not
   maximum: a sharpen reading one pixel of a blurred picture reads pixels that were built
   from six pixels away, so the margin has to be `6 + 1`, not `6`. `chain_halo` computes it,
   `frame_halo` takes the largest chain in the sequence, and the expansion is clamped to the
   frame because there is nothing outside it to read.
2. **Every frame-space value is computed from the absolute coordinate.** A tile's interior
   therefore sees exactly what a whole-frame pass would have given it, and no kernel ever
   reads past the expansion.

The interior is then copied out and the halo discarded. A grid of `1x1` is exactly
`render_frame`, and the test renders seven different grids — including one tile per pixel —
and compares digests.

What this does **not** claim: the tiled path is not parallel and is not faster. It exists
because correctness under tiling is the precondition for parallelism, and establishing it now
is cheaper than discovering a seam later.

## The manifest

Every render writes one, beside the still, as JSON:

| Field | Meaning |
|---|---|
| `schema` | Shape version of the manifest |
| `policy`, `certified` | Which determinism tier produced the pixels |
| `sequence`, `frame`, `width`, `height` | What was rendered |
| `project_revision`, `state_hash` | Which revision of which document |
| `halo`, `grid` | How the frame was divided, and how much margin the effects needed |
| `layers` | Every layer: clip, name, visibility, source kind, blend, enabled effects, mask count, halo, role, transition |
| `digest` | A hash of the frame's dimensions and every channel's bit pattern |
| `peak_thousandths` | The largest absolute channel value, in thousandths |

The manifest holds **no floating point**, for the same reason the project format does not: a
decimal rendering of a float is a place where two identical runs can quietly differ.

## What is deliberately absent

* **No GPU path.** `maj-gpu` and the preview graph are M4's business. This crate defines
  what they must agree with.
* **No media decoding.** Pictures arrive through `PictureResolver`: `Generated` charts, PNG
  and PPM stills, a flat colour and a title today; a decoder tomorrow, as one more
  implementation of a trait the renderer already had. Media the resolver cannot read is one
  condition with one declared policy, `MissingMedia`: a labelled placeholder that says
  `offline`, `damaged` or `undecodable`, which is what an editor wants, or a refused frame,
  which is what a delivery wants. The manifest names whichever happened, because a
  substitution nobody can see is a substitution that ends up in a master.
* **No audio.** Sound is `maj-audio`'s, and a video render ignores audio tracks entirely.
* **No plugin effects.** An effect that is not one of the ten is not on the certified path.
* **No opportunistic path.** `RenderPolicy::Opportunistic` is a real value with a real
  meaning and a refusal attached: this crate is the certified path, and a manifest that
  claimed otherwise would be false.
* **No parallelism, and no cache.** Both are measurable optimisations with a correctness
  obligation to this renderer, and neither is worth building before there is a measurement
  that says so.

## How to run it

```sh
# One frame, with its manifest beside it.
cargo run --release -p maj-cli -- render \
  --project demo.maj --frame 23 --out frame.0023.png --depth 16 --transfer srgb

# Two ways of rendering the same frame, compared bit for bit.
cargo run --release -p maj-cli -- verify --project demo.maj --frame 23 --tiles 3x2

# What a project holds, without opening an editor.
cargo run --release -p maj-cli -- probe --project demo.maj

# A real timing on this machine, and a check that it repeats.
cargo run --release -p maj-cli -- bench --project demo.maj --frame 23 --repeat 5
```

`verify` exits non-zero when the digests differ, so it composes with CI rather than asking a
person to read its output. The same check runs as a test:
`crates/maj-render/src/render.rs::a_tiled_render_is_bit_identical_to_a_whole_frame_render`,
and `crates/maj-cli/tests/cli.rs` renders the same frame three times and compares the files
byte for byte.
