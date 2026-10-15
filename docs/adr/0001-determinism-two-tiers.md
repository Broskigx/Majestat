# 0001. Determinism in two named tiers

* Status: Accepted
* Date: 2026-10-13

## Context

Majestat's differentiating claim is that a render is reproducible. That claim is easy to
make and impossible to make unconditionally: floating-point reduction order,
fused-multiply-add behaviour and vendor shader compilers differ between GPU
architectures, so bit-exact output across vendors is not achievable for a full colour
pipeline. A promise without a boundary is a promise that will be broken publicly.

There is also a real commercial need for the strong form. A render farm that a studio
cannot audit is a render farm a studio will not trust with a delivery.

## Decision

Determinism is defined as two tiers, both named in user-facing documentation from the
start rather than added later as a caveat.

**Tier 1, Reproducible: bit-exact.** Same build, same machine, same inputs, same bytes
out. This is what `maj render --verify` checks.

**Tier 2, Verifiable: cross-hardware, within a declared tolerance.** The certified render
path fixes a declared reduction order for every accumulating operation, forbids fast-math
and vendor-specific approximations, stamps a version on the graph so any node or shader
change changes the output hash, emits per-node hashes so a divergence can be localised,
and declares a PSNR tolerance as a number.

Everything outside the certified path may use hardware-specific fast routes and is
labelled as such in the render manifest.

## Consequences

* Marketing may not claim bit-exact cross-platform rendering, and does not need to: the
  honest claim is stronger than the vague one.
* Per-node hashes cost memory and bookkeeping on the certified path. Accepted, because
  localising a divergence between a workstation and a farm node is otherwise guesswork.
* Any node that cannot declare a reduction order cannot join the certified path. This
  constrains future effect authoring, deliberately.
* The structured and hashable form of the project document is a prerequisite, so this
  decision drives ADR 0002 and ADR 0008.
