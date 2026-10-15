# 0014. The product licence is `Apache-2.0 OR MIT`

* Status: Accepted
* Date: 2026-10-15

## Context

Every document in this repository so far has recorded the product licence as undecided,
and the dependency policy as "LGPL-safe until the decision is made" (ADR 0005, ADR 0009,
`docs/LICENSING.md`). That default was correct while nothing depended on the answer, and
it has stopped being free: the next phase is media I/O, which means FFmpeg, and the phase
after it is encoding, which means choosing between encoders whose licences differ in ways
that *only* the product licence can settle. `x264` is the usual answer and it is GPL. A
project that defers the licence decision past that point is a project that will discover
its licence by accident, from the first dependency it cannot remove.

The options were a proprietary commercial licence, a source-available licence, and open
source. The owner chose open source, permissive.

## Decision

The product is licensed **`Apache-2.0 OR MIT`**, at the recipient's choice.

* Declared once, as `license = "Apache-2.0 OR MIT"` in `[workspace.package]`, and
  inherited by all twenty-two crates through `license.workspace = true`. One place to
  read, one place to change.
* Full texts at the root: `LICENSE-APACHE` and `LICENSE-MIT`.
* `publish = false` is unchanged. The licence settles the terms; it does not publish
  anything.

Why this pair rather than one of them:

* **Apache-2.0 carries an explicit patent grant.** For a product that will link codec
  libraries and ship encoders, a licence that says something about patents is worth more
  than one that is silent. It does not resolve codec patent pools — nothing in a copyright
  licence does — but it settles the contributors' own position.
* **MIT stays in the pair because it is shorter.** A recipient who wants permission and
  not a document gets one.
* **It is the Rust ecosystem's convention**, and it is the exact licence all four current
  dependencies carry, so there is no boundary between the product's terms and its
  dependencies' terms to reason about.

## Consequences

* **No GPL or AGPL component may enter the shipping product.** A permissive binary cannot
  absorb copyleft code. This is stricter than the LGPL-safe default it replaces, and it is
  a refusal rather than a trade-off: `x264`, `x265` and every FFmpeg component behind
  `--enable-gpl` are excluded, with no exception path short of relicensing the product.
* **M8's encoder must come from the platform or from a permissive library.** Media
  Foundation on Windows, or OpenH264 (BSD-2). Recorded as a decision M8 owns in
  `.planning/ROADMAP.md`.
* **LGPL dependencies stay dynamically linked, behind an abstraction this workspace
  owns.** Under a permissive product licence this is what keeps the LGPL's relinking right
  exercisable by a recipient, so ADR 0005's structure is now load-bearing for licence
  compliance and not only for architecture. ADR 0005 carries an amendment saying so.
* **The dependency audit gate gains a hard answer.** Question 1 in `docs/LICENSING.md`
  now has a refusal attached: a GPL licence ends the discussion.
* **Codec patent pools are unaffected.** H.264 and HEVC obligations are a separate
  question from copyright, per jurisdiction, and a permissive licence neither grants nor
  resolves them. AV1 remains preferred where quality allows.
* **The copyright holder is still a placeholder.** `authors` and `LICENSE-MIT` say
  "Majestat", which is a project name and not a legal entity. That has to be set before
  any release, and it is the owner's to set.
* **Relicensing later is effectively impossible** once there are outside contributors,
  which is the point: this is the decision that stops being cheap to revisit, and it is
  being made deliberately rather than discovered.
