# State

Updated: 2026-10-16


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

## P0.4: the C toolchain, measured — and it is already there

**The blocker is gone, and not by the route that was planned.** P0.4 was recorded as
needing administrator rights and an MSVC Build Tools install. It does not: this host now
carries a **MinGW-w64 toolchain** (WinLibs, via winget) and binutils **`dlltool.exe`**, and
those are the two things that were actually missing. MSVC was one way to obtain them, never
the requirement. **Nothing needs to be installed and nothing needs administrator rights.**

What the host has, measured:

```
link.exe        NOT FOUND          cl.exe       NOT FOUND       lib.exe   NOT FOUND
vswhere.exe     NOT FOUND          no Visual Studio directory, no Windows Kits\10\bin
dlltool.exe     C:\Users\agust\.cargo\bin\dlltool.exe
                GNU dlltool (Binutils for MinGW-W64 x86_64, built by Brecht Sanders, r2)
                2.47.20260726
gcc.exe         ...\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_...\mingw64\bin\gcc.exe
ld.exe, ar.exe  same mingw64\bin
rustup          default stable-x86_64-pc-windows-gnu; -msvc and -gnullvm also installed
rustc           1.99.0 (b940084d7 2026-09-28), host x86_64-pc-windows-gnu
```

So MSVC is still absent and still cannot link, and that no longer matters.

**The verification**, by two throwaway crates outside the repository, in the session
scratchpad. The first links a `windows-sys` dependant and compiles a trivial C file through
a `cc` build script — the exact gate P0.4 was written to require:

```
   Compiling windows_x86_64_gnu v0.52.6
   Compiling find-msvc-tools v0.1.14
   Compiling cc v1.8.0
   Compiling windows-targets v0.52.6
   Compiling windows-sys v0.59.0
   Compiling p04-probe v0.0.0 (...\scratchpad\p04-probe)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.40s
     Running `target\debug\p04-probe.exe`
windows-sys GetTickCount64 = 18669265
C file returned = 42
=== exit code: 0 ===
```

That one is necessary and not sufficient: `windows-sys` 0.59 links the prebuilt import
libraries the `windows_x86_64_gnu` crate ships, so it may not touch `dlltool` at all. The
second crate tests what actually failed before — `parking_lot_core`, and a
`#[link(kind = "raw-dylib")]` import, which is the mode that makes rustc generate an import
library itself and is where `dlltool` is required on the GNU target:

```
   Compiling parking_lot_core v0.9.12
   Compiling parking_lot v0.12.5
   Compiling p04-probe2 v0.0.0 (...\scratchpad\p04-probe2)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.16s
     Running `target\debug\p04-probe2.exe`
parking_lot mutex = 7
raw-dylib GetCurrentProcessId = 30748
=== exit code: 0 ===
```

**And a control, because a passing test that cannot fail proves nothing.** The same crate,
cleaned and rebuilt with `.cargo\bin` and the WinLibs `mingw64\bin` removed from `PATH`, so
`dlltool` and `gcc` are invisible:

```
   Compiling parking_lot_core v0.9.12
error: failed to add native library ...\target\debug\deps\rustcFdwqKC\kernel32.dll_imports.lib:
       El sistema no puede encontrar el archivo especificado. (os error 2)
error: could not compile `parking_lot_core` (lib) due to 1 previous error
=== exit code: 101 ===
```

The probe is therefore sensitive to the condition it claims to test: hide `dlltool` and the
old failure returns, restore it and the link succeeds. ADR 0009 carries an amendment saying
which of its measured facts have expired and which clauses of its decision stand anyway —
the seeded property tests and the three-dependency `maj-core` stand, because the toolchain
was only ever their second reason.

**What was built on top of it**, since the blocker is discharged:

* **The FFmpeg build is pinned**, in `docs/FFMPEG.md`: FFmpeg 9.0.2 "Lei" (`n9.0.2`), a
  shared LGPL build with `--disable-autodetect`, without `--enable-gpl`, `--enable-nonfree`
  or `--enable-version3`, with `libpostproc` disabled explicitly and `swscale`/`swresample`
  disabled for ADR 0006's reasons rather than licensing ones. It separates the
  licence-mandatory flags from the determinism-motivated and the merely surface-minimising,
  records the obligations against the clauses of LGPL-2.1 that create them, and turns
  compliance into a **runtime gate**: `avutil_license()` must say LGPL, the loaded library's
  own configuration string must contain none of the three refusals, and its hash must match
  a manifest. A build that cannot prove it is the LGPL build is refused.
* **`maj-media`'s surface is designed**, in `docs/MEDIA.md`, against the `PictureResolver`
  seam as it actually is. Four findings came out of reading the signature rather than
  assuming it: this crate **cannot** implement `PictureResolver`, because that trait is
  layer 4 and the layering is downward-only, so the adapter lives in `maj-render`;
  `kind`'s `&'static str` return forces the manifest's container vocabulary to be a closed
  enum rather than a string from a library; `resolve(&self)` plus a stateful decoder forces
  interior mutability and names the day `PictureResolver` gains `Send + Sync` as the day
  that becomes a `Mutex`; and the decoder converts **nothing**, because YUV-to-RGB and the
  transfer function are certified-path arithmetic and belong to `maj-color` — which is the
  same decision as disabling swscale, and the two have to stay in step.
* **The one thing M2 cannot start without is now a named decision, not a surprise**:
  binding a C library needs `unsafe`, which `[workspace.lints.rust]` forbids outside
  `maj-win32`. `docs/MEDIA.md` §8 sets out the three shapes, recommends a leaf FFI crate
  with `maj-media` staying `forbid`, keeps out-of-process decoding available as a
  `MediaBackend` implementation rather than a rewrite — and **does not decide it**, because
  relaxing a workspace-wide `forbid` is an ADR of ADR 0013's weight.

The plan is `C:\Users\agust\.claude\plans\que-le-falta-para-whimsical-lemur.md`.

Gates after this work, on this machine:

```
cargo fmt --all -- --check                                  exit 0
cargo clippy --workspace --all-targets -- -D warnings        exit 0   no warnings
cargo test --workspace                                      exit 0   540 tests, 53 test targets
cargo doc --workspace --no-deps                             exit 0   no warnings
```

This was documentation and design. The one code file given content
(`crates/maj-media/src/lib.rs`) gained module documentation and kept its two constants and
its one test. **No decoder code was written**, deliberately.

**The count is 540 rather than the 539 above, and this work added no tests.** Another
session was editing `maj-ui` and `maj-win32` in this same checkout while these gates ran, so
the extra test is most likely theirs. The number is recorded as what was measured at a
moment in a moving tree, not as a stable total — which is the honest form for it to take.

Two red gates appeared during that window and neither was a defect in committed code, so
both are recorded rather than quietly resolved:

* **`clippy` failed on `constant SEAL_BGRA is never used`.** The cause was a stale `OUT_DIR`
  copy of `maj-win32`'s generated `brand.rs` declaring a constant the current `build.rs` no
  longer emits; `cargo clean -p maj-win32` regenerated it and the gate went green. The
  latent hazard is worth hardening and is left to the crate's owner: `build.rs` declares
  only `rerun-if-changed=assets/brand/logo.png`, so its generated output can outlive the
  script that writes it, and `rerun-if-changed=build.rs` would make this class of failure
  impossible.
* **`doc` warned that `snapshot`'s public documentation linked to the crate-private
  `App::refresh_preview`.** Fixed at the cause — the prose names it as code instead of
  linking it — in one line of `crates/maj-win32/src/app.rs`, a file the other session was
  also editing. Flagged rather than hidden, in case it collides with their work.

And one failure that was seen once and **not** reproduced: `maj-cli`'s
`edit_applies_a_command_and_the_render_follows` failed on `an edit must change the picture`
during one workspace run, then passed 3/3 alone and 9/9 as part of its own binary across six
runs. The scratch directories are per-test with unique names, so it is not a fixture
collision; the likeliest explanation is the tree changing underneath the run. **The
assertion was not weakened and the test was not "fixed"** — an unreproduced failure with a
plausible external cause belongs in the record, and this is the first sighting.

## M2a: the media vocabulary and a probe, with no FFI at all

**`maj-media` carries code.** The scaffold is now a layer-2 crate with the vocabulary of
`docs/MEDIA.md` §3 and §4 in it, a probe path behind `MediaBackend`, a cache in front of that,
and an adapter in `maj-render`. **No FFmpeg, no `unsafe`, no FFI**, and no decoder: reading a
container's headers is reading a documented byte layout, so the half of M2 that needs no C
library was built first — which is what lets §8's `unsafe` decision be made in M2b against a
**measured** crash rate rather than a prediction.

**The corpus exists, and it is thirty-nine files, not a hundred.** `testdata/media-corpus/`
described a corpus and committed none of it. There is now a committed generator —
`maj_media::corpus`, run by `cargo run -p maj-media --example make-corpus` — that writes
thirty-nine files byte by byte into the git-ignored `media/` directory the README nominates,
with a manifest recording each file's generation command and the reading a probe of it must
produce. No encoder was needed and none is installed: every file's sample data is deliberate
filler, because probe reads headers.

The gate, measured on this machine:

```
cargo run -p maj-media --example make-corpus     39 files, 30 readable, 9 refused on purpose
cargo test -p maj-media --test corpus            7 tests: 39/39 probe exactly as declared
                                                 ~2,500 truncations, ~5,000 flipped bytes
                                                 zero panics
cargo fmt --all -- --check                       exit 0
cargo clippy --workspace --all-targets -- -D warnings   exit 0   no warnings
cargo test --workspace                           exit 0   663 tests, 55 test targets
cargo doc --workspace --no-deps                  exit 0   no warnings
```

**Zero crashes over the corpus, and the three gate axes read correctly**: rotation as a
quarter-turn count off a `tkhd` display matrix (90, 180 and 270, with the displayed size
turning with it), field order off a `fiel` box in all four spellings the format has, and a
variable frame rate as the exact per-frame timestamps off an `stts` run-length table, where
the index-to-timestamp map is a lookup and not a multiplication. Those are three files, three
files and one file of the thirty-nine; the others carry rates from 23.976 to 60, eight to
twelve bits, 4:2:0 through 4:4:4, full and limited range, a 16:15 sample aspect, 5.1 sound at
48 kHz and 24-bit mono at 44.1, four containers, a timecode track that is counted and not
read, nine million frames in one table entry, and nine pathologies.

**Thirty-nine files is not the hundred-file gate, and this phase does not claim it.** What a synthetic corpus cannot reach is stated in code, in `corpus::NOT_COVERED`,
printed by the generator and repeated in the corpus README: **coded pictures** (the sample
data is filler, so no codec's bitstream is exercised past the parameter set that states chroma
and depth), **real codecs**, **camera RAW**, **real HDR and log footage**, **MXF**,
**genuinely damaged media** (these pathologies are damage this workspace caused on purpose),
**image sequences**, and **surround mixing**. Those need real files with their provenance
recorded, which is the owner's time and not an agent's. The honest claim is: the prober
survives thirty-nine synthetic files and some seven thousand hostile mutations of them, and
the hundred-file gate is still open.

**The `kind` debt is repaid.** `maj-render::MediaResolver` answers `PictureResolver::kind`
from a cached probe, so a **present-but-damaged** file is named `damaged` in the manifest as
well as in the frame — the exact defect recorded in the debts table below. Two further things
fell out of it, neither planned:

* **A missing movie is `offline`, not `undecodable`.** `WorkspaceResolver` has no parser for a
  `.mov`, so it said `undecodable` whether the file was there or not, and "wait for a decoder"
  is the wrong instruction for a file that has moved.
* **A still is opened as the container its bytes are**, not as the one its name claims. A PNG
  called `.mp4` was named `png` by `kind` and drawn as `undecodable` by `resolve` until a test
  of that invariant over the whole corpus caught it. The invariant — `kind` names what
  `resolve` would produce — is now checked over all thirty-nine files under both
  `MissingMedia` policies.

**One crash was found and fixed at its cause**, by the byte-flip sweep rather than by review:
a Matroska length eight bytes wide made `0xFF_u8 >> 8` overflow, which is a panic and not a
zero. It is fixed with a checked shift and kept as a named regression test, as the corpus
README asks.

Four departures from `docs/MEDIA.md` are recorded in that document's new §8a rather than
quietly taken: the transfer function is this crate's own H.273 code points (layer 3 is **not**
reachable from layer 2, and the design's comment saying otherwise was wrong), a stream's
timebase is an `Option` because a still imposes no rate, `ContainerKind` gained `Png` and
`Ppm` because the manifest already says them, and `maj-render`'s dependency on `maj-media` is
not optional because there is no C library behind it to gate.

What this does **not** do: decode a frame. `MediaBackend::open_video` refuses with
`undecodable`, every movie still draws a labelled placeholder, and the viewer is still empty.
That is M2b, and the shape above does not change when it arrives — `resolve` gains a branch.

## P1: the document floor

**Four facts about a document that a decoder cannot supply, added before the decoder because
re-writing a schema afterwards costs more than writing it once.** Each is a command, so each
is undoable, replayable and attributable like every other fact in the file.

**1. A sequence knows what its first frame is called.** `Sequence::start_timecode_frames` and
`SetSequenceStartTimecode`, refused below zero. It is a **label, not a move**: no clip changes
position and every internal number stays an index from zero. It is what an EDL, a burn-in and
a QC report speak, and the broadcast convention is an hour, so a programme does not begin at
`00:00:00:00` and a pre-roll has somewhere to live.

**2. An asset can be measured, and found again by what it measures.** `MediaFingerprint`
carries BLAKE3 of the file's bytes, its size, and the duration the media itself reported
**when anything has read the container** — the hash is required and the duration is optional,
because hashing bytes is `std::fs` and reading a duration needs the probe that does not exist
yet, and a required duration would force every fingerprint to be written by a build that
cannot take it. `SetAssetFingerprint` records a measurement as a command; `RelinkAsset` points
an asset at a different file **without changing its identity**, so every clip that reads it
keeps its trims, its effects and its place. Measuring is I/O and no layer that owns a document
does I/O, so the measurement is taken by the CLI (`maj fingerprint`, `maj relink`) and reaches
the document the same way a person's edit does. Relinking matches **by hash, never by name**:
reel and clip names collide, so a relink by name can resolve two shots to one asset and
nothing reports it.

**3. A marker is one entity for a marker, a comment, a review note and a chapter point.**
`Marker` holds a name, a note, a frame, a length, one of **eight closed colours** and **its own
author**. The author is a field rather than something derived from the log because a marker
outlives the command that made it, and a review needs to read whose note it is without
replaying anything. The colours are a closed set because a marker colour is a category an
editor filters by, and a free `#rrggbb` would make "show me every red marker" a string
comparison over whatever anyone happened to pick. `AddMarker`, `SetMarker` and `RemoveMarker`
are exact inverses, and removing a marker from the middle of the list and undoing puts it back
**where it was**, which is the promise clips already made.

**4. The work that is not in the document yet survives a crash.** `maj_core::journal` is a
sidecar (`<document>.maj.journal`), not part of the format, and the document never depends on
it. An entry is one *group* of commands with the document hash it was applied against and the
author who applied it. Reading is a **prefix that says where it stopped**: a line that cannot be
read ends the readable prefix and names the line, and an entry whose base hash no longer
matches is **skipped, never applied**. That distinction is the whole design: a half-written
line is the real shape of a crash, so the entries before it are recovered rather than the
journal being thrown away, while an entry written against an older revision is the case where
the document was saved afterwards and replaying it would double the work. The grouping lives
in the journal rather than in the document for the reason P0.2 gives: recording it would make
"which commands were one undo step" part of the record, and a reopened project would undo
differently from the session that wrote it.

The schema moved to **3**, with both steps chained in `format::migrate`: `1 → 2` gave clips
the compositing model, `2 → 3` gives sequences a start timecode and a marker list and assets a
place to record a measurement. Every added field's default is the truth about the older
document rather than a convenient blank — `fingerprint: null` says **nothing has measured this
file**, which is exactly what was true, and a migration that invented a fingerprint would be
asserting a measurement that never happened.

Measured on this machine, Windows, GNU toolchain:

```
cargo fmt --all -- --check                                  exit 0
cargo clippy --workspace --all-targets -- -D warnings        exit 0   no warnings
cargo test --workspace --no-fail-fast                        exit 0   579 tests, 54 test targets
cargo doc --workspace --no-deps                             exit 0   no warnings
cargo test --release -p maj-core --test properties --test roundtrip
                                                            exit 0   7 + 8 tests, 10,000-command fuzz
cargo run -p maj-win32 --bin majestat -- --self-check          exit 0   23 checks, all as written
cargo run --release -p maj-win32 --bin majestat -- --snapshot out.ppm
                                                            exit 0, 1440x900, 85 zones
cargo run --release -p maj-win32 --bin majestat -- --bench 300 exit 0, 39 zones
                                                               three runs, 300 frames at 1440x900:
                                                               whole 7.97-10.84 ms, incremental 2.00-2.80 ms
```

**P1's own gate, and how it was measured rather than asserted.** The journal has a test that
kills nothing but proves the claim the death would test: a session applies three commands with
`--no-save`, the journal is replayed onto the document, and the result is compared **byte for
byte** with a second session that applied the same three commands and saved after each one
(`a_session_recovered_from_its_journal_is_the_session_that_would_have_saved`). A second test
cuts the journal mid-line — the crash itself — and asserts the readable prefix is recovered and
the damage is named, with the torn tail distinguished from damage in the middle. `maj relink`
has the same shape: a file moved to another directory is found by hash, and an asset nothing
has measured is reported as unidentifiable rather than guessed at from its filename.

**39 tests more than the 540 recorded under P0.4**, and the new ones are named where they live:
six reversibility tests in `maj-core`'s undo suite (one per new command, each asserting state
equality through undo *and* redo, and that a refusal changes nothing), the journal's own unit
tests in `maj-core`, five in `maj-media` on measuring a file and finding it again, the four CLI
gates above, the marker intentions in `maj-ui`'s model suite, and four checks that read the
counts four documents claim out of the code those claims are about.

**A false red gate, traced and then fixed at its cause.** Two `cargo test --workspace` runs
early in this session reported failures in `maj-cli --test cli` (the relink gate and both
journal gates), `maj-cli --test mcp` and `maj-export`'s unit tests. Every one of them passed in
the next full run and on its own, which is the shape of a collision rather than a defect. It
is a collision: **`cargo` takes the build directory lock for the build and releases it before
it runs the binaries**, so two sessions in one checkout run the same test suites at the same
time, and those tests' scratch directories were named per test and not per process — one run's
`remove_dir_all` deletes the directory the other is still writing into. The failure text said
so once it was read properly: `maj render` could not find a project that had been written a
moment earlier, and the journal gate read *five* entries where the test had applied three.

No assertion was weakened and no test was skipped. **The scratch directories are now unique
per process** — in `maj-cli`'s CLI and MCP suites, `maj-core`'s format and round-trip gates,
`maj-export`, `maj-mcp`'s four, `maj-render`'s resolver and `maj-ui`'s export, save and
preferences tests — and the two newest suites had already been written that way. The fix was
verified by running the whole workspace **twice at once**: both runs report 579 passed, zero
failed targets. What remains true is that two sessions *editing* one checkout still collide on
the files themselves, which is why the ultra plan puts the four lanes in separate git
worktrees; that is a different problem from two sessions running the same tests.

**The window's line base was re-measured and the recorded one is not reproducible.** At the M7
gate the paint measured *whole 4.5-5.6 ms, incremental 1.3-1.5 ms* at 1440x900; today's
release runs measure **whole 7.97-10.84 ms, incremental 2.00-2.80 ms**, and a debug build
measures 20.65 / 5.58 ms. The plan predicted exactly this ("the 4.5 ms recorded in `STATE.md`
is not reproducible on this machine today"), so the reason is not assumed: the frame draws more
than it did — the snapshot reports **106 zones against 85**, and the self-check's colour count
went from 1106 to 1230 — and part of the difference is the host rather than the paint. The
numbers above are the baseline from here, and a claim that the paint is unchanged would need a
measurement rather than this paragraph.

## M6m, in part: the editorial operations, as compositions rather than commands

The first half of the model's half of editing, and the half that decides the shape of the
rest. `crates/maj-timeline` (layer 5) holds **`ripple`, `roll`, `slip`, `slide`, `close_gap`
and `insert_blank`** as **pure functions from a project to a list of commands it has not
applied**, and `maj-mcp` exposes each as one tool that composes the list and applies it as a
single group.

**It adds no commands, and that is the decision the crate rests on.** A `ripple` command
would have been easy and wrong: the log would gain a second way to say what `trim_clip` and
`move_clip` already say, so two projects that made the same edit could hold different
commands, a diff would show a shape rather than a change, and replay would have to know both.
Nothing in the crate touches a `Project`, which is also what makes an operation inspectable
before it happens — the answer names the commands it composed (`trim_clip, move_clip,
move_clip`), so a model can be shown the edit and asked to approve it.

**A refusal rather than a rounding.** An edge moved by `n` timeline frames moves `n` × speed
source frames, and on a retimed clip that product can fall between two source frames. There is
no honest answer to which one, so the operation refuses and names the speed: *"3 timeline
frame(s) at speed 1/2 is not a whole number of source frames, and rounding it would drift"*.
A retimed clip can still be rippled by a multiple of its denominator, and the test asserts
both halves of that. Rounding silently is how an edit drifts a frame per adjustment, which is
the defect this engine exists to make impossible. The other refusals are as specific: a roll or
a slide with nothing on the other side of the cut, a trim that would leave a clip with no
frames, a slip that would leave the media, and a `close_gap` at a frame a clip covers —
because which nearby gap was meant is not something to guess. A move of zero appends nothing
at all.

**Measured on this machine**, Windows, GNU toolchain:

```
cargo fmt --all -- --check                                  exit 0
cargo clippy --workspace --all-targets -- -D warnings        exit 0   no warnings
cargo test --workspace --no-fail-fast                        exit 0   593 tests, 55 test targets
cargo run -p maj-cli --bin maj -- mcp --list-tools           exit 0   57 tools
```

**14 tests more than the 579 recorded under P1**: eleven in `maj-timeline`'s new
`tests/operations.rs` (each operation's shape and what it must *not* move, the whole
operation as one undo step and back, the refusals, and a spread of amounts in both directions
asserting that no track is left with a gap or an overlap), one more unit test in the crate
itself for the edge vocabulary, one over the wire in `maj-mcp`'s protocol suite — where a
ripple grows a sequence by twenty-five frames, one undo puts all three commands back, and a
refusal names the side of the cut that was missing — and one in `maj-ui`'s document checks.

**A count that had already gone stale is now a check.** `docs/MCP.md` said *"38 of the 57
tools append commands, one command per change"* after the six composing tools were added:
the total had been updated and the numerator had not, and one sentence was asserting something
no longer true of six of its tools. It now names the 36 tools that append exactly one command
and the six that append several, and `maj-ui`'s document checks assert the 36 against the
catalogue, so the next tool that writes makes the sentence fail rather than rot. The check was
verified by making it fail: `docs/MCP.md` was given 35 in the copy and the test reported the
mismatch, then the file was restored.

The work arrived with a failing `cargo fmt --check` and one clippy finding
(`needless_lifetimes` in the crate's own locator); both were fixed at the cause. No check was
suppressed.

**What M6m still owes is the majority of the phase**, and it is named rather than implied:
link groups, which change what a ripple *means* when picture and sound move together;
`duplicate` and per-clip grade copy; speed as a seventh animatable channel including negative;
the three- and four-point rules enforced rather than advised; and the shell calling these
functions instead of composing its ripples by hand in `maj-ui/src/action.rs`, where
`RippleDelete` today builds a `remove_clip` and a list of `move_clip`s itself.

## Position

**M0 (foundation), M1 (determinism), the three P0 items, P1 (the document floor) and the
first half of M6m (the editorial operations) are complete**, and the crates above them are
implemented as far as this host and this order allow: `maj-core` holds the document, its
command log, undo, markers, the autosave journal and the `.maj` format at schema 3;
`maj-media` measures a file and finds it again by hash; `maj-color` and `maj-render` draw
frames on the certified path; `maj-export` writes stills and manifests; `maj-timeline`
composes the editorial operations out of the commands the log already has; `maj-cli` renders,
verifies, measures, relinks and recovers headlessly; `maj-mcp` serves the editor over the Model
Context Protocol as 57 tools; and `maj-ui` is the shell —
every intention a person can express, translating into the same commands an agent's tools
append, with `maj-win32` as its window. The remaining crates are documented scaffolds that
declare their responsibility, their layer and their permitted lower-layer dependencies, so the
layering is enforced by the compiler from the first commit.

Nothing is blocked on a measurement of the host any more: the toolchain that blocked M2 is
there (P0.4), and the document floor that had to precede the decoder is in (P1). What comes
next is work, in the order the ultra plan sets out — **the rest of M6m** (link groups, the
structural commands, speed as a channel, enforced three- and four-point rules, and the shell
composing through `maj-timeline` instead of by hand) then **M-X** (interchange), with M8a able
to start immediately in its own lane because it needs no decoder. See **Not yet started** and **Next action** below, and
`ROADMAP.md`'s *The order the ultra plan settles on* for the sequence and its four lanes.

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

**1. ~~No C/C++ toolchain on the build host.~~ Discharged, 2026-10-15.** A MinGW-w64
toolchain and binutils `dlltool.exe` are present; a `windows-sys` dependant links, a C file
compiles, and a `raw-dylib` import generates its import library. The control run — the same
build with `dlltool` hidden from `PATH` — reproduces the old failure, so the result is a
measurement and not an assumption. See P0.4 above and the amendment to ADR 0009.

MSVC is still absent and still cannot link, and the project does not need it: the default
target is `x86_64-pc-windows-gnu` and the C compiler is `gcc`. **Nothing needs to be
installed.**

The consequences this blocker produced are kept on their own merits rather than reversed:
the property tests keep the seeded PRNG because an unseeded harness would make a reported
failure irreproducible, `blake3` keeps `pure` because a C-free graph is a smaller audit,
and `maj-core` keeps three dependencies because that is what makes the licence statement
checkable. ADR 0009's amendment says which clauses expired and which did not.

**What M2 is now waiting on** is work rather than a blocker, plus one decision of its own:
`unsafe` is forbidden outside `maj-win32` and binding a C library requires it, so
`docs/MEDIA.md` §8 recommends a leaf FFI crate and an ADR to authorise it. That ADR and the
FFmpeg build itself — configured per `docs/FFMPEG.md`, which does not exist as an artefact
yet — are M2's first two steps.

**2. ~~Product licence undecided.~~ Settled: `Apache-2.0 OR MIT`** (ADR 0014). What it
leaves open is narrower and more concrete: the copyright holder's legal name, the pinned
FFmpeg configuration, and M8's choice of H.264/HEVC encoder now that the GPL ones are
excluded. See `docs/LICENSING.md`.

## Not yet started, and what each needs first

| Phase | Blocked on |
|---|---|
| M6m Editing, the model's half | Nothing, and **in part done**: `ripple`, `roll`, `slip`, `slide`, `close_gap` and `insert_blank` are in `maj-timeline` as pure command compositions and on the agent's surface as six tools. Left: link groups, `duplicate` and per-clip grade copy, speed as the seventh animatable channel, three- and four-point editing as enforced rules, and the shell calling the library — `maj-ui/src/action.rs` still composes its ripples by hand |
| M-X Interchange | M6m, because an EDL or an OTIO export is a rendering of the editorial model |
| M8a Output correctness | Nothing, and no decoder: ranges, start timecode in the output, scopes as reductions over the framebuffer, a render queue |
| M2a Media vocabulary and probe | The corpus — twenty synthetic files first, then a hundred. The toolchain is no longer among them |
| M2b Decoding | M2a, the ADR authorising `unsafe` in a leaf FFI crate, and the FFmpeg build produced per `docs/FFMPEG.md` |
| M3, M4, M5, M9, M10, M11-M14 | The chain above; see `ROADMAP.md`'s sequence, which splits each of them rather than moving any of them |
| M7's gate — the three-minute edit | M6m and M-X. The mechanism (shell, window, pointer, clock, title studio, browsers) is built and gated; the edit it is a gate *of* needs the editorial operations, so M7 is sequenced behind them rather than left marked "in progress" |

## Next action

1. **~~Install the MSVC Build Tools.~~ Not needed, and P0 is complete.** The toolchain was
   already there in MinGW-w64 form; the verification and its control run are under P0.4
   above.
2. **~~Pin the FFmpeg configuration.~~ Done**: `docs/FFMPEG.md`. What remains is not a
   decision but an artefact — *produce* that build, under MSYS2 or a MinGW shell, and fill
   `third-party/ffmpeg/MANIFEST.toml` with the hashes the build itself reports. One item in
   it is not an engineering task and is the owner's: the EULA needs an explicit
   reverse-engineering-for-debugging carve-out, which LGPL-2.1 §6 requires and no code can
   supply.
3. **Write the ADR that authorises `unsafe` in a leaf FFI crate** (`docs/MEDIA.md` §8). It
   is the one thing M2 cannot start without, it relaxes a workspace-wide `forbid`, and it
   is deliberately not decided inside a design document.
4. **~~Perform M7's gate.~~ Deferred rather than dropped**: it cannot be met before M6m,
   which is why M7 is sequenced behind it in `ROADMAP.md`.
5. **Continue M6m in the model lane — its first half is in.** The six editorial operations
   are pure compositions in `maj-timeline` with their property gate green, and six tools
   apply each as one undoable step. **The order for the rest is unchanged**: link groups
   first (they change what a ripple means, so building the structural commands before them
   would mean writing them twice), then `duplicate` and grade copy, then speed as a channel,
   then the three- and four-point rules — and the shell moved onto the library, so one
   ripple does not have two implementations.
6. **Start M8a in its own lane at the same time**, because it needs no decoder: levels and
   range policy, the start timecode P1 added, scopes as reductions over the framebuffer with
   reference images to validate against, and a resumable render queue.
7. Curate a starter corpus of twenty synthetic files covering the M2 axes, so the
   hundred-file gate has somewhere to grow from.
8. Begin M2 in the order `docs/MEDIA.md` §9 sets out: the vocabulary first, with no backend
   behind it, so the layering and the closed `kind` vocabulary are provable before a
   decoder exists; then probe over the corpus, which is most of M2's gate and needs no
   frames at all; then the `MediaResolver` adapter in `maj-render`, which is the moment the
   viewer stops being empty. Hardware decode is capability-tested and measured last, and it
   is Tier 1 only — a certified render decodes in software, because a driver's output cannot
   be part of a claim that the same log produces the same bytes.

## Deliberate technical debts, recorded

| Debt | Where it lives | When it is repaid |
|---|---|---|
| `.maj` is text, not the planned binary container | ADR 0008 | M10, after the toolchain decision |
| Local undo does not survive a session; the inverse stack is session state | `maj_core::History` | Never — it is the honest design, documented rather than faked |
| Log replay is quadratic in project size | ADR 0002 | Only if measured to matter; the escape hatch is snapshots plus a tail |
| `maj-timeline` holds the six composed operations and nothing else: link groups, `duplicate`, grade copy, speed as a channel and the three- and four-point rules are still to be extracted, and `maj-ui` composes its own ripples by hand meanwhile | `maj-timeline`, `maj-ui/src/action.rs` | The rest of M6m |
| `wgpu` is not yet a dependency, so the render path is untested | ADR 0004 | M4 |
| ~~`PictureResolver::kind` costs a stat and cannot tell a damaged file from a readable one without reading it~~ | ~~`maj-render/src/source.rs`~~ | **Repaid in M2a**: `MediaResolver` answers from a cached probe, which is a header read and never a decode. `WorkspaceResolver` on its own still has the limitation, and its documentation still says so — it has no prober in it |
| A probe is reused while a file's length and modification time are unchanged, which is not a content hash: a file rewritten in place, to the same length, inside the timestamp's resolution, goes unnoticed | `maj_media::ProbeCache` | Only if measured to matter. Hashing on every `kind` call would cost a full read, which is the one cost that seam forbids; `invalidate` is the escape hatch, and P1's `measure` is the other half of the answer |
| The synthetic corpus is written and read by this workspace, so a misunderstanding both halves share cannot be caught by it | `maj_media::corpus` | Retired by real media, not by more synthesis. The pathologies are hostile whatever wrote them, which is the part that holds regardless |
| `maj-media` has two error types with one name: the probe's `MediaError` and `measure::MediaError`, the same three conditions spelled twice, so only the probe's is re-exported at the crate root | `maj_media` | M2b, where the decoder gives both a single caller. It is a rename, not a redesign — they already agree about what can go wrong |
| The grouping of commands into one undo step is session state, so it does not survive a save — a reopened project undoes command by command. The journal records the grouping, but only for the work that is not yet in the document | `maj_core::History`, `maj_core::journal` | Only if it turns out to matter; recording the grouping in the log would make it part of the document, which is a bigger decision than it looks |
| A fingerprint's measured duration is optional, so a trim is still clamped against a number somebody typed whenever nothing has probed the file | `maj_core::entity`, `maj-render/src/source.rs` | **M2a supplies the number and nothing spends it yet**: `VideoStream::frames` is read off `stts`/`stsz` for ISO base media and off `strh` for AVI, exactly where the container states it, and is `None` for Matroska, which states it in clusters. Wiring that into `MediaAsset`'s measured duration is a `maj-core` change and belongs to the lane that owns the schema |
| A marker has no reply, no resolve state and no thread — it is a note, not a conversation | `maj_core::entity` | M12, which owns anchored review |
| Two cargo sessions in one checkout still collide on the source files themselves, and on `target/` | the checkout | One git worktree per lane, as the ultra plan says. The *test* half of this was fixed rather than worked around: every scratch directory is unique per process now, so two concurrent suites no longer produce false red gates |

## Working agreements

* No phase is declared done without a measurement, and the measurement is recorded here.
* A failing check is fixed at its cause, never by weakening an assertion.
* Deterministic property tests: every case sequence is seeded, so a failure reported is a
  failure reproducible.
* Nothing is committed or pushed without being asked.
