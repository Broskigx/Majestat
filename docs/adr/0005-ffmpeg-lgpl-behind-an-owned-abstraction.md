# 0005. FFmpeg under LGPL, behind an abstraction Majestat owns

* Status: Accepted
* Date: 2026-10-13
* Amended: 2026-10-15 — the product licence is now `Apache-2.0 OR MIT`. See the amendment
  at the end: the decision below is unchanged and one of its reasons has strengthened.

## Context

Broad container and codec support is not optional, and no one is going to write a
demuxer for every camera format again. FFmpeg is the only realistic answer, and it comes
with two separate problems:

* **Copyright licensing.** FFmpeg's base is LGPL 2.1-or-later, but optional components
  make the whole build GPL, and the project publishes a compliance checklist. The
  product licence is undecided, which makes the most permissive practical build the
  required one (see `docs/LICENSING.md`).
* **Architectural gravity.** The tempting mistake is to let FFmpeg's structures become
  the model. `AVFrame`, `AVPacket` and stream time bases leak upward, and then the
  editor's timeline is a thin wrapper over a command-line tool's data structures. That
  couples every layer to one library's version, its threading model and its error
  reporting.

GStreamer is the alternative pipeline framework, and its pipeline model is genuinely
appealing for a realtime graph. It is also LGPL, and its plugin ecosystem carries the
same class of transitively-linked obligations.

## Decision

FFmpeg, built and distributed deliberately, behind an abstraction that Majestat owns.

* **LGPL build**: no `--enable-gpl`, no `--enable-nonfree`, dynamically linked, notices
  and corresponding-source obligations met. The exact configuration is recorded and
  reviewed before any release, not reconstructed afterwards.
* **`maj-media` owns the vocabulary.** Stream discovery, seeking, frame retrieval, colour
  metadata, audio blocks and errors are expressed in Majestat's types. FFmpeg types do
  not appear above layer 2, and the crate never re-exports them.
* **Send/receive decode is implemented properly**: timestamps and stream time bases
  preserved, variable frame rate, interlacing, rotation, damaged media and audio stream
  selection handled explicitly rather than assumed away.
* **Hardware decode is capability-tested, never assumed.** `AVHWFramesContext` lifetime
  is tracked explicitly. Hardware decode is *not* automatically zero-copy: the
  decoder-surface-to-texture bridge is prototyped and measured in M2, with a software
  fallback on every path.
* **Camera RAW uses vendor SDKs**, not FFmpeg: Blackmagic RAW and RED R3D, each behind a
  feature flag so a build without them is a build without their terms.
* **Codec patents are a separate question from copyright** and are handled per
  jurisdiction before release. AV1 is preferred where quality allows because it is the
  royalty-free path.

## Consequences

* The abstraction is real work up front, and every new format needs a mapping into it.
  Accepted: the alternative is a timeline model that cannot outlive one library version.
* Hardware decode may under-deliver on the zero-copy goal. The fallback path is designed
  first, and the measured expectation is recorded rather than promised.
* A GPL-only feature offered by FFmpeg is unavailable. If a concrete need appears, it is
  evaluated as a separate, optionally-linked component with its own licence analysis, not
  enabled in the main build.
* ProRes and DNxHR *encoding* require conversations with Apple and Avid that decoding
  support does not imply. Until then, delivery defaults to codecs with clear terms.

## Amendment, 2026-10-15: the product licence is `Apache-2.0 OR MIT`

The Context above reasons from an undecided product licence ("which makes the most
permissive practical build the required one"). The licence is now decided, and the
decision above does not change — but the reasoning behind it is no longer provisional, and
one consequence hardens:

* **The LGPL build is now required by the product licence itself, not by prudence.** A GPL
  FFmpeg build cannot be linked into an `Apache-2.0 OR MIT` binary at all. The line "a
  GPL-only feature offered by FFmpeg is unavailable" is no longer a cost the project
  accepts in order to keep its options open; it is a constraint with no exception path
  short of relicensing the product.
* **The encoder question is now sharper than the decoder question.** FFmpeg's LGPL base
  decodes H.264 and HEVC, but `x264` and `x265` — the usual encoders — are GPL and are
  therefore excluded. M8 must take its encoder from the platform (Media Foundation) or
  from a permissively licensed library (OpenH264, BSD-2). This is recorded in
  `.planning/ROADMAP.md` as a decision M8 owns.
* **Dynamic linking remains mandatory** and, under a permissive product licence, is the
  mechanism that keeps the LGPL's relinking right exercisable by a recipient.
