# The interface

`maj-ui` is the shell: the timeline, the viewer, the panels, the command palette and the
keyboard, as a **model** first and in a **window** since ADR 0013 — the model in `maj-ui`, the
window in `maj-win32`. Every intention a person can express — *cut here*, *disable this clip*,
*half-resolution preview*, *speak Spanish* — is a value that translates into `Command`s, and
those commands go into the same log an agent's edit goes into. There is one such translation,
and it is the only door; the window is a pair of eyes on it, not a second way in.

That is the same rule `maj-mcp` follows, and the reason is the same: an edit made with a
keyboard and an edit made by a tool call have to be the same kind of record, or the
determinism, the undo, the diff and the attribution are all fiction.

* Crate: `crates/maj-ui` (layer 6, next to `maj-cli` and `maj-script`)
* Window: `crates/maj-win32`, running as the `majestat` binary — the shell in a window,
  written directly against the Win32 API ([ADR 0013](adr/0013-the-window-is-written-against-win32.md))
* Surface: **77 intentions**, **30 command names**, **67 default strokes**, **199 strings in
  two languages**, seven pages, seven menus, three preview resolutions
* Modules: `action` (intentions), `app` (the shell), `keys` (strokes), `i18n` (English and
  Spanish), `prefs` (the file)

The shell is deliberately **free of any windowing dependency**, and the window is a separate
crate. That is not a taste: `eframe` cannot be built on this host, because every windowing
crate on the shelf reaches `windows-sys`, whose import libraries need `dlltool`. So the part
with the guarantees compiles, runs and is tested with no window in sight, and the window
imports the seventy-six Win32 calls it needs by hand. See
[ADR 0012](adr/0012-the-interface-emits-commands.md) and
[ADR 0013](adr/0013-the-window-is-written-against-win32.md).

## The one door

An intention is a value, and translating it is a function:

```rust
let commands: Vec<Command> = action.commands(&context)?;   // what it means
shell.dispatch(action)?;                                   // the only way that writes
```

`Context` is a borrow of what an intention needs in order to mean something — the project,
the active sequence, the selection, the playhead — so the translation is a pure read of the
state plus the intention. It returns a *list* of commands, because a few intentions are
several commands that have to apply together: duplicating a clip inserts a new clip and then
gives it the original's look, in that order, because an insert is not a copy.

`Shell::dispatch` is the only method that changes a document. It applies the commands
through the session's history, as **one group**, which means:

* each command is inverted as it is applied, so `Action::Undo` is the model's own undo and
  not the interface's bookkeeping;
* **one intention is one undo step.** The commands of an intention go into the log
  separately, in order, each with its own sequence number — but they form a single step, so
  a person presses undo once for an edit they performed once. Adding a title is two commands
  in the document and one thing somebody did, and those two counts have to agree where the
  person is standing. The grouping is session state and never reaches the document: the same
  commands grouped and ungrouped give the same state hash *and* the same document hash,
  which is a test.
* **a refused intention changes nothing.** Not "nothing after the refusal" — nothing at all.
  If the second of two commands is refused, the first is reverted and unlogged, so the
  project cannot be left holding half an edit that was valid as far as the model was
  concerned and wrong as far as the person was concerned. The interface reports the engine's
  own sentence rather than a friendlier paraphrase.
* every entry is authored **`ui:local`**, so a person's work and an agent's work stay
  distinguishable in the log without a second table.

Rendering, zooming, opening and closing panels, changing language and choosing a preview
resolution are **view state**, and they change no document. A test asserts that: a view
action must not move the revision. View state may hold an `f64` (a zoom in pixels per frame
is a preference, not geometry); nothing in the document ever does.

## The intentions

Sixty-five values, in six groups.

| Group | Intentions |
|---|---|
| Playback and view | step by frames or seconds, seek, go to start or end, zoom, toggle a panel, go to or cycle a page, move a divider and reset them, snap, link, loop, choose a draft resolution, tiled viewer, transfer function, language, select the active sequence |
| Selection | select a clip, clear the selection, select media |
| Palette | open, close, set the query, set the row, set an argument, run a tool |
| Document | undo, redo, save, save as, open, load the example, export a still |
| Editing a clip | split at the playhead, remove, duplicate, move, move to another track, trim the head, trim the tail, enable or disable, opacity, blend, transform, speed |
| Effects, masks, transitions, structure | add, remove, bypass and parameterise an effect; add, remove and invert a mask; add and remove a transition; add and remove a track; register and remove media; insert media at the playhead, drop it where the pointer is, or append it; import a still from disk; create and remove a sequence |
| Titles | set the studio's words, face, cap height and alignment — view state — and add the title as media and place it, which is two commands from one intention |

An intention is refused, precisely, when it does not mean anything here: there is no clip
selected, the playhead is outside the clip, a clip cannot move between a video track and an
audio track, the target track is the clip's own track (that is a move, not a reparent).
`UiError::NotApplicable` says which of those it is; `UiError::Refused` carries the engine's
own refusal unchanged.

The interface's vocabulary of command *names* — the thirty variants, spelled exactly
as the log stores them — is compared against the model's own serialisation by a test, so the
interface cannot invent a word the document does not have. `kind_of` exists for logs and
messages; the log itself is the source of truth.

## The keyboard

Keyboard-first here does not mean a keyboard shortcut for everything. It means the keys are
a table, and the table is data:

| Stroke | What it asks for |
|---|---|
| `←` `→` | step one frame; `shift` for ten; `ctrl` for a second |
| `↑` `↓` | move the selected clip to the track above or below |
| `shift` + `↑` / `↓`, `alt` + `←` / `→` | step and select; move the selected clip instead of the playhead (`shift` again for ten) |
| `home` / `end`, `page↑` / `page↓` | the start and end of the sequence, ten seconds at a time |
| `s` / `delete` | split at the playhead / remove the selection |
| `[` / `]` | trim the head / the tail to the playhead |
| `e` / `ctrl` + `d` | disable the selected clip / duplicate it |
| `i` | insert the selected media at the playhead |
| `n`, `shift` + `n` | add a video track, add an audio track |
| `+` `=` `-` | zoom the timeline in and out |
| `ctrl` + `z`, `ctrl` + `shift` + `z`, `ctrl` + `y` | undo, redo |
| `ctrl` + `s`, `ctrl` + `k` | save, the palette |
| `ctrl` + `l`, `ctrl` + `t` | language, tiled viewer |
| `ctrl` + `1` / `2` / `3` | full, half, quarter preview |
| `+` `=` `-`, `alt` + `0` | zoom in, zoom out, fit the whole sequence in the window |
| `space`, `j` `k` `l`, `alt` + `b` | play or pause; shuttle backwards, stop, forwards; loop at the end |
| `alt` + `o`, `ctrl` + `shift` + `s`, `alt` + `x` | open a document, save as, write the frame under the playhead as a still |
| `alt` + `e`, `alt` + `i` | snap dragged edges to their neighbours, link the selection |
| `ctrl` + `tab`, `shift` + `ctrl` + `tab`, `alt` + `1` / `2` / `3` | the next page, the previous one, the media / cut / deliver page |
| `,` / `.` | the previous / the next edit point; `shift` takes the clip there into the selection |
| `g`, `shift` + `g` | close the gap in front of the selection, open a one-second gap at the playhead |
| `shift` + `delete` | ripple delete: remove the selected clip and close the gap it leaves |
| `alt` + `,` / `alt` + `.` | the previous / the next keyframe of the selected clip |
| `alt` + `k`, `shift` + `alt` + `k` | key every channel that is already animated at the playhead, or take those keys away; clear every curve |

Sixty-seven strokes, and three properties of the table are checked rather than trusted: no two
strokes collide, an unbound stroke is silent rather than wrong, and a stroke that is absent
on purpose stays absent.

The transport is the one place where an earlier deliberate absence was **reversed by a
measurement rather than by taste**. The table used to have no `space` and no `j`/`k`/`l`, on
the argument that a transport key with no playback engine behind it is a lie with a triangle
on it, and a test asserted they were unbound so the day playback landed the test was the
thing that had to change. The argument was right about the keys and wrong about the clock:
there is now a transport that advances the playhead at the sequence's own rational rate,
repaints the preview on the certified path and measures the rate it actually sustains, so the
keys are bound and the test was changed on purpose rather than quietly. What the clock does
*not* do is decode media or make a sound — there is no decoder (M2) and no audio model (M9) —
and the status line says so rather than a moving picture implying otherwise. A minute of play
appends nothing to the log, because where the playhead is is view state.

## Two languages, from the first string

The interface speaks English and Spanish, and it did so from the first string rather than
after the fact, because retrofitting translation means finding every literal in a codebase
that has grown comfortable with them. Seventy-six keys, two tables, one lookup:

```rust
let label = text(shell.lang(), Key::PaletteClose);
```

Four tests hold the line: every key appears exactly once in the enumeration, no string is
empty in either language, the Spanish table is genuinely not the English one, and the
language round-trips. The language is a preference, not a document property: switching it
changes no revision.

The interface now sets type, and it does it without a typeface: see **Titles** below. What it
still does not own is a *wordmark* — a logotype is a drawn mark and a licensing decision, and
the brand directory still holds the owner's raster rather than a drawn word.

## The palette is the agent's catalogue in a person's hands

The palette's rows are the **MCP tool catalogue** — the same forty-five tools an agent has —
and the chosen one runs against the session the person is already holding:

```rust
shell.run_tool("maj_place_clip", &arguments)?;
```

The argument field is decoded as JSON when it is JSON and as text when it is not, so
`[0, 60]`, `1/2`, `true` and `shot-01` all arrive as what they look like. Three things are
true of the result, and each is a test:

* a reading tool appends nothing — the revision does not move;
* an editing tool appends to the log the person is working in, authored `ui:local`;
* a refusal is shown, not swallowed, and nothing changes.

The interface does not reimplement one line of it. There is no second tool dispatcher, and
`Action::KINDS` is asserted to be exactly the set of commands the two surfaces share, in
both directions: a command the interface can name must be a tool the agent has, and the two
sets are the same size.

## The same edit, two hands

The cross-interface test builds one project with intentions and one with tool calls, and
compares:

* the **state hash** is equal — the two documents are the same document;
* the **command logs** are the same commands in the same order, read one side through
  `kind_of` and the other through the log's own serialisation, so the comparison is between
  the interface and the document rather than between the interface and itself;
* the **document hash** is deliberately *not* equal, because it covers the log and the log
  records who made each edit. Two hands that make the same edit produce the same project and
  a different attribution, which is exactly what a shared door has to be able to show.

## The viewer and the exporter

`preview_request` and `export_still` both render through `maj-render` on the certified path,
so the interface cannot show a frame the exporter would not write. A preview is a policy on
top of it: full, half or a quarter of the sequence's own resolution, and optionally a 2×2
grid, which is the same tiling the renderer's own whole-versus-tiled verification uses.
`PreviewKey` is the identity of a frame — the sequence, the frame, the requested size, the
tiling, the transfer function and the project revision — so a viewer re-renders exactly when
the pixels would change and not otherwise, and an edit re-renders the frame already on
screen. The still is written by `maj-export`, with its manifest, the same writer the command
line uses.

## Preferences

One JSON file, `ui.json`, in the configuration directory (`MAJ_CONFIG_DIR` overrides it;
otherwise the platform's own convention, with a documented fallback). It holds the language,
the zoom, the draft resolution, the tiled flag, the transfer function and which panels are
open — and nothing that belongs to a document.

It is repaired rather than trusted: a zoom that is not a finite number falls back to the
default and a finite one outside `0.05..400` pixels per frame is clamped into it; a nonsense
enum name or a truncated file gives the defaults for the fields it cannot honour while
keeping the ones it can, and a missing file is simply the defaults. A partial file keeps the
fields it names. Writing creates the parent directory; nothing else on disk is touched.

## The window

```sh
cargo run -p maj-win32 --bin majestat                     # the example project
cargo run -p maj-win32 --bin majestat -- --project x.maj  # a document
cargo run -p maj-win32 --bin majestat -- --log session.log
```

The frame is a **menu bar** across the top that is also the strip that moves a frameless
window, a **toolbar** beneath it, the **body**, a **status line**, and a strip of **seven
pages** along the bottom. The body is three columns and the timeline: the **media pool** on the
left with the **command log** under it, both behind tabs; the **monitor** in the middle above
its **transport bar** and audio **meters**; the **inspector** of the selection on the right,
with the **mixer** beside it, also behind tabs; and the **timeline** across the bottom with its
own tool strip, the ruler, the track gutter, the lanes, the clips and the playhead. Dividers
between the columns and above the timeline are dragged with the pointer and remembered in the
preference file.

The seven pages are Media, Cut, Edit, Fusion, Color, Fairlight and Deliver. **Three of them are
built** — Media, Cut and Deliver — and the other four are reachable, drawn, and say which phase
owns them rather than pretending to be empty. A page that is not built refuses the click and
names its phase; so does a menu row whose subsystem does not exist yet, and so does a track
control whose model does not.

Every label and every name in the frame comes from the shell, in the shell's current language;
the window's only opinions are pixel positions and the palette. The keyboard is exactly the
table above — there is no second keymap, and a test walks the shell's table and requires that
every one of its sixty-seven strokes has a key on this window that produces it. The status line
shows the shell's own line, the transport's measured rate, and in red the last refusal in
whatever words the engine used.

The palette is reachable with `ctrl+k` and is the agent's own catalogue: typing `place_clip`
finds `maj_place_clip`, `enter` runs it against the session the person is holding, and what it
did appears in the log panel. Its argument fields are the tool's own fields, decoded by the
same code path an MCP client uses.

### The pointer

The mouse is not a second editing path. A click is a *selection*, a drag is an intention the
keyboard already has, and every one of them ends as a command authored `ui:local` — the same
door, with a different hand on it.

| Gesture | What it does |
|---|---|
| Click a clip, a media tile, a page, a tab, a menu, a toolbar button, a transport button | the intent that button stands for, or the selection |
| Drag a clip's body | move it so it follows the pointer frame for frame at the current zoom, snapped to the playhead and to other clips' edges while snap is on |
| Drag a clip's head or tail | trim that edge to the frame under the pointer |
| Drag in the ruler, or on the playhead | scrub: the playhead follows the pointer, and the preview is re-rendered no more often than the frame budget allows |
| Drag a divider | move that column or the timeline, within its range |
| Right-click a clip, a track, or the lane | open the menu for what is under the pointer: **Clip** or **Timeline** |
| Wheel over the lanes | scroll; with `ctrl`, zoom about the frame under the pointer, so the place you were looking at stays where it is |
| Wheel over the log or the inspector | scroll that list by rows |
| Double-click a media tile | insert it at the playhead |
| Click a panel tab | switch its column's panel; the tabs wrap onto a second row rather than falling off the edge of a narrow column |
| Click *Add media…* | the operating system's own file dialog; what it names is registered in the pool as a still |
| Click the studio's words field, then type | the words a title would say; `backspace` takes a letter back, `escape` or `enter` gives the keyboard up. Nothing typed here is in the document until the title is added |
| Click a face, a size step or an alignment | that choice in the studio, which is view state |
| Click *Add a title at the playhead* | two commands: the pool gains the title as media and the timeline gains a clip of it |
| Click a row in the effects or transitions browser | the effect on the selected clip, or the transition at the playhead |
| Move the pointer, leave the window | hover a zone, the cursor it declares, and the tooltip whose text the shell owns |

Three details are worth naming because they are what makes a pointer feel like an editor
rather than like a remote control:

* **A drag is a gesture, not a stream of commands.** A move drags the clip by the difference
  between the frame under the pointer and the frame it started on, so a drag of a second to the
  right is one command that moved the clip a second — not sixty commands of one frame each.
* **The hit-test table and the paint come from one geometry.** Every rectangle is produced once
  per frame by the layout, the painter draws those rectangles and the same rectangles become the
  regions a press is resolved against, so a control cannot move without its click target moving
  with it. `--regions` prints the map, and the self-check asserts that every zone is inside the
  window.
* **A gesture that would mean nothing says so.** Pressing a control whose subsystem does not
  exist sets the status line's refusal rather than silently doing nothing — the same rule the
  keyboard follows.

### Titles

A title is real type, set by the renderer, dropped on a track like any other shot. It is the
one place in this interface where a person *composes* rather than picks, so it is a form rather
than a list: the **Titles** tab of the left column holds the words, four faces, a cap height
and an alignment, and the button that sets them.

What it produces is deliberately unremarkable: a media asset whose URI is the whole title.

```
title:serif:23:ffffff:centre:MAJESTAT
```

Face, cap height, colour, alignment, words — nothing about a title lives anywhere else, so a
document is still a function of its command log, the pool reads like a shot list, and an agent
can add a title with the two commands the interface uses. Those two commands are the point of
the intention: `create_media_asset` then `insert_clip`, in that order, because everything is a
command and a title that is registered and not placed is a title a person has to go and find.

The alphabet is **in the renderer**, not on the machine:

* Four faces — Sans, Serif, Mono and Condensed — drawn from one stroke alphabet in
  `maj-render`. A face is a treatment: stroke weight, width, tracking and serifs. Lowercase is
  set as a small capital, the accented letters are their base letter with a mark, `¿` and `¡`
  are their upright forms turned half around, and a character the alphabet does not have is
  drawn as a missing-glyph box rather than silently dropped.
* Strokes are sampled analytically at half-pixel coverage, so the type is antialiased, and every
  sample outside a stroke carries an alpha of zero — which is why a title composes *over* a shot
  rather than covering it. The whole of it is a closed-form function of the words, the face and
  the frame, so the same title is the same pixels on two machines.
* No font file is read, and none is installed. That is a limitation and it is the honest shape
  of this phase: reading a system typeface is a different project, with a licensing question
  attached (`docs/LICENSING.md`).

The two browsers beside the inspector are the same idea for the engine's existing effects and
transitions: **Effects** lists the ten kinds the engine can run and adds one to the selection,
**Transitions** lists the eight and adds one at the playhead, and each row shows the kind's
stable name because that is the word the log stores. A kind the engine refuses is refused in
the engine's own words, on the status line, not approximated here.

### The clock

`space` starts a clock, `j`/`k`/`l` shuttle it through 0.25×, 0.5×, 1×, 2× and 4×, forwards and
backwards, and `alt+b` loops at the end. It advances the playhead at the sequence's own exact
rate, repaints the timeline every tick and the preview no more than thirty times a second, and
prints the rate it actually sustains in the status line.

It is deliberately called a clock and not playhead *playback*: there is no decoder and no audio
until M2, M3 and M9, so what moves is the playhead and the picture the certified path can draw
for that frame. The distinction is in the words the interface uses, not only in this document.
The clock does exactly one thing to the document — nothing. The playhead is view state, so a
minute of playing appends no command at all.

### Why it is written against Win32 rather than a toolkit

The toolkit does not build here. eframe was probed before the first line of the shell was
written: the probe reaches `parking_lot_core`, whose `windows-sys` dependency makes rustc
build a Windows import library, which needs `dlltool.exe` — absent, along with every assembler
and every C compiler on the machine.

```
error calling dlltool 'dlltool.exe': program not found
error: could not compile `parking_lot_core` (lib) due to 1 previous error
PROBE_BUILD_EXIT=101
```

A second probe answered the follow-up question — can a window be created here without
`dlltool`? — by declaring the imports by hand against the import libraries rustup ships:
`WIN32_PROBE_OK visible=1 messages=18`. That is what `maj-win32` does: seventy-six functions
from `user32`, `gdi32`, `kernel32`, `msimg32` and `comdlg32`, each wrapped in a safe signature,
in the only crate in the workspace whose lint level for `unsafe` is not `forbid`. See ADR 0013.

### Checking it without a person

| How | What it proves |
|---|---|
| `cargo test -p maj-win32` | Every stroke in the shell's table can be typed here, and no key reaches nothing |
| `majestat --self-check` | A real window, real posted keys and real posted pointer messages through the real queue: five arrows move the playhead five frames; a second of idling repaints nothing; a click selects a clip, a drag moves it eight frames, a drag on its tail trims it, a drag in the ruler scrubs without touching the revision, `ctrl`+wheel zooms; the deliver page opens; the strip at the top answers `HTCAPTION` and the toolbar `HTCLIENT`; `space` starts the clock and one tick advances the playhead at the sequence's rate, `space` stops it where it stood; `ctrl+d` appends and `ctrl+z` takes it back; `ctrl+k` opens the palette and typed text matches `maj_place_clip`; `ctrl+l` switches language without moving the revision; the Titles tab, its words field, a face and a size step are clicked and typed like a person's, and the button sets a real title — two commands, one asset and one clip; a click in each browser adds an effect to the selection and a transition to the track; and the paint produces a picture with something in it — 1106 distinct colours in 106 zones, all inside the window |
| `majestat --snapshot out.ppm` | The whole interface painted offscreen, as a file, so the layout can be looked at and diffed |
| `majestat --bench 300` | The paint cost per frame, whole and incremental, so "input to pixel" has a number rather than an adjective |
| `majestat --regions` | The colours actually on screen and where they are, which is how a layout regression is noticed without a person |

### What the window does not do yet

The gate for this phase — a three-minute edit performed end to end by keyboard, pointer and
agent alone — is now something a person *can* perform rather than something only tests touch. It is
still not recorded as met: nobody has sat down and done it, and M6's editorial operations are
not all here.

* **No rubber band.** Clicking selects one clip; a drag *moves* the clip rather than sweeping a
  selection across the lanes. Multi-selection is M6's, with the editorial operations that would
  need it.
* **No decoded picture.** The viewer renders what the certified path can compose, which is the
  project's own media, not a file's frames; there is no decoder until M2.
* **No audio.** The meters are drawn and say so: there is no audio model until M9.
* **No system typefaces.** A title is set in one of four faces built into the renderer. That is
  what makes it identical on two machines and free of a font licence, and it is also why a title
  cannot be set in the typeface a production already uses: reading the machine's fonts is a
  different piece of work with its own licensing question (`docs/LICENSING.md`).
* **The icon and the seal come from the owner's artwork, decoded once at build time.**
  `assets/brand/logo.png` is the only source: `build.rs` inflates it, crops the emblem out of
  the plate's margins and resamples it to the 16 and 32 px icons Windows asks for and the 48 px
  seal the page strip draws, so the shipping binary carries pixels and no PNG decoder. The brand
  notes' question about the white field is still the owner's to answer, and until it is, the
  file is used exactly as supplied.
* **Four of the seven pages are signs rather than rooms.** Edit, Fusion, Color and Fairlight
  are drawn, reachable, and name the phase that owns them — M6, M4, M5 and M9. Opening one is
  not an error and not a lie.

## What is deliberately not here

| Area | Why |
|---|---|
| Rubber-band selection, and the multi-clip edits behind it | The gestures are M7's; the operations that would need them are M6's |
| Decoded playback — real frames, real audio, real drops | There is no decoder (M2) and no audio model (M9). What exists is a clock that moves the playhead and redraws what the certified path can draw, and it is named that way everywhere |
| Video encoding, delivery presets | `maj-export` writes stills; encoders are M8 |
| Proxy and cache controls | `maj-cache` is M10 |
| Collaboration — presence, cursors, comments | M12, and it lands on the command log this interface already writes to |
| A second editing path | Forever. Every intention goes through `Action`, and every `Action` ends as a command |

## Adding an intention

1. Add the command to `maj-core` if the model does not have it. The interface never invents
   an operation the engine cannot express — and a test fails if it does.
2. Add the variant to `Action` and its arm in `Action::commands`: which commands, in what
   order, and which refusals are the interface's own.
3. If it is a command kind the interface can now name, add it to `KINDS`.
4. If a person should reach it from the keyboard, add the stroke to `keys::TABLE`.
5. If it has a label, add the key to `i18n::Key` and to both tables.
6. Extend `tests/model.rs` with the intention and the command it becomes, and
   `tests/interfaces.rs` if it reaches the agent's catalogue.
7. Run the gates: `cargo fmt --all -- --check`,
   `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.

## Verified evidence

Run on Windows, Rust 1.99, GNU toolchain, no C compiler:

```
cargo fmt --all -- --check                                  exit 0
cargo clippy --workspace --all-targets -- -D warnings        exit 0   (whole workspace, all targets)
cargo test --workspace                                      exit 0   539 tests, 53 test targets
cargo doc --workspace --no-deps                             exit 0   no warnings

cargo run -p maj-win32 --bin majestat -- --self-check          exit 0
cargo run -p maj-win32 --bin majestat -- --snapshot out.ppm    exit 0, 1440x900, 85 zones
cargo run -p maj-win32 --bin majestat -- --bench 300           exit 0, 39 zones
                                                               seven runs, 300 frames at 1440x900:
                                                               whole 4.5-5.6 ms, incremental 1.3-1.5 ms
```

| Suite | Count | What it holds |
|---|---|---|
| `maj-win32` unit tests | 11 | Every stroke in the shell's table can be typed on the window; no key reaches nothing; the modifiers land on the right bindings; a key with no stroke is silent; every zone the paint registers is inside the window; the same geometry answers a hit test and a hover; every glyph and every toolbar button carries a picture and a name; the layout puts every panel inside the client area; and the logo's emblem crop and its generated seal are what they claim to be |
| `maj-render` unit tests | 88 | The sources: a chart that is a function of its seed and frame, the PPM and PNG readers and their refusals, the title — its strokes, its four faces, its accents, its alignment, and the alpha that lets it compose over a shot — and media that cannot be read: one policy over every way it can happen, a placeholder that says which of `offline`, `damaged` and `undecodable` applies and keeps the engine's own sentence, a delivery mode that refuses instead, and the placeholder's hazard bands agreeing between a whole and a tiled render |
| `maj-ui` unit tests | 47 | The shell: the example document, the playhead's bounds, zoom, drafts, the palette's arguments, the status line in both languages, the binding table's collisions and its deliberate absences, the views and pages, the preference file's repairs, an export |
| `maj-ui` `tests/model.rs` | 21 | Every intention becomes the command it names; one of every command kind is spelled as the document spells it; the state is still a function of the log after the interface drove it; one intention is one undo step however many commands it is, and one press takes the whole of it back; a split covers what the clip did; a copy carries the look; a trim beyond the media is the engine's refusal; the draft changes the requested size; the viewer and the exporter agree on the pixels; adding a title is an asset and a clip and nothing more, and importing a still places nothing |
| `maj-ui` `tests/interfaces.rs` | 4 | Command-for-command parity with the agent's tools; the same edit by two hands is the same state hash and a different document hash; the author of everything the interface applies; the palette driving the agent's tools against the person's session |

See [ADR 0012](adr/0012-the-interface-emits-commands.md) for why the interface is shaped this
way, [`docs/MCP.md`](MCP.md) for the other door into the same log, and
[`docs/ARCHITECTURE.md`](ARCHITECTURE.md) for the layering rule that puts both of them in
layer 6 and 5 respectively.
