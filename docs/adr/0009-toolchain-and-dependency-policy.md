# 0009. Toolchain reality and the dependency policy it forces

* Status: Accepted
* Date: 2026-10-13

## Context

The M0/M1 build host was measured before dependencies were chosen, not after a build
failed. What was found:

| Fact | Consequence |
|---|---|
| Rust 1.99 with both `stable-x86_64-pc-windows-gnu` and `-msvc` installed | The GNU toolchain is the default and is used here |
| **No Visual Studio / VS Build Tools**: no `link.exe` | The MSVC toolchain cannot link on this host |
| **No C compiler at all**: no `cc`, `clang`, `gcc` | Any crate with a C build script is unbuildable |
| **No binutils `dlltool.exe`** | Any crate that imports a Windows system library through `windows-sys` fails to build |
| Network to `index.crates.io` works | Dependencies can be fetched, so this is a selection problem, not an offline problem |

The GNU toolchain does link successfully — rustup ships `rust-lld` — so ordinary Rust with
no C dependencies builds fine. The failures are specific: `proptest` was tried and
**failed to compile** because it pulls `windows-sys`, and `getrandom` fails for the same
class of reason.

This was discovered by probing, and both failures are real: see the notes below.

## Decision

A dependency policy narrow enough to build on this host while keeping layers 0 and 1
strictly honest.

* **Layer 0 (`maj-time`) has zero dependencies by default.** `serde` is an opt-in feature.
  The arithmetic core should not have a dependency graph at all, and this also means it
  compiles in under a second.
* **Layer 1 (`maj-core`) has three dependencies**, each verified to build here: `serde`
  (derive), `serde_json`, and `blake3` on its `pure` feature so no C code is compiled.
* **Property tests use a seeded PRNG written in this workspace**, not a property-testing
  framework. `proptest` does not build here, and — more importantly — an unseeded
  harness contradicts the project's own claim: a failure it reports would not be
  reproducible from the report. `maj_time::testing::DeterministicRng` gives fixed case
  sequences on every machine.
* **`wgpu` is not a dependency in M0.** It arrives in M4 with the render graph, so the
  foundation stays fast to build and the scaffold stays green.
* **No crate with a C or C++ build script** until a toolchain decision is made. That
  rules out a bundled SQLite, which is why the format is JSON-first (ADR 0008).
* Every dependency is declared once in `[workspace.dependencies]`, so the licence audit
  in `docs/LICENSING.md` is a single file to review.

## Consequences

* **M2 onwards needs a C/C++ toolchain.** Decoding FFmpeg, and vendor RAW SDKs, cannot be
  built without one. The two options are installing the Visual Studio Build Tools (which
  also restores MSVC linking and the `windows-sys` ecosystem, including `proptest`), or
  MSYS2 binutils for `dlltool` plus a mingw-w64 toolchain. This is a prerequisite for M2
  and is tracked as the first blocker in `.planning/STATE.md`.
* The test suite is deterministic by construction, which is a better fit for this project
  than a framework with random sampling. It is also more code to maintain.
* A decade of the Rust ecosystem expects `getrandom` somewhere in its transitive graph.
  Dependencies are chosen with that in mind until the toolchain decision is made.
* The JSON-first format decision is a direct consequence of this ADR and is revisited in
  M10, after the toolchain question is settled.
