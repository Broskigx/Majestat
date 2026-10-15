# State

Updated: 2026-10-15

## P0 of the ultra plan: the three things that were blocking a decision

The roadmap from M2 on was waiting on answers rather than on work, and three of them are
now settled. Each is recorded here with the run that proves it.

**1. The product licence is `Apache-2.0 OR MIT`** (ADR 0014). Declared once in
`[workspace.package]` and inherited by all twenty-two crates; `LICENSE-APACHE` and
`LICENSE-MIT` are at the root. The consequence that matters for M2 and M8: a permissive
binary cannot absorb copyleft, so **no GPL component may be linked in** — `x264` and `x265`
are out, FFmpeg is admissible only as an LGPL build behind an owned abstraction and
dynamically linked, and M8's H.264/HEVC encoder has to be the platform's or a permissive
library. ADR 0005 carries an amendment saying so, and `docs/LICENSING.md` turns the
dependency audit's first question into a refusal. Still open and still the owner's: the
copyright holder is "Majestat", which is a project name and not a legal entity.

**2. One edit is one undo step, and a refused edit changes nothing.** `maj-core`'s
[`History`] gained `apply_group`, and `maj-ui`'s `dispatch` now uses it, so the commands of
one intention form a single undo step and roll back together if any of them is refused.
Adding a title used to take two presses of undo and could leave an asset in the pool with no
clip to use it. The log is untouched: each command is still recorded separately, under its
own sequence number, so grouping never reaches the document — which is a test
(`grouping_is_session_state_and_never_reaches_the_document`: the same commands grouped and
ungrouped give the same state hash *and* the same document hash).

**3. Media that cannot be read is one condition with one policy.** The engine used to refuse
a frame when a `.png` was missing and quietly substitute a test chart when a `.mov` was, so
the same fact behaved two ways depending on the file extension — and the substitute was the
same chart a project gets from `generated:`, so a manifest could not tell a stand-in from a
picture somebody wanted. Now `MissingMedia` is a declared policy over all of it:
`Placeholder` (the default, what an editor wants) draws a labelled `Placeholder` source that
reports `offline`, `damaged` or `undecodable` and keeps the engine's own sentence; `Refuse`
(what a delivery wants) fails the frame. The manifest's label now comes from
`PictureResolver::kind` — the implementation that would do the opening — instead of from a
second function beside the trait that guessed from the URI and could drift. `maj render` and
`maj verify` take `--strict-media` for the delivery answer, because a policy the command
line cannot select is not a policy.

**4. A bug the placeholder found: the monospaced face was unreadable.** The placeholder sets
its words in `TitleFace::Mono`, and the first render of one came back with `NO DECODER`
printed on top of itself. It was not the placeholder: `Mono`'s fixed advance was `0.80`,
narrower than `D`, `O` and `R` actually draw, so *every* monospaced title had been
overlapping since the face was written. The alphabet was right and the layout was right, and
nothing had ever compared a glyph's ink to the room it was given — so a face could be
unreadable and still pass every test there was. The advance is now `1.12` and
`the_monospaced_face_gives_every_glyph_room_for_the_widest_one` holds it to the alphabet.
It was found by rendering a title and **looking at it**, which is the second time that has
been the thing that caught a defect in this crate.

Measured on this machine, Windows, GNU toolchain:

```
cargo fmt --all -- --check                                  exit 0
cargo clippy --workspace --all-targets -- -D warnings        exit 0   no warnings
cargo test --workspace                                      exit 0   539 tests, 53 test targets
cargo doc --workspace --no-deps                             exit 0   no warnings
cargo test --release -p maj-core --test properties --test roundtrip
                                                            exit 0   7 + 8 tests, 10,000-command fuzz
cargo run -p maj-win32 --bin majestat -- --self-check          exit 0   22 checks, all as written
```

Eleven tests more than the 528 recorded below: five on grouping in `maj-core`, one in
`maj-ui`'s model suite proving one intention is one undo step, three in `maj-render` on the
missing-media policy, its delivery mode and the placeholder agreeing between a whole and a
tiled render, one on the monospaced face's advance, and one in `maj-cli` on `--strict-media`.

The two placeholders were looked at, not only asserted: a project naming a `.mov` and a
missing `.png` was built through `maj edit` and the agent's own `maj_place_clip`, rendered at
640×360, and the two frames read `NO DECODER` and `MEDIA OFFLINE` over the hazard field,
with `"source": "undecodable"` and `"source": "offline"` in their manifests.

**What P0 did not do.** The C toolchain is not installed — that needs administrator rights
and is the owner's to run — so M2 is still blocked on the measurement below, and no decoder
work has started. The plan is `C:\Users\agust\.claude\plans\que-le-falta-para-whimsical-lemur.md`.

## Position

**M0 (foundation) and M1 (determinism) are complete**, and the crates above them are
implemented as far as this host allows: `maj-color` and `maj-render` draw frames on the
certified path, `maj-export` writes stills and manifests, `maj-cli` renders and verifies
headlessly, `maj-mcp` serves the editor over the Model Context Protocol, and `maj-ui` is the
shell — every intention a person can express, translating into the same commands an agent's
tools append. The remaining crates are documented scaffolds that declare their
responsibility, their layer and their permitted lower-layer dependencies, so the layering is
enforced by the compiler from the first commit.

What blocks the rest is one measurement rather than a set of opinions: there is no C
toolchain on this host, so nothing that links a Windows import library through `windows-sys`
can be built — no media decoder (M2), and no toolkit-based window. The window itself did not
need one: `maj-win32` declares the Win32 imports by hand, and the application runs on this
machine today (ADR 0013). See **Blockers** below.

## Verified evidence

Run on Windows, Rust 1.99, GNU toolchain, RTX 4060 Ti. These are the actual commands and
their actual outcomes, recorded at the **M1 gate**; each later section records the gate that
followed it, so the counts below are the ones that were true then, not a current total.

```
cargo fmt --all -- --check                                  exit 0
cargo clippy --workspace --all-targets -- -D warnings        exit 0   (whole workspace, all targets)
cargo test --workspace                                      exit 0   (all suites, all crates)
cargo test --release -p maj-core --test properties --test roundtrip
                                                            exit 0   15 tests, 10,000-command fuzz
```

Test counts:

| Suite | Count |
|---|---|
| `maj-time` unit tests | 57 |
| `maj-time` property tests (seeded) | 17 |
| `maj-time` doc tests | 9 |
| `maj-core` unit tests | 72 |
| `maj-core` round-trip gates | 7 |
| `maj-core` property tests (seeded) | 8 |
| `maj-core` doc tests | 1 |
| One test per scaffolded **library** crate (role and layer pinned) | 17 |
| **Total** | **188** |

There are eighteen scaffolded crates, but `maj-cli` is a binary: it has no test module,
and its scaffold check is that it runs (`cargo run -p maj-cli -- --version` prints
`maj 0.0.1`). Hence seventeen, not eighteen. The arithmetic in the table above sums to the
188 reported by `cargo test --workspace`.

The fuzz budget is 2,000 commands in debug builds and 10,000 in release builds. Both are
real runs; the release run completed the gate in about 2.5 seconds of execution.

## The MCP surface

Added after M8's CLI existed, and not one of the seventeen phases: it is the interface work
the command model was built to make possible, so it is recorded here rather than invented
as a phase and renumbered into the roadmap.

`crates/maj-mcp` (layer 5) serves the editor over the Model Context Protocol on stdio:
**45 tools, 5 resources, 3 resource templates, 3 prompts**. The design rule is that a tool
cannot change the project except by decoding into a `Command` and appending it to the log,
so an agent's edit is exactly as undoable, replayable and attributable as a person's. There
is no second path into the model. See `docs/MCP.md` for the surface, ADR 0011 for why it is
shaped this way, and `docs/COVERAGE.md` for what the surface can and cannot reach today.

Verified on Windows, Rust 1.99, GNU toolchain, from the workspace gates:

```
cargo fmt --all -- --check                                  exit 0
cargo clippy --workspace --all-targets -- -D warnings        exit 0
cargo test --workspace                                      exit 0   431 tests, 48 suites
cargo test --release -p maj-core --test properties --test roundtrip
                                                            exit 0   7 + 8 tests, 10,000-command fuzz
```

| Suite | Count | What it holds |
|---|---|---|
| `maj-mcp` unit tests (catalogue, arguments, sessions, dispatcher) | 100 | Every tool's schema, every refusal, session isolation, the effect of each tool |
| `maj-mcp` `tests/protocol.rs` | 6 | The wire: handshake and revision negotiation, batches, unknown tools and methods, resources, prompts, read-only, and a full editing session by tool calls |
| `maj-cli` `tests/mcp.rs` | 2 | The cross-interface gate: `maj render` and `maj mcp` produce the same digest for one document, a project an agent builds over the wire is verified by the command line, and a read-only server changes nothing |
| `maj-cli` unit tests + `tests/cli.rs` | 5 + 8 | The commands the MCP server shares the engine with |

Two notes about the evidence:

* One `cargo test --workspace` run early in the session reported a failure in
  `maj-cli --test mcp` (10.15s for the binary against the usual ~2s); twelve consecutive
  subsequent runs — including three immediately after a relink — passed. It is recorded
  here as an unreproduced flake rather than quietly dropped. The likely environmental cause
  is the checkout living under OneDrive, whose sync can hold a freshly written file in
  `target/` open long enough for a child process or a read to fail; the engine itself has
  no threads and no clocks on the render path, so nothing in it can drift.
* The MCP server's tools are only as wide as the engine. Media probing, audio, encoding,
  collaboration and plugins are not exposed because those subsystems do not exist yet;
  `docs/COVERAGE.md` audits the 130-item Fase II catalogue against that fact and finds 33
  items reachable in some form, 61 that need the engine to grow, and 36 outside a certified
  picture engine. It is the honest answer to "can an agent do professional editing yet?":
  the door is built and guarded, and what is behind it is the engine's surface, no wider.

## The interface

`crates/maj-ui` (layer 6) is the shell as a **model**, and it is gated: 77 intentions that
translate into commands and nothing else, a keyboard table of 67 strokes, 199 strings in
English and Spanish, seven pages, a preference file of view state, and a palette that runs
the agent's own MCP tools against the session the person is holding. Every command it
applies is authored `ui:local`, and the log, the undo, the replay and the hashes are the
model's own — there is no second editing path.

The parity claim is tested in both directions, and so is its limit: a project built by
keyboard and a project built by tool calls have the same **state hash** and the same
commands in the same order, while their **document hashes** differ, because the document
hash covers the log and the log records who made each edit. Two hands, one document, two
histories — which is exactly what an interface sharing one door with an agent has to be able
to show.

Verified on Windows, Rust 1.99, GNU toolchain, from the workspace gates:

```
cargo fmt --all -- --check                                  exit 0
cargo clippy --workspace --all-targets -- -D warnings        exit 0
cargo test --workspace                                      exit 0   528 tests, 53 test targets
cargo doc --workspace --no-deps                             exit 0   no warnings
```

| Suite | Count | What it holds |
|---|---|---|
| `maj-ui` unit tests | 47 | The example document, the playhead's bounds, zoom, the drafts, the pages and the split ranges, the palette's argument decoding, the status line in both languages, binding collisions and the deliberate absences, the preference file's repairs, an export |
| `maj-ui` `tests/model.rs` | 20 | Every intention becomes the command it names; one of every command kind is spelled as the document spells it; the state is still a function of the log after the interface drove it; a split covers what the clip did; a copy carries the look; a trim beyond the media comes back as the engine's own refusal; the draft changes the requested size; the viewer and the exporter agree on the pixels; adding a title is one asset and one clip in that order and a title with no words is refused; importing a still places nothing; and the studio's words, face, size and alignment move no revision until the title is added |
| `maj-ui` `tests/interfaces.rs` | 4 | Command-for-command parity with the agent's tools; the same edit by two hands is one state hash and two document hashes; the author of everything the interface applies; the palette driving the agent's tools against the person's session |

**M7's gate is not met, and the reason is no longer a mechanism.** The gate is a three-minute
edit performed end to end by keyboard and agent alone. Every edit, key, refusal and pixel
comparison behind it is checked by tests, and the pointer's gestures are now driven
mechanically against a real window — see **M7's second half** below — but nobody has sat down
and performed the edit, and M6's editorial operations are not all here yet.

The window took a measurement to get past, and it is kept here because it is what shaped the
presenter: the toolkit the cross-platform story wanted reaches `parking_lot_core`, which
imports Windows system libraries through `windows-sys`, which needs `dlltool.exe`, which this
host does not have.

```
error calling dlltool 'dlltool.exe': program not found
error: could not compile `parking_lot_core` (lib) due to 1 previous error
PROBE_BUILD_EXIT=101
```

## The window

The shell is presented by `crates/maj-win32` (layer 6), running as the `majestat` binary, and it
is a real window on this machine:

```sh
cargo run -p maj-win32 --bin majestat
```

It draws a menu bar, a toolbar, the three columns and the timeline, a status line and seven
pages — media pool over the command log, monitor over the transport bar and meters, inspector
beside the mixer, the timeline with ruler, gutter, clips and playhead — and it edits with
exactly the keys and gestures `docs/UI.md` documents. It decides nothing about the document:
Windows messages become the strokes the shell's own table already defines, the pointer becomes
the same intentions, every label and name comes from the shell in the shell's current
language, and every change goes through the command log.

Why it is not `egui`: the toolkit does not build here. `eframe` reaches `parking_lot_core`,
which needs a Windows import library, which needs `dlltool.exe`, which this host does not have
along with any other assembler or C compiler. The follow-up measurement — can a window be
created by declaring the imports by hand? — came back yes, from a probe that created a window,
showed it and pumped eighteen messages (`WIN32_PROBE_OK visible=1 messages=18`). That is the
whole of ADR 0013, including its cost: this crate is Windows-only, and it is the one crate in
the workspace whose lint level for `unsafe` is `deny` rather than `forbid`, with the single
module that holds it carrying an `allow` and its reasons.

Verified on Windows, Rust 1.99, GNU toolchain, from the workspace gates and the application's
own checks:

```
cargo clippy -p maj-win32 --all-targets -- -D warnings              exit 0
cargo test -p maj-win32                                             exit 0   11 tests
cargo run -p maj-win32 --bin majestat -- --self-check                 exit 0   22 checks, all as written
cargo run -p maj-win32 --bin majestat -- --snapshot target/preview/ui.ppm
                                                                    exit 0   1440x900 PPM, 85 zones
```

The eleven tests prove that every one of the shell's sixty-seven strokes has a key on this window
that produces it, that no key reaches nothing, that every zone the paint registers lies inside
the client area, that every glyph and toolbar button carries a picture and a name, and that the
logo's emblem crop and its generated seal are what they claim to be. The self-check creates a real window, posts real `WM_KEYDOWN` and
`WM_LBUTTONDOWN` messages, drains the real queue and asserts what happened: five arrows moved
the playhead five frames; a second of idling repainted nothing; a click selected the clip
under the pointer, a drag moved it eight frames, a drag on its tail trimmed it from 240 frames
to 73, a drag in the ruler scrubbed to frame 31 without appending a command, and `ctrl`+wheel
moved the zoom 4.00 to 4.60; the deliver page opened; the strip along the top answered
`HTCAPTION` where the toolbar answered `HTCLIENT`; `space` started the clock and one tick
advanced the playhead at the sequence's rate, then `space` stopped it where it stood and a
tick with it stopped moved nothing; `ctrl+d` appended a command and `ctrl+z` took it back;
`ctrl+k` opened the palette, typed text reached its field and matched `maj_place_clip`;
`ctrl+l` switched language without moving the revision; the **Titles** tab, its words field, a
face and a size step were clicked and typed as a person does, and the button set a real title —
the pool and the timeline each gained one, from two commands; a click in the effects browser
took the selected clip's chain from nothing to one entry and a click in the transitions browser
took the first track from nothing to one; and the paint produced a picture with something in it
— 1106 distinct colours in 106 zones. The last two lines of its own output are the ones this
phase added:

```
titles: the studio set "MAJESTAT" in a 23 px serif face from 6 clicks and 8 keys, and the pool and the timeline each gained one
browsers: an effect click took the selection's chain 0 -> 1, and a transition click took the first track 0 -> 1
```

**M7's gate is still not met.** A three-minute edit performed end to end by keyboard and agent
alone is now something a person *can* do on this machine, with a keyboard or a pointer, and
the same operations run mechanically — but nobody has sat down and performed it, and M6's
editorial operations are not all here yet. It is recorded as in progress rather than
approximated in prose.

The surface, the guarantees and the evidence are in `docs/UI.md`; the decisions are ADR 0012
(the shell emits commands) and ADR 0013 (the window is written against Win32).

## M7's second half: the pointer, the pages and the clock

The window used to be keyboard and palette only, and said so. It now answers the mouse as
well, and the same three rules hold: a gesture is an intention, an intention becomes a
command, and nothing reaches the document any other way.

* **The pointer.** Every mouse message the window receives — move, press, release, double
  click, right button, wheel, leave, capture lost, hit test, cursor — is turned into a
  `Pointer` and a zone, and resolved against the same region table the painter drew from. A
  click selects; a drag on a clip's body moves it by the difference between the frame under
  the pointer and the frame it started on; a drag on its head or tail trims that edge; a drag
  in the ruler or on the playhead scrubs; a drag on a divider moves the column; the wheel
  scrolls the lanes, the log or the inspector, and `ctrl`+wheel zooms about the frame under the
  pointer. A right click opens **Clip** or **Timeline**. A press whose control stands for a
  subsystem that does not exist yet sets the status line's refusal rather than doing nothing.
* **The menus.** Seven of them — App, File, Edit, Sequence, View, Tools, Help — plus the two context
  menus. A row is a `Binding` or a `Special`, and a row whose subsystem is absent says which
  phase owns it, so the menu is a map of the interface rather than a promise.
* **The pages.** Seven: Media, Cut, Edit, Fusion, Color, Fairlight and Deliver. Three are
  built; the other four are drawn, reachable and name their phase (M6, M4, M5, M9).
* **The clock.** `space`, `J`/`K`/`L` and `alt+B` drive a rational clock that advances the
  playhead at the sequence's exact rate, shuttles through 0.25×–4× in both directions, loops at
  the end, repaints the timeline every tick and the preview no more than thirty times a second,
  and prints the rate it sustains. It is called a clock and not playback because there is no
  decoder and no audio: the playhead is view state, so a minute of playing appends nothing.

Measured on this machine, from the same gates as everything else:

```
cargo fmt --all -- --check                                  exit 0
cargo clippy --workspace --all-targets -- -D warnings        exit 0
cargo test --workspace                                      exit 0   497 tests, 53 test targets
cargo doc --workspace --no-deps                             exit 0   no warnings
cargo run -p maj-win32 --bin majestat -- --self-check          exit 0   22 checks, all as written
cargo run -p maj-win32 --bin majestat -- --snapshot target/preview/ui.ppm
                                                    exit 0   1440x900, 3,888,016 bytes, 81 zones
cargo run -p maj-win32 --bin majestat -- --bench 300
                                                    exit 0   seven runs, 300 frames at 1440x900:
                                                             whole mean 4.5-5.6 ms (max 11.4-14.4)
                                                             incremental mean 1.3-1.5 ms (max 2.1-3.3)
```

The bench numbers are the half of the acceptance table's "interface input-to-pixel under 16 ms"
that can be measured without a person: the paint of a whole 1440×900 frame and of a damaged
rectangle, on this machine, and they are given as a range over seven runs because a single run
of a machine that is also running an editor is not a number worth recording as one. The input
half — the time from a `WM_MOUSEMOVE` in the queue to the pixels on screen — is **not**
measured, and is not claimed.

What it does not do, and says so where a person is looking: there is no rubber band, so a drag
moves a clip rather than sweeping a selection; there is no decoder, so the viewer draws the
project's own media and not a file's frames; there are no meters that move, because there is
no audio model (M9); and the logo is decoded once at build time into the window's 16 and 32 px
icons and the page strip's 48 px seal.

## Titles, and the two browsers

Three things the interface could name but not *do* are now things a person can do with the
pointer: set a **title**, add an **effect** to a clip, and add a **transition** to a cut.

**A title is real type, set by the renderer.** Text has to be set on a machine with no fonts
installed, inside a headless render, byte-identically in two runs, so the alphabet is data in
`maj-render`: one stroke alphabet and four treatments of it — Sans, Serif (serifs laid square
across every stroke end), Mono (one advance for every glyph) and Condensed. Lowercase is set as
a small capital, the accented letters are their base letter with a mark, `¿` and `¡` are the
upright forms turned half around, and a character outside the alphabet is drawn as a
missing-glyph box. Strokes are sampled analytically with half-pixel coverage, and every sample
outside a stroke carries an alpha of zero, which is what lets a title compose *over* a shot.

**The whole of a title is its URI** — `title:serif:23:ffffff:centre:MAJESTAT` — so nothing about
it lives outside the command log, the pool reads like a shot list, and an agent adds one with
the same two commands the interface uses (`create_media_asset`, then `insert_clip`). The studio
itself is view state: words, face, cap height and alignment move no revision until the title is
added, which is a test.

**The two browsers are lists of what the engine already has**: ten effect kinds, added to the
selection, and eight transition kinds, added at the playhead, with each row showing the stable
name the log stores. A kind the engine refuses is refused in the engine's own words on the
status line rather than being approximated.

What this deliberately is not: a typeface from the machine, a text box, or a title that wraps
to a second line. Reading a system font is a licensing question as well as a decoder one
(`docs/LICENSING.md`), and a second line is a layout this alphabet does not set.

Verified from the application's own checks, on this machine:

```
cargo test -p maj-render                                        exit 0   83 tests
cargo test -p maj-ui --test model                               exit 0   20 tests
cargo run -p maj-win32 --bin majestat -- --self-check             exit 0
    titles: the studio set "MAJESTAT" in a 23 px serif face from 6 clicks and 8 keys,
            and the pool and the timeline each gained one
    browsers: an effect click took the selection's chain 0 -> 1, and a transition click took
            the first track 0 -> 1
cargo run -p maj-win32 --bin majestat -- --snapshot target/preview/ui.ppm
                                                            exit 0   1440x900, 85 zones
```

The letterforms were looked at rather than only asserted: a throwaway preview printed the
faces as ASCII art at a readable size and was deleted once the shapes had been read.

## The logo

The application's logo is now an image supplied by the owner rather than one of the drawn
marks: `assets/brand/logo.png`, 1024 by 559, PNG, eight bits per channel, RGB with no
alpha, 113,753 bytes, SHA-256
`5c5e09b42343f20c07c3f007063aae5d4d18640d3ff31a3fd23ddc6557048e0b`. It was copied in byte
for byte and verified against the file it arrived as with `sha256sum` and `cmp`, both
identical.

Two things about it are decisions rather than accidents, and both are recorded rather than
absorbed:

* **The white field.** The image is ink on white and the application's ground is ink, so as
  supplied the white reads as a plate. `bash assets/brand/check-page.sh` (exit 0) writes
  `target/preview/brand.html` showing it on three grounds — as supplied on paper, as
  supplied on ink, inverted on ink — and the choice is the owner's. Until it is made the
  application uses the file as supplied.
* **The decoding.** The file is DEFLATE-compressed, so it is decoded once at build time into
  the raw form the shell and the operating system need, keeping an image decoder out of the
  shipping binary and this one file the only source of the artwork.

`assets/brand/README.md` carries the measurements, the hash and the consequences for the
small sizes. It is wired in: `crates/maj-win32/build.rs` inflates the file once, crops the
emblem out of the plate's margins and writes the pixels the window class and the interface
ask for, so the shipping binary carries pixels and no PNG decoder. The self-check reports it —
`brand: logo 1024x559 decoded at build time, emblem 491 px cropped to a 48 px seal and the 16
and 32 px icons` — and the white-field question in `assets/brand/README.md` is still the
owner's.

## Blockers

**1. No C/C++ toolchain on the build host.** This blocks M2 and everything after it.

Measured facts: no Visual Studio or VS Build Tools, so the MSVC toolchain cannot link and
`link.exe` is absent; no `cc`, `clang` or `gcc`; no binutils `dlltool.exe`, so crates
that import Windows system libraries through `windows-sys` fail to build. The GNU
toolchain does work, because rustup ships `rust-lld`.

Consequences already absorbed: `proptest` does not build (it pulls `windows-sys`), so the
property tests use a seeded PRNG written in this workspace; `blake3` is used on its
`pure` feature to avoid compiling C; and the `.maj` container is JSON-first because a
bundled SQLite cannot be built. See ADR 0009.

Resolution, either of:
* **Install Visual Studio Build Tools** — restores MSVC linking and the `windows-sys`
  ecosystem. Requires administrator rights and a multi-gigabyte install.
* **Install MSYS2 binutils and a mingw-w64 toolchain** — provides `dlltool.exe` and a C
  compiler. Smaller, but keeps the GNU toolchain in place.

Either unblocks M2. This is the single highest-priority decision on the project.

**2. ~~Product licence undecided.~~ Settled: `Apache-2.0 OR MIT`** (ADR 0014). What it
leaves open is narrower and more concrete: the copyright holder's legal name, the pinned
FFmpeg configuration, and M8's choice of H.264/HEVC encoder now that the GPL ones are
excluded. See `docs/LICENSING.md`.

## Not yet started, and what each needs first

| Phase | Blocked on |
|---|---|
| M2 Media I/O | The toolchain decision above, plus the corpus |
| M3 Playback | M2 |
| M4 Render + GPU | M3 (`wgpu` is deliberately not a dependency yet) |
| M7 Interface — the three-minute edit itself | Nothing mechanical. The shell, the window, the pointer and the clock are built and gated; the gate needs a person to sit down and perform it, and M6's editorial operations |
| Everything later | The chain above |

## Next action

1. **Install the MSVC Build Tools** — the owner's, because it needs administrator rights —
   then verify with a throwaway crate that links a `windows-sys`-dependent library and
   compiles a trivial C file. This is the one remaining thing in P0 and it blocks M2 and
   everything after it.
2. Pin the FFmpeg configuration: an LGPL shared build, no `--enable-gpl`, no
   `--enable-nonfree`, exact version recorded, with the notice and corresponding-source
   obligations written down before any code links it.
3. Perform M7's gate: a three-minute edit, end to end, by keyboard and pointer, with the
   agent's tools used for at least one step of it — and record what it felt like, because
   that is the measurement that phase is waiting on.
4. Curate a starter corpus of twenty synthetic files covering the M2 axes, so the
   hundred-file gate has somewhere to grow from.
5. Begin M2: `maj-media`'s stream discovery and probe path first, behind the
   `PictureResolver` seam that already exists, with the hardware decode bridge prototyped
   and measured before it is promised.

## Deliberate technical debts, recorded

| Debt | Where it lives | When it is repaid |
|---|---|---|
| `.maj` is text, not the planned binary container | ADR 0008 | M10, after the toolchain decision |
| Local undo does not survive a session; the inverse stack is session state | `maj_core::History` | Never — it is the honest design, documented rather than faked |
| Log replay is quadratic in project size | ADR 0002 | Only if measured to matter; the escape hatch is snapshots plus a tail |
| `maj-timeline` is a separate crate from `maj-core` but the editorial operations are not yet extracted | `docs/ARCHITECTURE.md` | M6 |
| `wgpu` is not yet a dependency, so the render path is untested | ADR 0004 | M4 |
| `PictureResolver::kind` costs a stat and cannot tell a damaged file from a readable one without reading it, so a manifest names a present-but-damaged still for its container while the frame gets a `damaged` placeholder | `maj-render/src/source.rs` | M2, when the decoder can answer the question cheaply from its own probe |
| The grouping of commands into one undo step is session state, so it does not survive a save — a reopened project undoes command by command | `maj_core::History` | Only if it turns out to matter; recording the grouping in the log would make it part of the document, which is a bigger decision than it looks |

## Working agreements

* No phase is declared done without a measurement, and the measurement is recorded here.
* A failing check is fixed at its cause, never by weakening an assertion.
* Deterministic property tests: every case sequence is seeded, so a failure reported is a
  failure reproducible.
* Nothing is committed or pushed without being asked.
