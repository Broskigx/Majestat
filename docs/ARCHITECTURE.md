# Majestat architecture

## The axis

Premiere and Resolve are applications with an engine inside. Majestat is an **engine
with an application on top**.

The engine's central claim is that *everything is a command*. Every edit — from a
user, from the AI agent, or from a collaborator across the network — is an immutable
command appended to a log, and the render is a pure function of that log. Four
capabilities fall out of that single arrangement instead of being bolted on:

| Capability | How it follows from the command log |
|---|---|
| Reproducible renders | The state is a function of the log, so the same log plus the same media yields the same output |
| An agent that really edits | The agent emits the same typed commands the UI does; its work is undoable and reviewable |
| Realtime collaboration | Syncing semantic commands is tractable; there is no opaque binary blob to lock |
| Verifiable cloud rendering | A remote render is checked by hash instead of trusted |

See `docs/DETERMINISM.md` for what reproducible means precisely, and
`docs/PROJECT-FORMAT.md` for the document.

## Layers

Dependencies point **downward only**. A crate may depend on crates in lower layers,
never on crates above it. The UI never touches media I/O directly; it emits commands.

```
Layer 6  maj-ui · maj-win32 · maj-cli · maj-script
Layer 5  maj-timeline · maj-export · maj-ai · maj-agent · maj-mcp · maj-collab · maj-cloud · maj-plugins
Layer 4  maj-audio · maj-playback · maj-render
Layer 3  maj-graph · maj-color · maj-gpu
Layer 2  maj-media · maj-cache
Layer 1  maj-core          <- authoritative model
Layer 0  maj-time
```

### Layer 0 — `maj-time`

Rational time, timecode (non-drop and drop-frame), frame ranges and retime maps.
**Zero dependencies** by default; `serde` support is an opt-in feature so a persistence
layer can store rates without the arithmetic core acquiring a dependency graph.

### Layer 1 — `maj-core`

The document. Entities (media, sequences, tracks, clips), the command log, undo and
redo, invariant validation, canonical serialisation and content hashing. Depends on
`maj-time` and on `serde`/`serde_json`/`blake3` for persistence.

### Layer 2 — `maj-media`, `maj-cache`

Media I/O behind an owned abstraction (probe, demux, decode, seek, waveforms,
thumbnails, hardware decode surfaces) and the four cache layers keyed by the hash of
every rendering input.

### Layer 3 — `maj-graph`, `maj-color`, `maj-gpu`

The typed render graph, colour management (OpenColorIO and ACES) and the `wgpu`
device, frame pools and decoder-surface bridges.

### Layer 4 — `maj-audio`, `maj-playback`, `maj-render`

The realtime audio engine, the deadline-driven playback scheduler, and the offline
renderer that evaluates the same graph with an offline policy.

### Layer 5 — the differentiation and the ecosystem

`maj-timeline` (editorial operations), `maj-export`, `maj-ai` (local inference host),
`maj-agent` (the Pilot), `maj-mcp` (the editor served over the Model Context Protocol),
`maj-collab` (CRDT over commands), `maj-cloud` and `maj-plugins`.

`maj-mcp` sits at layer 5 rather than 6 because it is an interface to the same command
surface `maj-core` already publishes, not an application on top of one: it decodes tool
calls into commands and appends them, and it renders through `maj-render` and `maj-export`.
A layer-6 placement would make `maj-cli` and `maj-agent` its dependents, which is the wrong
shape — see ADR 0011.

### Layer 6 — `maj-ui`, `maj-win32`, `maj-cli`, `maj-script`

The shell, its window, the headless CLI that will host `maj render --verify`, and the
scripting surface. All four drive the same command API as the agent does.

`maj-ui` is the shell as a **model**: 77 intentions, a keyboard table, seven pages, two
languages and a preference file, all of which translate into the same commands an agent's
tools append. It
carries no windowing dependency at all, deliberately: the toolkit does not build on a host
with no C toolchain, and the part with the guarantees was written so that it compiles and is
tested anyway.

`maj-win32` is that shell in a window, and it is the **one crate in the workspace whose lint
level for `unsafe_code` is not `forbid`**, because the Win32 imports are declared by hand: it
is the platform edge, and `docs/adr/0013` records why and what it costs. It depends on
`maj-ui` and never the other way round, and it decides nothing about the document — it turns
messages into the strokes the shell's own table defines and paints what the shell reports.
A presenter for another platform would be a sibling crate rather than a change to this one.
See ADR 0012, ADR 0013 and `docs/UI.md`.

## Crate to phase map

| Crate | Layer | Implemented in |
|---|---|---|
| `maj-time` | 0 | **done (M1)** |
| `maj-core` | 1 | **done (M1)** |
| `maj-media`, `maj-cache` | 2 | M2, M10 |
| `maj-graph`, `maj-gpu` | 3 | M4 |
| `maj-color` | 3 | **done (M5)** |
| `maj-playback` | 4 | M3 |
| `maj-render` | 4 | **done (M4)** |
| `maj-audio` | 4 | M9 |
| `maj-timeline` | 5 | M6 |
| `maj-export` | 5 | **stills and manifests done; encoders are M8** |
| `maj-ai`, `maj-agent` | 5 | M11 |
| `maj-mcp` | 5 | **done (added after M8)** |
| `maj-collab` | 5 | M12 |
| `maj-cloud` | 5 | M13 |
| `maj-plugins` | 5 | M14 |
| `maj-ui` | 6 | **done (M7)** |
| `maj-win32` | 6 | **done (M7, added with the window)** |
| `maj-cli` | 6 | **done (render, verify, probe, bench, edit, mcp)** |
| `maj-script` | 6 | M14 |

## Engineering rules

These are non-negotiable; the roadmap phases assume them.

1. **The project model is the only authority.** Preview, export, cache, agents and
   collaborators are consumers of the command log.
2. **Preview and export share semantics and differ only in policy** — quality,
   proxies, time budget, rate control.
3. **Rational time, never floats.** Every position is an integer frame count under an
   exact rational timebase. Nothing in the model is an `f64`.
4. **No allocation in the audio callback, no blocking on the UI thread.**
5. **Every effect declares its contract**: pixel format, colour space, alpha
   convention, region of definition and interest, determinism class, and whether it
   runs on CPU, GPU or both.
6. **Every cache is keyed by the hash of all of its inputs**, never by frame number.
7. **Nothing is invisible.** If a proxy, an optimised copy or a draft render is in
   play, the interface says so.
8. **Licence hygiene**: the product is `Apache-2.0 OR MIT`, so no GPL component may be
   linked in; LGPL dependencies are dynamically linked and behind an abstraction this
   workspace owns, and vendor SDKs are isolated behind feature flags.

## Where the crates are today

M0 and M1 are complete, and the renderer, the colour model, the exporter's stills, the command
line, the MCP server and the interface are built and gated: **nine crates carry real code** —
`maj-time`, `maj-core`, `maj-color`, `maj-render`, `maj-export`, `maj-cli`, `maj-mcp`, `maj-ui`
and `maj-win32`. The other **thirteen are documented scaffolds**: each declares its
responsibility, its layer and a test that pins both, and it declares the lower-layer
dependencies it is allowed to use, so the compiler enforces the layering from the first
commit rather than by convention.
