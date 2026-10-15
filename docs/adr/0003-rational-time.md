# 0003. Rational time, never floating point

* Status: Accepted
* Date: 2026-10-13

## Context

23.976 fps is `24000/1001`. It is not `23.976`, and no `f64` can represent `1001/24000`
exactly. An editor that stores positions as floating-point seconds accumulates error:
the drift is invisible over a few seconds and visible over a three-hour timeline, and it
appears as sub-frame slips between a picture and its sound, between a clip and its
transition, and between a project and a render.

Floating point also breaks the determinism claim in ADR 0001 in a second, subtler way:
`f64` formatting is a source of text-level ambiguity (`1.0` versus `1`), and comparison
of two mathematically equal values that took different routes is not reliable.

Timecode adds a third trap. Drop-frame timecode omits frame *numbers* at the start of
each minute except every tenth minute, at 29.97 and 59.94. Getting that wrong is a
frame-accurate bug that only appears near minute boundaries.

## Decision

Time is exact and integer throughout the model.

* `Rational` — canonical reduced rational in `i64`, with a strictly positive denominator
  and zero stored as `0/1`. Canonical form means one representation per value, so
  `derive(PartialEq)` is exact and a hash is stable.
* `Timebase` — an exact frames-per-second rate plus which timecode conventions it
  supports. Rates are written as ratios in a `const` table.
* Positions are `i64` frame counts under a timebase. A clip's trim is a half-open
  `FrameRange` in source frames; its timeline position is an `i64`.
* `Timecode` implements non-drop and drop-frame conversion for exactly the rates that
  define the rules, and refuses drop-frame for every other rate instead of guessing.
* **Division floors** (`div_euclid`) everywhere, never truncates toward zero. Truncation
  makes retiming behave differently on either side of frame zero, which is a determinism
  hazard and a source of off-by-one bugs.
* Reduction runs on unsigned magnitudes and every `i128` intermediate is checked, because
  `|i64::MIN|` fits in no signed 64-bit type and a silent wrap would corrupt a frame
  number invisibly.

Floating point is permitted for display, heuristics and thresholds. It is forbidden in
the model, and a test walks the canonical JSON to prove no number in it is a float.

## Consequences

* The canonical form has exactly one text representation per value, which is what makes
  content hashing meaningful (ADR 0001, ADR 0008).
* Rational arithmetic is slower than `f64` and, in this implementation, allocates
  nothing and stays in registers — the cost is paid in reduction on every operation.
  Acceptable: time arithmetic is not the hot path, decoding and pixel work are.
* API ergonomics suffer: positions need a timebase to be meaningful, and conversions are
  explicit. Accepted as the price of correctness, and mitigated by keeping `Timebase` in
  the sequence so most call sites never think about it.
* Drop-frame is implemented for two rates only. A third would need a rule, not a guess.
* Retiming a clip is expressed with `RetimeMap` under floor semantics, and its documented
  round-trip limit is that exact inversion holds for whole-number speeds.
  `timeline_at(source_at(t)) == t` is *not* claimed for fractional speeds, because it is
  not true; monotonicity is claimed and property-tested instead.
