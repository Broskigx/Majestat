# Licensing

**The product licence is `Apache-2.0 OR MIT`.** The recipient picks either one. It is
declared once, in `[workspace.package]`, and inherited by all twenty-two crates; the full
texts are `LICENSE-APACHE` and `LICENSE-MIT` at the root. Nothing is published yet — every
crate is still `publish = false` — but the terms are no longer an open question.

The choice is the Rust ecosystem's own convention, and it is the same dual licence all four
current dependencies carry, so there is no interface between the product's terms and its
dependencies' terms to reason about. Apache-2.0 also carries an explicit patent grant,
which matters for a codec-adjacent product in a way MIT alone does not; MIT stays in the
pair because it is the simpler licence for a recipient who only wants permission.

The copyright holder is recorded as "Majestat", matching `authors` in the workspace
manifest. **A legal name or entity belongs there before any release** — that is the owner's
to set, not an engineering decision.

## What the decision forecloses

A permissive licence is not a weaker constraint than the LGPL-safe default that preceded
it; it is a *different* one, and in one direction a stricter one. Copyleft code cannot be
linked into a permissively licensed binary, so:

* **No GPL component may enter the shipping product.** This excludes `x264` and `x265` and
  every FFmpeg component behind `--enable-gpl`. The H.264 and HEVC encoders therefore have
  to come from somewhere else — the platform's own (Media Foundation on Windows), or a
  permissively licensed library such as OpenH264 (BSD-2) — and that is a decision M8 owns.
* **FFmpeg is admissible only as an LGPL build, dynamically linked**, with the notice and
  corresponding-source obligations met. The build must avoid `--enable-gpl` and
  `--enable-nonfree`. See `docs/adr/0005-ffmpeg-lgpl-behind-an-owned-abstraction.md`.
* **LGPL dependencies stay dynamically linked and stay behind an abstraction this
  workspace owns**, so that the LGPL's reverse-engineering and relinking rights can
  actually be exercised by a recipient.
* **Vendor SDKs stay isolated behind feature flags**, so a build without them is a build
  without their terms in force. That covers Blackmagic RAW and RED R3D, and the licensing
  conversations needed for ProRes and DNxHR encoding.

A permissive licence does not grant patent rights the licensor does not hold. **Codec
patent pools are a separate question from copyright** and are unaffected by this choice;
see the obligations below.

This is still why `maj-time` has zero dependencies by default and why `maj-core`'s
dependency set is three crates: a small, audited dependency set is what makes the licence
statement checkable rather than aspirational.

## Dependency audit

| Crate | Version | Licence | Why it is acceptable |
|---|---|---|---|
| `serde` | 1 | MIT OR Apache-2.0 | Permissive; the ecosystem standard for serialisation |
| `serde_json` | 1 | MIT OR Apache-2.0 | Permissive; canonical JSON output |
| `blake3` | 1 (`pure` feature) | CC0-1.0 OR Apache-2.0 | Public domain dedication or permissive; `pure` avoids any C dependency |
| `miniz_oxide` | 0.8 (`with-alloc`) | MIT OR Zlib OR Apache-2.0 | Permissive, no C; it is what inflates a PNG without a codec library |

All four are declared once in `[workspace.dependencies]` and inherited, so the audit is
a single file to review. Every one of them is compatible with `Apache-2.0 OR MIT` in both
directions: three of the four *are* that same dual licence, and `blake3`'s CC0 arm is a
public-domain dedication.

## The audit gate for future dependencies

Before adding a dependency, answer, in the pull request:

1. What is its exact licence, and of its transitive dependencies? **A GPL or AGPL answer
   is a refusal**, not a discussion: it cannot enter an `Apache-2.0 OR MIT` binary. An
   LGPL answer is admissible only dynamically linked and behind an owned abstraction.
2. Does it link or vendor native code that carries its own terms?
3. Does it ship model weights, and are the weights licensed separately from the code?
4. Does it introduce a patent-licensing obligation separate from copyright?
5. Can the capability be written in this workspace in a few hundred lines instead?

Question 5 is not rhetorical. It is why the property tests use a seeded PRNG rather than
a test framework, and why `maj-time` has no dependencies at all.

## Known obligations to resolve before any release

* **The copyright holder's legal name**, in `authors` and in `LICENSE-MIT`. Currently
  "Majestat", which is a project name and not an entity.
* **~~FFmpeg build configuration.~~ Pinned: [`docs/FFMPEG.md`](FFMPEG.md).** FFmpeg 9.0.2
  (`n9.0.2`), shared LGPL build, `--disable-autodetect`, and none of `--enable-gpl`,
  `--enable-nonfree` or `--enable-version3`; the obligations are mapped to the LGPL-2.1
  clauses that create them, and the compliance claim is a runtime gate — `avutil_license()`
  must say LGPL and the loaded library's own configuration string must contain no refusal,
  or `maj-media` refuses to initialise. Two items in it are **not** engineering tasks and
  remain the owner's: the EULA needs the reverse-engineering-for-debugging carve-out §6
  requires, and the written source offer needs the legal name below.
  `docs/adr/0005-ffmpeg-lgpl-behind-an-owned-abstraction.md` is the decision; `docs/FFMPEG.md`
  is the configuration.
* **Codec patent pools.** H.264 and HEVC distribution and encoding are separate from
  copyright licensing and depend on jurisdiction, and the permissive product licence does
  nothing to resolve them. AV1 is the royalty-free path and is the default preference
  where quality allows.
* **The H.264 and HEVC encoder choice.** `x264` and `x265` are GPL and therefore
  excluded. The candidates are the platform encoder (Media Foundation) and OpenH264
  (BSD-2), and the decision belongs to M8.
* **ProRes and DNxHR encoding.** Apple's and Avid's programmes, respectively. Decoding
  availability does not imply a right to ship an encoder.
* **Camera RAW SDKs.** Blackmagic RAW and RED R3D terms, which permit redistribution of
  designated components in object form with conditions.
* **AI model weights.** Runtime licence, model code licence, checkpoint licence and
  training-data rights are four separate questions. Each model is audited before it is
  bundled, not after.
* **C2PA signing.** Key acquisition, protection and the user-facing validation
  interface, when content credentials land in M8.
* **Types.** The renderer sets its own alphabet — four faces drawn as strokes in `maj-render`
  — so nothing ships a font file and no foundry's terms travel with the product. Reading a
  typeface the machine already has (a system font, or a `.ttf` a production supplies) would
  bring a font licence with it, and is therefore not a small change.
