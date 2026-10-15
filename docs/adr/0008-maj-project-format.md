# 0008. The `.maj` project format

* Status: Accepted
* Date: 2026-10-13

## Context

The plan specifies a container of SQLite plus content-addressed blobs: SQLite for
metadata and the command log, hashed blobs for caches and previews. Two things push back
on that for the first schema version.

1. **Reviewability is a headline capability.** "Diff and code-review an edit" is only
   real if an edit is text. A SQLite file is opaque to every review tool in existence.
2. **The host toolchain for M0/M1 has no C compiler**, so a bundled SQLite does not build.
   See ADR 0009.

There is also a determinism requirement from ADR 0001 and ADR 0003: the document must
have exactly one canonical byte representation, so it can be hashed and compared.

## Decision

Version 1 is a **single UTF-8 JSON document**: the state plus the ordered command log.

* The file on disk is pretty-printed, so it reads and diffs line by line.
* Hashing and comparison use the **compact** canonical form, so whitespace never
  influences a hash.
* `magic` and `schema_version` are the first two fields.
* `deny_unknown_fields` everywhere: a typo is an error, not silently dropped data.
* Nested values validate themselves on the way in. `Rational` and `FrameRange` implement
  `try_from` against their canonical constructors, so an unreduced rational or an inverted
  range cannot enter the model even from a hand-edited file.
* Four load gates, all failing closed: parse, schema version, deserialisation, model
  validation.
* The state is redundant on purpose. It is a cache of the log, kept so a project opens
  without replaying, and `rebuild_from_log` must reproduce it exactly — checked in the
  test suite rather than assumed.

**The identifier allocator is not stored.** It is derived as "one past the highest
identifier in use". A stored counter breaks replay: create five entities, undo two, and
the counter matches nothing derivable from the log, so the same log yields two different
project hashes. See `maj_core::IdAllocator`.

## Consequences

* The binary container moves to M10 as a packaging change. SQLite and the blob store will
  hold the same document in separate, hashable sections plus content-addressed cache
  entries, so the migration is a container swap rather than a schema rethink.
* JSON is larger and slower to parse than a binary format. Measured acceptable at the
  scale of a project document; cache blobs are where size actually matters, and they are
  binary from the start.
* `deny_unknown_fields` means additive fields require a version bump. Accepted: the
  alternative is silently ignoring data a newer version wrote, which is worse.
* Replay renumbers log positions contiguously, so a slice of a log is a valid document
  while commands and authors are untouched. This is what makes prefix replay testable.
* OpenTimelineIO interchange in M8 is an interchange format, not the project model.
  Unsupported data is preserved in metadata rather than discarded.
