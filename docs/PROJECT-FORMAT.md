# The `.maj` project format

Schema version **1**, implemented in `maj_core::format`.

## Shape

A project is one UTF-8 JSON document: the state, plus the ordered command log that
produced it. Top-level fields are fixed and unknown fields are rejected, so a typo in a
hand-edited file is an error rather than silently dropped data.

```json
{
  "magic": "MAJESTAT-PROJECT",
  "schema_version": 1,
  "name": "documentary-cut-3",
  "assets": [
    {
      "id": 1,
      "name": "A001_C003.mov",
      "uri": "file:///media/A001_C003.mov",
      "timebase": { "num": 24000, "den": 1001 },
      "duration_frames": 14400,
      "has_video": true,
      "has_audio": true
    }
  ],
  "sequences": [
    {
      "id": 2,
      "name": "Main",
      "timebase": { "num": 24000, "den": 1001 },
      "width": 3840,
      "height": 2160,
      "sample_rate": 48000,
      "tracks": [
        {
          "id": 3,
          "kind": "video",
          "name": "V1",
          "enabled": true,
          "locked": false,
          "muted": false,
          "clips": [
            {
              "id": 4,
              "asset_id": 1,
              "name": "A001_C003",
              "source_range": { "start": 240, "end": 1200 },
              "timeline_start": 0,
              "speed": { "num": 1, "den": 1 },
              "enabled": true
            }
          ]
        }
      ]
    }
  ],
  "log": [
    { "seq": 0, "author": "local", "command": { "kind": "create_media_asset", "index": 0, "asset": { } } }
  ]
}
```

### Notes on the shape

* **Rationals are `{num, den}` pairs, never decimals.** `24000/1001` is not `23.976`.
  Both fields are integers, so the canonical form has exactly one text representation.
* **Identifiers are plain integers**, unique across the project for clips and per kind
  for the rest.
* **`clips` is insertion order, not timeline order.** Undoing a removal has to restore
  the position a clip occupied, so position is part of the state and part of the hash.
  Renderers and queries sort with `Track::clips_in_timeline_order`.
* **The log is authoritative and the state is redundant.** The state is a cached
  derivation, kept so a project opens without replaying. `rebuild_from_log` reproduces
  it exactly, and the test suite checks that on every seed.

## Load gates

All four fail closed, in this order:

1. **Parse.** Not JSON, not a project.
2. **Schema version.** Missing, or newer than this build, is refused with a clear
   message rather than half-read. An unsupported older version is also refused, because
   schema 1 is the first published one.
3. **Deserialisation.** `deny_unknown_fields` everywhere. Nested values re-run their own
   validation: an unreduced or negative-denominator rational and an inverted frame range
   cannot enter the model, because they are rejected by `try_from` on the way in.
4. **Model validation.** Unique identifiers per kind, non-degenerate sequence settings,
   positive clip speeds, every clip's source range inside its media, a command log
   numbered contiguously from zero.

`project.validate()` is public, so a caller can check a project at any time — the test
suite calls it after every one of ten thousand commands.

## Commands

Thirty variants, each with an exact inverse. Commands that create an entity carry their
insertion index, which is what lets an undo restore a clip to the middle of a track
rather than the end.

| Command | Inverse |
|---|---|
| `create_media_asset` | `remove_media_asset` |
| `remove_media_asset` | `insert_media_asset` |
| `insert_media_asset` | `remove_media_asset` |
| `create_sequence` | `remove_sequence` |
| `remove_sequence` | `insert_sequence` |
| `insert_sequence` | `remove_sequence` |
| `create_track` | `remove_track` |
| `remove_track` | `create_track` |
| `append_clip` | `remove_clip` |
| `insert_clip` | `remove_clip` |
| `remove_clip` | `insert_clip` |
| `trim_clip` | `trim_clip` with the previous values |
| `move_clip` | `move_clip` with the previous position |
| `reparent_clip` | `reparent_clip` with the previous track and index |
| `set_clip_speed` | `set_clip_speed` with the previous speed |
| `set_clip_enabled` | `set_clip_enabled` with the previous value |
| `set_track_enabled` | `set_track_enabled` with the previous value |
| `set_clip_transform` | `set_clip_transform` with the previous transform |
| `set_clip_opacity` | `set_clip_opacity` with the previous opacity |
| `set_clip_blend` | `set_clip_blend` with the previous blend mode |
| `add_effect` | `remove_effect` |
| `remove_effect` | `add_effect` with the index and the effect itself |
| `set_effect_enabled` | `set_effect_enabled` with the previous value |
| `set_effect_param` | `set_effect_param` with the previous value |
| `set_channel_keys` | `set_channel_keys` with the previous keys |
| `add_mask` | `remove_mask` |
| `remove_mask` | `add_mask` with the index and the mask itself |
| `set_mask` | `set_mask` with the previous mask |
| `add_transition` | `remove_transition` |
| `remove_transition` | `add_transition` with the index and the transition itself |

`Command::apply` returns the inverse. There is no separate undo bookkeeping, and no way
for one to drift out of sync with the other.

## Why text, and why not SQLite yet

Plan section 4 specifies a SQLite container with content-addressed blobs for caches and
previews. Version 1 ships a single text document instead, for two reasons:

1. **A text document diffs.** "Reviewable, line-level diffs of an edit" is a headline
   capability. It is worth having from the first schema version rather than later.
2. **The host toolchain for M0/M1 has no C compiler.** A bundled SQLite is not
   buildable here. See `docs/adr/0009-toolchain-and-dependency-policy.md`.

The binary container arrives with the cache work in M10 as a packaging change: SQLite
and the blob store hold the same document plus content-addressed cache entries. The
schema is designed for that — `assets`, `sequences` and `log` are already separate,
hashable sections.

## Compatibility rules

* A new field with a default is **not** a schema bump while the old document remains
  readable. `deny_unknown_fields` means readers reject newer documents, so the version
  must be bumped whenever a field is added.
* A migration is a step function from version *n* to *n+1*, chained in
  `format::migrate`. There is one arm today and the shape is documented so the next one
  is additive.
* Replay renumbers log positions to stay contiguous, so a log slice is a valid document.
  Commands and authors are untouched, which is why replaying a log reproduces the log
  itself byte for byte.

## Interchange

OpenTimelineIO import and export arrive with the export work in M8. OTIO is an
interchange schema, not a project model: track order, transition behaviour, retimes and
colour metadata do not always map losslessly between applications, so unsupported data
is preserved in metadata rather than discarded.
