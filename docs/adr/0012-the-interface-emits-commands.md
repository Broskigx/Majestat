# 0012. The interface emits commands, and it owns no windowing toolkit

* Status: Accepted
* Date: 2026-10-14

## Context

M7 is the interface phase, and its gate is a three-minute edit performed end to end by
keyboard and agent alone. Two decisions had to be made before a widget could be drawn, and
both of them are easier to get right before there is any code than after.

The first is what an interface *is* here. The obvious shape — an application with its own
state, its own undo stack, and a UI that mutates the model directly — is the one to reject.
Majestat's claim is that everything is a command, and the MCP surface (ADR 0011) has already
been built on it: an agent's edit is a command, attributed, invertible and replayable. If
the interface were a second path, the log would explain only half of a project's history,
the other half would be unattributable, and "the state is a function of the log" would be
false for every project a person had touched. That is a determinism claim, not a code
style, and it is the kind that is expensive to retrofit.

The second is what the interface is built *on*. `eframe`/`egui` is the obvious toolkit, and
it does not build on this host. That is a measurement rather than an impression:
`target/probe/eframe` is a throwaway crate that depends on eframe alone, and it fails before
it links.

```
error calling dlltool 'dlltool.exe': program not found
error: could not compile `parking_lot_core` (lib) due to 1 previous error
PROBE_BUILD_EXIT=101
```

The chain is exact: eframe reaches `parking_lot_core`, which imports Windows system
libraries through `windows-sys`, which makes rustc build an import library, which needs
`dlltool.exe`. The machine has no `dlltool`, no assembler, no C compiler and no MSVC — the
same blocker ADR 0009 and `.planning/STATE.md` record for M2. It is not a version problem
and not something a feature flag fixes.

## Decision

**The interface is a model that emits commands, and the widgets are a layer to be added on
top of it.** `crates/maj-ui` is layer 6 and contains the intention set, the keyboard, the
two languages, the preferences and the shell — and no window.

1. **One door.** An intention is a value (`Action`), translating it is a pure function of the
   intention and a read-only `Context` (`Action::commands`), and `Shell::dispatch` is the
   only method that writes. It applies the commands through the session's history and
   authors every one of them `ui:local`, so a person's edit is exactly as undoable,
   replayable, diffable and attributable as an agent's. Rendering, zoom, panels, language
   and preview resolution are view state and change no document, which a test asserts.
2. **No windowing dependency, deliberately.** Not one type from a toolkit appears anywhere
   in the crate's public surface. The window, when the toolchain allows it, will be an
   application that forwards strokes into `Shell::perform` and reads frames back out of
   `render_preview` — a few hundred lines of drawing, not a rewrite, and provable by the
   fact that the shell compiles and passes its tests without one.

   *Amended 2026-10-15.* The window exists. `maj-win32` (ADR 0013) is that application: it
   forwards the shell's own strokes and reads `render_preview` back, with the Win32 imports
   declared by hand because no toolkit builds on this host, and the shell still compiles,
   tests and documents itself with no window in sight. The decision did not change; the
   tense did.
3. **Keyboard-first, and no key that lies.** The bindings are one table read by a test, so a
   stroke cannot exist without being listed. There was deliberately **no spacebar and no
   `j`/`k`/`l`** while there was no playback engine: a transport key that does nothing is
   worse than an absent one, and a test kept them absent so that adding them was a decision
   somebody had to make rather than an accident.

   *Amended 2026-10-15.* The decision held and the premise changed, so the test was changed on
   purpose rather than the rule quietly dropped: a rational clock now exists, `space` and
   `j`/`k`/`l` are bound to it, and the rule they were an instance of — **no key that lies** —
   is why they were added with the clock rather than after it. What the clock does not do is
   decode media (M2) or make a sound (M9), and both the status line and `docs/UI.md` say so.
4. **Two languages from the first string.** Seventy-six keys with English and Spanish
   tables to begin with — a hundred and ninety-nine today — chosen now rather than after the
   interface has grown comfortable with literals.
   The language is a preference, not a document property: switching it changes no revision.
5. **Preferences are a file of view state, repaired rather than trusted.** `ui.json` holds
   the language, zoom, preview resolution, tiling, transfer function and open panels.
   Values that cannot be honoured are clamped, defaulted or dropped individually instead of
   failing the load, because a preferences file is not worth refusing to start over.
6. **The palette is the agent's catalogue in a person's hands.** `Shell::run_tool` calls the
   same `maj-mcp` dispatcher against the session the person is holding, and the author stays
   `ui:local`. There is no second argument parser and no second dispatcher: one
   implementation, two audiences.
7. **The interface renders through the certified path.** `preview_request` and
   `export_still` go through `maj-render` and `maj-export` with `RenderPolicy::Certified`.
   There is no private preview renderer, so a viewer cannot show a frame the exporter would
   refuse to write, and the whole-versus-tiled verification uses the same code.
8. **No new dependencies.** `serde` and `serde_json` were already in the workspace;
   everything else — the stroke model, the string tables, the palette's decoder, the
   preference sanitiser — is written here, on ADR 0009's rule that a dependency has to earn
   its place.

## Consequences

* **The gate is exercised by tests rather than demonstrated by hand.** The intentions, the
  keys, the refusals, the parity with the agent's tools and the pixels are all checked
  (`cargo test --workspace`, 528 tests across 53 targets today). The pointer, the
  drag and the scrub used to be unchecked because there was no window; there is one now, and
  the self-check posts real clicks and drags at it, so what is still unchecked is the person
  watching frames arrive. M7 is recorded as **in progress**, not done, and the reason is one
  measured blocker rather than a list of excuses.
* **Attribution is visible in the hash.** The state hash of a project edited by keyboard and
  by tool calls is identical; the document hash is not, because it covers the log and the log
  records who made each edit. Two hands, one document, two histories. That is a feature, and
  the cross-interface test asserts both halves of it.
* **An intention can be added without touching a widget**, and the tests make the addition
  mechanical: a command the interface can name must be a tool the agent has, and vice versa.
  The two surfaces cannot drift, because a drift is a failing test rather than a bug report.
* **Layout is not modelled here.** Which panels are open is data; where they sit is not. That
  is deliberate — a layout described as data before there is a surface to lay out would be a
  guess frozen into a type.
* **The logo is decoded once, at build time.** `assets/brand/logo.png` is inflated by
  `crates/maj-win32/build.rs` and turned into the pixels the window class and the interface
  ask for — the 16 and 32 px icons and the 48 px seal — so nothing ships a PNG decoder.
  There is still no texture for a GPU presenter to sample, because there is no GPU
  presenter; the white-field question the brand notes raise is still the owner's to answer,
  and the file stays as supplied until it is.
* **What remains open, and where it lands:** transport and scrub responses (M3), audio
  meters and levels (M9), delivery presets (M8), collaboration presence and comments (M12,
  on top of the command log this phase already writes to), accessibility and input-method
  behaviour (with the window), and the wordmark (a typeface decision nobody has made).
