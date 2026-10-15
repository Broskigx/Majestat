# 0010. A CPU reference renderer defines what a frame is

* Status: Accepted
* Date: 2026-10-14

## Context

ADR 0006 promises a certified render path, and ADR 0004 promises that the same graph
renders identically in preview and in export. Both promises are about *pixels*, and neither
can be checked against a GPU alone: a vendor driver cannot be the definition of correct, and
two drivers disagreeing tells you that they disagree, not which one is right.

The project also has a sequencing problem. The GPU path depends on `wgpu`, which is not yet
a dependency (ADR 0004), and the media path depends on a decoder, which is not yet built
(ADR 0005). If the renderer waits for both, then the definition of a correct pixel, the
tiling policy, the effect semantics and the verification tooling all arrive at once, at the
end, in the largest and least testable phase of the project.

What the renderer needs before it can be written is only a picture source and a model, and
both already exist behind abstractions: `maj-core` says what a project *is*, and a picture
source is a trait the renderer can be handed.

The counter-pressure is honest: a CPU renderer is slow, and writing one first risks
spending effort on a path that the GPU path then diverges from.

## Decision

Build a **single-threaded CPU reference renderer** in `maj-render`, and make it the
definition of the certified path.

* **It is the specification, not a fallback.** The GPU path will be correct when it agrees
  with this one, within the declared tolerance. Where the two disagree, this one is right
  until an argument says otherwise.
* **Certified arithmetic only.** Addition, subtraction, multiplication, division and square
  root, all correctly rounded by IEEE-754, plus the deterministic transcendentals in
  `maj_color::det`. No `powf`, no `sinf`, no interpolation over a table.
* **Frame-space effects.** An effect that is defined on the frame — a vignette, a grain
  field — is given the frame's size and the window's absolute origin, so its value depends
  on where a pixel is and never on where a tile begins. Grain is a hash of the absolute
  pixel coordinate rather than of a counter.
* **Tiling is bit-exact by construction, and it is tested.** A window is expanded by the
  **sum** of the supports of the effects in its chain — not the largest, because a sharpen
  reading a blurred pixel reads both margins — and the interior of the window is then
  identical to what a whole-frame pass would have produced.
* **Masks are multiplied into alpha before the effect chain runs**, in source-normalised
  coordinates, so a blurred layer has a blurred mask edge and a mask survives a change of
  sequence resolution.
* **Every layer is recorded.** A render writes a manifest naming the policy, the project
  revision and its hash, every layer with its source kind and enabled effects, the halo and
  the grid, and a digest of the frame. A render that cannot say what it did is not
  auditable.
* **The opportunistic path is refused rather than approximated.** `RenderPolicy` exists, and
  `RenderTask::render` returns an error for anything other than the certified one, because
  this crate is the certified path and a manifest that claimed otherwise would be false.

## Consequences

* **Slow, and knowingly so.** A 4K frame takes seconds. That is acceptable for export
  verification and unacceptable for playback, which is exactly the division of labour ADR
  0006 describes; the opportunistic path is playback's, and it is not this crate's.
* **The tiling policy is a theorem that has to keep being true.** Adding an effect means
  declaring its support honestly, and the tiling test — render the same frame as a grid and
  as a whole and compare digests — is what holds the theorem up. A wrong declaration shows
  up as a seam in that test rather than as a subtle artefact in a delivery.
* **Effect semantics are now fixed in code.** The ten built-in effects have concrete
  behaviour that a GPU shader must reproduce, and ADR 0007's contract now has an
  implementation behind it rather than only a description.
* **Single-threaded is a starting point, not a position.** The tiled path exists so that
  parallelising it is a change of scheduling rather than a change of results; the correctness
  it establishes is what makes parallelism safe to add.
* **Media, transforms, transitions and animation are exercised before a decoder exists.**
  Generated charts and PPM files stand in for real media behind `PictureResolver`, so the
  image path is tested end to end today, and a decoder slots in as one more implementation
  of a trait the renderer already had.
