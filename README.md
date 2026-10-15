# Majestat

A professional video editor built as a **deterministic engine** rather than an
application with an engine inside.

Premiere and Resolve are applications with an engine in them. Majestat is an engine with
an application on top. The engine's central claim: **everything is a command**. Every
edit — from a user, from the AI agent, or from a collaborator across the network — is an
immutable command appended to a log, and the render is a pure function of that log. Four
capabilities then fall out of one arrangement instead of being bolted on:

* **Reproducible renders** — the same log and the same media produce the same bytes, and
  `maj render --verify` will prove it rather than assert it.
* **An agent that really edits** — the Pilot (M11) drives the same typed commands the UI
  does, so its work is undoable, reviewable and diffable. The Model Context Protocol
  server that exposes them is already here: `maj-mcp`, forty-five tools over one log.
* **Realtime collaboration** — syncing semantic commands is tractable; there is no opaque
  binary blob to lock (M12).
* **Verifiable cloud rendering** — a remote render is checked by hash instead of trusted
  (M13).

## Status

**M0 (foundation) and M1 (determinism) are complete, the engine renders, and there is a
window to edit in.** Nine crates carry real code — time, model, colour, render, export, the
CLI, the MCP server, the shell and its window — and the other thirteen are documented
scaffolds: each declares its responsibility and its layer, and each declares the lower-layer
dependencies it is allowed to use, so the compiler enforces the architecture from the first
commit.

| | |
|---|---|
| Implemented | `maj-time` (rational time, timecode, retiming) · `maj-core` (model, command log, undo, `.maj`) · `maj-color` (transfers, determinism policy) · `maj-render` (the offline renderer: charts, PPM and PNG stills, titles) · `maj-export` (stills, manifests) · `maj-cli` (`render`, `verify`, `probe`, `bench`, `edit`, `mcp`) · `maj-mcp` (the editor over MCP) · `maj-ui` (the shell: intentions, keyboard, two languages, preferences) · `maj-win32` (the window: the `majestat` app) |
| Tested | 539 tests across 53 test targets, plus a ten-thousand-command fuzz on the release gate |
| Scaffolds | `maj-media` (M2), `maj-playback` (M3), `maj-graph` and `maj-gpu` (M4), `maj-timeline` (M6), `maj-audio` (M9), `maj-cache` (M10), `maj-agent` and `maj-ai` (M11), `maj-collab` (M12), `maj-cloud` (M13), `maj-plugins` and `maj-script` (M14) |

A frame renders to a still and a manifest, three renders of one revision are byte-identical,
an agent can build a project over the Model Context Protocol ([`docs/MCP.md`](docs/MCP.md)),
and a person can make the same edits with a keyboard and a pointer through the same log
([`docs/UI.md`](docs/UI.md)) — the two surfaces are tested to be command-for-command
equivalent. The window is real and runs on Windows today:

```sh
cargo run -p maj-win32 --bin majestat
```

What is not here yet: reading or decoding media files, audio, video encoding, and a playback
engine behind the transport clock — the window is keyboard-first, and it declares its Win32
imports by hand because the toolkit the cross-platform story wants (`egui`) could not be
built on this host when the window was written
([ADR 0013](docs/adr/0013-the-window-is-written-against-win32.md)). That constraint has
since lifted: a C toolchain and `dlltool` are present as of P0.4, so the toolkit question
is open again and is a measurement nobody has taken rather than a settled impossibility.
Media that cannot be decoded cannot be rendered, and the renderer says so rather than
inventing pixels: a clip whose picture is not available — a movie this build has no decoder
for, a still that has moved, a file whose bytes are wrong — draws a labelled placeholder
that names which of the three it is, and the render manifest names it too. A delivery can
ask for the other answer and have the frame refused instead. One condition, one policy, and
nothing substituted invisibly.

## Build and verify

Requires Rust 1.85 or newer. `cargo` must be on `PATH`.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# The M1 acceptance gate: 10,000 random commands with exhaustive undo and redo.
cargo test --release -p maj-core --test properties --test roundtrip
```

The fuzz budget is 2,000 commands in debug builds and 10,000 in release builds, so the
gate above is the full one.

## Layout

```
crates/           22 crates in seven layers; see docs/ARCHITECTURE.md
  maj-time/       layer 0 — rational time, timecode, retiming        (done)
  maj-core/       layer 1 — model, command log, undo, .maj           (done)
  maj-render/     layer 4 — the offline renderer                     (done)
  maj-export/     layer 5 — stills, manifests, encoders              (stills done)
  maj-mcp/        layer 5 — the editor over MCP                      (done)
  maj-ui/         layer 6 — the shell: intentions, keys, languages   (done)
  maj-win32/      layer 6 — the window, as the `maj-ui` application  (done)
  maj-cli/        layer 6 — headless render and verification         (done)
  ...             the rest are scaffolds, one per roadmap phase
docs/             architecture, determinism, format, licensing, MCP, UI, coverage, ADRs
assets/brand/     the marks, the supplied logo, and the page that shows them
.planning/        project context, requirements, the seventeen-phase roadmap, state
testdata/         the media corpus description (files are not committed)
```

## Documentation

* [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — the layers, the crate map, the
  engineering rules.
* [`docs/DETERMINISM.md`](docs/DETERMINISM.md) — what reproducible means precisely: two
  tiers, what is hashed, what is deliberately not deterministic, and the gates.
* [`docs/PROJECT-FORMAT.md`](docs/PROJECT-FORMAT.md) — the `.maj` document, its schema,
  its load gates and its compatibility rules.
* [`docs/LICENSING.md`](docs/LICENSING.md) — the `Apache-2.0 OR MIT` decision, what it
  forecloses, the dependency audit, and the gate a new dependency has to pass.
* [`docs/FFMPEG.md`](docs/FFMPEG.md) — the pinned FFmpeg build: the exact version, the
  configuration with its licence-mandatory flags separated from the rest, the LGPL-2.1
  obligations against the clauses that create them, and the runtime gate that refuses a
  library which cannot prove it is the LGPL build.
* [`docs/MEDIA.md`](docs/MEDIA.md) — `maj-media`'s surface, designed against the
  `PictureResolver` seam: the probe and decode vocabulary, why the adapter lives at layer 4,
  what the decoder deliberately does not convert, and the one decision M2 cannot start
  without.
* [`docs/COVERAGE.md`](docs/COVERAGE.md) — the Fase II catalogue audited against what the
  engine actually does: 130 items, 33 reachable today, and what to build in what order.
* [`docs/MCP.md`](docs/MCP.md) — the agent's door into the editor: forty-five tools, five
  resources, three prompts, the guarantees, and what is deliberately not exposed.
* [`docs/UI.md`](docs/UI.md) — the person's door into the same log: the intentions, the
  keyboard, the two languages, the preferences, the parity with the agent's tools, and the
  window on top of it, with what it does and does not do yet.
* [`.planning/ROADMAP.md`](.planning/ROADMAP.md) — the seventeen phases with their
  acceptance gates.

## Architecture in one diagram

```
Layer 6  maj-ui · maj-win32 · maj-cli · maj-script
Layer 5  maj-timeline · maj-export · maj-ai · maj-agent · maj-mcp · maj-collab · maj-cloud · maj-plugins
Layer 4  maj-audio · maj-playback · maj-render
Layer 3  maj-graph · maj-color · maj-gpu
Layer 2  maj-media · maj-cache
Layer 1  maj-core          <- authoritative model
Layer 0  maj-time
```

Dependencies point downward only. The interface never touches media I/O directly; it
emits commands.

## A first project, in code

```rust
use maj_core::{Command, History, MediaAsset, MediaAssetId, Project};
use maj_time::Timebase;

let mut project = Project::new("demo");
let mut history = History::new();

history.apply_local(
    &mut project,
    Command::CreateMediaAsset {
        index: 0,
        asset: MediaAsset::new(
            MediaAssetId::new(1),
            "shot-01.mov",
            "file:///media/shot-01.mov",
            Timebase::FPS_23_976,
            2_400,
        )
        .unwrap(),
    },
)?;

let revision = project.revision();
history.undo(&mut project)?;   // exactly reversed, including list position
history.redo(&mut project)?;   // and exactly re-applied
assert_eq!(project.revision(), revision);
```

## Licence

**`Apache-2.0 OR MIT`** — the recipient picks either. The full texts are
[`LICENSE-APACHE`](LICENSE-APACHE) and [`LICENSE-MIT`](LICENSE-MIT), and the licence is
declared once in `[workspace.package]` and inherited by all twenty-two crates.

The consequence worth knowing before adding anything: a permissive licence cannot absorb
copyleft code, so **no GPL component may enter the shipping product** — `x264` and `x265`
are out, FFmpeg is admissible only as an LGPL build behind an owned abstraction and
dynamically linked, and codec patent pools remain a separate question that a copyright
licence does not touch. See [`docs/LICENSING.md`](docs/LICENSING.md) for the audit and the
gate every new dependency has to pass.
