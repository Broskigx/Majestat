# Roadmap

Seventeen phases. Each has a verifiable deliverable and an acceptance gate, and no phase
is declared done without a measurement. Phases M0 and M1 are complete; the MCP surface
beside them is complete; **P1 (the document floor) is complete**; M7's mechanism is built
and its gate — a three-minute edit — cannot be met before M6m, so M7 is sequenced behind
M6m and M-X below rather than left marked "in progress".

The ultra plan (`que-le-falta-para-whimsical-lemur.md`) **reorders** what comes after P1 and
splits five phases that each mixed something needing no new subsystem with something needing
the hardest one. It drops nothing: every one of the seventeen phases keeps its number and its
place, and the splits are named after their parent (M6m, M2a, M2b, M8a, M8b, M3s, M9a) so the
roadmap below and the catalogue audit stay readable against each other. That order is in
**The order the ultra plan settles on**, at the end of this file.

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
| **M7** | Interface | Timeline, viewer, panels, command palette, pages, menus, titles, pointer and keyboard-first editing | A three-minute edit performed end to end by keyboard, pointer and agent alone | **[ ]** — the mechanism (the shell, the window, the pointer, the clock, the title studio, the two browsers) is built and gated, and the shell's own numbers are in STATE.md; the gate itself cannot be met before M6m supplies the editorial operations it is an edit *of*, so M7 is sequenced behind M6m and M-X |
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
adds a P0 in front of M2 for them. **All four are now done**, and the fourth turned out not
to need the administrator rights it was recorded as needing: the toolchain was already on
the host, in MinGW-w64 form rather than MSVC.

| Item | Deliverable | Gate | Status |
|---|---|---|---|
| **P0.1** | The product licence, declared and propagated | `Apache-2.0 OR MIT` in `[workspace.package]`, inherited by 22 crates, both texts at the root, ADR 0014, ADR 0005 amended, the audit's first question turned into a refusal | **[done]** |
| **P0.2** | One edit is one undo step, and a refused edit changes nothing | `History::apply_group`, used by `Shell::dispatch`; grouped and ungrouped commands give the same state *and* document hash | **[done]** |
| **P0.3** | Media that cannot be read is one condition with one policy | `MissingMedia` over every way media can be unavailable; a labelled `Placeholder` that says `offline`, `damaged` or `undecodable`, or a refused frame; the manifest's label comes from the resolver that would open it | **[done]** |
| **P0.4** | The C toolchain | A throwaway crate links a `windows-sys` dependant, compiles a trivial C file through `cc`, and generates a `raw-dylib` import library; a control run with `dlltool` hidden from `PATH` reproduces the old failure. MinGW-w64 and `dlltool`, not MSVC — nothing to install | **[done]** |
| **P0.4a** | The FFmpeg build, pinned | `docs/FFMPEG.md`: FFmpeg 9.0.2 `n9.0.2`, shared LGPL, `--disable-autodetect`, none of `--enable-gpl`/`--enable-nonfree`/`--enable-version3`, the LGPL-2.1 obligations mapped to clauses, and a runtime gate that refuses a library which cannot prove it is the LGPL build | **[done]** |
| **P0.4b** | `maj-media`'s surface | `docs/MEDIA.md`: the vocabulary, the adapter at layer 4 because `PictureResolver` cannot be implemented at layer 2, a closed container enum because `kind` returns `&'static str`, no conversion in the decoder because that arithmetic is the certified path's, and the `unsafe` question named as an ADR rather than decided | **[done]** |

Evidence is in `STATE.md`. What M2 is waiting on is now work plus one decision: the ADR
that authorises `unsafe` in a leaf FFI crate (`docs/MEDIA.md` §8), and an FFmpeg build
actually produced from the pinned configuration.

## Added: P1 — the document floor

The three schema changes the ultra plan puts before any decoder, because adjusting a schema
afterwards costs more than writing it once: **a sequence's start timecode**, **an asset's
fingerprint and a relink by hash**, **markers**, and the autosave journal that makes a crash
cost the edit in flight rather than the session.

| Item | Deliverable | Gate | Status |
|---|---|---|---|
| **P1** | `Sequence::start_timecode_frames`; `MediaFingerprint` (BLAKE3 of the bytes, size, and a measured duration when anything has read the container) with `SetAssetFingerprint` and `RelinkAsset`; the `Marker` entity with `AddMarker`, `SetMarker`, `RemoveMarker`; `maj-core::journal` with `maj recover`; the CLI's `fingerprint`, `relink` and `recover` verbs; six new tools; the shell's autosave and the marker keys. Schema 1 and 2 documents migrate on load | A session's work is journaled, the process dies, and the recovered document is **byte-identical to the one the session would have saved**; media moved to another directory relink by hash and not by name; the ten-thousand-command fuzz is still green after the schema change | **[done]** |

Evidence is in `STATE.md`. What P1 leaves for later is deliberate and named there too: the
measured duration is optional until a probe exists, and a marker has no reply or resolve
state.

## Added: the MCP surface

The seventeen phases above are the roadmap. This one addition sits beside them because it
is the interface the command model was built to make possible, and it was the next
unblocked thing to build once M8's headless CLI existed.

| Phase | Name | Deliverable | Gate | Status |
|---|---|---|---|---|
| **MCP** | Agent interface over the Model Context Protocol | `maj-mcp` (layer 5): 51 tools, 5 resources, 3 templates, 3 prompts, sessions, read-only mode; `maj mcp` in the CLI | An editing session driven entirely by tool calls produces a document the command line renders to the same digest as through the server; no tool exists that is not one command; `cargo test --workspace` green | **[done]** |

Acceptance evidence is in `STATE.md`; the surface, the guarantees and what is deliberately
not exposed are in [`docs/MCP.md`](../docs/MCP.md); the decision is ADR 0011. The
130-item Fase II catalogue is audited against that surface in
[`docs/COVERAGE.md`](../docs/COVERAGE.md) — 38 items reachable today, 57 that need the
engine to grow, 35 outside a certified picture engine.

## M7: the shell is built, and so is the window

The interface is a **model** with a window on it. The model is 80 intentions that translate
into commands and nothing else, a keyboard table of 70 strokes, 199 keys in English and
Spanish, seven pages, and a preference file of view state. The window is a second crate that
draws the frame, the pages and their panels, and turns Windows messages — keys, presses,
drags, the wheel and the pointer's comings and goings — into the strokes and gestures the
model already defines. Both write to the same command log an agent's tools write to, under the
author `ui:local`, and the two surfaces are tested to be command-for-command equivalent — a
project edited by keyboard and by tool calls has the same state hash and a different document
hash, because attribution is part of the trace.

The window exists because of one measurement: `eframe` reaches `parking_lot_core`, which
needs an import library, which needs `dlltool.exe`, which this host did not have at the time.
It does now (P0.4), so the toolkit question is reopened rather than closed — but reopening it
means measuring `egui` against a native reference, which is the deferred decision M7 already
owns, not an assumption that the window should change. So the imports are declared by hand against the `libuser32.a`, `libgdi32.a` and `libkernel32.a` that
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
| Link groups: the model has no link to break or restore | M6m | 2.3, and every ripple that should move a shot and its sound together |
| Structural commands: `close_gap`, `insert_blank`, `duplicate`, per-clip grade copy | M6m | 2.5–2.7, 2.17–2.18, 4.10, 4.13 |
| Speed as an animatable channel, and speed below zero | M6m | 2.8, 2.10 |
| A media pool with tags and queries | M10, M6m | 1.7, 1.9, 1.17–1.19 (the identity half of 1.15 arrived with P1) |
| Richer kernels and per-effect parameters (per-channel ops, mask invert, effect opacity, pivot) | M4, M5 | Most of blocks 3 and 4 |
| Text: transcription and subtitles | M11 (transcript layer) | 6.1–6.6 |
| Type beyond four built-in faces: a machine's fonts, a text box, a lower third that wraps | M7 (presentation), M2 (any decoder a font needs) | 6.1–6.6, and every template a title would fill |
| Rubber-band selection, and the multi-clip edits behind it | M6 | Multi-clip trim, ripple and roll, which is what a sweep would be for |

Two things the catalogue asks for are already consequences of the architecture and need no
phase: **timeline comparison** is a read of the command log, and **attribution** is already
recorded per client name (`mcp:claude-desktop`).

## The order the ultra plan settles on

The seventeen phases keep their numbers; this is the *sequence*, and the reason the plan
orders it this way is that each step is the cheapest thing that unblocks the most.

| Order | Phase | Deliverable | Gate | Crates |
|---|---|---|---|---|
| 1 | **P1** | The document floor | **[done]** — see above | `maj-core`, `maj-cli`, `maj-mcp` |
| 2 | **M6m** | Editing, the model's half: `close_gap`, `insert_blank`, `duplicate`, per-clip grade copy, A/V link groups, speed as a seventh animatable channel including negative, ripple/roll/slip/slide as command groups, three- and four-point editing as **enforced rules** | Property tests over random timelines: no operation leaves a gap or an overlap, one undo takes it back, and a refused operation changes neither the state nor the document hash | `maj-core`, `maj-timeline`, `maj-ui`, `maj-mcp` |
| 3 | **M-X** | Interchange: OTIO, EDL CMX3600 and FCPXML export; OTIO and EDL import | Export → import → export gives byte-identical OTIO; a 200-event EDL imports with every timecode exact under drop-frame; what cannot be represented is **refused with a named reason**, never flattened silently | `maj-timeline` (`interchange`), `maj-cli`, `maj-mcp` |
| 4 | **M8a** | Output correctness: range and level policy, start timecode in the output, scopes as reductions over the framebuffer, a render queue, multi-channel EXR | Legal limited-range output measured by our own scope; scope values validated against reference images; a 20-job queue that resumes | `maj-export`, `maj-render`, `maj-cli` |
| 5 | **M2a** | Media vocabulary and probe: the types, a closed `ContainerKind`, a probe over the corpus. No FFI yet | A hundred-file corpus, zero crashes; rotation, field order and variable frame rate read correctly; `kind` answers `damaged` from a cached probe | `maj-media` |
| 6 | **M2b** | Software decoding: the ADR authorising `unsafe` in `maj-media-sys`, a runtime check that the library is the LGPL build, `MediaResolver` in `maj-render` | A frame of real footage renders to a still whose manifest names the container; three renders byte-identical **with the decoder in the path** | `maj-media-sys`, `maj-media`, `maj-render` |
| 7 | **M10c** | Frame cache: `PreviewKey` extended with decoder identity, backed by disk | Scrub p95 **under 40 ms** on ProRes 4K; hit rate above 85%; no excessive invalidation when an unrelated clip is edited | `maj-cache`, `maj-ui`, `maj-render` |
| 8 | **M3s** | Scrub and video playback: a deadline scheduler, a frame pool, a picture-only transport on the existing rational clock | 4K50, zero dropped frames over five minutes | `maj-playback` |
| 9 | **M9a** | Audio offline: the model, decoding, a deterministic mixdown to WAV and stems. No device, no callback | Fifty tracks mixed byte-identically in three runs, and sample-exact against a reference sum | `maj-audio`, `maj-media` |
| 10 | **M8b** | Encoding and muxing: Media Foundation or NVENC and OpenH264, muxed with M9a's audio | Three renders, same SHA-256; A/V sync verified after the mux to within a sample | `maj-export` |
| 11 | **M3b / M9b** | Realtime A/V: the audio clock, sync, device I/O, the mixer | Zero underruns; drift under one frame in thirty minutes | `maj-audio`, `maj-playback` |
| 12 | **M6f** | The feel of editing: modal keyboard trimming with a persistent selection showing both sides of the cut, multi-selection and a rubber band | A trim costs one press and one cached frame; input-to-pixel measured under 16 ms | `maj-ui`, `maj-win32` |
| 13 | **W1–W5** | The wedge, which only this architecture can ship: editing as a reviewable diff (`maj diff`), bisecting the log, replaying over new media, a conform and QC agent, and a native render in CI | Each is its own gate; see the catalogue audit for what each needs | `maj-cli`, `maj-mcp`, `maj-agent` |

Everything else — M4, M5, M11 to M14, multicam, nesting, OpenFX, VST3, collaboration,
cloud — keeps its number and its place behind this. Nothing is dropped; it is sequenced.

Four lanes run in parallel, each in its own **git worktree** rather than one checkout:
`STATE.md` already records two red gates caused by concurrent sessions in a single folder.
Model (P1, M6m, M-X → `maj-core`, `maj-timeline`, `maj-mcp`), media (M2a, M2b →
`maj-media`, `maj-render/source.rs`), output (M8a → `maj-export`, `maj-render`, which needs
no decoder and can start today), and feel (M10c, M6f → `maj-cache`, `maj-ui`,
`maj-win32`, serialised against the model lane on `action.rs`).

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
| Binary `.maj` container | M10 | The toolchain precondition is met (P0.4), so this is now a real decision rather than a foregone one: a bundled SQLite is buildable today |
| Cloud storage provider | M13 | Zero-egress cost first, because editors pull proxies constantly; must be S3-compatible to avoid lock-in |
| Authentication | M13 | Device-code flow with OIDC; nothing earlier depends on it |
| Interface toolkit beyond `egui` | M7 | Visual fidelity measured against a native reference |
| ~~Product licence~~ | **Decided** | `Apache-2.0 OR MIT`; see `docs/LICENSING.md` |
| H.264 and HEVC encoder | M8 | `x264` and `x265` are GPL and therefore excluded; the candidates are the platform encoder and OpenH264 |
| Atmos | M9 | Licensed feature, scoped separately from channel-based surround |
| Dolby Vision | M5 | Licensed tier, scoped separately from HDR10 and HLG |
