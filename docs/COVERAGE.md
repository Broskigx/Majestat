# Coverage: the Fase II catalogue against what the engine actually does

This document audits the 130-item *Catálogo Técnico Fase II* (seven blocks: ingest, editing,
VFX, colour, audio, automation/AI, delivery) against the surface Majestat has **today**. It
exists because the catalogue reads like a specification of shipping behaviour, and a reader —
or an agent — has no way to tell from it which parts are reachable. Every status here was
checked by running the code, not by reading the catalogue favourably.

Audited 2026-10-14, at the revision where the MCP server went live, and re-audited
2026-10-16 at P1 — the document floor: a sequence's start timecode, per-asset fingerprints
and relink by hash, a marker entity, and the autosave journal. Four rows moved up and three
moved down; both directions are in the table.

## The surface that exists

Verified with `maj mcp --list-tools`, the command tests and `cargo test --workspace`:

| Thing | What is there |
|---|---|
| Interfaces | `maj render`, `verify`, `probe`, `bench`, `edit`, `mcp` |
| MCP | 51 tools, 5 resources, 3 templates, 3 prompts, `--read-only`, stdio JSON-RPC 2.0 |
| Commands | 36 tags in `Command`, one tool each, checked against `serde` by test |
| Document | a start timecode per sequence, a BLAKE3 fingerprint per asset with a `verified` flag, markers carrying their own author and colour, and an autosave journal beside the document that `maj recover` replays |
| Renderer | scene-linear `f32` picture, tiled and whole renders agree bit for bit |
| Effects | 10: exposure, brightness, contrast, saturation, gamma, hue_rotate, blur, sharpen, vignette, grain |
| Transitions | 8: cross_dissolve, dip_to_black, dip_to_white, wipe_left/right/up/down, circle |
| Blend modes | 10: normal, add, multiply, screen, overlay, darken, lighten, difference, hard_light, soft_light |
| Masks | 2 shapes (rectangle, ellipse), multiplied into the layer's alpha before the effect chain |
| Animatable | 6 channels: opacity, x, y, scale_x, scale_y, rotation; 2 interpolations (hold, linear) |
| Colour | 5 transfer functions (linear, srgb, rec709, pq, hlg); `Rational` everywhere, no floats on the certified path |
| Media | `file://*.png`, `file://*.ppm`, `generated:<label>`, `solid:r,g,b`, `title:…`; anything else is media this build cannot read, and one declared policy covers it — a labelled placeholder (`offline`, `damaged` or `undecodable`) or a refused frame. A real file can be *measured* (BLAKE3 of its bytes) and found again anywhere under a root, which is not the same as being decodable |
| Output | PNG (8/16-bit, alpha) and PPM, each with a JSON render manifest and a frame digest |
| Provenance | every edit is a command in the log, hashed; `maj verify` re-derives a frame two ways |

**There is no audio engine and no video encoder.** `maj-audio` and the video half of
`maj-export` are named in the architecture and unimplemented; `maj-render`'s own docs say
"this crate produces picture, and `maj-audio` owns sound". An audio track in the model
contributes no picture. That single fact decides most of blocks 5 and 7.

## Legend

* **NOW** — reachable today through an advertised MCP tool, no new code.
* **PART** — reachable in substance by composing existing commands, or with a stated limit.
* **ENGINE** — the model or renderer must grow a feature first; the layer is `maj-core`
  (model), `maj-render` (picture), `maj-export` (files) or a new crate.
* **M13** — belongs to the networked phase with the cloud, sessions and review.
* **OUT** — outside a certified, headless, deterministic picture engine: a decoder farm, GPU
  compute, a DAW, or a user interface.

Nothing in the catalogue is marked NOW merely because it sounds like an effect that exists.

## 1. Ingest, codecs and media structure — 20 items

| # | Item | Status |
|---|---|---|
| 1.1 | Multi-GPU decoding cascade | OUT (decode farm) |
| 1.2 | Proprietary RAW (ARRIRAW, R3D, X-OCN, CRM, BRAW) | OUT (decode farm) |
| 1.3 | Input transform from camera metadata | PART — transfer functions exist; nothing reads a file's header |
| 1.4 | Multi-channel WAV/MXF split on import | OUT (no audio) |
| 1.5 | Proxy generation by resolution and codec | ENGINE (`maj-export`: no encoder) |
| 1.6 | Jam-sync / LTC timecode alignment | PART — `Rational`, `Timebase`, SMPTE drop-frame exist; no LTC ingest |
| 1.7 | Duplicate-name detection in the pool | ENGINE (`maj-core`: no pool) |
| 1.8 | Checksum verification while copying cards | ENGINE (offline ingest tool) |
| 1.9 | Subfolders from metadata | ENGINE (`maj-core`) |
| 1.10 | XDCAM / P2 / XF card XML | OUT (decode farm) |
| 1.11 | Span clip reassembly | ENGINE (`maj-core`) |
| 1.12 | SEI / gyro metadata extraction | OUT (decode farm) |
| 1.13 | VBR and dropped-frame analysis | ENGINE (decode farm, reporting) |
| 1.14 | 10/12/16-bit float ingest | PART — the pipeline is `f32` linear and PNG is 16-bit; no source decode |
| 1.15 | Broken-link recovery by hash | PART — `maj fingerprint` records the hash of the bytes and `maj relink` finds them again under a root; an asset nothing has measured has no identity to match on, and matching is by hash alone, never by name |
| 1.16 | Pool audio preview with in/out | OUT (no audio, no UI) |
| 1.17 | Colour labels in the pool | ENGINE (`maj-core`: no tagging) |
| 1.18 | Smart bin query builder | ENGINE (`maj-core`: no query layer) |
| 1.19 | CSV / PDF project reports | PART — `maj probe` and the `maj://project` resource report; no file export |
| 1.20 | Source-media write lock | PART — the renderer never writes media; there is no lock flag |

## 2. Editing, timeline, precision — 20 items

| # | Item | Status |
|---|---|---|
| 2.1 | Unlimited multitrack | NOW — the model has no track limit; `maj_create_track` |
| 2.2 | Drag constrained to one axis | PART — `maj_move_clip` (time) and `maj_reparent_clip` (track) are the two axes |
| 2.3 | Unlink / link audio and video | ENGINE (`maj-core`: clips are independent, so there is no link to break — a link group is a model feature, not a composition) |
| 2.4 | Make subclip | PART — an asset plus a `source_range` on a new clip is a subclip |
| 2.5 | Insert blank space | PART — a `solid:` clip as filler; no dedicated command |
| 2.6 | Close gap across the sequence | PART — one `maj_move_clip` of the later clip onto the earlier clip's tail; the shell composes exactly that, and refuses when the two already touch or overlap |
| 2.7 | Duplicate a sequence | ENGINE (`maj-core`: no deep copy) |
| 2.8 | Reverse playback | ENGINE — `SetClipSpeed` refuses speed ≤ 0 (verified in `command.rs`) |
| 2.9 | Freeze frame | PART — a still asset trimmed to a duration |
| 2.10 | Speed ramping by keyframe | ENGINE — speed is not one of the 6 animatable channels |
| 2.11 | Auto-align by audio waveform | OUT (no audio) |
| 2.12 | Markers with duration and comments | NOW — `maj_add_marker`, `maj_set_marker`, `maj_remove_marker`: a name, a body, a frame, a length, one of eight closed colours, and its own author |
| 2.13 | Marker navigation | PART — the tools read and change what is under the playhead; stepping from marker to marker is a UI gesture that does not exist yet |
| 2.14 | Hide tracks by kind | OUT (UI) |
| 2.15 | Independent track heights | OUT (UI) |
| 2.16 | Alt-drag copy | NOW — `maj_place_clip` / `maj_insert_clip` at another index |
| 2.17 | Three-point editing | ENGINE — three-point editing is a *rule* over a source range and a record range, and no rule is enforced: an agent can trim and insert, but nothing refuses the edit that contradicts the in/out points it set |
| 2.18 | Four-point editing with speed fit | ENGINE — the same, and the speed fit that a four-point edit needs to reconcile two durations does not exist either |
| 2.19 | Ripple trim frame by frame | PART — `maj_trim_clip` is exact frame arithmetic and the shell's ripple operations compose `TrimClip` with `MoveClip`; there is no single ripple-trim command, and the keyboard is the client's |
| 2.20 | Trim clamped to the source's own limits | NOW — `a_trim_beyond_the_media_is_rejected` enforces it |

## 3. Composition and VFX — 20 items

| # | Item | Status |
|---|---|---|
| 3.1 | Mask inputs on effect node ports | PART — masks are clip-level alpha cutouts, not per-node ports |
| 3.2 | Colour corrector inside the node graph | PART — the grade exists as 10 effects; there is no node graph |
| 3.3 | Bokeh with a polygonal aperture | ENGINE (`maj-render`) |
| 3.4 | Heat haze | ENGINE (`maj-render`) |
| 3.5 | Digital glitch / corrupted blocks | ENGINE (`maj-render`) |
| 3.6 | Cartoon and comic stylisation | ENGINE (`maj-render`) |
| 3.7 | Rain and snow particles with collision | ENGINE (new subsystem) |
| 3.8 | Volumetric fire and smoke | ENGINE (new subsystem) |
| 3.9 | Optical-flow / gyro stabilisation | ENGINE (`maj-render`) |
| 3.10 | Clone brush with tracking | ENGINE (new subsystem) |
| 3.11 | Corner pin / perspective warp | ENGINE (`maj-render`: transform has no perspective) |
| 3.12 | Volumetric light shafts | ENGINE (`maj-render`) |
| 3.13 | Drop shadow | PART — a second clip with transform, opacity, mask and `blur` composes one |
| 3.14 | Optical vignette | NOW — the `vignette` effect |
| 3.15 | Video wall / multi-screen matrix | PART — `scale_x`/`scale_y`/`x`/`y` and the blend modes compose it |
| 3.16 | Channel swap and photographic negative | ENGINE (`maj-render`: no per-channel op) |
| 3.17 | Vector shape generators | PART — rectangle and ellipse exist as masks (cutouts), not as fills or strokes |
| 3.18 | Luma-mix transition | ENGINE (`maj-render`: the 8 transitions are fixed shapes) |
| 3.19 | Deconvolution sharpening (Richardson-Lucy) | PART — `sharpen` exists; it is not Richardson-Lucy |
| 3.20 | Mathematical modifier expressions | PART — keyframes with `hold` and `linear` only |

## 4. Grading and colour models — 20 items

| # | Item | Status |
|---|---|---|
| 4.1 | Pivot control on the primary wheel | ENGINE (`maj-render`: no pivot parameter) |
| 4.2 | Saturation by luminance | ENGINE |
| 4.3 | Skin-tone indicator on the vectorscope | OUT (scopes and UI) |
| 4.4 | RGB splitter / combiner nodes | ENGINE (no node graph, no channel ops) |
| 4.5 | Cineon log-to-linear | PART — PQ, HLG, Rec.709 and linear exist; Cineon does not |
| 4.6 | Test charts, grey stairs, resolution charts | PART — `generated:<label>` is a seeded pattern and `solid:` a flat field; not SMPTE bars |
| 4.7 | Shot match by luminance histogram | ENGINE |
| 4.8 | HDR stepping wheels by nits zones | ENGINE |
| 4.9 | Timeline space vs output space | PART — one transfer per render (`--transfer`), not two spaces |
| 4.10 | Batch copy of a grade to many clips | ENGINE (`maj-core`: no copy or batch command) |
| 4.11 | 3D qualifier with softness bounds | ENGINE |
| 4.12 | Luminance-versus-saturation shaper | ENGINE |
| 4.13 | Named colour versions per clip | ENGINE (`maj-core`) |
| 4.14 | Per-shot colour notes | PART — a marker is a note at a frame, with an author and a colour, anchored to the sequence rather than to the clip's grade |
| 4.15 | Channel blur | ENGINE (no per-channel op) |
| 4.16 | Invert a Power Window | ENGINE (no invert flag on a mask) |
| 4.17 | Per-node opacity and gain | ENGINE — `maj_set_effect_enabled` is on/off; opacity is per clip |
| 4.18 | Solo channel view | ENGINE |
| 4.19 | Hue-versus-hue vector shift | PART — `hue_rotate` is global, not per-hue |
| 4.20 | Dolby Vision dynamic metadata pass | ENGINE |

## 5. Audio — 20 items

Every item: **OUT**. There is no audio engine to extend — `maj-audio` does not exist, so
bussing, PDC, HRTF panning, de-reverb, multiband dynamics, goniometry, LUFS (BS.1770-4),
pitch shift, room tone, punch-in, de-click, de-ess, dynamic EQ, pan/volume automation, clip
gain, tempo delay, saturation, polarity invert, sidechain and stem tails are all one missing
crate, not twenty missing features. The model's `audio` track kind is a placeholder that
renders nothing.

## 6. Automation, AI, subtitles, collaboration — 20 items

| # | Item | Status |
|---|---|---|
| 6.1 | Multilingual speech to text | ENGINE (no text entity; needs a model) |
| 6.2 | Open and closed captions with styles | ENGINE |
| 6.3 | Spell and grammar correction | ENGINE |
| 6.4 | CPS / WPM reading-speed limits | ENGINE |
| 6.5 | SRT, VTT, SCC, EBU-STL import and export | ENGINE |
| 6.6 | Machine translation of subtitles | ENGINE |
| 6.7 | Users, roles, permissions | M13 |
| 6.8 | Cloud project versioning | M13 |
| 6.9 | In-app chat | M13 (UI) |
| 6.10 | Timeline comparison and diff | ENGINE — and cheap: the command log already holds every change, in order |
| 6.11 | Password-protected review links | M13 |
| 6.12 | Comments pinned to a frame | PART — the marker entity is a comment pinned to a frame and it names its author; there is no reply, no resolve state and no thread |
| 6.13 | Scripting console | PART — `maj edit` and the MCP server are the script surface; no embedded console |
| 6.14 | Hardware resource reports | PART — `maj bench` times real renders; no RAM/VRAM monitoring |
| 6.15 | Autosave and crash recovery | NOW — the shell journals every group beside the document and `maj recover` replays it; the gate is that a recovery and a clean replay of the same commands produce the same `document_hash` |
| 6.16 | Background rendering cache | ENGINE |
| 6.17 | Cache cleanup | ENGINE |
| 6.18 | Tangent / Loupedeck panels | OUT (no UI) |
| 6.19 | Keyboard shortcut profiles | OUT (no UI) |
| 6.20 | Log archive bundle | PART — typed errors and deterministic hashes; no bundle to collect |

## 7. Render, mastering, delivery — 10 items

| # | Item | Status |
|---|---|---|
| 7.1 | Alpha-channel export (ProRes 4444, DNxHR HQX) | PART — PNG carries alpha; there is no video container |
| 7.2 | Video range versus full range | ENGINE (`maj-export`: no levels policy) |
| 7.3 | Start timecode on export | PART — a sequence has a start timecode and `maj_set_sequence_start_timecode` sets it; there is no encoder to burn it in yet |
| 7.4 | Discrete audio streams in MP4/MOV/MXF | OUT (no encoder, no audio) |
| 7.5 | Multi-profile delivery queue | ENGINE (`maj-export`) |
| 7.6 | Queue completion notification | ENGINE |
| 7.7 | Multi-channel EXR | ENGINE (`maj-export`) |
| 7.8 | Strict CBR for broadcast | OUT (no encoder) |
| 7.9 | Two-pass VBR | OUT (no encoder) |
| 7.10 | Leader with countdown and 1 kHz tone | PART — a generated chart plus timecode gets close; no countdown generator, no tone |

## Tally

| Status | Items |
|---|---|
| NOW — an advertised tool does it | 6 |
| PART — by composing tools, or with a named limit | 32 |
| ENGINE / M13 — the engine must grow it | 57 |
| OUT — decode farm, GPU, DAW or UI | 35 |
| **Total** | **130** |

**130 items, 38 reachable in some form today.** The catalogue is a roadmap, and reading it as
a description of the current build would be a mistake — most of all for an agent, which would
try tools that do not exist.

## What to build, in the order that unlocks the most

Each of these removes a whole cluster of the table above, rather than one row:

1. **`maj-audio`** (new crate, layer 4) with the model, mixdown and stem writes. Unlocks all
   20 items of block 5, plus 1.4, 1.16, 2.11, 7.4. Nothing else is as large or as isolated.
2. **A video encoder and muxer in `maj-export`.** Unlocks 1.5, 7.1, 7.5–7.9 and every
   "deliver a file" item. The frame writer is already a pure function of a `Framebuffer`, which
   is the seam the encoder plugs into.
3. **Link groups in `maj-core`.** 2.3, and every ripple that has to know which clips move
   together: today each clip is independent, so an edit that should have carried a shot and
   its sound moves one of the two. (Markers, 2.12, landed in P1 and closed 4.14 and 6.12
   with them.)
4. **Speed as an animatable channel, and negative speed.** 2.8 and 2.10 together; the channel
   animation machinery already exists for six channels.
5. **Structural editing commands in `maj-core`:** `close_gap`, `insert_blank`, `duplicate`
   (sequence and clip), and a per-clip `copy_grade`. Clears 2.5–2.7, 2.17–2.18, 4.10, 4.13.
6. **A media pool with tags and queries.** 1.7, 1.9, 1.17, 1.18 and the inspection half of
   1.19 — the identity half of 1.15 arrived in P1 as a fingerprint and a relink by hash.
7. **Rich effects in `maj-render`:** per-channel operations, mask invert, per-effect opacity
   and a pivot. Each is one parameter or one kernel, and together they cover most of blocks 3
   and 4 that is marked ENGINE.
8. **Text: subtitles and transcription.** A new `maj-text` crate plus a model, unlocking 6.1–6.6.
9. **A decoder.** RAW, span assembly, camera metadata and card structures are one subsystem —
   and it is the one part of the catalogue that must not be allowed to compromise the
   certified path. It belongs behind `PictureResolver`, where an undecodable file already
   resolves to a labelled placeholder — or to a refusal, under a delivery policy — rather
   than to a chart that could be mistaken for a picture.
10. **GPU and panels last.** 1.1, 1.5's speed, 6.18 and 6.19 are performance and interface
    work; none of them changes what the engine can express, and all of them can break
    determinism if they are allowed onto the certified path.

## What the catalogue asks for that already exists, and the catalogue does not know

The catalogue's block 6 asks for versioning, comparison, attribution and automation, as if
they were features to add. They are consequences of the architecture:

* **6.10, timeline comparison, is nearly free.** Every change is a `Command` in an ordered log
  with a hash per revision. A diff is a read of the log, not a new subsystem.
* **Attribution is already there.** An agent's edits are recorded under its client name
  (`mcp:claude-desktop`), and a project an agent built over MCP is byte-identical to one built
  by `maj edit` — the same document, the same hashes, the same frame digests.
* **Verification is already there.** `maj verify` and `maj_verify_frame` re-derive a frame two
  ways and compare digests, so an agent's render can be checked by a person who never ran it.
* **Automation is already there.** `maj edit` takes a command as JSON and the MCP server takes
  the same commands as tools; a script and a model are the same kind of author.
* **Titles are already there, and the catalogue never asks for them.** The catalogue's block 6
  is about *text it has to read* — transcription and captions — while a title is text the editor
  has to *set*. The renderer now sets its own alphabet, and a title is two ordinary commands
  (`create_media_asset` with a `title:` URI, then `insert_clip`), so an agent can add one today
  without a tool that is not a command. What is still ENGINE is 6.2 and 6.4: a caption is a
  timed text entity with styles and reading-speed limits, and neither exists.

That is the part of the catalogue that is genuinely satisfied — not by adding features, but by
having refused to add a second way to edit.
