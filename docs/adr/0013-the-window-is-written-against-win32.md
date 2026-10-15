# 0013. The window is written against Win32 by hand

* Status: Accepted
* Date: 2026-10-14

## Context

ADR 0012 built the interface as a model and left the window as "a thin layer to be added when
the toolchain allows it". The toolchain has not changed: `dlltool` is still missing, there is
still no C compiler and still no MSVC, so the probe in `target/probe/eframe` still fails to
build `eframe` — and with it every windowing crate on the shelf, because all of them reach
`windows-sys` on Windows, and `windows-sys` builds its import libraries with `dlltool`.

That left three options, and only one of them ends with a window on the screen.

1. **Wait.** Correct, cheap and indefinitely long. The interface would keep passing its tests
   and nobody would see it.
2. **Vendor or patch the toolchain path.** Installing MSVC Build Tools or MSYS2 binutils is
   an administrator-level, multi-gigabyte change to somebody else's machine, and the
   project's own position (ADR 0009, `STATE.md`) is that this decision belongs to the owner,
   not to a build step.
3. **Declare the imports by hand.** The Win32 API is a flat C ABI, and `rustup` already ships
   `libuser32.a`, `libkernel32.a` and `libgdi32.a` for `x86_64-pc-windows-gnu` — they are
   what `std` itself links against. So `#[link(name = "user32")]` resolves without `dlltool`.
   A probe (`target/probe/win32`, kept out of the workspace like the eframe one) created a
   window, showed it, pumped eighteen messages and closed it on a timer:

   ```
   WIN32_PROBE_OK visible=1 messages=18
   ```

   No dependency, no build script, no toolchain change, on this machine, today.

## Decision

**A new crate, `maj-win32`, at layer 6, presents `maj-ui` in a window written directly against
the Win32 API, with the imports declared by hand.**

1. **The shell stays platform-free.** Not one line of windowing code enters `maj-ui`; ADR 0012's
   split is preserved exactly, and the proof is that the shell still compiles, tests and
   documents itself with no window in sight. `maj-win32` depends on `maj-ui`, never the
   reverse.
2. **One crate holds `unsafe`, and it says so.** The workspace forbids `unsafe_code`
   everywhere. This crate's lint level is `deny` instead of `forbid`, and the single module
   that declares the foreign functions carries `#![allow(unsafe_code)]` with its reasons
   attached, because `forbid` cannot be overridden anywhere and would have made the exception
   invisible. Inside that file every foreign call sits in an `unsafe` block with a `SAFETY`
   note, and nothing crossing the module boundary takes or returns a pointer the caller has to
   dereference: the presenter is ordinary safe Rust. CI's `-D warnings` still makes any other
   `unsafe` in the crate an error.
3. **The window decides nothing.** It translates messages into the strokes the shell's own
   table defines, hands them to `Shell::perform`, and paints what the shell says. Every label,
   name, count and state comes from the shell, in the language the shell was told to speak.
   There is no second keyboard table, no second command vocabulary, and no path to the model
   except the log.
4. **The keymap is proven, not asserted.** A test walks `keys::TABLE` and requires that every
   stroke the shell answers to has a virtual key on this window that produces it — sixty-seven
   of them, including `ctrl+shift+z`, `alt+arrow` and the punctuation keys. A second test forbids
   the reverse: a key that reaches no binding.
5. **The window is checked without a person.** `--self-check` creates a real window, posts
   real `WM_KEYDOWN`, `WM_LBUTTONDOWN` and `WM_MOUSEMOVE` messages at it, drains the real
   message queue and asserts what happened — the playhead moved five frames; a click selected
   a clip, a drag moved it, a drag on its tail trimmed it, a drag in the ruler scrubbed
   without touching the document, `ctrl`+wheel zoomed; the clock started on `space` and a tick
   advanced the playhead; a copy was appended and undone; the palette opened, took text and
   matched the tool; the language switched without moving the revision; the page strip opened
   the deliver page; and the paint produced a picture with something in it, every zone inside
   the window. `--snapshot` paints the whole interface into an offscreen context and writes a
   PPM, so the layout can be looked at — and diffed — without a screen; `--bench` times that
   paint and `--regions` reports the colours it produced.
6. **No dialog, no file browser.** `ctrl+k` is the palette and the palette is the catalogue;
   `ctrl+s` writes to the shell's suggested path when a document has none. The narrowness is
   deliberate: every dialog is an API surface with a policy behind it, and this phase's job is
   the editing surface.

   *Amended 2026-10-15.* A menu bar was added, and it did not become a second editing path:
   a row is a `Binding` the keyboard already has or a `Special` only the window can do (open,
   save as, quit), and a row whose subsystem does not exist yet refuses and names its phase.
   The file browser is still the operating system's own dialog, reached from that menu, rather
   than one written here.

## Consequences

* **A window exists on this machine, today, with no dependency added.** The `majestat` binary
  opens the example project, draws the frame — menu bar, toolbar, the media pool over the
  command log, the monitor over its transport bar, the inspector beside the mixer, the timeline
  with its clips and playhead, the status line and the page strip — and it edits with exactly
  the keys and gestures `docs/UI.md` documents.
* **It is Windows-only, and that is the price.** A presenter for macOS or Linux would be
  another crate, not a change to this one — which is the same shape ADR 0012 anticipated for
  `egui`, and the reason the shell was kept free of platform types in the first place. The
  cost is real and is recorded rather than hidden: this crate's code is not portable, and a
  second presenter is a second file of drawing.
* **The unsafe surface is small and auditable.** Two modules: one that declares seventy-six
  functions from `user32`, `gdi32`, `kernel32`, `msimg32` and `comdlg32`, and one that wraps
  each in a safe signature. Nothing else in the workspace may hold `unsafe`, and the lint level
  is what enforces that rather than review.
* **What the window still does not do:** no rubber band, so a drag moves a clip rather than
  sweeping a selection; no decoded picture in the viewer (M2) and no audio (M9), which is why
  the transport is a clock over the playhead rather than playback; the icon and the page
  strip's seal now come from the owner's logo, decoded once at build time; and four of the
  seven pages are drawn and named but empty, each saying which phase owns it.

  *Amended 2026-10-15.* When this ADR was written the window had no pointer, no menus, no page
  strip and no panel toggles. It has all four now: mouse messages become zones and gestures
  resolved against the same region table the painter draws from, seven menus and two context
  menus carry the intentions the keyboard already had, the seven pages give the frame a place
  to put each subsystem as it arrives, and the dividers are dragged and remembered. None of it
  changed the decision this ADR records — the window is still written against Win32 by hand —
  which is why it is an amendment to the consequences rather than a new ADR.
* **M7's gate is now reachable rather than merely tested.** A three-minute edit by keyboard
  and agent alone is something a person can now perform on this machine, and the self-check
  runs the same operations mechanically. The gate is still not recorded as *met*: nobody has
  sat down and done it, and the engine's editorial operations (M6) are not all here yet.
