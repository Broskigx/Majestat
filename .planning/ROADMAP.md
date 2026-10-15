# Roadmap

Seventeen phases. Each has a verifiable deliverable and an acceptance gate, and no phase
is declared done without a measurement. Phases M0 and M1 are complete; the MCP surface
beside them is complete; M7 is in progress — the shell, its window, the keyboard, the pointer,
the pages, the menus and the transport clock are built and gated, and what the gate still
waits for is a person at the keyboard rather than a mechanism.

Legend: **[done]** verified by a run · **[current]** in progress · **[ ]** not started

| Phase | Name | Deliverable | Gate | Status |
|---|---|---|---|---|
| **M0** | Foundation | Cargo workspace, 20 layered crates, CI, nine ADRs, corpus plan | `cargo test --workspace` green on Windows and Linux; ADRs written | **[done]** |
| **M1** | Determinism | `maj-time`, `maj-core`, the `.maj` format, command log, undo and redo | Ten-thousand-command fuzz with exhaustive undo and redo, no divergence | **[done]** |
| **M2** | Media I/O | `maj-media`: probe, demux, decode, seek, waveforms, thumbnails, hardware decode | A hundred-file corpus with zero crashes; variable frame rate, interlacing and rotation handled correctly | **[ ]** |
| **M3** | Playback | `maj-playback`: deadline scheduler, frame pools, audio-clock A/V sync | 4K50 on an RTX 4060 Ti: zero dropped frames over five minutes; scrub p95 under 40 ms; zero audio underruns | **[ ]** |
| **M4** | Render protocol + GPU | `maj-graph`, `maj-gpu`: typed graph, pass fusion, transforms, LUTs | Preview and export of the same graph differ by zero pixels on the Reproducible tier | **[ ]** |
| **M5** | Colour | `maj-color`: OpenColorIO and ACES 2.0, node graph, scopes, HDR | ACES round trip on real log footage; scopes validated against references | **[ ]** |
| **M6** | Editorial | `maj-timeline`: trim, ripple, roll, slip, slide, multicam, nesting, retime | Full editorial operation suite green, including timeline constraints | **[ ]** |
| **M7** | Interface | Timeline, viewer, panels, command palette, pages, menus, titles, pointer and keyboard-first editing | A three-minute edit performed end to end by keyboard, pointer and agent alone | **[current]** — the shell, the window, the pointer, the clock, the title studio and the two browsers are built and gated; the edit itself has not been performed (see below) |
| **M8** | Export | `maj-export`: NVENC, x264/x265, two-pass, output validation, C2PA; `maj-cli` | Three renders of one revision produce identical SHA-256; A/V sync verified post-mux | **[ ]** |
| **M9** | Audio | `maj-audio`: mixer, routing, folder tracks, ASIO, VST3 host | Two thousand tracks without xruns; third-party VST3 loading | **[ ]** |
| **M10** | Proxies and caches | `maj-cache`: four layers keyed by input hashes, downstream invalidation; binary `.maj` container | Cache hit rate above 85% on a representative project; no over-invalidation | **[ ]** |
| **M11** | Pilot (agent) | Transcript layer, typed tools, plan-preview-apply, evaluation harness | Evaluation score above the agreed threshold on the test corpus; zero commands outside the log | **[ ]** |
| **M12** | Collaboration | CRDT over commands, presence, cursors, comments anchored to the timeline, revision review | Four clients editing for thirty minutes: zero state divergence, convergence proven | **[ ]** |
| **M13** | Cloud and render farm | Resumable uploads, proxy streaming, hash-verified distributed render | Cross-node determinism check; offline-to-online editing with no loss | **[ ]** |
| **M14** | Plugins | OpenFX and VST3 hosts, Lua scripting, `maj-cli` in CI | Five real third-party OpenFX plugins working; headless render from CI | **[ ]** |
| **M15** | Hardening | Crash containment, autosave, recovery, telemetry, signed installer | Zero project loss under induced crashes | **[ ]** |
| **M16** | Public beta | Performance and determinism certification | Every number in the acceptance table met and published | **[ ]** |

## Added: P0 of the ultra plan

The seventeen phases are the roadmap. The ultra plan
(`C:\Users\agust\.claude\plans\que-le-falta-para-whimsical-lemur.md`) reorders what comes
after M1 around the observation that the remaining blockers were *decisions*, not work, and
adds a P0 in front of M2 for them. Three of its four items are done; the fourth needs
administrator rights on the build host and is the owner's.

| Item | Deliverable | Gate | Status |
|---|---|---|---|
| **P0.1** | The product licence, declared and propagated | `Apache-2.0 OR MIT` in `[workspace.package]`, inherited by 22 crates, both texts at the root, ADR 0014, ADR 0005 amended, the audit's first question turned into a refusal | **[done]** |
| **P0.2** | One edit is one undo step, and a refused edit changes nothing | `History::apply_group`, used by `Shell::dispatch`; grouped and ungrouped commands give the same state *and* document hash | **[done]** |
| **P0.3** | Media that cannot be read is one condition with one policy | `MissingMedia` over every way media can be unavailable; a labelled `Placeholder` that says `offline`, `damaged` or `undecodable`, or a refused frame; the manifest's label comes from the resolver that would open it | **[done]** |
| **P0.4** | The C toolchain | MSVC Build Tools installed, verified by a throwaway crate that links a `windows-sys` dependant and compiles a trivial C file | **[ ]** — needs administrator rights |

Evidence is in `STATE.md`. P0.4 is what M2 is waiting on.

## Added: the MCP surface

The seventeen phases above are the roadmap. This one addition sits beside them because it
is the interface the command model was built to make possible, and it was the next
unblocked thing to build once M8's headless CLI existed.

| Phase | Name | Deliverable | Gate | Status |
|---|---|---|---|---|
| **MCP** | Agent interface over the Model Context Protocol | `maj-mcp` (layer 5): 45 tools, 5 resources, 3 templates, 3 prompts, sessions, read-only mode; `maj mcp` in the CLI | An editing session driven entirely by tool calls produces a document the command line renders to the same digest as through the server; no tool exists that is not one command; `cargo test --workspace` green | **[done]** |

Acceptance evidence is in `STATE.md`; the surface, the guarantees and what is deliberately
not exposed are in [`docs/MCP.md`](../docs/MCP.md); the decision is ADR 0011. The
130-item Fase II catalogue is audited against that surface in
[`docs/COVERAGE.md`](../docs/COVERAGE.md) — 33 items reachable today, 61 that need the
engine to grow, 36 outside a certified picture engine.

## M7: the shell is built, and so is the window

The interface is a **model** with a window on it. The model is 77 intentions that translate
into commands and nothing else, a keyboard table of 67 strokes, 199 strings in English and
Spanish, seven pages, and a preference file of view state. The window is a second crate that
draws the frame, the pages and their panels, and turns Windows messages — keys, presses,
drags, the wheel and the pointer's comings and goings — into the strokes and gestures the
model already defines. Both write to the same command log an agent's tools write to, under the
author `ui:local`, and the two surfaces are tested to be command-for-command equivalent — a
project edited by keyboard and by tool calls has the same state hash and a different document
hash, because attribution is part of the trace.

The window exists because of one measurement: `eframe` reaches `parking_lot_core`, which
needs an import library, which needs `dlltool.exe`, which this host does not have. So the
imports are declared by hand against the `libuser32.a`, `libgdi32.a` and `libkernel32.a` that
rustup ships — a probe created a window, showed it and pumped eighteen messages — and the
result is a real application on this machine, Windows-only, in the one crate whose lint level
for `unsafe` is not `forbid`. See ADR 0013 for the decision and its cost.

```sh
cargo run -p maj-win32 --bin majestat
```

| | |
|---|---|
| Built and gated | The intention set, the one door into the log, undo and redo through the model, the keyboard table, the pointer, the seven menus and two context menus, seven pages, the transport clock with its shuttle and loop, the two languages, the preferences, the palette driving the agent's own tools, preview and still export on the certified path, the title studio and its four built-in faces, the effects and transitions browsers, importing a still from disk, the cross-interface parity tests, and a window that paints all of it and edits with the documented keys and gestures |
| Checked without a person | Eight tests proving every stroke in the table is reachable from a key on the window, that no key reaches nothing and that every zone the paint registers lies inside the client area; `--self-check`, which posts real keys and real clicks at a real window and asserts what they did; `--snapshot`, which paints the interface offscreen to a file; `--bench`, which times the paint whole and incremental; `--regions`, which reports the colours actually on screen |
| Not built, and why | Rubber-band selection and the multi-clip edits behind it (M6); decoded media in the viewer (M2); audio levels (M9); video encoding and delivery presets (M8); four pages that are drawn and named but empty (M4, M5, M6, M9); the window icon and the logo's build-time decode step; and type from the machine — a title is set in one of four faces built into the renderer, because reading a system typeface is a licensing question as well as a decoder one |
| The gate, honestly | A three-minute edit by keyboard, pointer and agent alone is now something a person *can* perform on this machine, and the same operations run mechanically in `--self-check`. It is **not recorded as met**: nobody has sat down and done it, and M6's editorial operations are not all here yet |

Surface, guarantees and evidence: [`docs/UI.md`](../docs/UI.md). The decisions: ADR 0012
(the shell emits commands) and ADR 0013 (the window is written against Win32).

## What the engine still needs before agent editing is professional

The MCP surface is done; what an agent can *do* through it is exactly the engine's
surface. Each gap below closes a whole cluster of the catalogue rather than one item, and
belongs to a phase that already exists:

| Gap | Owning phase | Closes |
|---|---|---|
| A decoder behind `PictureResolver` (RAW, span assembly, card structures, camera metadata) | M2, M10 | Block 1, and the media half of every later block |
| An encoder and muxer in `maj-export` | M8 | Block 7, proxies, delivery |
| `maj-audio`: model, mixdown, stems | M9 | Block 5 in full, plus 1.4, 1.16, 2.11, 7.4 |
| Markers and comments as one entity | M6 (editing), M12 (anchored review) | 2.12, 2.13, 4.14, 6.12 |
| Structural commands: `close_gap`, `insert_blank`, `duplicate`, per-clip grade copy | M6 | 2.5–2.7, 2.17–2.18, 4.10, 4.13 |
| Speed as an animatable channel, and speed below zero | M6 | 2.8, 2.10 |
| A media pool with tags and queries | M10, M6 | 1.7, 1.9, 1.15, 1.17–1.19 |
| Richer kernels and per-effect parameters (per-channel ops, mask invert, effect opacity, pivot) | M4, M5 | Most of blocks 3 and 4 |
| Text: transcription and subtitles | M11 (transcript layer) | 6.1–6.6 |
| Type beyond four built-in faces: a machine's fonts, a text box, a lower third that wraps | M7 (presentation), M2 (any decoder a font needs) | 6.1–6.6, and every template a title would fill |
| Rubber-band selection, and the multi-clip edits behind it | M6 | Multi-clip trim, ripple and roll, which is what a sweep would be for |

Two things the catalogue asks for are already consequences of the architecture and need no
phase: **timeline comparison** is a read of the command log, and **attribution** is already
recorded per client name (`mcp:claude-desktop`).

## Acceptance numbers

Measured on an RTX 4060 Ti under Windows, and stated with the hardware attached because
hardware-specific claims should be.

| Metric | Target |
|---|---|
| 4K50 H.265 10-bit playback with six GPU effects | Zero dropped frames over five minutes |
| Scrub latency, p95 | Under 40 ms |
| Audio underruns | Zero |
| Time to first frame | Under 250 ms |
| Export determinism | Three renders, identical SHA-256 |
| Opening a 2,000-clip project | Under 2 seconds |
| Cold start | Under 1.5 seconds |
| Media corpus | Zero crashes on one hundred files |
| Interface input-to-pixel | Under 16 ms |
| Six-hour timeline, 500 GB of media | No memory degradation |

## Phase ordering rationale

* **M1 before M2.** Rational time and the command model are the two things everything
  else is built on. Retrofitting either is a rewrite.
* **M3 before M4.** Playback is the hardest scheduling problem and it constrains how the
  render graph must be structured. Building the graph first produces a beautiful graph
  that cannot keep up.
* **M7 after M6.** The interface needs the editorial operations to exist, and a
  keyboard-first interface over a command surface is far less work than a bespoke one.
* **M8 before M11.** The headless CLI turns every later phase into something that can be
  tested and reproduced from a script, and it is what makes `--verify` possible.
* **M10 before M11.** The agent needs the cache and proxy subsystems to be predictable,
  or its plans will be slow in ways it cannot explain.
* **M12 and M13 after M11.** Collaboration and cloud both rest on the command log being
  correct under replay, which the agent work stress-tests first.

## Deferred decisions, with the phase that owns them

| Decision | Owning phase | Criterion to decide |
|---|---|---|
| Binary `.maj` container | M10 | After the toolchain question in M2 is settled |
| Cloud storage provider | M13 | Zero-egress cost first, because editors pull proxies constantly; must be S3-compatible to avoid lock-in |
| Authentication | M13 | Device-code flow with OIDC; nothing earlier depends on it |
| Interface toolkit beyond `egui` | M7 | Visual fidelity measured against a native reference |
| ~~Product licence~~ | **Decided** | `Apache-2.0 OR MIT`; see `docs/LICENSING.md` |
| H.264 and HEVC encoder | M8 | `x264` and `x265` are GPL and therefore excluded; the candidates are the platform encoder and OpenH264 |
| Atmos | M9 | Licensed feature, scoped separately from channel-based surround |
| Dolby Vision | M5 | Licensed tier, scoped separately from HDR10 and HLG |
