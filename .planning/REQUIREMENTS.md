# Requirements — foundation milestone (M0 + M1)

Scope: the repository foundation and deterministic core. **No rendering, decoding, colour,
audio or interface.** Those are M2 onwards and are explicitly out of scope here.

Status legend: **met** (verified by a run), *partial*, **not started**.

## M0 — Foundation

| # | Requirement | Acceptance | Status |
|---|---|---|---|
| M0.1 | Repository with a Cargo workspace | Workspace resolves; `cargo metadata` succeeds | **met** |
| M0.2 | Twenty crates in seven layers, dependencies pointing downward only | Every crate declares its layer; lower-layer deps are declared as `workspace = true`, so the compiler enforces direction | **met** |
| M0.3 | Workspace-wide lint policy | `unsafe_code` forbidden — with exactly one documented exception, `maj-win32`, whose lint level is `deny` and whose single module of Win32 imports carries the `allow` and its reasons (ADR 0013); missing docs, unreachable pub and missing `Debug` warned; promoted to errors in CI via `-D warnings` | **met** |
| M0.4 | Formatting is mechanical and enforced | `cargo fmt --all -- --check` exits 0 | **met** |
| M0.5 | Continuous integration on two platforms | GitHub Actions runs fmt, clippy, tests and docs on Windows and Linux, plus a separate release determinism job | **met** |
| M0.6 | Architecture decision records | Nine ADRs written, covering determinism, the command model, rational time, the stack, FFmpeg, numerics, effects, the format and the toolchain | **met** |
| M0.7 | Architecture, determinism, format and licensing documents | `docs/ARCHITECTURE.md`, `DETERMINISM.md`, `PROJECT-FORMAT.md`, `LICENSING.md` | **met** |
| M0.8 | Media validation corpus planned | `testdata/media-corpus/README.md` specifies the axes and patterns; files are referenced, not committed | **met** |
| M0.9 | Dependency and licence audit | `docs/LICENSING.md` audits all three dependencies and sets the change gate | **met** |

## M1 — Determinism

| # | Requirement | Acceptance | Status |
|---|---|---|---|
| M1.1 | Exact rational time | `Rational` canonical under all inputs; reduction preserves value; ordering exact; overflow reported not wrapped | **met** |
| M1.2 | Timebase and drop-frame timecode | Non-drop round trip at all eight standard rates; drop-frame round trip at 29.97 and 59.94 | **met** |
| M1.3 | Frame ranges | Half-open `[start, end)`; `intersects` agrees with `intersection`; set laws property-tested | **met** |
| M1.4 | Retime maps under floor semantics | Monotonicity for arbitrary speeds; exact round trip for whole-number speeds; the round-trip limit documented rather than overclaimed | **met** |
| M1.5 | Editable model | Media, sequences, tracks, clips; clip instance distinct from the media it reads | **met** |
| M1.6 | Every command invertible | `Command::apply` returns the inverse; thirty variants; removal restores list position, not just existence | **met** |
| M1.7 | Undo and redo | Exact including clip position; redo branch discarded on a new edit; session-scoped and documented as such | **met** |
| M1.8 | The `.maj` document | Canonical JSON, magic, schema version, strict loading, four fail-closed gates, migration scaffold | **met** |
| M1.9 | Content hashing | `state_hash`, `document_hash`, `command_trace_hash` over canonical JSON with BLAKE3 | **met** |
| M1.10 | State is a function of the log | `rebuild_from_log` reproduces state and log byte for byte, across eight seeds | **met** |
| M1.11 | Model validation | Unique identifiers, non-degenerate sequences, positive speeds, trims inside media, contiguous log | **met** |
| M1.12 | Deterministic property tests | Seeded PRNG, no framework; fixed case sequences on every machine | **met** |
| M1.13 | Ten-thousand-command fuzz without divergence | Release run: apply, validate after every command, undo everything, redo everything, compare hashes | **met** |

## The M1 gate, verbatim

```
cargo fmt --all -- --check                                    → exit 0
cargo clippy --workspace --all-targets -- -D warnings          → exit 0
cargo test --workspace                                        → all pass
cargo test --release -p maj-core --test properties --test roundtrip
                                                              → all pass, 10,000 commands
```

## Explicitly not required for this milestone

* Reading a single byte of media, or producing a single frame of picture.
* Any GPU resource, shader or render graph.
* Any interface, or any way for a human to use this without writing Rust.
* Any network, storage or collaboration behaviour.
* A product licence.

That list is the point of building the engine first: none of it is needed to prove the
foundation, and all of it is easier once the foundation is proven.
