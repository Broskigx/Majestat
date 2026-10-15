# Majestat — project context

## What this is

A professional non-linear video editor, built to beat Premiere Pro and DaVinci Resolve
Studio 21 by changing the axis they are built on rather than by matching their feature
lists.

Premiere and Resolve are **applications with an engine inside**. Majestat is an **engine
with an application on top**.

## The thesis

> **Everything is a command.** Every edit — from a user, from the AI agent, or from a
> collaborator across the network — is an immutable command appended to a log, and the
> render is a pure function of that log.

Four capabilities follow from that one arrangement rather than being bolted on:

| Capability | Mechanism |
|---|---|
| Reproducible renders | State is a function of the log, so the same log and media give the same bytes |
| An agent that really edits | The Pilot emits the same typed commands the interface does; its work is undoable and reviewable |
| Realtime collaboration | Syncing semantic commands is tractable; there is no opaque blob to lock |
| Verifiable cloud rendering | A remote render is checked by hash rather than trusted |

## Where the advantage comes from

Strengths borrowed deliberately, from the competition's own products:

* **From Resolve 21** — a finishing-grade colour engine (nodes, power windows, ACES/OCIO,
  scopes), complete audio post, embedded compositing, a perpetual licence, and
  multi-role collaboration included.
* **From Premiere 26.x** — frictionless track-based editing, a real extension ecosystem,
  text-based editing from transcripts, and media streaming without a download step.
* **From neither** — a reproducible, diffable, reviewable, agent-drivable edit.

## Settled decisions

| Decision | Choice | Recorded in |
|---|---|---|
| Engine and platform | Rust, with `wgpu` over D3D12/Vulkan | ADR 0004 |
| First milestone shape | Build the deep engine first, not a demo | `.planning/ROADMAP.md` |
| Differentiators | All four: agent, determinism, collaboration, cloud | `.planning/ROADMAP.md` |
| Determinism | Two named tiers: Reproducible and Verifiable | ADR 0001 |
| Time | Exact rational, never floating point | ADR 0003 |
| Model semantics | Everything is a command | ADR 0002 |
| Media I/O | FFmpeg under LGPL behind an abstraction Majestat owns | ADR 0005 |
| Numerics | Declared policy on a certified render path | ADR 0006 |
| Effects | Mandatory declaration contract | ADR 0007 |
| Document | JSON-first `.maj`, binary container deferred to M10 | ADR 0008 |
| Product licence | **`Apache-2.0 OR MIT`**; no GPL component may enter the binary | ADR 0014 |
| Cloud provider | Deferred to M13; egress cost is the deciding criterion | `.planning/ROADMAP.md` |
| Interface toolkit | `egui` first, kept thin and replaceable | ADR 0004 |

## Constraints

* **Host toolchain.** Windows, Rust 1.99, GNU toolchain (no Visual Studio installed), no
  C compiler, no binutils `dlltool`. This constrains dependencies sharply and blocks M2
  until resolved. See ADR 0009 and `.planning/STATE.md` for the blocker.
* **Licence hygiene.** The product is `Apache-2.0 OR MIT`, so no GPL component may be
  linked in: `x264` and `x265` are excluded, LGPL dependencies are dynamic and behind an
  abstraction this workspace owns, and vendor SDKs stay behind feature flags.
* **Hardware for measurement.** The acceptance numbers are measured on an NVIDIA RTX
  4060 Ti, and hardware-specific claims are stated as such.

## Terminology

| Term | Meaning |
|---|---|
| Command | One immutable, invertible edit |
| Log | The ordered commands that produced a project; the durable document |
| State | The derived model — assets, sequences, tracks, clips |
| Tier 1 / Reproducible | Bit-exact on the same build and machine |
| Tier 2 / Verifiable | Cross-hardware, within a declared tolerance and manifest |
| Pilot | The natural-language editing agent (M11) |
| Certified path | The render path bound by the declared numeric policy |
