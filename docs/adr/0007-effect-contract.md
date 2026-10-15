# 0007. Every effect declares a contract

* Status: Accepted
* Date: 2026-10-13

## Context

An effect that does not declare what it does to an image cannot be scheduled, cached,
fused, colour-managed or verified. Concretely, an undeclared effect makes it impossible
to answer: may these two passes be fused into one; can this be cached and invalidated
correctly; is this operating in the right colour space; may two frames be processed in
parallel; does a region-of-interest change the result.

Video effects also differ structurally in ways that must be visible to the scheduler.
A pointwise colour transform, a spatial blur and a temporal noise reduction have
completely different costs, different caching behaviour and different parallelism.

Third-party plugins make this harder, not easier. The OpenFX standard is explicit that
hosts must respect regions of definition and interest, draft renders, and declared
thread-safety, and that not every plugin is safe to call concurrently or in arbitrary
frame order.

## Decision

Every effect, built-in or third-party, declares:

1. **Pixel format and colour space** it accepts and produces, plus the alpha convention.
2. **Region of definition**, and how much of the input it needs for a given output
   region. The region of interest propagates through the graph.
3. **Temporal class**: pointwise, spatial, or temporal. Temporal effects break frame
   parallelism and must say so.
4. **Determinism class**: whether it can join the certified numeric path (ADR 0006).
   An effect that cannot is legal and is excluded from verification.
5. **Concurrency class**, including whether concurrent calls are safe and whether frames
   may be requested out of order.
6. **Cache key contribution**: exactly which parameters, and which version of the
   implementation, participate in the key. A shader change that does not change the key
   is a correctness bug.
7. **Draft behaviour**: what it does at reduced render scale, and whether the result is
   merely cheaper or genuinely different.

Plugins are treated as potentially slow, stateful and untrusted: failures are contained
per plugin and fall back to passing the input through unchanged, rather than taking the
render down. A missing plugin degrades with a visible marker, never silently.

## Consequences

* Built-in effects are more ceremony to write. Accepted: the declaration is what makes
  fusion, caching and verification possible at all.
* The native effect API and the OpenFX host must both express this contract, and the OFX
  host has to translate the standard's declarations into it — regions of definition and
  interest, draft renders, sequential and concurrency flags.
* Plugin isolation has a real cost in address-space or process separation. The plan is to
  start in-process with containment, and escalate to out-of-process for hostile or
  crashing plugins based on measurement in M14.
* A plugin that cannot declare a cache key contribution must be marked as
  non-cacheable, which will make some third-party effects slow. That is the honest
  outcome; caching an effect whose parameters are not fully known is how editors produce
  stale frames.
