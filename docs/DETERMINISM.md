# Determinism

Determinism is Majestat's central claim, so it is worth being exact about what is
claimed, what is checked, and what is deliberately not claimed.

## Two tiers, named up front

### Tier 1 — Reproducible (bit-exact)

**Same build, same machine, same inputs, same bytes out.**

This is what `maj-core` implements today at the document level and what
`maj render --verify` will check at the pixel level in M8. It is the tier that makes
a render farm auditable, a regression bisectable, and a delivery defensible.

### Tier 2 — Verifiable (cross-hardware)

**Different GPU, different driver, same declared manifest, output within tolerance.**

Bit-exact output across vendors is not achievable: floating-point reduction order,
fused-multiply-add behaviour and vendor shader compilers all differ. Claiming otherwise
would be marketing. Instead, the certified render path fixes what can be fixed:

* a declared reduction order for every accumulating operation;
* no fast-math and no `-ffast-math`-equivalent relaxation on the certified path;
* no vendor-specific approximations on the certified path;
* a graph version stamp, so any change to a node or a shader changes the output hash;
* per-node hashes, so a divergence can be localised instead of merely detected;
* a declared PSNR tolerance, so "verifiable" has a number attached rather than a vibe.

Anything outside the certified path may take faster, hardware-specific routes, and is
labelled as such in the render manifest.

## How the document level is made byte-exact

Three properties do the work, and each is enforced rather than assumed.

### 1. No floating point anywhere in the model

Rates are `Rational` (`24000/1001`, not `23.976`), frame ranges are `i64` pairs, speeds
are `Rational`. `Rational` is stored in canonical form — reduced, denominator positive,
zero as `0/1` — so two equal values have one representation and `derive(PartialEq)` is
exact rather than approximate.

The consequence is that canonical JSON has exactly one text form per value. There is no
`1.0` versus `1` ambiguity, no locale-dependent decimal separator, no NaN ordering, and
no accumulated drift over a long timeline.

This is tested, not asserted: `the_canonical_form_contains_no_floating_point_numbers`
walks the serialised state and fails on any non-integer number, and
`the_float_detector_actually_detects_floats` proves the check would catch a `29.97`.

### 2. No maps, and field order is fixed

Every collection in the model is a `Vec` or a slice, so iteration order is insertion
order — and insertion order is itself part of the state, because `derive(PartialEq)` and
the serialiser both see it. `insertion_order_is_part_of_the_state` pins this: two
projects with the same clips at the same positions but inserted in a different order
hash differently, and that is correct, because undoing a removal has to restore a
position, not merely an existence.

Struct field order is declaration order, fixed at compile time, so there is no
canonicalisation step to get wrong.

### 3. Reduction happens on unsigned magnitudes, with checked intermediates

`Rational` reduces using `u128` magnitudes. Reducing in signed space would need
`|i64::MIN|`, which no signed 64-bit type can hold — the naive version panics in debug
and silently wraps in release, which would corrupt frame numbers invisibly. Every
`i128` intermediate in addition, subtraction, multiplication and division is
`checked_*`, because the product of two `i64` magnitudes reaches `2^126` and their sum
can leave the `i128` range entirely. Overflow is reported, never wrapped.

## What is hashed

| Hash | Covers | Excludes | Used for |
|---|---|---|---|
| `state_hash` | schema version, name, assets, sequences | the command log | comparing two projects, verifying a replay |
| `document_hash` | the whole document, log included | nothing | identifying a revision, detecting a hand edit |
| `command_trace_hash` | author and command per entry, in order | log positions | attribution, review hashes, collaboration |

`command_trace_hash` deliberately ignores `seq`, because that is the position in the
log: two identical edit sequences must not hash differently just because one started
life further along. `the_trace_hash_ignores_log_position_and_covers_author_and_command`
pins that.

Hashes are BLAKE3 over the compact canonical JSON, hex encoded.

## What is *not* deterministic, and is not asked to be

* **Absolute timing.** Playback adapts to the machine. Frame-accurate, not
  microsecond-accurate.
* **Cache contents.** Caches are keyed by the hash of their inputs, so a cache miss
  changes *when* something is computed, never *what*.
* **Local undo across sessions.** The inverse stack is session state, computed from the
  state each command observed. Opening a project starts an empty history. This is
  documented on `maj_core::History` rather than papered over with a reconstructed
  history that might not be exact.
* **Non-certified render paths.** Hardware-specific fast paths are outside Tier 2 until
  they declare a manifest.
* **Hardware decode.** When M2 lands, a hardware-decoded frame depends on a driver and a
  GPU, so it is **Tier 1 only and never certified**: export and `--verify` decode in
  software, playback may not, and the manifest says which it was. Same rule as any other
  opportunistic path, applied to the decoder — see `docs/MEDIA.md` §6.

## What a decoder will add to the claim

Today "the same log and the same media produce the same bytes" needs no qualification,
because the only media the engine reads are PNG and PPM stills inflated in pure Rust. A
decoder changes that, and it is worth stating before it arrives rather than after:

**The decoder becomes part of the hashed inputs.** Two FFmpeg versions can both be correct
and produce different pixels from the same H.264 file, so `DecoderIdentity` — backend, exact
version, and a hash of the library's own configuration string — is recorded in the render
manifest and is part of every cache key (`docs/MEDIA.md` §4). A render is reproducible
*against a named decoder*, and the name is in the manifest rather than implied by the
machine. This is why `docs/FFMPEG.md` pins one version and refuses a library whose hash is
not the pinned one: the pin is what makes the determinism claim survive an upgrade instead
of silently changing meaning.

Two consequences follow, and both are deliberate: pixel-format conversion and audio
resampling are done in this workspace rather than by swscale and swresample, whose results
are stable for a build and not across builds; and a decoder upgrade is a **declared break**
in Tier 1 equality for footage it touches, announced as such rather than discovered when a
`--verify` fails.

## The gates that hold this today

Run `cargo test --release -p maj-core --test properties --test roundtrip`.

* `the_state_is_a_function_of_the_log_for_many_seeds` — eight seeds, each rebuilt from
  its own log, compared on `state_hash` and `document_hash`.
* `rebuilding_from_a_log_twice_is_idempotent` — log to state to log is a fixed point.
* `every_sampled_prefix_of_the_log_replays_to_the_same_state` — for every sampled cut,
  replay and history-driven application agree byte for byte, and undoing the prefix
  returns exactly to the empty project.
* `random_undo_and_redo_sequences_land_on_the_traced_state` — after every command, the
  state hash is recorded; random undo and redo walks must land on the recorded hash for
  the resulting revision, every time.
* `undoing_everything_returns_to_the_initial_state_and_redoing_returns_to_the_peak`.
* `the_same_seed_builds_the_same_project`.
* `a_random_project_stays_valid_after_every_single_command` — invariants hold after
  every one of ten thousand commands, not just at the end.

The fuzz budget is 2,000 commands in debug builds and 10,000 in release builds. Both are
real runs; the smaller one exists so iteration stays fast.
