# 0006. Numeric policy on the certified render path

* Status: Accepted
* Date: 2026-10-13

## Context

ADR 0001 promises a second determinism tier that holds across different GPUs. Floating
point arithmetic does not give that for free. Three things break bit-reproducibility
across hardware:

1. **Reduction order.** Summing a set of floats in a different order gives a different
   result. Parallel reductions reorder by definition.
2. **Contraction and precision relaxation.** Compilers and drivers fuse multiply-add,
   reassociate, and substitute approximate reciprocals and transcendentals when allowed.
3. **Vendor intrinsics.** Fast paths that exist on one architecture and not another.

An editor also spends most of its arithmetic budget in the colour pipeline, where a
one-bit difference compounds across a grade and becomes visible after a display
transform.

The counter-pressure is real: strict numerics cost throughput, and a NLE that cannot play
back in real time is not a professional tool.

## Decision

Two render paths, explicitly declared, and a manifest that says which one was used.

**Certified path (Tier 2):**

* Fixed reduction order for every accumulating operation — determined by the graph, not
  by thread scheduling. Warp-level or workgroup-level reduction is permitted only with a
  fixed tree shape.
* No fast-math. No reassociation. No contraction of multiply-add unless the contraction
  is written explicitly and deterministically by the shader author.
* No approximate transcendentals: no fast reciprocal, fast `rsqrt`, fast `log`/`exp`
  where an accurate form exists.
* Every node emits a hash of its output, so a divergence is localised to a node rather
  than merely detected at the end.
* Intermediates are at least 16-bit float for composite work, with a working colour space
  at higher precision. Integer rounding happens once, at the declared output boundary,
  never between effects.

**Opportunistic path (Tier 1 only):**

* Vendor intrinsics, relaxed precision, adaptive scaling and frame-dropping for
  interactive playback.
* Always labelled in the manifest and in the interface.

## Consequences

* The certified path is slower. The mitigation is that it is used for **export and
  verification**, not for interactive playback, and playback has its own budget anyway.
* Effects must declare their reduction behaviour to join the certified path. Effects that
  cannot are legal, and are excluded from verification and labelled.
* Per-node hashing costs memory and bandwidth. Hashing a 4K frame per node is not
  affordable, so the manifest will hash at declared checkpoints plus a whole-frame digest
  at the end — a compromise to be implemented and measured in M4, with the tolerance
  documented rather than assumed.
* Numerical tightness is a design input to the effect API (ADR 0007) and to how the
  render graph is compiled, so both must be built with it in mind rather than retrofitted.
