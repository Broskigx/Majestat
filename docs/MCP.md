# Majestat over MCP

`maj-mcp` is the Majestat editor exposed to an AI client over the Model Context Protocol.
It is not a bridge, a macro recorder or a second API: every editing tool **decodes into a
`Command` and appends it to the same log a person's edit goes into**. There is one path
into the model, and the agent walks it.

That single decision is what makes an agent's work safe to hand over. Anything it does can
be undone exactly, replayed from the log, diffed against a colleague's revision, attributed
to the client that made it, and rendered — because the renderer cannot tell whether a
command came from a keyboard or from a model.

* Crate: `crates/maj-mcp` (layer 5, alongside `maj-export` and `maj-agent`)
* Binary: `maj-mcp`, also reachable as `maj mcp`
* Transport: JSON-RPC 2.0 over stdio, one message per line
* Surface: **51 tools, 5 resources, 3 resource templates, 3 prompts**

## Running it

```sh
maj-mcp --project reel-a.maj               # open a document, start editing it
maj-mcp --name scratch --session b-roll    # an unsaved project, in a named session
maj-mcp --project locked.maj --read-only   # answer every question, refuse every edit
maj mcp --project reel-a.maj               # the same server, from the headless CLI
```

| Option | Meaning |
|---|---|
| `--project <path>` | Open a `.maj` document into the starting session. Optional; without it the project is empty and unsaved. |
| `--name <name>` | Name of the project to start when no document is given. |
| `--session <name>` | Name of the starting session (default: `default`). |
| `--read-only` | Every editing tool refuses, every reading tool answers. For handing an agent a project you do not want changed. |
| `--list-tools` | Print the whole catalogue as JSON and exit. The fastest way to see the current surface. |
| `--version`, `-V` | Print the version. |
| `--help`, `-h` | Print usage. |

The banner goes to **stderr**, so it never corrupts the protocol stream:

```
maj-mcp 0.0.1 — 51 tools, 5 resources, 3 prompts on stdio
```

## Connecting a client

Any MCP client that speaks stdio works. A minimal `mcpServers` entry:

```json
{
  "mcpServers": {
    "majestat": {
      "command": "maj-mcp",
      "args": ["--project", "C:/media/reel-a.maj"]
    }
  }
}
```

Point `command` at the built binary directly (`target/release/maj-mcp`) if it is not on
`PATH`. Nothing else is required — no port, no daemon, no account, no network.

## What it speaks

Methods: `initialize`, `ping`, `tools/list`, `tools/call`, `resources/list`,
`resources/templates/list`, `resources/read`, `prompts/list`, `prompts/get`, plus
notifications (`notifications/initialized` and friends, which are read and never answered).

* **Protocol revision** is negotiated: the server echoes the revision the client asks for
  when it knows it (`2025-06-18`, `2024-11-05`) and answers with its latest otherwise, so an
  unknown future revision is a downgrade rather than a failure.
* **Batches** are accepted: one JSON array is answered with one message per element,
  including for elements that were malformed, because a batch is answered as a batch.
* **Blank lines** are ignored.
* **Attribution** is set at the handshake: an agent's commands are logged as
  `mcp:<clientName>`, so a person's edit and a model's edit stay distinguishable in the log
  without a second table to join against.
* **Initialisation is enforced**: before `initialize`, every method except `initialize` and
  `ping` is refused.
* **Numbers are exact.** Durations, positions and rates are rational — write `0.5`,
  `"1/2"` or `{"num":1,"den":2}` and the engine converts a decimal to a ratio without
  rounding. `24000/1001` is the honest way to say 23.976.

The `initialize` result carries an `instructions` string. It is written for a model and is
the shortest statement of the contract:

> Read before you write. `maj_project_state` and `maj_timeline` are the only source of truth
> about what exists, and every identifier you use must come from them. Every editing tool
> appends exactly one command. Identifiers are allocated for you when you omit them, so
> never invent one. Numbers are exact. Look at your work. A refusal is information, not a
> failure. Nothing you do is committed until `maj_save_project`, and `maj_undo` reverts
> exactly.

### Identifiers

Identifiers are allocated by the engine and are unique **across the whole project**, not
per kind. A session that registers one asset, creates one sequence with two tracks and
appends one clip consumes ids 1 through 5 — the sequence's tracks take 3 and 4, and the clip
is 5. Every editing tool therefore answers with the ids it allocated, and a client must use
those rather than the numbers it asked for. `maj_project_state` and `maj_timeline` list
what exists.

## The tools

Fifty-one tools, in three kinds. **Thirty-six** map one-to-one onto a command variant and
append exactly one command, named after the tool. **Two** — `maj_setup_sequence` and
`maj_place_clip` — are shorthands that append a variable number of commands, which is why
they declare no single variant: their work is still one command per change, so it is as
undoable as anything else. **Thirteen** read the project, change a session, render or undo,
and append nothing at all. A test asserts that the first group covers every command the
model defines, name for name, so the surface cannot drift from the engine.

### Media

| Tool | What it does |
|---|---|
| `maj_create_media_asset` | Register media in the project at an index, or at the end. Registering is not importing: nothing is read from disk and the URI may be procedural (`generated:bars`). |
| `maj_insert_media_asset` | The same registration at an explicit position in the pool. |
| `maj_remove_media_asset` | Remove a registration, refusing when clips depend on it. |
| `maj_relink_asset` | Point an asset at a different file. Its identity does not change, so every clip that reads it keeps its trims, its effects and its place: this is a relink, not a re-registration. |
| `maj_set_asset_fingerprint` | Record what the file measured — BLAKE3 of its bytes, its size, and its duration if anything read the container — or clear the measurement with null. Measuring is I/O and this server does none, so the number arrives from wherever the file was read; it reaches the document as a command, which is what makes it attributable and undoable like every other fact here. |

### Sequence and track structure

| Tool | What it does |
|---|---|
| `maj_setup_sequence` | The one-call setup: a sequence at a size and timebase with one track per kind. |
| `maj_create_sequence` / `maj_insert_sequence` / `maj_remove_sequence` | Add a sequence, restore one at an index, remove one. |
| `maj_create_track` / `maj_remove_track` | Add a video or audio track on a sequence, or remove one with everything on it. |
| `maj_set_track_enabled` | Turn a whole track on or off: a disabled track stays on the sequence and the renderer skips it. |
| `maj_set_sequence_start_timecode` | Say what timecode frame zero is called, as a frame count at the sequence's own timebase. A label rather than a move: no clip changes position. The broadcast convention is one hour — 90000 frames at 25 fps — so the programme does not start at `00:00:00:00` and a pre-roll has somewhere to live. A negative offset is refused. |

### Clips

| Tool | What it does |
|---|---|
| `maj_append_clip` | Append a clip to the end of a track. |
| `maj_insert_clip` | Insert a clip at an index, rippling what follows. |
| `maj_place_clip` | The editorial shorthand: register the media if it is new, pick the track by kind and position, and append. Emits one command per change, so its work is as undoable as anything else. |
| `maj_remove_clip` | Remove a clip from a track. |
| `maj_trim_clip` | Adjust the source range and the timeline start. |
| `maj_move_clip` | Move a clip to a new timeline position. |
| `maj_reparent_clip` | Move a clip to another track and position. |

### Clip properties

`maj_set_clip_enabled`, `maj_set_clip_opacity`, `maj_set_clip_blend`,
`maj_set_clip_speed`, `maj_set_clip_transform` — visibility, opacity, blend mode, a
retiming rate, and position/scale/rotation.

### Effects, masks, transitions, colour

| Tool | What it does |
|---|---|
| `maj_add_effect` / `maj_remove_effect` | Attach or detach an effect on a clip, in order. |
| `maj_set_effect_enabled` | Bypass or re-enable one effect. |
| `maj_set_effect_param` | Set one parameter, merging into the parameters already set. |
| `maj_add_mask` / `maj_set_mask` / `maj_remove_mask` | Shape and geometry applied to a clip's effects. |
| `maj_add_transition` / `maj_remove_transition` | A transition between two clips. |
| `maj_set_channel_keys` | Keyframes on an animatable channel — a colour channel or an effect parameter. |

### Markers

Notes anchored to a span of a sequence. A marker carries its own author, so a review can
read whose note it is without replaying anything, and one of eight closed colours, so
filtering by colour means something.

| Tool | What it does |
|---|---|
| `maj_add_marker` | Anchor a note at an index, at a frame, for a length or as a bare point. A marker needs a name or a note; one with neither is refused. |
| `maj_set_marker` | Replace a note, keeping its identifier and its place in the list. |
| `maj_remove_marker` | Take a note away. Undo puts it back where it was, not merely back. |

### Reading

| Tool | What it does |
|---|---|
| `maj_project_state` | Everything that exists: sequences, tracks, clips, media, revision, log depth. The source of truth. |
| `maj_timeline` | One sequence's timeline, as ordered layers per track. |
| `maj_clip_detail` | One clip in full — source, timeline range, speed, transform, effects, masks. |
| `maj_history` | The command log itself, with authors. |
| `maj_replay_check` | Rebuild the project from the log and compare, proving the log explains the state. |

### Sessions and history

`maj_new_project`, `maj_open_project`, `maj_save_project` — sessions are named, a server may
hold several, and nothing is written to disk until a save. `maj_undo` and `maj_redo` revert
and re-apply exactly, including list position.

### Render and export

| Tool | What it does |
|---|---|
| `maj_render_frame` | Render a frame on the certified path. Returns the text summary, the full digest, **and the image itself** (base64 PNG) — so the model looks at its own work rather than guessing about it. |
| `maj_verify_frame` | Render a frame whole and tiled and compare the digests, proving the render repeats. |
| `maj_export_frame` | Write a frame and its manifest to disk. `.png` at eight or sixteen bits, or `.ppm` with no alpha. |

## Resources

Read-only, stable URIs — cheap to poll, free of side effects.

| URI | Content |
|---|---|
| `maj://project` | The project as the format stores it. |
| `maj://project/summary` | One line: session, name, revision, counts, undo and redo depth. |
| `maj://project/timeline` | The default session's timeline. |
| `maj://project/log` | The command log. |
| `maj://project/hashes` | Content hashes: the document digest and the state digest. |

Templates, which take an argument:

| Template | Content |
|---|---|
| `maj://sequence/{id}` | One sequence. |
| `maj://clip/{id}` | One clip. |
| `maj://frame/{sequence}/{frame}` | A rendered frame, as an image. |

## Prompts

Named, argument-taking workflows a client can offer as slash commands.

| Prompt | Argument | Description it carries |
|---|---|---|
| `maj_rough_cut` | `brief` — what the cut is for: duration, shape, and the order of the material | Set up a timeline and place media on it. |
| `maj_color_pass` | `look` — the look to aim for | Grade a sequence clip by clip with the certified effects. |
| `maj_conform_check` | — | Audit a project: does its state come from its log; gaps, overlaps, missing media, dead air. |

## A worked session

Real messages, real answers, verbatim. Each line is one message on the wire.

```jsonc
// 1 · handshake
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"transcript","version":"1"}}}
// → capabilities, serverInfo {name: "majestat", version "0.0.1"}, the instructions above

// 2 · register the material
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"maj_create_media_asset","arguments":{"asset":{"name":"shot-01","uri":"generated:shot-01","timebase":"24","duration_frames":240}}}}
// → "Create media asset · now at revision 1
//    create_media_asset: asset 1, index 0
//    state hash 23f3872deabff56a…"

// 3 · a sequence with a video and an audio track
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"maj_setup_sequence","arguments":{"name":"Reel A","timebase":"24","width":64,"height":36,"tracks":["video","audio"]}}}
// → "sequence#2 `Reel A` created at 64x36 48000 Hz
//    track#3 V1
//    track#4 A1"

// 4 · put the material on V1
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"maj_place_clip","arguments":{"sequence":2,"kind":"video","asset":1,"source_range":[0,48],"timeline_start":0}}}
// → "clip#5 `shot-01` appended to track#3 of sequence#2 · 48 frames · revision 5"

// 5 · prove the render repeats
{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"maj_verify_frame","arguments":{"sequence":2,"frame":12}}}
// → "verified: whole and 2x2 tiled renders agree at db38e5f36df88377…"

// 6 · look at it
{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"maj_render_frame","arguments":{"sequence":2,"frame":12,"image":true}}}
// → text ("64x36 frame 12 of sequence 2 on the certified path: 1 layer(s), halo 0,
//         grid 1x1, digest db38e5f36df8…") AND an image block: the frame as a PNG

// 7 · where are we
{"jsonrpc":"2.0","id":7,"method":"resources/read","params":{"uri":"maj://project/summary"}}
// → "default · demo · revision 5 · 1 sequences · 1 assets · undo 5, redo 0"
```

Note the ids the agent never chose: asset 1, sequence 2, tracks 3 and 4, clip 5. It reads
them from the answers.

## Guarantees

* **One path into the model.** A tool cannot change the project except by appending a
  command, so an agent's edit is exactly as reversible, replayable and attributable as a
  person's.
* **Determinism on the certified path.** `maj_verify_frame` renders the same frame whole
  and tiled and compares digests; `maj_replay_check` rebuilds from the log and compares
  state. Both are answers, not assertions.
* **Digests everywhere.** Every editing answer carries the revision and, where it matters,
  a state hash; renders carry the framebuffer digest. A digest is something a human can
  check afterwards with `maj verify`.
* **Read-only is a real mode.** `--read-only` refuses every editing tool with a message
  naming the tool and the reason, while leaving every reading tool intact.
* **Errors teach.** Application failures come back as tool results with `isError: true`
  and a sentence that says which rule was broken — *"no asset with identifier 1 is in the
  media pool; register it first"* — so a model can correct itself instead of retrying
  blindly.

## Errors

Two distinct kinds, and the distinction matters to a client:

| Situation | Shape | Meaning |
|---|---|---|
| A bad edit: unknown id, missing media, out-of-range frame, edit attempted in read-only mode | `result.isError: true` with a text content block | The protocol worked; the request was understood and refused. The model should read the sentence and try something else. |
| Unknown tool, unknown resource, or arguments that do not fit the tool | `error.code` `-32602` | The request was wrong about the surface. Call `tools/list`. |
| Unknown method | `error.code` `-32601` | This server does not implement it. |
| Malformed JSON | `error.code` `-32700` | Parse failure. |
| A JSON-RPC message that is not an object | `error.code` `-32600` | Invalid request. |
| Any method before `initialize` | `error.code` `-32002` | Handshake first. |

## Integration status

What is exposed today, and what is deliberately not.

| Area | Status |
|---|---|
| The command surface — structure, media, clips, properties, effects, masks, transitions, markers, colour keys, undo and redo | **Exposed.** 38 of the 51 tools append commands, one command per change. |
| Rendering and verification — frame render with the image, whole-vs-tiled verification, frame export with manifests | **Exposed.** The agent looks at its own work. |
| Sessions — several named projects in one server, open, save, new | **Exposed.** Nothing is written to disk except on `maj_save_project` and `maj_export_frame`. |
| Reading — project state, timeline, clip detail, log, replay check | **Exposed** as tools and as resources. |
| Workflows — rough cut, colour pass, conform check | **Exposed** as prompts. |
| Media I/O — probing and importing real files from disk | **Not exposed.** `maj-media` has a measurement and no decoder, so `maj_create_media_asset` registers a URI without reading it. Registering is not importing, and the renderer is honest about the difference: `.png` and `.ppm` stills are read from disk, `generated:`, `solid:` and `title:` URIs are procedural, and **anything else is media this build cannot read** — one condition with one declared policy. By default it renders a labelled placeholder, named `offline`, `damaged` or `undecodable` in the render's own layer line and manifest, so an agent can tell a frame that stood in for something from a frame that did not. It never shows invented camera pixels, and it never silently shows a test chart in place of a shot. |
| Audio — mixing, routing, effects | **Not exposed.** No audio model yet. |
| Transcoding and delivery — encoders, muxing, C2PA | **Not exposed.** `maj-export` carries stills and manifests; video encodes arrive with M8 proper. |
| Collaboration and cloud — presence, remote sessions, farm rendering | **Not exposed.** A server is one process over one log, which is what keeps its answers honest. |
| Plugins and scripting | **Not exposed.** OpenFX, VST3 and Lua are M14. |

One thing about a file does reach the document, and it arrives as a command rather than as a
tool. `maj_set_asset_fingerprint` records what a file measured — the hash of its bytes, its
size, and its duration if anything read the container — and `maj_relink_asset` follows those
bytes when a drive letter changes. The measurement itself is taken by `maj fingerprint` and
`maj relink` on the command line, because hashing is I/O a build does rather than an edit
somebody makes, and the distinction is ADR 0011's: the server exposes *commands*, and neither
verb is one. So an agent can read what an asset measured, and can relink it, but it cannot
make this server hash a file behind the door.

The surface grows by adding one tool per command, in one place, with the catalogue, the
schema and the description generated from the same definitions the dispatcher uses. The
count is not the point; the fact that a tool cannot exist without a command is.

## Adding a tool

1. Add the command to `maj-core` if it does not exist. The interface never invents an
   operation the engine does not have.
2. Declare the tool in `crates/maj-mcp/src/catalog.rs`: name, title, one paragraph written
   for a model, and a worked argument example.
3. Decode and dispatch it in `crates/maj-mcp/src/tools.rs` — decode arguments, build the
   command, apply it, answer with what changed.
4. Extend the protocol suite in `crates/maj-mcp/tests/protocol.rs`: every advertised tool
   must be callable and every callable tool advertised, and a full editing session must
   still work end to end.
5. Run the gates: `cargo fmt --all -- --check`,
   `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.

Three tests make this mechanical rather than a matter of discipline. The catalogue must
cover **every** command the model defines, name for name — a command without a tool, or a
tool for a command that does not exist, fails the build. Every name-valued argument may
only offer names the model accepts, round-tripped through `serde` rather than retyped. And
every tool's published worked call is decoded and deserialised as part of the test, so a
documentation example cannot rot.

See [ADR 0011](adr/0011-exposing-commands-over-mcp.md) for why the surface is shaped this
way, and [`docs/ARCHITECTURE.md`](ARCHITECTURE.md) for the layering rule that puts this
crate in layer 5.

## What the surface cannot reach

The tools are exactly as wide as the engine, and the engine today renders picture: no
decoder, no audio, no encoder, no text. [`docs/COVERAGE.md`](COVERAGE.md) audits the
130-item Fase II catalogue against the live tool list and names, for every block — ingest,
editing, VFX, colour, audio, automation, delivery — what an agent can do now, what it can
compose out of existing tools, and what needs the engine to grow first. An agent that reads
this file should read that one next, because a model that tries a tool that does not exist
wastes a turn and learns nothing.
